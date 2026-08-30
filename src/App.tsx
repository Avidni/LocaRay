import {
  Check,
  Circle,
  CircleAlert,
  Copy,
  ExternalLink,
  FileText,
  Globe2,
  Laptop,
  Radio,
  RadioTower,
  RefreshCw,
  Settings2,
  Square,
  Timer,
  X,
} from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import {
  FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import type { AboutInfo } from "./contracts/generated/AboutInfo";
import type { DetectedService } from "./contracts/generated/DetectedService";
import type { Settings } from "./contracts/generated/Settings";
import type { TunnelSnapshot } from "./contracts/generated/TunnelSnapshot";
import { QR_PALETTE } from "./design-tokens";
import {
  copyDiagnostics,
  copyPublicUrl,
  discoverServices,
  getAboutInfo,
  getDiagnostics,
  getSettings,
  getTunnelSnapshot,
  onCloseRequested,
  onTunnelSnapshot,
  openInformation,
  openPublicUrl,
  resetTunnel,
  resolveCloseRequest,
  startTunnel,
  stopTunnel,
  updateSettings,
  validatePort,
} from "./ipc";
import type { CloseAction } from "./ipc";

const defaultSettings: Settings = {
  closeBehavior: "ask",
  defaultStopAfterMinutes: null,
  launchAtLogin: false,
  diagnosticLogging: true,
  lastSuccessfulPort: null,
};

const phaseLabels: Record<TunnelSnapshot["phase"], string> = {
  idle: "Public access off",
  checking_origin: "Checking local app",
  starting: "Connecting to Cloudflare",
  verifying_public_url: "Verifying public address",
  connected: "Publicly accessible",
  reconnecting: "Reconnecting",
  stopping: "Stopping public access",
  error: "Tunnel error",
  exited: "Tunnel stopped",
};

export function App() {
  const [snapshot, setSnapshot] = useState<TunnelSnapshot | null>(null);
  const [services, setServices] = useState<DetectedService[]>([]);
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [port, setPort] = useState("");
  const [portError, setPortError] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [closeRequested, setCloseRequested] = useState(false);
  const savedPortRef = useRef<number | null>(null);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);

  const refreshServices = useCallback(async () => {
    setRefreshing(true);
    try {
      const detected = await discoverServices();
      setServices(detected);
      setPort(
        (current) =>
          current ||
          String(settings.lastSuccessfulPort ?? detected[0]?.port ?? ""),
      );
    } catch {
      setServices([]);
    } finally {
      setRefreshing(false);
    }
  }, [settings.lastSuccessfulPort]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void Promise.all([getTunnelSnapshot(), getSettings()])
      .then(([nextSnapshot, nextSettings]) => {
        setSnapshot(nextSnapshot);
        setSettings(nextSettings);
        if (nextSnapshot.port) setPort(String(nextSnapshot.port));
      })
      .catch(() => setCommandError("The desktop backend did not respond."));
    void onTunnelSnapshot(setSnapshot).then((stopListening) => {
      unlisten = stopListening;
    });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onCloseRequested(() => setCloseRequested(true)).then(
      (stopListening) => {
        unlisten = stopListening;
      },
    );
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    void refreshServices();
  }, [refreshServices]);

  useEffect(() => {
    if (snapshot?.phase !== "connected" || !snapshot.port) return;
    if (
      snapshot.port === settings.lastSuccessfulPort ||
      snapshot.port === savedPortRef.current
    ) {
      return;
    }
    savedPortRef.current = snapshot.port;
    const next = { ...settings, lastSuccessfulPort: snapshot.port };
    void updateSettings(next)
      .then(setSettings)
      .catch(() => {
        savedPortRef.current = null;
      });
  }, [settings, snapshot]);

  const parsedPort = Number(port);
  const canStart =
    Number.isInteger(parsedPort) && parsedPort >= 1 && parsedPort <= 65_535;
  const isBusy = snapshot
    ? [
        "checking_origin",
        "starting",
        "verifying_public_url",
        "stopping",
      ].includes(snapshot.phase)
    : true;

  async function checkPort() {
    if (!port) {
      setPortError("Enter a local port.");
      return false;
    }
    try {
      await validatePort(parsedPort);
      setPortError(null);
      return true;
    } catch {
      setPortError("Enter a port from 1 through 65535.");
      return false;
    }
  }

  async function handleStart(event: FormEvent) {
    event.preventDefault();
    setCommandError(null);
    if (!(await checkPort())) return;
    try {
      setSnapshot(
        await startTunnel({
          port: parsedPort,
          stopAfterMinutes: settings.defaultStopAfterMinutes,
        }),
      );
    } catch (error) {
      setCommandError(readCommandMessage(error));
    }
  }

  async function handleStop() {
    setCommandError(null);
    try {
      setSnapshot(await stopTunnel());
    } catch (error) {
      setCommandError(readCommandMessage(error));
    }
  }

  async function handleChangePort() {
    setCommandError(null);
    try {
      setSnapshot(await resetTunnel());
      setPort("");
    } catch (error) {
      setCommandError(readCommandMessage(error));
    }
  }

  const status = snapshot
    ? phaseLabels[snapshot.phase]
    : "Reading tunnel state";

  function closeSettings() {
    setSettingsOpen(false);
    window.requestAnimationFrame(() => settingsButtonRef.current?.focus());
  }

  return (
    <div className="app-shell min-h-screen">
      <header className="product-bar">
        <span className="product-name">LocaRay</span>
        <div className="product-actions">
          <div className="status-pill" aria-label={`Tunnel status: ${status}`}>
            <Circle aria-hidden="true" size={16} strokeWidth={1.5} />
            <span>{status}</span>
          </div>
          <button
            ref={settingsButtonRef}
            className="icon-button"
            type="button"
            aria-label="Open settings"
            title="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings2 aria-hidden="true" size={18} strokeWidth={1.5} />
          </button>
        </div>
      </header>

      <main className="workspace" aria-busy={!snapshot || isBusy}>
        {!snapshot ? (
          <LoadingState />
        ) : snapshot.phase === "connected" ||
          snapshot.phase === "reconnecting" ? (
          <ConnectedState
            snapshot={snapshot}
            onStop={() => void handleStop()}
            onCommandError={setCommandError}
          />
        ) : snapshot.phase === "error" || snapshot.phase === "exited" ? (
          <ErrorState
            snapshot={snapshot}
            port={port}
            onRetry={(event) => void handleStart(event)}
            onResetPort={() => void handleChangePort()}
          />
        ) : snapshot.phase === "idle" ? (
          <IdleState
            port={port}
            portError={portError}
            services={services}
            refreshing={refreshing}
            canStart={canStart}
            onPortChange={setPort}
            onPortBlur={() => void checkPort()}
            onRefresh={() => void refreshServices()}
            onSubmit={(event) => void handleStart(event)}
          />
        ) : (
          <ProgressState
            snapshot={snapshot}
            onCancel={() => void handleStop()}
          />
        )}

        {commandError ? (
          <p className="command-error" role="alert">
            <CircleAlert aria-hidden="true" size={18} strokeWidth={1.5} />
            {commandError}
          </p>
        ) : null}
      </main>

      {settingsOpen ? (
        <SettingsDialog
          settings={settings}
          onClose={closeSettings}
          onSave={async (next) => {
            const saved = await updateSettings(next);
            setSettings(saved);
          }}
        />
      ) : null}

      {closeRequested ? (
        <CloseConfirmation
          onChoose={async (action) => {
            await resolveCloseRequest(action);
            setCloseRequested(false);
          }}
        />
      ) : null}

      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {status}
      </div>
    </div>
  );
}

