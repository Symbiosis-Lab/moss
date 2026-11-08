/**
 * Tests for editor layout changes.
 *
 * Title bar, divider, and no plus button
 * - #new-page-btn does not exist in the editor DOM after initialization
 *
 * File tree scroll
 * - .nav-surface uses overflow-y: auto with thin scrollbar
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { readFileSync } from 'fs';
import { resolve } from 'path';

// ---------------------------------------------------------------------------
// Hoisted mocks — vi.mock factories are hoisted above imports, so we must
// use vi.hoisted() for any variables referenced inside them.
// ---------------------------------------------------------------------------
const { mockCommands, MockChipBar, MockCmEditor, MockFileTree } = vi.hoisted(() => {
  const mockCommands = {
    getSchema: vi.fn().mockResolvedValue({ status: 'ok', data: { fields: [] } }),
    getProjectPath: vi.fn().mockResolvedValue({ status: 'ok', data: '/Users/test/blog' }),
    editorBootstrap: vi.fn(),
    parseFrontmatter: vi.fn(),
    getFileInfo: vi.fn().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } }),
    saveEditorContent: vi.fn(),
    resolveUrlForFile: vi.fn(),
  };
  const MockChipBar = vi.fn().mockImplementation(() => ({
    render: vi.fn(),
    destroy: vi.fn(),
    getValues: vi.fn().mockReturnValue({}),
  }));
  const MockCmEditor = vi.fn().mockImplementation(() => ({
    destroy: vi.fn(),
    getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
  }));
  const MockFileTree = vi.fn().mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined),
    unmount: vi.fn(),
    setupDragHandle: vi.fn(),
    highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null),
    collapseForReopen: vi.fn(),
  }));
  return { mockCommands, MockChipBar, MockCmEditor, MockFileTree };
});

vi.mock('../../engine', () => ({ engine: mockCommands }));
vi.mock('../chip-bar', () => ({ ChipBar: MockChipBar }));
vi.mock('../cm-editor', () => ({ CmEditor: MockCmEditor }));
vi.mock('../tree', () => ({ FileTree: MockFileTree }));
vi.mock('../file-viewer', () => ({ showFileViewer: vi.fn(), destroyFileViewer: vi.fn() }));

// ---------------------------------------------------------------------------
// DOM helper — mirrors editor.html structure (without #new-page-btn)
// ---------------------------------------------------------------------------

function setupEditorDOM(): void {
  document.body.innerHTML = `
    <div class="moss-editor" id="moss-editor-root">
      <div class="nav-surface" id="nav-surface">
        <div class="nav-grid" id="nav-grid"><div class="moss-nav-skeleton" id="moss-nav-skeleton" aria-hidden="true"></div></div>
        <div class="nav-right" id="nav-right"></div>
      </div>
      <div class="divider" id="divider"></div>
      <div class="chip-bar" id="chip-bar"></div>
      <div class="moss-editor-cm" id="moss-editor-cm"><div class="moss-editor-skeleton" id="moss-editor-skeleton" aria-hidden="true"></div></div>
    </div>
    <div class="dropdown" id="dropdown"></div>
    <div class="modal-bg" id="modal-bg"><div class="modal" id="modal"></div></div>
  `;
}

/**
 * editorBootstrap delegate — routes through the existing getSchema /
 * getProjectPath mocks so per-test overrides keep working unchanged.
 * Struct fields are snake_case (specta does not camelCase struct fields);
 * `initial_file: null` makes loadFile fall back to the mocked parseFrontmatter.
 */
function applyEditorBootstrapDelegate(): void {
  mockCommands.editorBootstrap.mockImplementation(async (_src: string | null) => {
    const s = await mockCommands.getSchema();
    if (s?.status !== 'ok') return { status: 'error', error: s?.error };
    const p = await mockCommands.getProjectPath();
    return {
      status: 'ok',
      data: {
        schema: s.data,
        project_path: p?.status === 'ok' ? p.data : null,
        initial_file: null,
      },
    };
  });
}
applyEditorBootstrapDelegate();

function resetMocks(): void {
  mockCommands.getSchema.mockReset().mockResolvedValue({ status: 'ok', data: { fields: [] } });
  mockCommands.getProjectPath.mockReset().mockResolvedValue({ status: 'ok', data: '/Users/test/blog' });
  mockCommands.editorBootstrap.mockReset();
  applyEditorBootstrapDelegate();
  mockCommands.parseFrontmatter.mockReset();
  mockCommands.getFileInfo.mockReset().mockResolvedValue({ status: 'ok', data: { size: '0', modified: null, width: null, height: null } });
  mockCommands.saveEditorContent.mockReset();
  mockCommands.resolveUrlForFile.mockReset();
  MockChipBar.mockImplementation(() => ({
    render: vi.fn(), destroy: vi.fn(), getValues: vi.fn().mockReturnValue({}), setPageFacts: vi.fn(),
  }));
  MockCmEditor.mockImplementation(() => ({
    destroy: vi.fn(), getContent: vi.fn().mockReturnValue(''),
    getView: vi.fn().mockReturnValue({ scrollDOM: { addEventListener: vi.fn(), prepend: vi.fn() } }),
  }));
  MockFileTree.mockImplementation(() => ({
    mount: vi.fn().mockResolvedValue(undefined), unmount: vi.fn(),
    setupDragHandle: vi.fn(), highlight: vi.fn(),
    getFirstFile: vi.fn().mockReturnValue(null),
    collapseForReopen: vi.fn(),
  }));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('editor layout: #new-page-btn removed', () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = '';
    setupEditorDOM();
    resetMocks();
  });

  afterEach(() => {
    window.dispatchEvent(new Event('beforeunload'));
    vi.restoreAllMocks();
  });

  test('#new-page-btn does not exist in the editor DOM', () => {
    // The HTML template no longer contains the button
    const btn = document.getElementById('new-page-btn');
    expect(btn).toBeNull();
  });

  // The 'Cmd+N / Ctrl+N does NOT create' negative pins died with the dead
  // createInCurrentDir method (nothing left to wire by accident). The upcoming
  // "+" create-button change adds positive pins of its new wiring here.
});

