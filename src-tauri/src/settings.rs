use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_store::StoreExt;
use ts_rs::TS;

use crate::state::CommandError;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "preferences";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(
    rename_all = "snake_case",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub enum CloseBehavior {
    Ask,
    KeepRunningInTray,
    StopAndQuit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
pub struct Settings {
    pub close_behavior: CloseBehavior,
    pub default_stop_after_minutes: Option<u16>,
    pub launch_at_login: bool,
    pub diagnostic_logging: bool,
    pub last_successful_port: Option<u16>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_behavior: CloseBehavior::Ask,
            default_stop_after_minutes: None,
            launch_at_login: false,
            diagnostic_logging: true,
            last_successful_port: None,
        }
    }
}

pub fn load(app: &AppHandle) -> Settings {
    app.store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(STORE_KEY))
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), CommandError> {
    if !matches!(
        settings.default_stop_after_minutes,
        None | Some(30 | 60 | 120)
    ) {
        return Err(CommandError::new(
            "INVALID_TIMER",
            "Choose no timer, 30, 60, or 120 minutes.",
        ));
    }

    if settings.launch_at_login {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|_| {
        CommandError::new(
            "AUTOSTART_FAILED",
            "The launch at login preference could not be updated.",
        )
    })?;

    let store = app
        .store(STORE_FILE)
        .map_err(|_| CommandError::new("SETTINGS_FAILED", "Settings could not be opened."))?;
    let value = serde_json::to_value(settings)
        .map_err(|_| CommandError::new("SETTINGS_FAILED", "Settings could not be saved."))?;
    store.set(STORE_KEY, value);
    store
        .save()
        .map_err(|_| CommandError::new("SETTINGS_FAILED", "Settings could not be saved."))
}
