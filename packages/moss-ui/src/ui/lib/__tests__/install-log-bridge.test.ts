/**
 * Tests for the console + uncaught-error bridge that forwards
 * console.error / console.warn / window.error / unhandledrejection
 * into @tauri-apps/plugin-log so they land in moss.log.
 *
 * Critical: redaction goldens. The bridge is the load-bearing privacy
 * piece for Send Logs — if these tests pass, PII never reaches moss.log
 * via the bridge path.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';

const mockLogError = vi.fn((..._args: unknown[]): Promise<void> => Promise.resolve());
const mockLogWarn = vi.fn((..._args: unknown[]): Promise<void> => Promise.resolve());
const mockLogDebug = vi.fn((..._args: unknown[]): Promise<void> => Promise.resolve());

function getFirstCallArg(mock: Mock): string {
  const calls = mock.mock.calls as unknown[][];
  return String(calls[0]?.[0] ?? '');
}

vi.mock('../../log', () => ({
  uiEvent: vi.fn(),
  error: (...args: unknown[]) => mockLogError(...args),
  warn: (...args: unknown[]) => mockLogWarn(...args),
  debug: (...args: unknown[]) => mockLogDebug(...args),
}));

// Import AFTER the mock is registered.
let installLogBridge: typeof import('../install-log-bridge').installLogBridge;
let resetForTests: typeof import('../install-log-bridge').resetForTests;

beforeEach(async () => {
  vi.clearAllMocks();
  // Re-import to get fresh module state. Vitest caches by default.
  vi.resetModules();
  const mod = await import('../install-log-bridge');
  installLogBridge = mod.installLogBridge;
  resetForTests = mod.resetForTests;
  resetForTests();
});

afterEach(() => {
  resetForTests();
});

describe('installLogBridge', () => {
  it('is idempotent — second install is a no-op', () => {
    installLogBridge('app-shell');
    const ref1 = console.error;
    installLogBridge('app-shell');
    expect(console.error).toBe(ref1);
  });

  it('forwards console.error with windowLabel tag', () => {
    installLogBridge('app-shell');
    console.error('test message');
    expect(mockLogError).toHaveBeenCalledWith('[console:app-shell] test message');
  });

  it('forwards console.warn with windowLabel tag', () => {
    installLogBridge('action-panel');
    console.warn('soft fail');
    expect(mockLogWarn).toHaveBeenCalledWith('[console:action-panel] soft fail');
  });

  // Tauri's own
  // IPC bridge warns this text when a reload orphans an in-flight invoke's
  // callback — moss doesn't own that call site, only where it lands.
  it('demotes the Tauri callback-id warning to debug instead of warn', () => {
    installLogBridge('preview');
    console.warn(
      "[TAURI] Couldn't find callback id 12345. This might happen when the app is reloaded while Rust is running an asynchronous operation.",
    );
    expect(mockLogWarn).not.toHaveBeenCalled();
    expect(mockLogDebug).toHaveBeenCalledWith(
      expect.stringContaining("Couldn't find callback id"),
    );
  });

  it('an ordinary warn containing unrelated text is not demoted', () => {
    installLogBridge('preview');
    console.warn('a real warning about something else');
    expect(mockLogDebug).not.toHaveBeenCalled();
    expect(mockLogWarn).toHaveBeenCalled();
  });

  it('does NOT forward console.log / console.info / console.debug', () => {
    installLogBridge('app-shell');
    console.log('chatty');
    console.info('info');
    console.debug('debug');
    expect(mockLogError).not.toHaveBeenCalled();
    expect(mockLogWarn).not.toHaveBeenCalled();
  });

  it('still calls the original console.error after forwarding', () => {
    const origError = console.error;
    const spy = vi.fn();
    console.error = spy;
    // Re-prime the bridge with our spy as the "original"
    installLogBridge('app-shell');
    console.error('hi');
    expect(spy).toHaveBeenCalledWith('hi');
    console.error = origError;
  });

  it('forwards window error events with file/line context', () => {
    installLogBridge('app-shell');
    const ev = new ErrorEvent('error', {
      message: 'boom',
      filename: 'foo.ts',
      lineno: 42,
      colno: 7,
      error: new Error('boom'),
    });
    window.dispatchEvent(ev);
    expect(mockLogError).toHaveBeenCalledWith(
      expect.stringMatching(/^\[uncaught:app-shell\] .*boom.*foo\.ts:42:7/)
    );
  });

  it('forwards unhandled promise rejections', () => {
    installLogBridge('app-shell');
    const ev = new Event('unhandledrejection') as PromiseRejectionEvent;
    Object.defineProperty(ev, 'reason', { value: new Error('async fail') });
    window.dispatchEvent(ev);
    expect(mockLogError).toHaveBeenCalledWith(
      expect.stringContaining('[unhandled:app-shell]')
    );
  });

  it('does not throw or recurse if logError itself throws', () => {
    mockLogError.mockImplementationOnce(() => {
      throw new Error('IPC down');
    });
    installLogBridge('app-shell');
    expect(() => console.error('triggers throw')).not.toThrow();
  });

  it('attaches .catch to the IPC promise so rejections do not become unhandled', async () => {
    // The bridge's contract: a rejecting `logError`/`logWarn` must not
    // produce an unhandled-rejection notification. We attach `.catch`
    // explicitly. Verified by adding an `unhandledrejection` listener
    // that should NOT see the failure.
    installLogBridge('app-shell');
    mockLogError.mockClear();
    mockLogError.mockImplementationOnce(() => Promise.reject(new Error('async fail')));

    let unhandled = false;
    const listener = () => { unhandled = true; };
    window.addEventListener('unhandledrejection', listener);

    console.error('hi');
    await new Promise((r) => setTimeout(r, 20));
    window.removeEventListener('unhandledrejection', listener);

    expect(unhandled).toBe(false);
  });
});

describe('PII redaction goldens (load-bearing privacy tests)', () => {
  beforeEach(() => {
    installLogBridge('app-shell');
  });

  it('redacts emails from string args', () => {
    console.error('user is leak@example.com');
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<email>');
    expect(line).not.toContain('leak@example.com');
  });

  it('redacts 13-19 digit card numbers from string args', () => {
    console.error('card 4242424242424242 declined');
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<card>');
    expect(line).not.toContain('4242424242424242');
  });

  it('redacts Stripe sk_live / pk_live / rk_test keys', () => {
    console.error('used key sk_live_ABCDEFGHIJ12345'); // gitleaks:allow (a made-up key the redaction must catch)
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<key>');
    expect(line).not.toContain('sk_live_ABCDEFGHIJ12345'); // gitleaks:allow
  });

  it('redacts pi_*_secret_* PaymentIntent client secrets', () => {
    console.error('client secret pi_3RABCDEF_secret_xyz123');
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<pi_secret>');
    expect(line).not.toContain('pi_3RABCDEF_secret_xyz123');
  });

  it('redacts "ending in NNNN" Stripe last-4 patterns', () => {
    console.error('Card ending in 4242 declined');
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('ending in <last4>');
    expect(line).not.toContain('ending in 4242');
  });

  it('does NOT over-redact bare 4-digit numbers (ports, line numbers)', () => {
    console.error('Listening on port 3000, error at line 1234');
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('3000');
    expect(line).toContain('1234');
  });

  it('redacts denylisted keys in object args (nested too)', () => {
    console.error({ outer: { clientSecret: 'pi_3X_secret_y', other: 'ok' } });
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<redacted>');
    expect(line).not.toContain('pi_3X_secret_y');
    expect(line).toContain('"other":"ok"');
  });

  it('caps object args at MAX_ARG_LEN', () => {
    const huge = { x: 'y'.repeat(5000) };
    console.error(huge);
    const line = getFirstCallArg(mockLogError);
    // Bridge tag + 2000 + a bit of overhead
    expect(line.length).toBeLessThan(2200);
  });

  it('formats Error objects with name + message + stack', () => {
    const err = new Error('the failure');
    console.error(err);
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('Error: the failure');
  });

  it('scrubs the message field of Error objects', () => {
    const err = new Error('Card 4242424242424242 declined');
    console.error(err);
    const line = getFirstCallArg(mockLogError);
    expect(line).toContain('<card>');
    expect(line).not.toContain('4242424242424242');
  });

  it('Error.name survives PII_KEYS=name redaction (formatArg bypasses replacer for Errors)', () => {
    // PII_KEYS includes 'name' to redact billing-name fields. But we
    // construct Error formatting from a.name directly in formatArg —
    // bypassing JSON.stringify/redactReplacer — so error class names like
    // "TypeError" remain visible. If a future refactor routes Error
    // through the replacer, this test will fail (loud, intentional).
    const err = new TypeError('boom');
    console.error(err);
    expect(getFirstCallArg(mockLogError)).toMatch(/TypeError: boom/);
  });
});

describe('scrubString cost', () => {
  it('scrubs a 64 KiB unbroken token in well under a second', async () => {
    // A base64 blob or data URI is one long run of email-local characters
    // with no "@"; an unbounded local-part pattern retries from every offset.
    const { scrubString } = await import('../install-log-bridge');
    const token = 'x'.repeat(64 * 1024);
    const start = performance.now();
    expect(scrubString(token)).toBe(token);
    expect(performance.now() - start).toBeLessThan(1000);
    expect(scrubString('mail jane.doe+x@example.co.uk now')).toBe('mail <email> now');
  });
});
