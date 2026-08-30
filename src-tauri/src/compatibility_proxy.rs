use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::{
    body::Incoming,
    client::conn::http1 as client_http1,
    header::{HeaderValue, HOST, ORIGIN},
    server::conn::http1 as server_http1,
    service::service_fn,
    Request, Response, StatusCode, Uri,
};
use hyper_util::rt::TokioIo;
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use url::Url;

use crate::url_parser::validate_public_url;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const BAD_GATEWAY_MESSAGE: &str = "The local development server is unavailable.";

type ProxyBody = UnsyncBoxBody<Bytes, hyper::Error>;

#[derive(Clone)]
struct ProxyContext {
    target_port: u16,
    public_origin: Arc<RwLock<Option<String>>>,
}

pub struct CompatibilityProxy {
    address: SocketAddr,
    public_origin: Arc<RwLock<Option<String>>>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl CompatibilityProxy {
    pub async fn start(target_port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let public_origin = Arc::new(RwLock::new(None));
        let context = ProxyContext {
            target_port,
            public_origin: Arc::clone(&public_origin),
        };
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let task = tokio::spawn(run_listener(listener, context, shutdown_receiver));

        Ok(Self {
            address,
            public_origin,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn set_public_url(&self, public_url: &str) -> bool {
        let Some(validated) = validate_public_url(public_url) else {
            return false;
        };
        let Ok(parsed) = Url::parse(&validated) else {
            return false;
        };
        let origin = parsed.origin().ascii_serialization();
        if let Ok(mut public_origin) = self.public_origin.write() {
            *public_origin = Some(origin);
            return true;
        }
        false
    }

    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(mut task) = self.task.take() {
            if timeout(SHUTDOWN_TIMEOUT, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

impl Drop for CompatibilityProxy {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_listener(
    listener: TcpListener,
    context: ProxyContext,
    mut shutdown: oneshot::Receiver<()>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else {
                    break;
                };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let connection_context = context.clone();
                connections.spawn(serve_connection(stream, connection_context));
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                let _ = completed;
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn serve_connection(stream: TcpStream, context: ProxyContext) {
    let service = service_fn(move |request| proxy_request(request, context.clone()));
    let connection = server_http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    let _ = connection.await;
}

async fn proxy_request(
    mut request: Request<Incoming>,
    context: ProxyContext,
) -> Result<Response<ProxyBody>, Infallible> {
    rewrite_next_development_origin(&mut request, &context);
    rewrite_target(&mut request, context.target_port);

    let downstream_upgrade = if is_upgrade_request(&request) {
        Some(hyper::upgrade::on(&mut request))
    } else {
        None
    };

    let Ok(origin_stream) = TcpStream::connect(("127.0.0.1", context.target_port)).await else {
        return Ok(bad_gateway());
    };
    let Ok((mut sender, connection)) = client_http1::handshake(TokioIo::new(origin_stream)).await
    else {
        return Ok(bad_gateway());
    };
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });

    let Ok(mut response) = sender.send_request(request).await else {
        return Ok(bad_gateway());
    };

    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(downstream_upgrade) = downstream_upgrade {
            let upstream_upgrade = hyper::upgrade::on(&mut response);
            tokio::spawn(async move {
                let Ok((downstream, upstream)) =
                    tokio::try_join!(downstream_upgrade, upstream_upgrade)
                else {
                    return;
                };
                let mut downstream = TokioIo::new(downstream);
                let mut upstream = TokioIo::new(upstream);
                let _ = copy_bidirectional(&mut downstream, &mut upstream).await;
            });
        }
    }

    Ok(response.map(BodyExt::boxed_unsync))
}

fn rewrite_next_development_origin(request: &mut Request<Incoming>, context: &ProxyContext) {
    let Some(incoming_origin) = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return;
    };
    let public_origin = context
        .public_origin
        .read()
        .ok()
        .and_then(|origin| origin.clone());
    let translated = translated_development_origin(
        request.uri().path(),
        incoming_origin,
        public_origin.as_deref(),
        context.target_port,
    );
    if let Some(value) = translated.and_then(|origin| HeaderValue::from_str(&origin).ok()) {
        request.headers_mut().insert(ORIGIN, value);
    }
}

fn translated_development_origin(
    path: &str,
    incoming_origin: &str,
    public_origin: Option<&str>,
    target_port: u16,
) -> Option<String> {
    if !path.starts_with("/_next/") || public_origin != Some(incoming_origin) {
        return None;
    }
    Some(format!("http://localhost:{target_port}"))
}

fn rewrite_target(request: &mut Request<Incoming>, target_port: u16) {
    let path_and_query = request
        .uri()
        .path_and_query()
        .map_or("/", hyper::http::uri::PathAndQuery::as_str);
    if let Ok(uri) = path_and_query.parse::<Uri>() {
        *request.uri_mut() = uri;
    }
    let authority = format!("localhost:{target_port}");
    if let Ok(value) = HeaderValue::from_str(&authority) {
        request.headers_mut().insert(HOST, value);
    }
}

fn is_upgrade_request(request: &Request<Incoming>) -> bool {
    request
        .headers()
        .get(hyper::header::UPGRADE)
        .is_some_and(|value| !value.is_empty())
}

fn bad_gateway() -> Response<ProxyBody> {
    let body = Full::new(Bytes::from_static(BAD_GATEWAY_MESSAGE.as_bytes()))
        .map_err(|never| match never {})
        .boxed_unsync();
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .header(hyper::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .unwrap_or_else(|_| Response::new(empty_body()))
}

fn empty_body() -> ProxyBody {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed_unsync()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    use super::{translated_development_origin, CompatibilityProxy};

    const PUBLIC_ORIGIN: &str = "https://example.trycloudflare.com";

    #[test]
    fn rewrites_only_exact_validated_next_development_origins() {
        assert_eq!(
            translated_development_origin(
                "/_next/static/chunk.js",
                PUBLIC_ORIGIN,
                Some(PUBLIC_ORIGIN),
                3000,
            ),
            Some("http://localhost:3000".to_owned())
        );
        assert_eq!(
            translated_development_origin("/api/action", PUBLIC_ORIGIN, Some(PUBLIC_ORIGIN), 3000),
            None
        );
        assert_eq!(
            translated_development_origin(
                "/_next/static/chunk.js",
                "https://attacker.example",
                Some(PUBLIC_ORIGIN),
                3000,
            ),
            None
        );
        assert_eq!(
            translated_development_origin("/_next/static/chunk.js", PUBLIC_ORIGIN, None, 3000,),
            None
        );
    }

    #[tokio::test]
    async fn proxies_http_and_rewrites_the_next_origin() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_task = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            let expected_origin = format!("origin: http://localhost:{origin_port}");
            let translated = request
                .lines()
                .any(|line| line.eq_ignore_ascii_case(&expected_origin));
            let body = if translated {
                "translated"
            } else {
                "unchanged"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let proxy = CompatibilityProxy::start(origin_port).await.unwrap();
        assert!(proxy.set_public_url(PUBLIC_ORIGIN));
        let proxy_address = SocketAddr::from(([127, 0, 0, 1], proxy.port()));
        let response = reqwest::Client::new()
            .get(format!("http://{proxy_address}/_next/static/chunk.js"))
            .header("Origin", PUBLIC_ORIGIN)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "translated");
        origin_task.await.unwrap();
        proxy.shutdown().await;
        assert!(TcpStream::connect(proxy_address).await.is_err());
    }

    #[tokio::test]
    async fn proxies_upgraded_connections_without_buffering_the_stream() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_task = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            let request = read_headers(&mut stream).await;
            let expected_origin = format!("origin: http://localhost:{origin_port}");
            assert!(request
                .lines()
                .any(|line| line.eq_ignore_ascii_case(&expected_origin)));
            stream
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n",
                )
                .await
                .unwrap();
            let mut payload = [0_u8; 4];
            stream.read_exact(&mut payload).await.unwrap();
            stream.write_all(&payload).await.unwrap();
        });

        let proxy = CompatibilityProxy::start(origin_port).await.unwrap();
        assert!(proxy.set_public_url(PUBLIC_ORIGIN));
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port()))
            .await
            .unwrap();
        client
            .write_all(
                format!(
                    "GET /_next/hmr HTTP/1.1\r\nHost: example.trycloudflare.com\r\nOrigin: {PUBLIC_ORIGIN}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let response = read_headers(&mut client).await;
        assert!(response.starts_with("HTTP/1.1 101"));
        client.write_all(b"ping").await.unwrap();
        let mut echoed = [0_u8; 4];
        client.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"ping");

        origin_task.await.unwrap();
        proxy.shutdown().await;
    }

    async fn read_headers(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut byte = [0_u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            bytes.push(byte[0]);
        }
        String::from_utf8(bytes).unwrap()
    }
}
