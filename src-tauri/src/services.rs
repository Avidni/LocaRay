use std::{collections::BTreeMap, net::Ipv4Addr, time::Duration};

use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};
use tokio::{net::TcpStream, time::timeout};
use ts_rs::TS;

use crate::state::CommandError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct DetectedService {
    pub port: u16,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct OriginProbe {
    pub reachable: bool,
    pub http_status: Option<u16>,
    pub http_responded: bool,
}

pub fn discover_services() -> Result<Vec<DetectedService>, CommandError> {
    let sockets = get_sockets_info(AddressFamilyFlags::IPV4, ProtocolFlags::TCP).map_err(|_| {
        CommandError::new("DISCOVERY_FAILED", "Local services could not be scanned.")
    })?;
    let system = System::new_all();
    let mut services = BTreeMap::new();

    for socket in sockets {
        if let ProtocolSocketInfo::Tcp(tcp) = socket.protocol_socket_info {
            if tcp.state == TcpState::Listen
                && matches!(tcp.local_addr, std::net::IpAddr::V4(address) if address == Ipv4Addr::LOCALHOST || address == Ipv4Addr::UNSPECIFIED)
            {
                let hint = socket
                    .associated_pids
                    .iter()
                    .find_map(|pid| system.process(Pid::from_u32(*pid)))
                    .and_then(|process| process_hint(&process.name().to_string_lossy()));
                services
                    .entry(tcp.local_port)
                    .and_modify(|existing: &mut Option<String>| {
                        if existing.is_none() {
                            existing.clone_from(&hint);
                        }
                    })
                    .or_insert(hint);
            }
        }
    }

    Ok(services
        .into_iter()
        .map(|(port, hint)| DetectedService { port, hint })
        .collect())
}

fn process_hint(process_name: &str) -> Option<String> {
    let name = process_name.to_ascii_lowercase();
    let hint = if name.contains("node") || name.contains("deno") || name.contains("bun") {
        "JavaScript runtime"
    } else if name.contains("python") {
        "Python"
    } else if name.contains("cargo") || name.contains("rust") {
        "Rust"
    } else if name.contains("dotnet") || name.contains("iisexpress") {
        ".NET"
    } else if name.contains("java") {
        "Java"
    } else {
        return None;
    };
    Some(hint.to_owned())
}

pub async fn probe_origin(port: u16) -> Result<OriginProbe, CommandError> {
    let address = (Ipv4Addr::LOCALHOST, port);
    match timeout(Duration::from_millis(1_500), TcpStream::connect(address)).await {
        Ok(Ok(stream)) => drop(stream),
        _ => {
            return Err(CommandError::new(
                "ORIGIN_CLOSED",
                format!("Nothing is running on port {port}."),
            ));
        }
    }

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(1_500))
        .timeout(Duration::from_millis(2_500))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| CommandError::new("ORIGIN_UNRESPONSIVE", "The HTTP probe could not start."))?;
    let url = format!("http://127.0.0.1:{port}/");

    match client.get(url).send().await {
        Ok(response) => Ok(OriginProbe {
            reachable: true,
            http_status: Some(response.status().as_u16()),
            http_responded: true,
        }),
        Err(_) => Ok(OriginProbe {
            reachable: true,
            http_status: None,
            http_responded: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::{probe_origin, process_hint};

    #[tokio::test]
    async fn distinguishes_a_closed_port() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener should bind");
        let port = listener
            .local_addr()
            .expect("listener should have an address")
            .port();
        drop(listener);

        let error = probe_origin(port)
            .await
            .expect_err("closed port should fail");
        assert_eq!(error.code, "ORIGIN_CLOSED");
    }

    #[test]
    fn reports_only_reliable_process_hints() {
        assert_eq!(
            process_hint("node.exe").as_deref(),
            Some("JavaScript runtime")
        );
        assert_eq!(process_hint("python3.exe").as_deref(), Some("Python"));
        assert!(process_hint("unknown-server.exe").is_none());
    }
}
