use std::{sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use regex::Regex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::{
    process::{CommandChild, CommandEvent},
    ShellExt,
};
use tokio::{sync::Mutex, time::timeout};
use uuid::Uuid;

use crate::{
    compatibility_proxy::CompatibilityProxy,
    diagnostics::DiagnosticBuffer,
    services::probe_origin,
    state::{
        CommandError, TransitionEvent, TunnelError, TunnelErrorCode, TunnelMachine, TunnelPhase,
        TunnelSnapshot,
    },
    url_parser::PublicUrlExtractor,
};

const SNAPSHOT_EVENT: &str = "tunnel://snapshot";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(25);
const DIAGNOSTIC_LIMIT: usize = 64 * 1024;

struct RuntimeState {
    machine: TunnelMachine,
    child: Option<CommandChild>,
    compatibility_proxy: Option<CompatibilityProxy>,
    diagnostics: DiagnosticBuffer,
    diagnostic_logging: bool,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            machine: TunnelMachine::default(),
            child: None,
            compatibility_proxy: None,
            diagnostics: DiagnosticBuffer::new(DIAGNOSTIC_LIMIT),
            diagnostic_logging: true,
        }
    }
}

#[derive(Default)]
pub struct TunnelManager {
    runtime: Mutex<RuntimeState>,
}

impl TunnelManager {
    pub async fn snapshot(&self) -> TunnelSnapshot {
        self.runtime.lock().await.machine.snapshot().clone()
    }

