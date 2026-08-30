mod compatibility_proxy;
mod diagnostics;
mod services;
mod settings;
mod state;
mod tunnel;
mod url_parser;

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use services::{DetectedService, OriginProbe};
use settings::{CloseBehavior, Settings};
pub use state::{CommandError, TunnelPhase, TunnelSnapshot};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use ts_rs::TS;
use tunnel::TunnelManager;

#[derive(Default)]
struct AppState {
    tunnel: Arc<TunnelManager>,
    settings: RwLock<Settings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
struct StartTunnelRequest {
    port: u32,
    stop_after_minutes: Option<u16>,
}

#[tauri::command]
async fn get_tunnel_snapshot(state: State<'_, AppState>) -> Result<TunnelSnapshot, CommandError> {
    Ok(state.tunnel.snapshot().await)
}

#[tauri::command]
fn validate_port(port: u32) -> Result<u16, CommandError> {
    u16::try_from(port)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(CommandError::invalid_port)
}

#[tauri::command]
async fn discover_services() -> Result<Vec<DetectedService>, CommandError> {
    tauri::async_runtime::spawn_blocking(services::discover_services)
        .await
        .map_err(|_| {
            CommandError::new("DISCOVERY_FAILED", "Local services could not be scanned.")
        })?
}

#[tauri::command]
async fn probe_origin(port: u32) -> Result<OriginProbe, CommandError> {
    services::probe_origin(validate_port(port)?).await
}

#[tauri::command]
async fn start_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartTunnelRequest,
) -> Result<TunnelSnapshot, CommandError> {
    let port = validate_port(request.port)?;
    let diagnostic_logging = state
        .settings
        .read()
        .map(|settings| settings.diagnostic_logging)
        .map_err(|_| CommandError::state_unavailable())?;
    state
        .tunnel
        .start(app, port, request.stop_after_minutes, diagnostic_logging)
        .await
}

#[tauri::command]
async fn stop_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TunnelSnapshot, CommandError> {
    state.tunnel.stop(app).await
}