function IdleState(props: {
  port: string;
  portError: string | null;
  services: DetectedService[];
  refreshing: boolean;
  canStart: boolean;
  onPortChange: (value: string) => void;
  onPortBlur: () => void;
  onRefresh: () => void;
  onSubmit: (event: FormEvent) => void;
}) {
  return (
    <>
      <StateHeading title="Share your local app">
        Create a temporary public address for a local development server.
      </StateHeading>
      <section className="state-shell" aria-labelledby="idle-title">
        <form
          className="state-core idle-form"
          onSubmit={props.onSubmit}
          noValidate
        >
          <div className="field-group">
            <label htmlFor="local-port">Local port</label>
            <div className="port-row">
              <input
                id="local-port"
                name="port"
                type="number"
                min="1"
                max="65535"
                inputMode="numeric"
                list="detected-ports"
                value={props.port}
                aria-invalid={Boolean(props.portError)}
                aria-describedby={props.portError ? "port-error" : "port-help"}
                onChange={(event) => props.onPortChange(event.target.value)}
                onBlur={props.onPortBlur}
              />
              <datalist id="detected-ports">
                {props.services.map((service) => (
                  <option key={service.port} value={service.port}>
                    {service.hint ?? "Detected local service"}
                  </option>
                ))}
              </datalist>
              <button
                className="icon-button field-action"
                type="button"
                aria-label="Refresh detected services"
                title="Refresh services"
                disabled={props.refreshing}
                onClick={props.onRefresh}
              >
                <RefreshCw aria-hidden="true" size={18} strokeWidth={1.5} />
              </button>
            </div>
            {props.portError ? (
              <p className="field-error" id="port-error" role="alert">
                <CircleAlert aria-hidden="true" size={16} strokeWidth={1.5} />
                {props.portError}
              </p>
            ) : (
              <p className="field-help" id="port-help">
                {props.services.length
                  ? `${props.services.length} local service${props.services.length === 1 ? "" : "s"} detected.`
                  : "Start a server or enter its port manually."}
              </p>
            )}
          </div>
          <div className="exposure-callout">
            <Globe2 aria-hidden="true" size={20} strokeWidth={1.5} />
            <p>
              Anyone with the generated URL may access this local app. Quick
              Tunnels do not add authentication.
            </p>
          </div>
          <button
            className="primary-button"
            type="submit"
            disabled={!props.canStart}
          >
            <RadioTower aria-hidden="true" size={20} strokeWidth={1.5} />
            Start tunnel
          </button>
        </form>
      </section>
      <QuickTunnelNote />
    </>
  );
}

