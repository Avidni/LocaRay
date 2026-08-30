import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AboutInfo } from "./contracts/generated/AboutInfo";
import type { DetectedService } from "./contracts/generated/DetectedService";
import type { Settings } from "./contracts/generated/Settings";
import type { StartTunnelRequest } from "./contracts/generated/StartTunnelRequest";
import type { TunnelSnapshot } from "./contracts/generated/TunnelSnapshot";
import * as e2e from "./e2e-backend";

const isE2e = import.meta.env.MODE === "e2e";

export async function getTunnelSnapshot(): Promise<TunnelSnapshot> {
  if (isE2e) return e2e.getTunnelSnapshot();
  return invoke<TunnelSnapshot>("get_tunnel_snapshot");
}

export async function validatePort(port: number): Promise<number> {
  if (isE2e) return e2e.validatePort(port);
  return invoke<number>("validate_port", { port });
}

export async function discoverServices(): Promise<DetectedService[]> {
  if (isE2e) return e2e.discoverServices();
  return invoke<DetectedService[]>("discover_services");
}

export async function startTunnel(
  request: StartTunnelRequest,
): Promise<TunnelSnapshot> {
  if (isE2e) return e2e.startTunnel(request);
  return invoke<TunnelSnapshot>("start_tunnel", { request });
}

export async function stopTunnel(): Promise<TunnelSnapshot> {
  if (isE2e) return e2e.stopTunnel();
  return invoke<TunnelSnapshot>("stop_tunnel");
}

export async function resetTunnel(): Promise<TunnelSnapshot> {
  if (isE2e) return e2e.resetTunnel();
  return invoke<TunnelSnapshot>("reset_tunnel");
}

export type CloseAction = "stop_and_quit" | "keep_running_in_tray" | "cancel";

export async function resolveCloseRequest(action: CloseAction): Promise<void> {
  if (isE2e) return e2e.resolveCloseRequest(action);
  return invoke("resolve_close_request", { action });
}

export async function copyPublicUrl(): Promise<void> {
  if (isE2e) return e2e.noop();
  return invoke("copy_public_url");
}

export async function openPublicUrl(): Promise<void> {
  if (isE2e) return e2e.noop();
  return invoke("open_public_url");
}

export async function getDiagnostics(): Promise<string[]> {
  if (isE2e) return e2e.getDiagnostics();
  return invoke<string[]>("get_diagnostics");
}

export async function copyDiagnostics(): Promise<void> {
  if (isE2e) return e2e.noop();
  return invoke("copy_diagnostics");
}

export async function getSettings(): Promise<Settings> {
  if (isE2e) return e2e.getSettings();
  return invoke<Settings>("get_settings");
}

export async function updateSettings(settings: Settings): Promise<Settings> {
  if (isE2e) return e2e.updateSettings(settings);
  return invoke<Settings>("update_settings", { settings });
}

export async function getAboutInfo(): Promise<AboutInfo> {
  if (isE2e) return e2e.getAboutInfo();
  return invoke<AboutInfo>("get_about_info");
}

export async function openInformation(
  resource: "quick_tunnel_documentation" | "cloudflared_license",
): Promise<void> {
  if (isE2e) return e2e.noop();
  return invoke("open_information", { resource });
}

export function onTunnelSnapshot(
  handler: (snapshot: TunnelSnapshot) => void,
): Promise<UnlistenFn> {
  if (isE2e) return e2e.onTunnelSnapshot(handler);
  return listen<TunnelSnapshot>("tunnel://snapshot", (event) =>
    handler(event.payload),
  );
}

export function onCloseRequested(handler: () => void): Promise<UnlistenFn> {
  if (isE2e) return e2e.onCloseRequested(handler);
  return listen<void>("app://close-requested", handler);
}
