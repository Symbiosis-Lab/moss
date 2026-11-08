/**
 * Console + uncaught-error bridge.
 *
 * Forwards `console.error`, `console.warn`, `window.error`, and
 * `unhandledrejection` into `@tauri-apps/plugin-log` so the lines land
 * in `moss.log` for later inclusion in a Send Logs bundle.
 *
 * Local-first posture: nothing leaves the user's machine. The user
 * still controls upload via the Send Logs menu item.
 *
 * Privacy posture:
 *  - PII denylist (`PII_KEYS`) on object args, applied via
 *    `JSON.stringify(value, redactReplacer)` so nested keys are
 *    redacted too.
 *  - Stripe-shape regexes on string args via `scrubString`. Avoid bare
 *    `\d{4}` patterns — they over-redact ports and line numbers.
 *  - Each formatted arg is capped at MAX_ARG_LEN.
 *  - Synchronous reentrancy guard via `inBridge` so a throw inside
 *    formatArg cannot retrigger console.error in a loop.
 *  - Async IPC failures are swallowed. Original console.error has
 *    already run; the bridge is best-effort observability.
 */

import { error as logError, warn as logWarn, debug as logDebug } from '../log';

/**
 * Tauri's own IPC bridge (the tauri crate compiles it into every webview;
 * it is not code moss authors) calls `console.warn` with this
 * text whenever a backend response arrives for a frontend call whose
 * callback the page already tore down (typically a webview reload while
 * Rust was still running). moss cannot change that call site's log level;
 * this bridge is the one place moss controls before the line reaches
 * `moss.log`, where it would repeat 200+ times per session. It is demoted to
 * DEBUG rather than silenced, since it is still useful evidence for the call
 * sites (`deploySite`, `emailSendDraft`) that do not yet have the receipt and
 * queue fix `push_site` has for this failure mode.
 */
const TAURI_CALLBACK_ID_WARNING = "Couldn't find callback id";

const MAX_ARG_LEN = 2000;

/**
 * Object keys whose values are unconditionally redacted to `<redacted>`
 * during JSON.stringify of an object arg. Key-name-based — only fires
 * for object args, not for primitive strings (those go through
 * scrubString instead).
 *
 * **This is an opinionated Stripe-shape allowlist, NOT a general PII
 * filter.** It catches the keys we know commonly carry sensitive values
 * in this codebase. New SDK integrations may introduce keys we don't
 * recognize; prefer that callers redact before logging, not that this
 * list grow unboundedly. Note `'name'` is included for billing-name
 * coverage but will also redact `Error.name` (acceptable trade-off:
 * Error.name is a JS class identifier, not PII).
 */
const PII_KEYS = new Set([
  'clientSecret', 'client_secret',
  'email', 'phone', 'name',
  'card', 'number', 'cvc', 'cvv', 'postal_code', 'postalCode',
  'password', 'token', 'secret', 'authorization', 'cookie',
  'apiKey', 'api_key', 'privateKey', 'private_key', 'bearer', 'pin',
]);

let installed = false;
let inBridge = false; // synchronous reentrancy guard

export function installLogBridge(windowLabel: string): void {
  if (installed) return;
  installed = true;

  const origError = console.error.bind(console);
  const origWarn = console.warn.bind(console);

  console.error = (...args: unknown[]) => {
    origError(...args);
    safeForward('error', windowLabel, args);
  };
  console.warn = (...args: unknown[]) => {
    origWarn(...args);
    safeForward('warn', windowLabel, args);
  };

  if (typeof window !== 'undefined') {
    window.addEventListener('error', (e: ErrorEvent) => {
      const stack = e.error?.stack ? `\n${scrubString(String(e.error.stack)).slice(0, MAX_ARG_LEN)}` : '';
      const line = `[uncaught:${windowLabel}] ${scrubString(e.message)} @ ${e.filename}:${e.lineno}:${e.colno}${stack}`;
      safeForward('error-raw', windowLabel, [line]);
    });

    window.addEventListener('unhandledrejection', (e: PromiseRejectionEvent) => {
      const line = `[unhandled:${windowLabel}] ${formatArg(e.reason)}`;
      safeForward('error-raw', windowLabel, [line]);
    });
  }
}

/**
 * Test-only escape hatch — restores the module to "uninstalled" so a
 * fresh `installLogBridge` call works in the next test. Production
 * code MUST NOT call this.
 */
export function resetForTests(): void {
  installed = false;
  inBridge = false;
}

type ForwardKind = 'error' | 'warn' | 'error-raw';

function safeForward(kind: ForwardKind, label: string, args: unknown[]): void {
  if (inBridge) return;
  inBridge = true;
  try {
    let line: string;
    if (kind === 'error-raw') {
      // Pre-formatted line from window.error / unhandledrejection — use as-is.
      line = String(args[0] ?? '');
    } else {
      line = `[console:${label}] ${formatArgs(args)}`;
    }
    const fn =
      kind === 'warn' && line.includes(TAURI_CALLBACK_ID_WARNING) ? logDebug
      : kind === 'warn' ? logWarn
      : logError;
    try {
      const ret = fn(line);
      if (ret && typeof (ret as Promise<void>).catch === 'function') {
        (ret as Promise<void>).catch(() => { /* IPC down */ });
      }
    } catch {
      // sync throw from fn — swallowed
    }
  } catch {
    // Swallow synchronous errors during formatting (e.g., a getter that throws).
  } finally {
    inBridge = false;
  }
}

function formatArgs(args: unknown[]): string {
  return args.map(formatArg).join(' ');
}

function formatArg(a: unknown): string {
  try {
    if (a instanceof Error) {
      const msg = scrubString(a.message);
      const stack = a.stack ? `\n${scrubString(a.stack).slice(0, MAX_ARG_LEN)}` : '';
      return `${a.name}: ${msg}${stack}`;
    }
    if (typeof a === 'object' && a !== null) {
      return JSON.stringify(a, redactReplacer).slice(0, MAX_ARG_LEN);
    }
    return scrubString(String(a)).slice(0, MAX_ARG_LEN);
  } catch {
    return Object.prototype.toString.call(a);
  }
}

function redactReplacer(key: string, value: unknown): unknown {
  if (PII_KEYS.has(key)) return '<redacted>';
  if (typeof value === 'string') return scrubString(value);
  return value;
}

/**
 * Stripe-shape redactions. Avoid bare `\d{4}` — over-redacts ports,
 * line numbers, timestamps. Use explicit Stripe-message patterns
 * instead.
 */
export function scrubString(s: string): string {
  return s
    // Bounded (RFC 5321 lengths) so a long run with no "@" scrubs in linear time.
    .replace(/[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9.-]{1,253}\.[A-Za-z]{2,}/g, '<email>')
    .replace(/\b\d{13,19}\b/g, '<card>')
    .replace(/\b(sk|pk|rk)_(live|test)_[A-Za-z0-9]+/g, '<key>')
    .replace(/\bpi_[A-Za-z0-9]+_secret_[A-Za-z0-9]+/g, '<pi_secret>')
    .replace(/\bending in \d{4}\b/gi, 'ending in <last4>')
    .replace(/\b\*{4}\s*\d{4}\b/g, '<last4>');
}