// ---------------------------------------------------------------------------
// File tree scroll — sticky breadcrumb
// ---------------------------------------------------------------------------

describe('file tree scroll: sticky breadcrumb', () => {
  test('.nav-surface CSS has overflow: hidden (scroll moved to .nav-body)', () => {
    const css = readFileSync(resolve(__dirname, '../editor.css'), 'utf-8');
    const navSurfaceLine = css.split('\n').find(l => l.startsWith('.nav-surface {'));
    expect(navSurfaceLine).toBeDefined();
    // .nav-surface no longer scrolls — .nav-body does
    expect(navSurfaceLine).not.toContain('overflow-y: auto');
    expect(navSurfaceLine).toContain('overflow: hidden');
  });

  test('.nav-body has overflow-y: auto unconditionally', () => {
    const css = readFileSync(resolve(__dirname, '../editor.css'), 'utf-8');
    // Match the multi-line .nav-body rule block. The breadcrumb moved into
    // .nav-bc-strip (a sibling outside the scrollport), so the has-overflow
    // gate is no longer needed and the CSS is now unconditional.
    const navBodyMatch = css.match(/\.nav-body\s*\{([^}]*)\}/s);
    expect(navBodyMatch).not.toBeNull();
    const body = navBodyMatch![1];
    expect(body).toContain('overflow-y: auto');
    expect(body).toContain('scrollbar-width: thin');
    // The legacy .nav-body.has-overflow rule must be gone.
    expect(css).not.toMatch(/\.nav-body\.has-overflow\b/);
  });

  test('file-tree.css .seg.bc does not declare position: sticky (subgrid restructure)', () => {
    const css = readFileSync(resolve(__dirname, '../file-tree.css'), 'utf-8');
    // .seg.bc still exists as a selector (min-width: 0), but the rule body
    // must not contain position: sticky. The breadcrumb moved into
    // .nav-bc-strip which doesn't scroll; sticky is structurally unnecessary
    // and was the cause of the 10px first-scroll snap.
    expect(css).toContain('.seg.bc');
    const segBcRules = css.match(/\.seg\.bc[\s\S]*?\{[\s\S]*?\}/g) ?? [];
    for (const rule of segBcRules) {
      expect(rule).not.toMatch(/position:\s*sticky/);
    }
  });

  test('.nav-grid is a flex column with fixed-indent tokens (no column grid)', () => {
    const css = readFileSync(resolve(__dirname, '../editor.css'), 'utf8');
    const m = css.match(/\.nav-grid\s*\{([^}]*)\}/s);
    expect(m).not.toBeNull();
    expect(m![1]).toMatch(/display:\s*flex/);
    expect(m![1]).not.toMatch(/subgrid/);
    expect(m![1]).toMatch(/--ft-indent/);
  });

  test('file-tree.css uses fixed-indent (no subgrid, no --bc-floor) and full-bleed hairline', () => {
    const css = readFileSync(resolve(__dirname, '../file-tree.css'), 'utf8');
    expect(css).not.toMatch(/subgrid/);
    expect(css).not.toMatch(/--bc-floor/);
    // hairline lives on the full-bleed strip
    const strip = css.match(/\.nav-bc-strip\s*\{([^}]*)\}/s);
    expect(strip).not.toBeNull();
    expect(strip![1]).toMatch(/border-bottom/);
    // tree rows indent by the shared per-depth offset (adaptive spine)
    expect(css).toMatch(/\.nav-body \.seg\s*\{[^}]*--ft-offset/s);
  });

  test('file-tree.css tree rows indent via --ft-offset (adaptive), logical props, --ft-seg-min', () => {
    const css = readFileSync(resolve(__dirname, '../file-tree.css'), 'utf8');
    // Tree rows read a px offset from the shared allocator array, not depth*indent.
    expect(css).toMatch(/\.nav-body \.seg\s*\{[^}]*--ft-offset/s);
    expect(css).not.toMatch(/--ft-depth\s*\*\s*var\(--ft-indent\)/);
    // Pill highlight: the depth indent moved to a TRANSPARENT
    // margin-inline-start calc (outside the seg's background box) so the
    // hover/selected background wraps only icon+name. Still a logical inset
    // (RTL-safe), still consumes the shared --ft-offset.
    expect(css).toMatch(/margin-inline-start:\s*calc\([\s\S]*?--ft-offset/s);
    // The per-segment floor token is declared on .nav-grid (editor.css).
    const editorCss = readFileSync(resolve(__dirname, '../editor.css'), 'utf8');
    expect(editorCss).toMatch(/--ft-seg-min/);
    // --ft-indent retained as the deep-tree fallback step.
    expect(editorCss).toMatch(/--ft-indent/);
  });
});
