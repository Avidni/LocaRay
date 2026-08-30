use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(
    rename_all = "snake_case",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub enum TunnelPhase {
    Idle,
    CheckingOrigin,
    Starting,
    VerifyingPublicUrl,
    Connected,
    Reconnecting,
    Stopping,
    Error,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(
    rename_all = "SCREAMING_SNAKE_CASE",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub enum TunnelErrorCode {
    InvalidPort,
    OriginClosed,
    OriginUnresponsive,
    SidecarMissing,
    SidecarSpawnFailed,
    QuickTunnelConfigConflict,
    TunnelTimeout,
    TunnelServiceError,
    PublicUrlInvalid,
    NetworkLost,
    ChildExited,
    StopFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct TunnelError {
    pub code: TunnelErrorCode,
    pub cause: String,
    pub exposure_active: bool,
    pub recovery: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct TunnelSnapshot {
    pub session_id: Option<String>,
    pub phase: TunnelPhase,
    pub port: Option<u16>,
    pub local_url: Option<String>,
    pub public_url: Option<String>,
    pub started_at: Option<String>,
    pub stop_at: Option<String>,
    pub origin_reachable: Option<bool>,
    pub tunnel_connected: bool,
    pub error: Option<TunnelError>,
}

impl Default for TunnelSnapshot {
    fn default() -> Self {
        Self {
            session_id: None,
            phase: TunnelPhase::Idle,
            port: None,
            local_url: None,
            public_url: None,
            started_at: None,
            stop_at: None,
            origin_reachable: None,
            tunnel_connected: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_port() -> Self {
        Self::new("INVALID_PORT", "Enter a port from 1 through 65535.")
    }

    #[must_use]
    pub fn state_unavailable() -> Self {
        Self::new(
            "STATE_UNAVAILABLE",
            "Tunnel state is temporarily unavailable.",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEvent {
    Begin {
        session_id: String,
        port: u16,
        started_at: String,
        stop_at: Option<String>,
    },
    OriginAvailable {
        session_id: String,
    },
    SidecarStarted {
        session_id: String,
    },
    CandidateFound {
        session_id: String,
        public_url: String,
    },
    PublicUrlVerified {
        session_id: String,
    },
    ConnectionLost {
        session_id: String,
    },
    ConnectionRestored {
        session_id: String,
    },
    RequestStop {
        session_id: String,
    },
    Stopped {
        session_id: String,
    },
    Failed {
        session_id: String,
        error: TunnelError,
    },
    ChildExited {
        session_id: String,
        error: TunnelError,
    },
    Reset,
}

impl TransitionEvent {
    fn session_id(&self) -> Option<&str> {
        match self {
            Self::Begin { session_id, .. }
            | Self::OriginAvailable { session_id }
            | Self::SidecarStarted { session_id }
            | Self::CandidateFound { session_id, .. }
            | Self::PublicUrlVerified { session_id }
            | Self::ConnectionLost { session_id }
            | Self::ConnectionRestored { session_id }
            | Self::RequestStop { session_id }
            | Self::Stopped { session_id }
            | Self::Failed { session_id, .. }
            | Self::ChildExited { session_id, .. } => Some(session_id),
            Self::Reset => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Begin { .. } => "begin",
            Self::OriginAvailable { .. } => "origin_available",
            Self::SidecarStarted { .. } => "sidecar_started",
            Self::CandidateFound { .. } => "candidate_found",
            Self::PublicUrlVerified { .. } => "public_url_verified",
            Self::ConnectionLost { .. } => "connection_lost",
            Self::ConnectionRestored { .. } => "connection_restored",
            Self::RequestStop { .. } => "request_stop",
            Self::Stopped { .. } => "stopped",
            Self::Failed { .. } => "failed",
            Self::ChildExited { .. } => "child_exited",
            Self::Reset => "reset",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionOutcome {
    Applied,
    IgnoredStale,
    Unchanged,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransitionError {
    #[error("transition {event} is not allowed from {phase:?}")]
    Illegal {
        phase: TunnelPhase,
        event: &'static str,
    },
}

#[derive(Debug, Default)]
pub struct TunnelMachine {
    snapshot: TunnelSnapshot,
    candidate_public_url: Option<String>,
}

impl TunnelMachine {
    pub fn snapshot(&self) -> &TunnelSnapshot {
        &self.snapshot
    }

    #[allow(clippy::too_many_lines)]
    pub fn apply(&mut self, event: TransitionEvent) -> Result<TransitionOutcome, TransitionError> {
        if !matches!(
            event,
            TransitionEvent::Begin { .. } | TransitionEvent::Reset
        ) {
            let active_session = self.snapshot.session_id.as_deref();
            if event.session_id() != active_session {
                return Ok(TransitionOutcome::IgnoredStale);
            }
        }

        let phase = self.snapshot.phase;
        match (phase, event) {
            (
                TunnelPhase::Idle | TunnelPhase::Error | TunnelPhase::Exited,
                TransitionEvent::Begin {
                    session_id,
                    port,
                    started_at,
                    stop_at,
                },
            ) => {
                self.snapshot = TunnelSnapshot {
                    session_id: Some(session_id),
                    phase: TunnelPhase::CheckingOrigin,
                    port: Some(port),
                    local_url: Some(format!("http://127.0.0.1:{port}")),
                    started_at: Some(started_at),
                    stop_at,
                    ..TunnelSnapshot::default()
                };
                self.candidate_public_url = None;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::CheckingOrigin, TransitionEvent::OriginAvailable { .. }) => {
                self.snapshot.origin_reachable = Some(true);
                self.snapshot.phase = TunnelPhase::Starting;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::Starting, TransitionEvent::SidecarStarted { .. })
            | (TunnelPhase::Stopping, TransitionEvent::RequestStop { .. }) => {
                Ok(TransitionOutcome::Unchanged)
            }
            (TunnelPhase::Starting, TransitionEvent::CandidateFound { public_url, .. }) => {
                self.candidate_public_url = Some(public_url);
                self.snapshot.phase = TunnelPhase::VerifyingPublicUrl;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::VerifyingPublicUrl, TransitionEvent::PublicUrlVerified { .. }) => {
                self.snapshot.public_url = self.candidate_public_url.take();
                self.snapshot.tunnel_connected = true;
                self.snapshot.phase = TunnelPhase::Connected;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::Connected, TransitionEvent::ConnectionLost { .. }) => {
                self.snapshot.tunnel_connected = false;
                self.snapshot.origin_reachable = Some(false);
                self.snapshot.phase = TunnelPhase::Reconnecting;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::Reconnecting, TransitionEvent::ConnectionRestored { .. }) => {
                self.snapshot.tunnel_connected = true;
                self.snapshot.origin_reachable = Some(true);
                self.snapshot.phase = TunnelPhase::Connected;
                Ok(TransitionOutcome::Applied)
            }
            (
                TunnelPhase::CheckingOrigin
                | TunnelPhase::Starting
                | TunnelPhase::VerifyingPublicUrl
                | TunnelPhase::Connected
                | TunnelPhase::Reconnecting
                | TunnelPhase::Error,
                TransitionEvent::RequestStop { .. },
            ) => {
                self.snapshot.phase = TunnelPhase::Stopping;
                self.snapshot.tunnel_connected = false;
                Ok(TransitionOutcome::Applied)
            }
            (TunnelPhase::Stopping, TransitionEvent::Stopped { .. }) => {
                self.snapshot = TunnelSnapshot::default();
                self.candidate_public_url = None;
                Ok(TransitionOutcome::Applied)
            }
            (
                TunnelPhase::CheckingOrigin
                | TunnelPhase::Starting
                | TunnelPhase::VerifyingPublicUrl
                | TunnelPhase::Connected
                | TunnelPhase::Reconnecting
                | TunnelPhase::Stopping,
                TransitionEvent::Failed { error, .. },
            ) => {
                self.snapshot.phase = TunnelPhase::Error;
                self.snapshot.tunnel_connected = error.exposure_active;
                if !error.exposure_active {
                    self.snapshot.public_url = None;
                    self.candidate_public_url = None;
                }
                self.snapshot.error = Some(error);
                Ok(TransitionOutcome::Applied)
            }
            (
                TunnelPhase::Connected | TunnelPhase::Reconnecting,
                TransitionEvent::ChildExited { error, .. },
            ) => {
                self.snapshot.phase = TunnelPhase::Exited;
                self.snapshot.tunnel_connected = false;
                self.snapshot.public_url = None;
                self.snapshot.error = Some(error);
                self.candidate_public_url = None;
                Ok(TransitionOutcome::Applied)
            }
            (
                TunnelPhase::Idle | TunnelPhase::Error | TunnelPhase::Exited,
                TransitionEvent::Reset,
            ) => {
                if self.snapshot == TunnelSnapshot::default() {
                    Ok(TransitionOutcome::Unchanged)
                } else {
                    self.snapshot = TunnelSnapshot::default();
                    self.candidate_public_url = None;
                    Ok(TransitionOutcome::Applied)
                }
            }
            (_, event) => Err(TransitionError::Illegal {
                phase,
                event: event.name(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TransitionError, TransitionEvent, TransitionOutcome, TunnelError, TunnelErrorCode,
        TunnelMachine, TunnelPhase,
    };

    fn begin(machine: &mut TunnelMachine, session_id: &str) {
        let outcome = machine
            .apply(TransitionEvent::Begin {
                session_id: session_id.to_owned(),
                port: 3_000,
                started_at: "2026-08-30T00:00:00Z".to_owned(),
                stop_at: None,
            })
            .unwrap_or_else(|error| panic!("begin should succeed: {error}"));
        assert_eq!(outcome, TransitionOutcome::Applied);
    }

    fn connected_machine(session_id: &str) -> TunnelMachine {
        let mut machine = TunnelMachine::default();
        begin(&mut machine, session_id);
        machine
            .apply(TransitionEvent::OriginAvailable {
                session_id: session_id.to_owned(),
            })
            .unwrap_or_else(|error| panic!("origin transition should succeed: {error}"));
        machine
            .apply(TransitionEvent::CandidateFound {
                session_id: session_id.to_owned(),
                public_url: "https://session.trycloudflare.com".to_owned(),
            })
            .unwrap_or_else(|error| panic!("candidate transition should succeed: {error}"));
        machine
            .apply(TransitionEvent::PublicUrlVerified {
                session_id: session_id.to_owned(),
            })
            .unwrap_or_else(|error| panic!("verification transition should succeed: {error}"));
        machine
    }

    #[test]
    fn completes_the_happy_path() {
        let machine = connected_machine("session-a");
        let snapshot = machine.snapshot();

        assert_eq!(snapshot.phase, TunnelPhase::Connected);
        assert_eq!(
            snapshot.public_url.as_deref(),
            Some("https://session.trycloudflare.com")
        );
        assert!(snapshot.tunnel_connected);
    }

    #[test]
    fn does_not_publish_a_candidate_before_verification() {
        let mut machine = TunnelMachine::default();
        begin(&mut machine, "session-a");
        machine
            .apply(TransitionEvent::OriginAvailable {
                session_id: "session-a".to_owned(),
            })
            .unwrap_or_else(|error| panic!("origin transition should succeed: {error}"));
        machine
            .apply(TransitionEvent::CandidateFound {
                session_id: "session-a".to_owned(),
                public_url: "https://candidate.trycloudflare.com".to_owned(),
            })
            .unwrap_or_else(|error| panic!("candidate transition should succeed: {error}"));

        assert_eq!(machine.snapshot().phase, TunnelPhase::VerifyingPublicUrl);
        assert!(machine.snapshot().public_url.is_none());
    }

    #[test]
    fn ignores_events_from_a_stale_session() {
        let mut machine = connected_machine("current");
        let before = machine.snapshot().clone();
        let outcome = machine
            .apply(TransitionEvent::ConnectionLost {
                session_id: "stale".to_owned(),
            })
            .unwrap_or_else(|error| panic!("stale event should not fail: {error}"));

        assert_eq!(outcome, TransitionOutcome::IgnoredStale);
        assert_eq!(machine.snapshot(), &before);
    }

    #[test]
    fn rejects_a_second_start_while_active() {
        let mut machine = TunnelMachine::default();
        begin(&mut machine, "session-a");
        let error = machine
            .apply(TransitionEvent::Begin {
                session_id: "session-b".to_owned(),
                port: 5_173,
                started_at: "2026-08-30T00:00:01Z".to_owned(),
                stop_at: None,
            })
            .expect_err("a second start must be rejected");

        assert_eq!(
            error,
            TransitionError::Illegal {
                phase: TunnelPhase::CheckingOrigin,
                event: "begin",
            }
        );
    }

    #[test]
    fn repeated_stop_requests_are_idempotent() {
        let mut machine = connected_machine("session-a");
        let first = machine
            .apply(TransitionEvent::RequestStop {
                session_id: "session-a".to_owned(),
            })
            .unwrap_or_else(|error| panic!("first stop should succeed: {error}"));
        let second = machine
            .apply(TransitionEvent::RequestStop {
                session_id: "session-a".to_owned(),
            })
            .unwrap_or_else(|error| panic!("second stop should succeed: {error}"));

        assert_eq!(first, TransitionOutcome::Applied);
        assert_eq!(second, TransitionOutcome::Unchanged);
        assert_eq!(machine.snapshot().phase, TunnelPhase::Stopping);
    }

    #[test]
    fn unexpected_child_exit_clears_public_exposure() {
        let mut machine = connected_machine("session-a");
        machine
            .apply(TransitionEvent::ChildExited {
                session_id: "session-a".to_owned(),
                error: TunnelError {
                    code: TunnelErrorCode::ChildExited,
                    cause: "The tunnel process ended unexpectedly.".to_owned(),
                    exposure_active: false,
                    recovery: "Retry the tunnel.".to_owned(),
                },
            })
            .unwrap_or_else(|error| panic!("child exit should succeed: {error}"));

        assert_eq!(machine.snapshot().phase, TunnelPhase::Exited);
        assert!(machine.snapshot().public_url.is_none());
        assert!(!machine.snapshot().tunnel_connected);
    }
}