    pub async fn start(
        self: &Arc<Self>,
        app: AppHandle,
        port: u16,
        stop_after_minutes: Option<u16>,
        diagnostic_logging: bool,
    ) -> Result<TunnelSnapshot, CommandError> {
        if !matches!(stop_after_minutes, None | Some(30 | 60 | 120)) {
            return Err(CommandError::new(
                "INVALID_TIMER",
                "Choose no timer, 30, 60, or 120 minutes.",
            ));
        }

        let session_id = Uuid::new_v4().to_string();
        let started_at = Utc::now();
        let stop_at = stop_after_minutes
            .map(|minutes| started_at + ChronoDuration::minutes(i64::from(minutes)))
            .map(|time| time.to_rfc3339());
        let snapshot = {
            let mut runtime = self.runtime.lock().await;
            runtime.diagnostics.clear();
            runtime.diagnostic_logging = diagnostic_logging;
            runtime
                .machine
                .apply(TransitionEvent::Begin {
                    session_id: session_id.clone(),
                    port,
                    started_at: started_at.to_rfc3339(),
                    stop_at,
                })
                .map_err(|_| {
                    CommandError::new(
                        "TUNNEL_ALREADY_ACTIVE",
                        "Stop the current tunnel before starting another.",
                    )
                })?;
            runtime.machine.snapshot().clone()
        };
        emit_snapshot(&app, &snapshot);

        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            manager
                .run_session(app, session_id, port, stop_after_minutes)
                .await;
        });
        Ok(snapshot)
    }

    pub async fn reset(&self, app: &AppHandle) -> Result<TunnelSnapshot, CommandError> {
        let snapshot = {
            let mut runtime = self.runtime.lock().await;
            runtime.machine.apply(TransitionEvent::Reset).map_err(|_| {
                CommandError::new(
                    "TUNNEL_ACTIVE",
                    "Stop the current tunnel before changing the port.",
                )
            })?;
            runtime.machine.snapshot().clone()
        };
        emit_snapshot(app, &snapshot);
        Ok(snapshot)
    }

    #[allow(clippy::too_many_lines)]
    async fn run_session(
        self: Arc<Self>,
        app: AppHandle,
        session_id: String,
        port: u16,
        stop_after_minutes: Option<u16>,
    ) {
        match probe_origin(port).await {
            Ok(probe) => {
                self.transition(
                    &app,
                    TransitionEvent::OriginAvailable {
                        session_id: session_id.clone(),
                    },
                )
                .await;
                if !probe.http_responded {
                    self.push_diagnostic(
                        "The port accepted a connection, but the HTTP probe did not complete.",
                    )
                    .await;
                }
            }
            Err(error) => {
                self.fail(
                    &app,
                    &session_id,
                    TunnelErrorCode::OriginClosed,
                    error.message,
                    "Start the local server, refresh services, then retry.",
                    false,
                )
                .await;
                return;
            }
        }
        if self.snapshot().await.phase != TunnelPhase::Starting {
            return;
        }

        let config_path = match isolated_config_path(&app).await {
            Ok(path) => path,
            Err(error) => {
                self.fail(
                    &app,
                    &session_id,
                    TunnelErrorCode::QuickTunnelConfigConflict,
                    error.message,
                    "Retry the tunnel.",
                    false,
                )
                .await;
                return;
            }
        };

        let Ok(compatibility_proxy) = CompatibilityProxy::start(port).await else {
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::SidecarSpawnFailed,
                "LocaRay could not start its local compatibility service.",
                "Check whether local security software blocked LocaRay, then retry.",
                false,
            )
            .await;
            return;
        };
        let local_url = format!("http://127.0.0.1:{}", compatibility_proxy.port());
        if self.snapshot().await.phase != TunnelPhase::Starting {
            compatibility_proxy.shutdown().await;
            return;
        }
        let config_value = config_path.to_string_lossy().into_owned();
        let Ok(command) = app.shell().sidecar("cloudflared") else {
            compatibility_proxy.shutdown().await;
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::SidecarMissing,
                "The bundled tunnel component is unavailable.",
                "Repair or reinstall LocaRay.",
                false,
            )
            .await;
            return;
        };

        let spawn_result = command
            .args([
                "tunnel",
                "--config",
                config_value.as_str(),
                "--no-autoupdate",
                "--loglevel",
                "info",
                "--transport-loglevel",
                "warn",
                "--url",
                local_url.as_str(),
            ])
            .set_raw_out(true)
            .spawn();
        let Ok((mut receiver, child)) = spawn_result else {
            compatibility_proxy.shutdown().await;
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::SidecarSpawnFailed,
                "Windows did not allow the tunnel component to start.",
                "Open diagnostics, then repair or reinstall LocaRay.",
                false,
            )
            .await;
            return;
        };
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.machine.snapshot().session_id.as_deref() != Some(&session_id)
                || runtime.machine.snapshot().phase != TunnelPhase::Starting
            {
                let _ = child.kill();
                drop(runtime);
                compatibility_proxy.shutdown().await;
                return;
            }
            runtime.child = Some(child);
            runtime.compatibility_proxy = Some(compatibility_proxy);
        }
        self.transition(
            &app,
            TransitionEvent::SidecarStarted {
                session_id: session_id.clone(),
            },
        )
        .await;

        let Ok(mut extractor) = PublicUrlExtractor::new() else {
            self.cleanup_owned_resources().await;
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::PublicUrlInvalid,
                "Public address validation could not start.",
                "Retry the tunnel.",
                false,
            )
            .await;
            return;
        };

        let connected_url = timeout(STARTUP_TIMEOUT, async {
            loop {
                let event = receiver.recv().await?;
                match event {
                    CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                        self.record_output(&bytes).await;
                        if let Some(url) = extractor.push(&bytes) {
                            break Some(url);
                        }
                    }
                    CommandEvent::Error(message) => self.record_output(message.as_bytes()).await,
                    CommandEvent::Terminated(_) => break None,
                    _ => {}
                }
            }
        })
        .await;

        let Ok(Some(public_url)) = connected_url else {
            self.cleanup_owned_resources().await;
            if self.snapshot().await.phase == TunnelPhase::Stopping {
                self.transition(
                    &app,
                    TransitionEvent::Stopped {
                        session_id: session_id.clone(),
                    },
                )
                .await;
                return;
            }
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::TunnelTimeout,
                "Cloudflare did not provide a trusted public address in time.",
                "Check the network, open diagnostics, then retry.",
                false,
            )
            .await;
            return;
        };

        let proxy_configured = self
            .runtime
            .lock()
            .await
            .compatibility_proxy
            .as_ref()
            .is_some_and(|proxy| proxy.set_public_url(&public_url));
        if !proxy_configured {
            self.cleanup_owned_resources().await;
            self.fail(
                &app,
                &session_id,
                TunnelErrorCode::PublicUrlInvalid,
                "LocaRay could not secure the development compatibility route.",
                "Retry the tunnel.",
                false,
            )
            .await;
            return;
        }

        self.transition(
            &app,
            TransitionEvent::CandidateFound {
                session_id: session_id.clone(),
                public_url,
            },
        )
        .await;
        self.transition(
            &app,
            TransitionEvent::PublicUrlVerified {
                session_id: session_id.clone(),
            },
        )
        .await;

        if let Some(minutes) = stop_after_minutes {
            let manager = Arc::clone(&self);
            let app_for_timer = app.clone();
            let session_for_timer = session_id.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(u64::from(minutes) * 60)).await;
                if manager.snapshot().await.session_id.as_deref() == Some(&session_for_timer) {
                    let _ = manager.stop(app_for_timer).await;
                }
            });
        }

        let mut origin_check = tokio::time::interval(Duration::from_secs(5));
        origin_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        origin_check.tick().await;
        let mut origin_reachable = true;

        loop {
            tokio::select! {
                event = receiver.recv() => {
                    match event {
                        Some(CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes)) => {
                            self.record_output(&bytes).await;
                        }
                        Some(CommandEvent::Error(message)) => {
                            self.record_output(message.as_bytes()).await;
                        }
                        Some(CommandEvent::Terminated(_)) | None => {
                            let phase = self.snapshot().await.phase;
                            let compatibility_proxy = {
                                let mut runtime = self.runtime.lock().await;
                                runtime.child = None;
                                runtime.compatibility_proxy.take()
                            };
                            if let Some(proxy) = compatibility_proxy {
                                proxy.shutdown().await;
                            }
                            if phase == TunnelPhase::Stopping {
                                self.transition(
                                    &app,
                                    TransitionEvent::Stopped {
                                        session_id: session_id.clone(),
                                    },
                                )
                                .await;
                            } else {
                                self.transition(
                                    &app,
                                    TransitionEvent::ChildExited {
                                        session_id: session_id.clone(),
                                        error: TunnelError {
                                            code: TunnelErrorCode::ChildExited,
                                            cause: "The tunnel process ended unexpectedly.".to_owned(),
                                            exposure_active: false,
                                            recovery: "Retry the tunnel.".to_owned(),
                                        },
                                    },
                                )
                                .await;
                            }
                            break;
                        }
                        Some(_) => {}
                    }
                }
                _ = origin_check.tick() => {
                    let reachable = probe_origin(port).await.is_ok();
                    if reachable != origin_reachable {
                        origin_reachable = reachable;
                        let event = if reachable {
                            TransitionEvent::ConnectionRestored {
                                session_id: session_id.clone(),
                            }
                        } else {
                            TransitionEvent::ConnectionLost {
                                session_id: session_id.clone(),
                            }
                        };
                        self.transition(&app, event).await;
                    }
                }
            }
        }
    }

    pub async fn stop(&self, app: AppHandle) -> Result<TunnelSnapshot, CommandError> {
        let session_id = self
            .snapshot()
            .await
            .session_id
            .ok_or_else(|| CommandError::new("NO_ACTIVE_TUNNEL", "No tunnel is running."))?;
        self.transition(
            &app,
            TransitionEvent::RequestStop {
                session_id: session_id.clone(),
            },
        )
        .await;
        let killed = self.cleanup_owned_resources().await;
        if !killed {
            self.transition(&app, TransitionEvent::Stopped { session_id })
                .await;
        }
        Ok(self.snapshot().await)
    }

    pub async fn shutdown(&self, app: &AppHandle) {
        if self.snapshot().await.session_id.is_some() {
            let _ = self.stop(app.clone()).await;
        }
        self.cleanup_owned_resources().await;
    }

    pub async fn diagnostics(&self) -> Vec<String> {
        self.runtime.lock().await.diagnostics.snapshot()
    }

    async fn cleanup_owned_resources(&self) -> bool {
        let (child, compatibility_proxy) = {
            let mut runtime = self.runtime.lock().await;
            (runtime.child.take(), runtime.compatibility_proxy.take())
        };
        let killed = child.is_some_and(|child| child.kill().is_ok());
        if let Some(proxy) = compatibility_proxy {
            proxy.shutdown().await;
        }
        killed
    }

    async fn transition(&self, app: &AppHandle, event: TransitionEvent) {
        let snapshot = {
            let mut runtime = self.runtime.lock().await;
            if runtime.machine.apply(event).is_err() {
                return;
            }
            runtime.machine.snapshot().clone()
        };
        emit_snapshot(app, &snapshot);
    }

    async fn fail(
        &self,
        app: &AppHandle,
        session_id: &str,
        code: TunnelErrorCode,
        cause: impl Into<String>,
        recovery: impl Into<String>,
        exposure_active: bool,
    ) {
        self.transition(
            app,
            TransitionEvent::Failed {
                session_id: session_id.to_owned(),
                error: TunnelError {
                    code,
                    cause: cause.into(),
                    exposure_active,
                    recovery: recovery.into(),
                },
            },
        )
        .await;
    }

    async fn record_output(&self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let redacted = redact_diagnostic(&text);
        self.push_diagnostic(redacted).await;
    }

    async fn push_diagnostic(&self, entry: impl Into<String>) {
        let mut runtime = self.runtime.lock().await;
        if runtime.diagnostic_logging {
            runtime.diagnostics.push(entry);
        }
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &TunnelSnapshot) {
    let _ = app.emit(SNAPSHOT_EVENT, snapshot);
    if let Some(tray) = app.tray_by_id("main-tray") {
        let status = match snapshot.phase {
            TunnelPhase::Connected => "Publicly accessible",
            TunnelPhase::Reconnecting => "Reconnecting",
            TunnelPhase::CheckingOrigin
            | TunnelPhase::Starting
            | TunnelPhase::VerifyingPublicUrl => "Starting tunnel",
            TunnelPhase::Stopping => "Stopping tunnel",
            TunnelPhase::Error | TunnelPhase::Exited => "Tunnel stopped with an error",
            TunnelPhase::Idle => "Public access off",
        };
        let _ = tray.set_tooltip(Some(format!("LocaRay: {status}")));
    }
}