#[tauri::command]
async fn reset_tunnel(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TunnelSnapshot, CommandError> {
    state.tunnel.reset(&app).await
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CloseAction {
    StopAndQuit,
    KeepRunningInTray,
    Cancel,
}

#[tauri::command]
async fn resolve_close_request(
    app: AppHandle,
    state: State<'_, AppState>,
    action: CloseAction,
) -> Result<(), CommandError> {
    match action {
        CloseAction::StopAndQuit => {
            state.tunnel.shutdown(&app).await;
            app.exit(0);
        }
        CloseAction::KeepRunningInTray => {
            if let Some(window) = app.get_webview_window("main") {
                window.hide().map_err(|_| {
                    CommandError::new("WINDOW_FAILED", "The window could not be hidden.")
                })?;
            }
        }
        CloseAction::Cancel => {}
    }
    Ok(())
}

#[tauri::command]
async fn get_diagnostics(state: State<'_, AppState>) -> Result<Vec<String>, CommandError> {
    Ok(state.tunnel.diagnostics().await)
}

#[tauri::command]
async fn copy_public_url(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let url = state.tunnel.snapshot().await.public_url.ok_or_else(|| {
        CommandError::new("NO_PUBLIC_URL", "No validated public URL is available.")
    })?;
    app.clipboard()
        .write_text(url)
        .map_err(|_| CommandError::new("CLIPBOARD_FAILED", "The URL could not be copied."))
}

#[tauri::command]
async fn open_public_url(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let url = state.tunnel.snapshot().await.public_url.ok_or_else(|| {
        CommandError::new("NO_PUBLIC_URL", "No validated public URL is available.")
    })?;
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|_| CommandError::new("OPEN_FAILED", "The system browser could not open the URL."))
}

#[tauri::command]
async fn copy_diagnostics(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let diagnostics = state.tunnel.diagnostics().await.join("\n");
    app.clipboard()
        .write_text(diagnostics)
        .map_err(|_| CommandError::new("CLIPBOARD_FAILED", "Diagnostics could not be copied."))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, CommandError> {
    state
        .settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|_| CommandError::state_unavailable())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, CommandError> {
    settings::save(&app, &settings)?;
    *state
        .settings
        .write()
        .map_err(|_| CommandError::state_unavailable())? = settings.clone();
    Ok(settings)
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    rename_all = "camelCase",
    export,
    export_to = "../../src/contracts/generated/"
)]
struct AboutInfo {
    app_version: String,
    cloudflared_version: String,
    platform: String,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_about_info(app: AppHandle) -> AboutInfo {
    AboutInfo {
        app_version: app.package_info().version.to_string(),
        cloudflared_version: "2026.8.2".to_owned(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InformationResource {
    QuickTunnelDocumentation,
    CloudflaredLicense,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn open_information(app: AppHandle, resource: InformationResource) -> Result<(), CommandError> {
    let url = match resource {
        InformationResource::QuickTunnelDocumentation => {
            "https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/"
        }
        InformationResource::CloudflaredLicense => {
            "https://github.com/cloudflare/cloudflared/blob/master/LICENSE"
        }
    };
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|_| CommandError::new("OPEN_FAILED", "The information page could not be opened."))
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn close_requires_decision(snapshot: &TunnelSnapshot) -> bool {
    matches!(
        snapshot.phase,
        TunnelPhase::CheckingOrigin
            | TunnelPhase::Starting
            | TunnelPhase::VerifyingPublicUrl
            | TunnelPhase::Connected
            | TunnelPhase::Reconnecting
            | TunnelPhase::Stopping
    ) || snapshot.tunnel_connected
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show LocaRay", true, None::<&str>)?;
    let copy = MenuItem::with_id(app, "copy", "Copy public URL", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop tunnel", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LocaRay", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &copy, &stop, &separator, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_owned()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("LocaRay: Public access off")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "copy" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Some(url) = state.tunnel.snapshot().await.public_url {
                        let _ = app.clipboard().write_text(url);
                    }
                });
            }
            "stop" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let manager = app.state::<AppState>().tunnel.clone();
                    let _ = manager.stop(app).await;
                });
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let manager = app.state::<AppState>().tunnel.clone();
                    manager.shutdown(&app).await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Builds and runs the native `LocaRay` application.
///
/// # Errors
///
/// Returns a Tauri error when application construction fails.
pub fn run() -> Result<(), tauri::Error> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState::default())
        .setup(|app| {
            let loaded = settings::load(app.handle());
            if let Ok(mut settings) = app.state::<AppState>().settings.write() {
                *settings = loaded;
            }
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            let WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let app = window.app_handle();
            let state = app.state::<AppState>();
            let snapshot = tauri::async_runtime::block_on(state.tunnel.snapshot());
            if !close_requires_decision(&snapshot) {
                return;
            }
            let close_behavior = state
                .settings
                .read()
                .map_or(CloseBehavior::Ask, |settings| settings.close_behavior);
            api.prevent_close();
            match close_behavior {
                CloseBehavior::Ask => {
                    let _ = window.emit("app://close-requested", ());
                }
                CloseBehavior::KeepRunningInTray => {
                    let _ = window.hide();
                }
                CloseBehavior::StopAndQuit => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let manager = app.state::<AppState>().tunnel.clone();
                        manager.shutdown(&app).await;
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_tunnel_snapshot,
            validate_port,
            discover_services,
            probe_origin,
            start_tunnel,
            stop_tunnel,
            reset_tunnel,
            resolve_close_request,
            get_diagnostics,
            copy_public_url,
            open_public_url,
            copy_diagnostics,
            get_settings,
            update_settings,
            get_about_info,
            open_information
        ])
        .build(tauri::generate_context!())?;

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let manager = app_handle.state::<AppState>().tunnel.clone();
            tauri::async_runtime::block_on(manager.shutdown(app_handle));
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{close_requires_decision, validate_port, TunnelPhase, TunnelSnapshot};

    #[test]
    fn validates_port_boundaries() {
        assert_eq!(validate_port(1), Ok(1));
        assert_eq!(validate_port(65_535), Ok(65_535));
        assert!(validate_port(0).is_err());
        assert!(validate_port(65_536).is_err());
    }

    #[test]
    fn close_only_needs_confirmation_during_an_active_lifecycle() {
        let mut snapshot = TunnelSnapshot::default();
        assert!(!close_requires_decision(&snapshot));

        snapshot.phase = TunnelPhase::Starting;
        assert!(close_requires_decision(&snapshot));

        snapshot.phase = TunnelPhase::Exited;
        snapshot.session_id = Some("finished-session".to_owned());
        assert!(!close_requires_decision(&snapshot));

        snapshot.tunnel_connected = true;
        assert!(close_requires_decision(&snapshot));
    }
}
