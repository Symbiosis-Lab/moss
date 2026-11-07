// The UI's unit-test setup: jsdom's gaps, a stub host record, and the checks
// every test file shares. It knows no host; the desktop's tests add their own
// setup on top.

import { afterEach, vi } from 'vitest';
import { resetTestHost } from './__tests__/helpers/test-host';

resetTestHost();

if (typeof window !== 'undefined') {
  // jsdom ships HTMLCanvasElement.getContext as a "Not implemented" stub that
  // writes to stderr and throws. The blueprint grid package (consumed by the
  // launcher, folder-mode empty state, and preview empty state) already handles
  // a missing context as a no-op, but stubbing here silences the jsdom warning
  // across the whole suite so tests that mount any of those surfaces stay quiet.
  if (typeof HTMLCanvasElement !== 'undefined') {
    HTMLCanvasElement.prototype.getContext = vi.fn(() => null) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  }

  // jsdom doesn't implement CSS.escape, which production code uses to build safe
  // `[data-path="…"]` selectors (the file tree, the chip bar and the editor's
  // breadcrumb flash). Without it, any code path that queries/flashes a segment
  // throws an uncaught TypeError that fails the whole vitest run even when every
  // test passes. Shim it once here instead of per test file.
  if (typeof globalThis.CSS === 'undefined') {
    (globalThis as unknown as { CSS: { escape(s: string): string } }).CSS = {
      escape: (s: string) => s.replace(/(["\\])/g, '\\$1'),
    };
  } else if (typeof globalThis.CSS.escape !== 'function') {
    (globalThis.CSS as { escape: (s: string) => string }).escape = (s: string) =>
      s.replace(/(["\\])/g, '\\$1');
  }

  // jsdom has no matchMedia; shared/theme.ts (every webview's theme follower)
  // calls it unconditionally. Light OS, never changes. Tests that drive OS
  // changes replace it themselves.
  if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
    window.matchMedia = ((query: string) => ({
      media: query, matches: false, onchange: null,
      addEventListener: () => {}, removeEventListener: () => {},
      addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
    })) as unknown as typeof window.matchMedia;
  }
}

// DataTransfer polyfill — jsdom has no DataTransfer implementation. Several
// drag-and-drop test files need setData/getData/types/effectAllowed. Defined
// here once so per-file copies are not needed.
if (typeof globalThis.DataTransfer === 'undefined') {
  class DataTransferPolyfill {
    items: { [k: string]: string } = {};
    files: FileList = [] as unknown as FileList;
    types: string[] = [];
    effectAllowed: string = 'none';
    dropEffect: string = 'none';
    setData(type: string, data: string): void {
      this.items[type] = data;
      if (!this.types.includes(type)) this.types.push(type);
    }
    getData(type: string): string { return this.items[type] ?? ''; }
    setDragImage(_el: Element, _x: number, _y: number): void { /* noop in jsdom */ }
    clearData(type?: string): void {
      if (type) delete this.items[type];
      else this.items = {};
    }
  }
  (globalThis as unknown as { DataTransfer: typeof DataTransferPolyfill }).DataTransfer =
    DataTransferPolyfill;
}

vi.mock('./log', () => ({
  trace: vi.fn(),
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
  uiEvent: vi.fn(),
  markStartup: vi.fn(),
}));

// A ctx-menu lives on <body>, outside whatever container a test mounted, so a
// menu a test forgot to close would survive into the next test's "no menu is
// open" assertion. Fail the test that leaked it, after clearing the leftover.
afterEach(() => {
  if (typeof document === 'undefined') return;
  // An input popover borrows the .ctx-menu class without being a menu; sweep it, don't fail on it.
  const leaked = document.querySelectorAll('.ctx-menu[role="menu"]');
  document.querySelectorAll('.ctx-menu').forEach((m) => m.remove());
  if (leaked.length === 0) return;
  throw new Error(`${leaked.length} .ctx-menu left open; close it (handle.close(), Escape, or unmount the owner) before the test ends`);
});
