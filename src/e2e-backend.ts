import type { UnlistenFn } from "@tauri-apps/api/event";

import type { AboutInfo } from "./contracts/generated/AboutInfo";
import type { DetectedService } from "./contracts/generated/DetectedService";
import type { Settings } from "./contracts/generated/Settings";
import type { StartTunnelRequest } from "./contracts/generated/StartTunnelRequest";
import type { TunnelSnapshot } from "./contracts/generated/TunnelSnapshot";
import type { CloseAction } from "./ipc";

const baseSnapshot: TunnelSnapshot = {
  sessionId: null,
  phase: "idle",
  port: null,
  localUrl: null,
  publicUrl: null,
  startedAt: null,
  stopAt: null,
  originReachable: null,
  tunnelConnected: false,
  error: null,
};

function initialSnapshot(): TunnelSnapshot {
  const phase = new URLSearchParams(window.location.search).get("state");
  if (phase === "connected" || phase === "reconnecting") {
    return {
      ...baseSnapshot,
      sessionId: "render-session",
      phase,
      port: 5173,
      localUrl: "http://127.0.0.1:5173",
      publicUrl: "https://calm-river-5173.trycloudflare.com",
      startedAt: new Date(Date.now() - 184_000).toISOString(),
      stopAt: new Date(Date.now() + 1_800_000).toISOString(),
      originReachable: phase === "connected",
      tunnelConnected: phase === "connected",
    };
  }
  if (phase === "error") {
    return {
      ...baseSnapshot,
      sessionId: "render-session",
      phase: "error",
      port: 5173,
      localUrl: "http://127.0.0.1:5173",
      startedAt: new Date().toISOString(),
      originReachable: false,
      error: {
        code: "ORIGIN_CLOSED",
        cause: "Nothing is running on port 5173.",
        exposureActive: false,
        recovery: "Start the local server, refresh services, then retry.",
      },
    };
  }
  return { ...baseSnapshot };
}

let snapshot = initialSnapshot();
let settings: Settings = {
  closeBehavior: "ask",
  defaultStopAfterMinutes: null,
  launchAtLogin: false,
  diagnosticLogging: true,
  lastSuccessfulPort: 5173,
};
const snapshotListeners = new Set<(next: TunnelSnapshot) => void>();

function publish(next: TunnelSnapshot) {
  snapshot = next;
  snapshotListeners.forEach((listener) => listener(next));
}

export async function getTunnelSnapshot() {
  return snapshot;
}

export async function validatePort(port: number) {
  if (!Number.isInteger(port) || port < 1 || port > 65_535)
    throw new Error("Invalid port");
  return port;
}

export async function discoverServices(): Promise<DetectedService[]> {
  return [
    { port: 5173, hint: "Vite" },
    { port: 3000, hint: "Local web app" },
  ];
}

export async function startTunnel(request: StartTunnelRequest) {
  const startedAt = new Date().toISOString();
  publish({
    ...baseSnapshot,
    sessionId: "render-session",
    phase: "checking_origin",
    port: request.port,
    localUrl: `http://127.0.0.1:${request.port}`,
    startedAt,
  });
  window.setTimeout(() => {
    publish({
      ...snapshot,
      phase: "connected",
      publicUrl: "https://calm-river-5173.trycloudflare.com",
      originReachable: true,
      tunnelConnected: true,
      stopAt: request.stopAfterMinutes
        ? new Date(Date.now() + request.stopAfterMinutes * 60_000).toISOString()
        : null,
    });
  }, 650);
  return snapshot;
}

export async function stopTunnel() {
  publish({ ...snapshot, phase: "stopping", tunnelConnected: false });
  window.setTimeout(() => publish({ ...baseSnapshot }), 400);
  return snapshot;
}

export async function resetTunnel() {
  publish({ ...baseSnapshot });
  return snapshot;
}

export async function getDiagnostics() {
  return [
    "Origin probe completed.",
    "Trusted Quick Tunnel hostname validated.",
    "Public and local URLs are redacted from diagnostics.",
  ];
}

export async function getSettings() {
  return settings;
}

export async function updateSettings(next: Settings) {
  settings = next;
  return settings;
}

export async function getAboutInfo(): Promise<AboutInfo> {
  return {
    appVersion: "0.1.0",
    cloudflaredVersion: "2026.8.2",
    platform: "windows x86_64",
  };
}

export async function noop() {}

export async function resolveCloseRequest(action: CloseAction) {
  void action;
}

export async function onTunnelSnapshot(
  handler: (next: TunnelSnapshot) => void,
): Promise<UnlistenFn> {
  snapshotListeners.add(handler);
  return () => snapshotListeners.delete(handler);
}

export async function onCloseRequested(
  handler: () => void,
): Promise<UnlistenFn> {
  if (new URLSearchParams(window.location.search).get("close") === "1") {
    window.setTimeout(handler, 50);
  }
  return () => undefined;
}
