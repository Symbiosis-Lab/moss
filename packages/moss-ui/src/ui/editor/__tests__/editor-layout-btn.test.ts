/**
 * Tests for the editor layout toggle button (#editor-layout-btn).
 *
 * The button lives in the editor webview's titlebar and toggles between
 * split and full-width editor modes by emitting Tauri events:
 * - In split mode: clicking emits `editor:request-full-width`
 * - In full-width mode: clicking emits `editor:request-split`
 *
 * The button icon updates when the app responds with `editor:full-width-changed`.
 */

import { describe, test, expect, vi, beforeEach, afterEach, type MockInstance } from 'vitest';
import { host, inPageBus } from "../../host";

// ---------------------------------------------------------------------------
// vi.hoisted() — variables referenced inside vi.mock() factories
// ---------------------------------------------------------------------------
// The bus is the UI's own message path; each test gets a fresh one and spies on it.
let mockEmit: MockInstance;
let mockOn: MockInstance;

// ---------------------------------------------------------------------------
// Mock modules
// ---------------------------------------------------------------------------
// Mock CM6 and other editor modules to prevent real initialization
vi.mock('../cm-editor', () => ({
  CmEditor: vi.fn(),
}));

vi.mock('../chip-bar', () => ({
  ChipBar: vi.fn(),
}));

vi.mock('../tree', () => ({
  FileTree: vi.fn(),
}));

vi.mock('../selection-toolbar', () => ({
  wireSelectionToolbar: vi.fn(),
}));

vi.mock('../cm-scroll-sync', () => ({
  findNearestHeading: vi.fn(),
  headingToAnchor: vi.fn(),
  getFocusLine: vi.fn(),
  scrollEditorToLine: vi.fn(),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const EXPAND_SVG_MARKER = 'points="15 3 21 3 21 9"'; // maximize-2 icon
const COLUMNS_SVG_MARKER = 'M12 3v18'; // columns icon

function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div class="moss-editor" id="moss-editor-root">
      <div class="nav-surface" id="nav-surface">
        <div class="nav-grid" id="nav-grid"></div>
        <div class="nav-right" id="nav-right">
          <button class="editor-layout-btn" id="editor-layout-btn" data-tooltip="Toggle full width" data-tooltip-kbd="⌘⇧E">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/></svg>
          </button>
          <span id="moss-save-status" data-inline-task-anchor="save"></span>
        </div>
      </div>
      <div class="divider" id="divider"></div>
      <div class="chip-bar" id="chip-bar"></div>
      <div class="moss-editor-cm" id="moss-editor-cm"></div>
    </div>
    <div class="dropdown" id="dropdown"></div>
    <div class="modal-bg" id="modal-bg"><div class="modal" id="modal"></div></div>
  `;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('editor layout toggle button', () => {
  beforeEach(() => {
    setupEditorDOM();
    host.bus = inPageBus();
    mockEmit = vi.spyOn(host.bus, 'emit');
    mockOn = vi.spyOn(host.bus, 'on');
    vi.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('#editor-layout-btn exists in the editor HTML', () => {
    const btn = document.getElementById('editor-layout-btn');
    expect(btn).not.toBeNull();
    expect(btn!.tagName).toBe('BUTTON');
  });

  test('button precedes the save-status anchor in nav-right', () => {
    // nav-right = [layout button, #moss-save-status]. The save inline-status
    // badge anchors to the titlebar (#moss-save-status), not the per-file
    // chip-bar; the layout button stays the leading element.
    const navRight = document.getElementById('nav-right')!;
    const ids = Array.from(navRight.children).map((el) => el.id);
    expect(ids.indexOf('editor-layout-btn')).toBeLessThan(ids.indexOf('moss-save-status'));
  });

  test('button starts with expand icon (maximize-2) in split mode', () => {
    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.innerHTML).toContain(EXPAND_SVG_MARKER);
  });
});

describe('editor layout button visibility (split vs full-width)', () => {
  beforeEach(() => {
    setupEditorDOM();
    host.bus = inPageBus();
    mockEmit = vi.spyOn(host.bus, 'emit');
    mockOn = vi.spyOn(host.bus, 'on');
    vi.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('button starts hidden after wireLayoutButton (split mode)', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.style.display).toBe('none');
  });

  test('button becomes visible when entering full-width mode', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.style.display).toBe('none');

    host.bus.emit('editor:full-width-changed', { fullWidth: true });
    expect(btn.style.display).toBe('');
  });

  test('button hides again when returning to split mode', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    host.bus.emit('editor:full-width-changed', { fullWidth: true });
    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.style.display).toBe('');

    host.bus.emit('editor:full-width-changed', { fullWidth: false });
    expect(btn.style.display).toBe('none');
  });
});

describe('editor layout button event wiring', () => {
  beforeEach(() => {
    setupEditorDOM();
    host.bus = inPageBus();
    mockEmit = vi.spyOn(host.bus, 'emit');
    mockOn = vi.spyOn(host.bus, 'on');
    vi.clearAllMocks();
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('clicking button in split mode emits editor:request-full-width', async () => {
    // Import the module which wires the button
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    const btn = document.getElementById('editor-layout-btn')!;
    btn.click();

    expect(mockEmit).toHaveBeenCalledWith('editor:request-full-width');
  });

  test('clicking button in full-width mode emits editor:request-split', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    // Simulate the app responding with full-width mode active
    expect(mockOn).toHaveBeenCalledWith('editor:full-width-changed', expect.any(Function));
    host.bus.emit('editor:full-width-changed', { fullWidth: true });

    mockEmit.mockClear();

    const btn = document.getElementById('editor-layout-btn')!;
    btn.click();

    expect(mockEmit).toHaveBeenCalledWith('editor:request-split');
  });

  test('⌘⇧E in the editor webview emits the same toggle as the button', async () => {
    // The tooltip chip on the button advertises ⌘⇧E; the shell binds it too,
    // but keydown never crosses webviews, so the editor must bind it itself.
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    document.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'E', metaKey: true, shiftKey: true, bubbles: true, cancelable: true,
    }));
    expect(mockEmit).toHaveBeenCalledWith('editor:request-full-width');

    // App responds; the same shortcut now toggles back.
    host.bus.emit('editor:full-width-changed', { fullWidth: true });
    mockEmit.mockClear();
    document.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'E', metaKey: true, shiftKey: true, bubbles: true, cancelable: true,
    }));
    expect(mockEmit).toHaveBeenCalledWith('editor:request-split');
  });

  test('receiving editor:full-width-changed with fullWidth=true shows columns icon', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    expect(mockOn).toHaveBeenCalledWith('editor:full-width-changed', expect.any(Function));
    host.bus.emit('editor:full-width-changed', { fullWidth: true });

    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.innerHTML).toContain(COLUMNS_SVG_MARKER);
    expect(btn.innerHTML).not.toContain(EXPAND_SVG_MARKER);
  });

  test('receiving editor:full-width-changed with fullWidth=false shows expand icon', async () => {
    const { wireLayoutButton } = await import('../editor-layout-btn');
    wireLayoutButton();

    // First go to full-width
    host.bus.emit('editor:full-width-changed', { fullWidth: true });
    // Then back to split
    host.bus.emit('editor:full-width-changed', { fullWidth: false });

    const btn = document.getElementById('editor-layout-btn')!;
    expect(btn.innerHTML).toContain(EXPAND_SVG_MARKER);
    expect(btn.innerHTML).not.toContain(COLUMNS_SVG_MARKER);
  });
});
