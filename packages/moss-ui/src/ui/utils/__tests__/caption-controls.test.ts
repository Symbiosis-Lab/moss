import { describe, test, expect, beforeEach, vi } from 'vitest';

// Capture the handlers registered via onMossEvent so tests can fire the
// SnapOverlayHover/SnapOverlayClick events the way the native Snap Layouts
// overlay actually would, and stub the host's window so the
// wiring can be proven without a real Tauri window — mirrors
// utils/__tests__/fullscreen-class.test.ts's mocking shape.
const mocks = vi.hoisted(() => {
  const handlers: Record<string, (msg: unknown) => void> = {};
  return {
    handlers,
    onMossEvent: vi.fn(async (kind: string, handler: (msg: unknown) => void) => {
      handlers[kind] = handler;
      return vi.fn(); // UnlistenFn
    }),
    minimizeCurrentWindow: vi.fn(),
    toggleMaximizeCurrentWindow: vi.fn(),
    closeCurrentWindow: vi.fn(),
    isCurrentWindowMaximized: vi.fn().mockResolvedValue(false),
  };
});

vi.mock('../moss-events', () => ({ onMossEvent: mocks.onMossEvent }));

import { host } from '../../host';
host.window = {
  isFullscreen: vi.fn().mockResolvedValue(false),
  isMaximized: mocks.isCurrentWindowMaximized,
  minimize: mocks.minimizeCurrentWindow,
  toggleMaximize: mocks.toggleMaximizeCurrentWindow,
  close: mocks.closeCurrentWindow,
};

import {
  wireMainShellCaptionButtons,
  __resetCaptionControlsForTest,
  MAXIMIZE_HOVER_CLASS,
} from '../fullscreen-class';

function mountButtons(): void {
  document.body.innerHTML = `
    <button id="moss-caption-minimize"></button>
    <button id="moss-caption-maximize" data-maximized="false"></button>
    <button id="moss-caption-close"></button>
  `;
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  __resetCaptionControlsForTest();
  document.body.innerHTML = '';
  document.body.className = ''; // innerHTML reset above leaves classList (e.g. owns-caption) alone
  mocks.onMossEvent.mockClear();
  mocks.minimizeCurrentWindow.mockClear();
  mocks.toggleMaximizeCurrentWindow.mockClear();
  mocks.closeCurrentWindow.mockClear();
  mocks.isCurrentWindowMaximized.mockReset().mockResolvedValue(false);
  for (const k of Object.keys(mocks.handlers)) delete mocks.handlers[k];
});

