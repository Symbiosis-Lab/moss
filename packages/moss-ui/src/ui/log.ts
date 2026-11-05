/**
 * The UI's log. Lines go to the host's log where it keeps one (the desktop
 * writes moss's log file) and to the console otherwise; a failing host log
 * falls back to the console and never reaches the caller.
 */
import { host, type LogLevel } from './host';
import { formatUiEvent } from './shared/ui/toast/toast-runtime';

// Bound at load, before install-log-bridge patches console: a fallback through
// the patched console.error is forwarded back here, and a host log that keeps
// rejecting would then loop through promises past the bridge's sync guard.
const toConsole: Record<LogLevel, (message: string) => void> = {
  info: console.info.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  debug: console.debug.bind(console),
  trace: console.debug.bind(console),
};

function at(level: LogLevel): (message: string) => Promise<void> {
  return (message) => {
    const sink = host.log;
    if (!sink) {
      toConsole[level](message);
      return Promise.resolve();
    }
    return sink(level, message).catch(() => toConsole[level](message));
  };
}

export const info = at('info');
export const warn = at('warn');
export const error = at('error');
export const debug = at('debug');
export const trace = at('trace');

/**
 * One INFO breadcrumb per meaningful user-visible action: a context-menu item
 * picked, a panel view mounted or a switch refused, a named control clicked, a
 * command failure surfaced. `name` is a dotted event id (`menu.pick`); `fields`
 * render as flat `key=value` pairs, greppable in a log bundle. Never a
 * keystroke, a pointer move or a full filesystem path.
 */
export function uiEvent(name: string, fields?: Record<string, string | number | boolean | undefined>): void {
  void info(formatUiEvent(name, fields));
}

// Launch timing marks, read by the desktop app's launch-time measurement.
// Each carries its own wall-clock time, so a slow log round trip does not
// move it.

export type StartupSurface = "launcher" | "main";
export type StartupStage = "waiting" | "ready";

declare global {
  interface Window {
    __mossFirstFrameUnixMs?: number;
    __mossFirstFrameContent?: boolean;
  }
}

const sent = new Set<string>();

/**
 * Logs `startup.mark` once per surface and stage for this page load, on the
 * next frame, so `unix_ms` is when the stage was on screen. A hidden window
 * draws no frame, so its mark waits until the window is shown.
 */
export function markStartup(surface: StartupSurface, stage: StartupStage): void {
  const id = `${surface}/${stage}`;
  if (sent.has(id)) return;
  sent.add(id);
  requestAnimationFrame(() => {
    const now = Date.now();
    const frame = window.__mossFirstFrameUnixMs ?? 0;
    const content = window.__mossFirstFrameContent ? 1 : 0;
    void info(
      `startup.mark surface=${surface} stage=${stage} unix_ms=${now} first_frame_unix_ms=${frame} first_frame_content=${content}`,
    );
  });
}