function ProgressState({
  snapshot,
  onCancel,
}: {
  snapshot: TunnelSnapshot;
  onCancel: () => void;
}) {
  const text = phaseLabels[snapshot.phase];
  const stopping = snapshot.phase === "stopping";
  return (
    <>
      <StateHeading
        title={stopping ? "Stopping public access" : "Starting public access"}
      >
        {stopping
          ? "LocaRay is confirming that the tunnel process has ended."
          : "LocaRay is preparing a temporary Quick Tunnel."}
      </StateHeading>
      <section className="state-shell" aria-labelledby="progress-title">
        <div className="state-core state-content">
          <h2 id="progress-title">{text}</h2>
          {snapshot.publicUrl || snapshot.localUrl ? (
            <p className="technical-value">
              {snapshot.publicUrl ?? snapshot.localUrl}
            </p>
          ) : null}
          <div className="progress-track" aria-hidden="true">
            <span className="progress-segment" />
          </div>
          {!stopping ? (
            <button className="ghost-button" type="button" onClick={onCancel}>
              Cancel
            </button>
          ) : null}
        </div>
      </section>
    </>
  );
}

function ConnectedState(props: {
  snapshot: TunnelSnapshot;
  onStop: () => void;
  onCommandError: (message: string | null) => void;
}) {
  const [copied, setCopied] = useState(false);
  const publicUrl = props.snapshot.publicUrl;
  const reconnecting = props.snapshot.phase === "reconnecting";
  if (!publicUrl) return null;

  async function copy() {
    try {
      await copyPublicUrl();
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (error) {
      props.onCommandError(readCommandMessage(error));
    }
  }

  return (
    <>
      <StateHeading
        title={reconnecting ? "Connection interrupted" : "Your app is live"}
      >
        <span className="live-label">
          <Radio aria-hidden="true" size={18} strokeWidth={1.5} />
          {reconnecting ? "Trying to reconnect" : "Publicly accessible"}
        </span>
      </StateHeading>
      <section
        className="state-shell connected-shell"
        aria-labelledby="public-url-title"
      >
        <div className="state-core connected-grid">
          <div className="connection-details">
            <h2 id="public-url-title">Public URL</h2>
            <p className="url-value">{publicUrl}</p>
            <div className="button-row">
              <button
                className="secondary-button"
                type="button"
                onClick={() => void copy()}
              >
                {copied ? (
                  <Check aria-hidden="true" size={18} />
                ) : (
                  <Copy aria-hidden="true" size={18} />
                )}
                {copied ? "URL copied" : "Copy URL"}
              </button>
              <button
                className="secondary-button"
                type="button"
                disabled={reconnecting}
                onClick={() =>
                  void openPublicUrl().catch((error) =>
                    props.onCommandError(readCommandMessage(error)),
                  )
                }
              >
                <ExternalLink aria-hidden="true" size={18} />
                Open
              </button>
            </div>
            <span className="sr-only" aria-live="polite">
              {copied ? "Public URL copied." : ""}
            </span>
            <dl className="session-facts">
              <div>
                <dt>
                  <Laptop aria-hidden="true" size={16} /> Local destination
                </dt>
                <dd>{props.snapshot.localUrl}</dd>
              </div>
              <div>
                <dt>
                  <Timer aria-hidden="true" size={16} /> Elapsed
                </dt>
                <dd>
                  <Elapsed startedAt={props.snapshot.startedAt} />
                </dd>
              </div>
              {props.snapshot.stopAt ? (
                <div>
                  <dt>
                    <Timer aria-hidden="true" size={16} /> Automatic stop
                  </dt>
                  <dd>
                    {new Date(props.snapshot.stopAt).toLocaleTimeString([], {
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </dd>
                </div>
              ) : null}
            </dl>
            <button
              className="stop-button"
              type="button"
              onClick={props.onStop}
            >
              <Square aria-hidden="true" size={20} fill="currentColor" />
              Stop tunnel
            </button>
          </div>
          <div className="qr-card" aria-label="QR code for the public URL">
            <QRCodeSVG
              value={publicUrl}
              size={208}
              level="M"
              bgColor={QR_PALETTE.background}
              fgColor={QR_PALETTE.foreground}
              marginSize={4}
            />
          </div>
        </div>
      </section>
      <DiagnosticsDisclosure />
    </>
  );
}

function ErrorState(props: {
  snapshot: TunnelSnapshot;
  port: string;
  onRetry: (event: FormEvent) => void;
  onResetPort: () => void;
}) {
  const error = props.snapshot.error;
  const headingRef = useRef<HTMLHeadingElement>(null);
  useEffect(() => headingRef.current?.focus(), []);
  return (
    <>
      <StateHeading
        title={
          props.snapshot.phase === "exited"
            ? "The tunnel stopped"
            : "The tunnel could not start"
        }
      >
        Public access is {error?.exposureActive ? "still active" : "off"}.
      </StateHeading>
      <section className="state-shell" aria-labelledby="error-title">
        <div className="state-core error-panel" role="alert">
          <CircleAlert aria-hidden="true" size={24} strokeWidth={1.5} />
          <div>
            <h2 id="error-title" ref={headingRef} tabIndex={-1}>
              {error?.cause ?? "The tunnel process ended."}
            </h2>
            <p>
              {error?.recovery ??
                "Retry when the local app and network are ready."}
            </p>
          </div>
          <form className="button-row error-actions" onSubmit={props.onRetry}>
            <button
              className="primary-button"
              type="submit"
              disabled={!props.port}
            >
              <RefreshCw aria-hidden="true" size={18} /> Retry
            </button>
            <button
              className="ghost-button"
              type="button"
              onClick={props.onResetPort}
            >
              Change port
            </button>
          </form>
        </div>
      </section>
      <DiagnosticsDisclosure />
    </>
  );
}

function DiagnosticsDisclosure() {
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  async function toggle() {
    const next = !open;
    setOpen(next);
    if (next) setLines(await getDiagnostics());
  }
  return (
    <div className="disclosure">
      <button
        className="disclosure-button"
        type="button"
        aria-expanded={open}
        onClick={() => void toggle()}
      >
        <FileText aria-hidden="true" size={18} /> Diagnostics
      </button>
      {open ? (
        <div className="diagnostics-panel">
          <pre>
            {lines.length
              ? lines.join("\n")
              : "No diagnostic output for this session."}
          </pre>
          <button
            className="ghost-button"
            type="button"
            onClick={() => void copyDiagnostics()}
          >
            <Copy aria-hidden="true" size={16} /> Copy diagnostics
          </button>
        </div>
      ) : null}
    </div>
  );
}

function SettingsDialog(props: {
  settings: Settings;
  onClose: () => void;
  onSave: (settings: Settings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(props.settings);
  const [about, setAbout] = useState<AboutInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    void getAboutInfo().then(setAbout);
  }, []);
  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={dialogRef}
      className="settings-dialog"
      aria-labelledby="settings-title"
      onCancel={(event) => {
        event.preventDefault();
        props.onClose();
      }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
      onKeyDown={trapDialogFocus}
    >
      <section>
        <header className="dialog-header">
          <h2 id="settings-title">Settings</h2>
          <button
            className="icon-button"
            type="button"
            aria-label="Close settings"
            onClick={props.onClose}
          >
            <X aria-hidden="true" size={20} />
          </button>
        </header>
        <div className="settings-rows">
          <label>
            <span>When closing an active tunnel</span>
            <select
              value={draft.closeBehavior}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  closeBehavior: event.target
                    .value as Settings["closeBehavior"],
                })
              }
            >
              <option value="ask">Ask every time</option>
              <option value="keep_running_in_tray">Keep running in tray</option>
              <option value="stop_and_quit">Stop and quit</option>
            </select>
          </label>
          <label>
            <span>Default automatic stop</span>
            <select
              value={draft.defaultStopAfterMinutes ?? ""}
              onChange={(event) =>
                setDraft({
                  ...draft,
                  defaultStopAfterMinutes: event.target.value
                    ? Number(event.target.value)
                    : null,
                })
              }
            >
              <option value="">No timer</option>
              <option value="30">30 minutes</option>
              <option value="60">60 minutes</option>
              <option value="120">120 minutes</option>
            </select>
          </label>
          <label className="switch-row">
            <span>Launch at login</span>
            <input
              type="checkbox"
              checked={draft.launchAtLogin}
              onChange={(event) =>
                setDraft({ ...draft, launchAtLogin: event.target.checked })
              }
            />
          </label>
          <label className="switch-row">
            <span>Keep session diagnostics</span>
            <input
              type="checkbox"
              checked={draft.diagnosticLogging}
              onChange={(event) =>
                setDraft({ ...draft, diagnosticLogging: event.target.checked })
              }
            />
          </label>
        </div>
        {about ? (
          <dl className="about-list">
            <div>
              <dt>LocaRay</dt>
              <dd>{about.appVersion}</dd>
            </div>
            <div>
              <dt>cloudflared</dt>
              <dd>{about.cloudflaredVersion}</dd>
            </div>
            <div>
              <dt>Platform</dt>
              <dd>{about.platform}</dd>
            </div>
            <div>
              <dt>Update channel</dt>
              <dd>Not configured for this build</dd>
            </div>
          </dl>
        ) : null}
        <p className="privacy-summary">
          Tunnel URLs, local ports, and diagnostics stay on this device unless
          you copy them.
        </p>
        <div className="information-links" aria-label="Product information">
          <button
            type="button"
            onClick={() => void openInformation("quick_tunnel_documentation")}
          >
            Quick Tunnel documentation
          </button>
          <button
            type="button"
            onClick={() => void openInformation("cloudflared_license")}
          >
            cloudflared license
          </button>
        </div>
        {error ? (
          <p className="field-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="dialog-actions">
          <button
            className="ghost-button"
            type="button"
            onClick={props.onClose}
          >
            Cancel
          </button>
          <button
            className="primary-button"
            type="button"
            onClick={() =>
              void props
                .onSave(draft)
                .then(props.onClose)
                .catch((reason) => setError(readCommandMessage(reason)))
            }
          >
            Save settings
          </button>
        </div>
      </section>
    </dialog>
  );
}

function CloseConfirmation(props: {
  onChoose: (action: CloseAction) => Promise<void>;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    return () => dialog?.close();
  }, []);

  async function choose(action: CloseAction) {
    setBusy(true);
    setError(null);
    try {
      await props.onChoose(action);
    } catch (reason) {
      setError(readCommandMessage(reason));
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialogRef}
      className="settings-dialog close-confirmation"
      aria-labelledby="close-title"
      aria-describedby="close-description"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) void choose("cancel");
      }}
      onKeyDown={trapDialogFocus}
    >
      <h2 id="close-title">A public tunnel is still active</h2>
      <p id="close-description">
        Stop public access before quitting, or keep LocaRay visible in the
        Windows tray.
      </p>
      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
      <div className="close-actions">
        <button
          className="primary-button"
          type="button"
          autoFocus
          disabled={busy}
          onClick={() => void choose("stop_and_quit")}
        >
          Stop and quit
        </button>
        <button
          className="secondary-button"
          type="button"
          disabled={busy}
          onClick={() => void choose("keep_running_in_tray")}
        >
          Keep running in tray
        </button>
        <button
          className="ghost-button"
          type="button"
          disabled={busy}
          onClick={() => void choose("cancel")}
        >
          Cancel
        </button>
      </div>
    </dialog>
  );
}

