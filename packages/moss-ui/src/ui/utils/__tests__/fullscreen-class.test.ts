import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';

// Capture the handlers registered via onMossEvent so tests can fire events, and
// expose a controllable isFullscreen() for the initial-state pull.
const mocks = vi.hoisted(() => {
  const handlers: Record<string, (msg: unknown) => void> = {};
  return {
    handlers,
    onMossEvent: vi.fn(async (kind: string, handler: (msg: unknown) => void) => {
      handlers[kind] = handler;
      return vi.fn(); // UnlistenFn
    }),
    isFullscreen: vi.fn().mockResolvedValue(false),
  };
});

vi.mock('../moss-events', () => ({ onMossEvent: mocks.onMossEvent }));

import { host } from '../../host';
const nativeWindow = {
  isFullscreen: mocks.isFullscreen,
  isMaximized: vi.fn().mockResolvedValue(false),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
};

import {
  installFullscreenBodyClass,
  applyOwnTitlebarClass,
  applyCaptionOwnerClass,
  __resetFullscreenClassForTest,
} from '../fullscreen-class';

const has = () => document.body.classList.contains('fullscreen-mode');
const fireWill = (will: boolean) => mocks.handlers['FullscreenWillChange']({ payload: { will_be_fullscreen: will } });
const fireChanged = (is: boolean) => mocks.handlers['FullscreenChanged']({ payload: { is_fullscreen: is } });

beforeEach(() => {
  host.window = nativeWindow;
  __resetFullscreenClassForTest();
  document.body.className = '';
  mocks.onMossEvent.mockClear();
  mocks.isFullscreen.mockReset().mockResolvedValue(false);
  for (const k of Object.keys(mocks.handlers)) delete mocks.handlers[k];
});

describe('installFullscreenBodyClass', () => {
  test('a host with no native window: class set immediately, no subscriptions', async () => {
    host.window = undefined;
    await installFullscreenBodyClass();
    expect(has()).toBe(true);
    expect(mocks.onMossEvent).not.toHaveBeenCalled();
  });

  test('subscribes to both fullscreen events', async () => {
    await installFullscreenBodyClass();
    const kinds = mocks.onMossEvent.mock.calls.map((c) => c[0]);
    expect(kinds).toContain('FullscreenWillChange');
    expect(kinds).toContain('FullscreenChanged');
  });

  test('FullscreenWillChange toggles the class both ways', async () => {
    await installFullscreenBodyClass();
    fireWill(true);
    expect(has()).toBe(true);
    fireWill(false);
    expect(has()).toBe(false);
  });

  test('FullscreenChanged (backstop) toggles the class both ways', async () => {
    await installFullscreenBodyClass();
    fireChanged(true);
    expect(has()).toBe(true);
    fireChanged(false);
    expect(has()).toBe(false);
  });

  test('initial isFullscreen() pull applies the class when already fullscreen', async () => {
    mocks.isFullscreen.mockResolvedValue(true);
    await installFullscreenBodyClass();
    expect(has()).toBe(true);
  });

  test('initial pull leaves the class off when windowed', async () => {
    mocks.isFullscreen.mockResolvedValue(false);
    await installFullscreenBodyClass();
    expect(has()).toBe(false);
  });

  test('degrades gracefully if isFullscreen() rejects (no capability)', async () => {
    mocks.isFullscreen.mockRejectedValue(new Error('not allowed'));
    await expect(installFullscreenBodyClass()).resolves.toBeUndefined();
    // Events still drive subsequent transitions.
    fireWill(true);
    expect(has()).toBe(true);
  });

  test('is idempotent — a second call does not re-subscribe', async () => {
    await installFullscreenBodyClass();
    await installFullscreenBodyClass();
    expect(mocks.onMossEvent).toHaveBeenCalledTimes(2); // one per event, not four
  });
});

/** jsdom reports a Linux platform, so mac has to be simulated. */
function withPlatform(value: string, run: () => void): void {
  const original = Object.getOwnPropertyDescriptor(navigator, 'platform');
  Object.defineProperty(navigator, 'platform', { value, configurable: true });
  try {
    run();
  } finally {
    if (original) Object.defineProperty(navigator, 'platform', original);
  }
}

describe('applyOwnTitlebarClass', () => {
  afterEach(() => {
    document.body.classList.remove('own-titlebar', 'controls-trailing');
    delete (navigator as Navigator & { userAgentData?: unknown }).userAgentData;
  });

  test('macOS: own-titlebar, no controls-trailing (traffic lights sit leading)', () => {
    withPlatform('MacIntel', () => {
      applyOwnTitlebarClass();
      expect(document.body.classList.contains('own-titlebar')).toBe(true);
      expect(document.body.classList.contains('controls-trailing')).toBe(false);
    });
  });

  test('Windows: BOTH own-titlebar and controls-trailing — moss draws the whole caption', () => {
    withPlatform('Win32', () => {
      applyOwnTitlebarClass();
      expect(document.body.classList.contains('own-titlebar')).toBe(true);
      expect(document.body.classList.contains('controls-trailing')).toBe(true);
    });
  });

  test('Linux: controls-trailing only — native caption stays, but its controls sit trailing', () => {
    withPlatform('Linux x86_64', () => {
      applyOwnTitlebarClass();
      expect(document.body.classList.contains('own-titlebar')).toBe(false);
      expect(document.body.classList.contains('controls-trailing')).toBe(true);
    });
  });

  test('removes a stale own-titlebar class when called again on a non-mac UA', () => {
    // toggle(), not add(): a second call on a different platform must be able
    // to retract the class, not just set it once.
    withPlatform('MacIntel', () => applyOwnTitlebarClass());
    expect(document.body.classList.contains('own-titlebar')).toBe(true);
    withPlatform('Linux x86_64', () => applyOwnTitlebarClass());
    expect(document.body.classList.contains('own-titlebar')).toBe(false);
  });

  test('removes a stale controls-trailing class when called again on macOS', () => {
    // Same retraction guarantee, the other direction: Windows/Linux ->
    // macOS must drop controls-trailing, not just leave it stuck true.
    withPlatform('Win32', () => applyOwnTitlebarClass());
    expect(document.body.classList.contains('controls-trailing')).toBe(true);
    withPlatform('MacIntel', () => applyOwnTitlebarClass());
    expect(document.body.classList.contains('controls-trailing')).toBe(false);
  });
});

describe('applyCaptionOwnerClass', () => {
  afterEach(() => {
    document.body.classList.remove('owns-caption');
  });

  test('true sets body.owns-caption', () => {
    applyCaptionOwnerClass(true);
    expect(document.body.classList.contains('owns-caption')).toBe(true);
  });

  test('false removes it', () => {
    applyCaptionOwnerClass(true);
    applyCaptionOwnerClass(false);
    expect(document.body.classList.contains('owns-caption')).toBe(false);
  });

  test('toggles, not just sets — a second call can retract a stale true', () => {
    // Mirrors applyOwnTitlebarClass's own retraction guarantee above: this is
    // called LIVE on every editor:full-width-changed, so entering then
    // exiting full-width must actually clear the class, not leave it stuck
    // from the first call.
    applyCaptionOwnerClass(true);
    applyCaptionOwnerClass(true);
    applyCaptionOwnerClass(false);
    expect(document.body.classList.contains('owns-caption')).toBe(false);
  });
});