async fn isolated_config_path(app: &AppHandle) -> Result<std::path::PathBuf, CommandError> {
    let directory = app.path().app_cache_dir().map_err(|_| {
        CommandError::new(
            "CONFIG_UNAVAILABLE",
            "Tunnel isolation could not be prepared.",
        )
    })?;
    tokio::fs::create_dir_all(&directory).await.map_err(|_| {
        CommandError::new(
            "CONFIG_UNAVAILABLE",
            "Tunnel isolation could not be prepared.",
        )
    })?;
    let path = directory.join("quick-tunnel.yml");
    tokio::fs::write(&path, "{}\n").await.map_err(|_| {
        CommandError::new(
            "CONFIG_UNAVAILABLE",
            "Tunnel isolation could not be prepared.",
        )
    })?;
    Ok(path)
}

fn redact_diagnostic(input: &str) -> String {
    let public_url = Regex::new(r"https://[^\s]+trycloudflare\.com[^\s]*");
    let local_origin = Regex::new(r"https?://(?:127\.0\.0\.1|localhost):\d+");
    match (public_url, local_origin) {
        (Ok(public_url), Ok(local_origin)) => {
            let without_public = public_url.replace_all(input, "[public URL redacted]");
            local_origin
                .replace_all(&without_public, "[local origin redacted]")
                .into_owned()
        }
        _ => "Diagnostic output could not be safely displayed.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_diagnostic;

    #[test]
    fn redacts_public_and_local_urls() {
        let redacted =
            redact_diagnostic("route https://secret.trycloudflare.com to http://127.0.0.1:5173");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("5173"));
    }
}