describe('wireMainShellCaptionButtons', () => {
  test('minimize click calls minimizeCurrentWindow', async () => {
    mountButtons();
    await wireMainShellCaptionButtons();
    document.getElementById('moss-caption-minimize')!.dispatchEvent(new MouseEvent('click'));
    expect(mocks.minimizeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  test('close click calls closeCurrentWindow', async () => {
    mountButtons();
    await wireMainShellCaptionButtons();
    document.getElementById('moss-caption-close')!.dispatchEvent(new MouseEvent('click'));
    expect(mocks.closeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  test('maximize click calls toggleMaximizeCurrentWindow — the direct-click fallback path', async () => {
    // Proves the fallback exists independent of the overlay: a real click on
    // the DOM element (the only kind of click that can reach it once the
    // Snap Layouts overlay wins the hit-test — see the module doc) still
    // drives the same toggle.
    mountButtons();
    await wireMainShellCaptionButtons();
    document.getElementById('moss-caption-maximize')!.dispatchEvent(new MouseEvent('click'));
    expect(mocks.toggleMaximizeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  test('SnapOverlayClick calls toggleMaximizeCurrentWindow — the overlay-forwarded path, when this webview owns the caption row', async () => {
    mountButtons();
    document.body.classList.add('owns-caption');
    await wireMainShellCaptionButtons();
    mocks.handlers['SnapOverlayClick']({});
    expect(mocks.toggleMaximizeCurrentWindow).toHaveBeenCalledTimes(1);
  });

  test('SnapOverlayClick is a no-op when this webview does not own the caption row', async () => {
    // The double-toggle guard. `emit_moss_event_to` narrows the SEND to one
    // webview LABEL, but `preview` and `action-panel` are sibling CHILD
    // webviews of the same `main` window and Tauri's `emit_to` does not
    // disambiguate them — proved empirically (paired Rust/frontend debug
    // log: Rust sent to `preview` only, both webviews' handlers still
    // fired). This gate is what actually stops the second call.
    mountButtons();
    await wireMainShellCaptionButtons();
    mocks.handlers['SnapOverlayClick']({});
    expect(mocks.toggleMaximizeCurrentWindow).not.toHaveBeenCalled();
  });

  test('SnapOverlayHover toggles the hover class on the maximize button, both directions, when this webview owns the caption row', async () => {
    mountButtons();
    document.body.classList.add('owns-caption');
    await wireMainShellCaptionButtons();
    const btn = document.getElementById('moss-caption-maximize')!;
    mocks.handlers['SnapOverlayHover']({ payload: { hovering: true } });
    expect(btn.classList.contains(MAXIMIZE_HOVER_CLASS)).toBe(true);
    mocks.handlers['SnapOverlayHover']({ payload: { hovering: false } });
    expect(btn.classList.contains(MAXIMIZE_HOVER_CLASS)).toBe(false);
  });

  test('SnapOverlayHover is a no-op when this webview does not own the caption row', async () => {
    mountButtons();
    await wireMainShellCaptionButtons();
    const btn = document.getElementById('moss-caption-maximize')!;
    mocks.handlers['SnapOverlayHover']({ payload: { hovering: true } });
    expect(btn.classList.contains(MAXIMIZE_HOVER_CLASS)).toBe(false);
  });

  test('SnapOverlayHover never touches minimize or close', async () => {
    mountButtons();
    document.body.classList.add('owns-caption');
    await wireMainShellCaptionButtons();
    mocks.handlers['SnapOverlayHover']({ payload: { hovering: true } });
    expect(document.getElementById('moss-caption-minimize')!.className).toBe('');
    expect(document.getElementById('moss-caption-close')!.className).toBe('');
  });

  test('data-maximized is set from isCurrentWindowMaximized() on mount', async () => {
    mountButtons();
    mocks.isCurrentWindowMaximized.mockResolvedValue(true);
    await wireMainShellCaptionButtons();
    expect(document.getElementById('moss-caption-maximize')!.getAttribute('data-maximized')).toBe('true');
  });

  test('a FullscreenChanged event refreshes data-maximized', async () => {
    // Double-click, Win+Up, drag-to-top and Snap Layouts all maximize/restore
    // without ever calling toggleMaximizeCurrentWindow() — but every one of
    // them still fires a native Resized, which the host's window code turns into
    // FullscreenChanged unconditionally. This is the one path that has to
    // refresh the glyph now that the click handler itself no longer does.
    mountButtons();
    mocks.isCurrentWindowMaximized.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await wireMainShellCaptionButtons();
    const btn = document.getElementById('moss-caption-maximize')!;
    expect(btn.getAttribute('data-maximized')).toBe('false');
    mocks.handlers['FullscreenChanged']({ payload: { is_fullscreen: false } });
    await flush();
    expect(btn.getAttribute('data-maximized')).toBe('true');
  });

  test('a maximize click alone does not refresh data-maximized — only FullscreenChanged does', async () => {
    // toggleMaximize() must not poll isCurrentWindowMaximized() itself: the
    // previous test would pass with such a poll too, so only this one proves
    // the glyph follows FullscreenChanged alone.
    mountButtons();
    mocks.isCurrentWindowMaximized.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await wireMainShellCaptionButtons();
    const btn = document.getElementById('moss-caption-maximize')!;
    expect(btn.getAttribute('data-maximized')).toBe('false');
    btn.dispatchEvent(new MouseEvent('click'));
    await flush();
    expect(btn.getAttribute('data-maximized')).toBe('false');
  });

  test('a document with no caption buttons (mac/linux, the elements never render) is tolerated', async () => {
    document.body.innerHTML = '';
    await expect(wireMainShellCaptionButtons()).resolves.toBeUndefined();
  });

  test('is idempotent — a second call does not re-subscribe', async () => {
    mountButtons();
    await wireMainShellCaptionButtons();
    await wireMainShellCaptionButtons();
    const kinds = mocks.onMossEvent.mock.calls.map((c) => c[0]);
    expect(kinds.filter((k) => k === 'SnapOverlayHover')).toHaveLength(1);
    expect(kinds.filter((k) => k === 'SnapOverlayClick')).toHaveLength(1);
  });
});