function LoadingState() {
  return (
    <>
      <StateHeading title="Share your local app">
        Reading the current tunnel state.
      </StateHeading>
      <section className="state-shell">
        <div className="state-core state-content">
          <h2>Reading tunnel state</h2>
          <div className="progress-track">
            <span className="progress-segment" />
          </div>
        </div>
      </section>
    </>
  );
}

function StateHeading({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="state-heading">
      <h1>{title}</h1>
      <div className="heading-support">{children}</div>
    </div>
  );
}

function QuickTunnelNote() {
  return (
    <p className="exposure-note">
      Quick Tunnels are for development and testing. They have no uptime
      guarantee, the hostname changes after restart, and Server-Sent Events are
      not supported.
    </p>
  );
}

function Elapsed({ startedAt }: { startedAt: string | null }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const seconds = useMemo(
    () =>
      startedAt
        ? Math.max(0, Math.floor((now - Date.parse(startedAt)) / 1000))
        : 0,
    [now, startedAt],
  );
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  return (
    <span>
      {hours ? `${hours}:` : ""}
      {String(minutes).padStart(2, "0")}:{String(remainder).padStart(2, "0")}
    </span>
  );
}

function readCommandMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error &&
    "message" in error &&
    typeof error.message === "string"
  )
    return error.message;
  if (typeof error === "string") return error;
  return "The action could not be completed.";
}

function trapDialogFocus(event: React.KeyboardEvent<HTMLDialogElement>) {
  if (event.key !== "Tab") return;
  const focusable = Array.from(
    event.currentTarget.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
    ),
  );
  const first = focusable[0];
  const last = focusable.at(-1);
  if (!first || !last) return;
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}
