import { describe, it, test, expect } from 'vitest';
import {
  spineBody,
  stateRowKind,
  navH,
  desiredHeight,
  maxTreeHeight,
  ROW_H,
  ROW_GAP,
  NAV_PAD_BOTTOM,
  TITLEBAR_H,
  MIN_EDITOR_H,
  type ForestRow,
} from '../spine-projection';

// A small fixture tree (registry-domain paths; '' = project root):
//   ''            (root)
//   ├── AGENTS.md
//   ├── docs/            (folder)
//   │   ├── index.md
//   │   ├── getting-started.md
//   │   └── api/        (folder)
//   │       └── api/ref.md
//   └── assets/          (folder, empty-ish)
const CHILDREN: Record<string, string[]> = {
  '': ['AGENTS.md', 'docs', 'assets'],
  docs: ['docs/index.md', 'docs/getting-started.md', 'docs/api'],
  'docs/api': ['docs/api/ref.md'],
  assets: [],
};
const DIRS = new Set(['', 'docs', 'docs/api', 'assets']);

const getChildren = (p: string): string[] | null => CHILDREN[p] ?? [];
const isDir = (p: string): boolean => DIRS.has(p);

/** Convenience: just the paths a body renders, in order. */
const paths = (rows: ForestRow[]): string[] => rows.map((r) => r.path);
/** Convenience: a map path → depth. */
const depthOf = (rows: ForestRow[]): Record<string, number> =>
  Object.fromEntries(rows.map((r) => [r.path, r.depth]));

describe('spineBody — deepest-ancestor-first tree body', () => {
  // ── Breadcrumb-only default ───────────────────────────────────────────────

  it('renders NOTHING when no folder is open (breadcrumb-only default)', () => {
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map();
    expect(spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir)).toEqual([]);
  });

  it('renders NOTHING when only the root active file has no opens', () => {
    const activePath = ['', 'index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map();
    expect(spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir)).toEqual([]);
  });

  // ── Root-level active file: only i=0 runs (preserves original behavior) ────

  it('a root-level active file with root open → root children minus the active file', () => {
    // active = ['', 'index.md']; index.md is a same-named root file. (Our
    // fixture root children are AGENTS.md / docs / assets — none is the active
    // leaf — so all three render at depth 1.)
    const activePath = ['', 'index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual(['AGENTS.md', 'docs', 'assets']);
    expect(rows.every((r) => r.depth === 1)).toBe(true);
  });

  it('a root-level active file EXCLUDES itself from the root children', () => {
    // Active file IS one of the root children (docs is a dir, but treat
    // AGENTS.md as the active leaf here). It must not re-appear in the body.
    const activePath = ['', 'AGENTS.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual(['docs', 'assets']);
  });

  // ── Deepest-first ordering: the headline behavior ──────────────────────────

  it('renders the active file’s SIBLINGS first (deepest ancestor), then the root level', () => {
    // active = index.md; root + docs open. docs's NON-active children
    // (depth 2) come FIRST — directly under the breadcrumb — then root's
    // non-active children (depth 1).
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual([
      // i=1 (docs, depth 2) — deepest first; index.md (active leaf) excluded
      'docs/getting-started.md',
      'docs/api',
      // i=0 (root, depth 1); docs (active ancestor) excluded
      'AGENTS.md',
      'assets',
    ]);
    const d = depthOf(rows);
    expect(d['docs/getting-started.md']).toBe(2);
    expect(d['docs/api']).toBe(2);
    expect(d['AGENTS.md']).toBe(1);
    expect(d['assets']).toBe(1);
  });

  it('the active LEAF and active ANCESTOR are both excluded from the body', () => {
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).not.toContain('docs/index.md'); // leaf → breadcrumb
    expect(paths(rows)).not.toContain('docs'); // ancestor → breadcrumb crumb
  });

  // ── Per-level gating: a level hides when its folder is closed ──────────────

  it('hides the DEEPEST level when its ancestor folder is closed (only root level shows)', () => {
    // docs (active ancestor) NOT open → its children don't render. Root open →
    // root's non-active children render. docs (active node) excluded regardless.
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual(['AGENTS.md', 'assets']);
    expect(rows.every((r) => r.depth === 1)).toBe(true);
  });

  it('hides the ROOT level when root is closed (only the deepest open level shows)', () => {
    // root NOT open → its children don't render. docs open → its non-active
    // children render at depth 2.
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual(['docs/getting-started.md', 'docs/api']);
    expect(rows.every((r) => r.depth === 2)).toBe(true);
  });

  // ── A non-active open subfolder recurses NORMALLY (root-first within it) ────

  it('recurses into a NON-active open subfolder one depth deeper, in normal order', () => {
    // active = index.md; root + docs open, AND the api folder (a non-active
    // sibling folder of the active leaf) is open. api's child surfaces at
    // depth 3, right after api at depth 2 — within the deepest level block.
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([
      ['', 'open'],
      ['docs', 'open'],
      ['docs/api', 'open'],
    ]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual([
      // i=1 (docs, depth 2)
      'docs/getting-started.md',
      'docs/api',
      'docs/api/ref.md', // recursed child of the open non-active folder
      // i=0 (root, depth 1)
      'AGENTS.md',
      'assets',
    ]);
    expect(depthOf(rows)['docs/api/ref.md']).toBe(3);
  });

  it('does NOT recurse into a non-active subfolder that is not itself open', () => {
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]); // api NOT open
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).not.toContain('docs/api/ref.md');
  });

  // ── Deeper active path: two ancestor levels, deepest first ─────────────────

  it('a DEEPER active path renders each ancestor level deepest-first, excluding active nodes', () => {
    // active = api/ref.md. Ancestors: root (i=0), docs (i=1), api (i=2).
    const activePath = ['', 'docs', 'docs/api', 'docs/api/ref.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([
      ['', 'open'],
      ['docs', 'open'],
      ['docs/api', 'open'],
    ]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    // i=2 (api, depth 3): only child is the active leaf → nothing.
    // i=1 (docs, depth 2): index.md, getting-started.md (api is active → excluded).
    // i=0 (root, depth 1): AGENTS.md, assets (docs is active → excluded).
    expect(paths(rows)).toEqual([
      'docs/index.md',
      'docs/getting-started.md',
      'AGENTS.md',
      'assets',
    ]);
    expect(paths(rows)).not.toContain('docs');
    expect(paths(rows)).not.toContain('docs/api');
    expect(paths(rows)).not.toContain('docs/api/ref.md');
    const d = depthOf(rows);
    expect(d['docs/index.md']).toBe(2);
    expect(d['docs/getting-started.md']).toBe(2);
    expect(d['AGENTS.md']).toBe(1);
  });

  // ── isDir flag ─────────────────────────────────────────────────────────────

  it('marks folder rows isDir=true and file rows isDir=false', () => {
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    const byPath = Object.fromEntries(rows.map((r) => [r.path, r.isDir]));
    expect(byPath['docs/api']).toBe(true);
    expect(byPath['docs/getting-started.md']).toBe(false);
    expect(byPath['AGENTS.md']).toBe(false);
  });

  // ── Same-named siblings: only the EXACT active path is excluded ────────────

  it('a same-named sibling under a different folder still renders (only the exact active path is excluded)', () => {
    // Two different folders both contain an `index.md`. Active = the one under docs;
    // blog/index.md must still render (only the EXACT active path is excluded).
    const children: Record<string, string[]> = {
      '': ['docs', 'blog'],
      docs: ['docs/index.md'],
      blog: ['blog/index.md', 'blog/post.md'],
    };
    const dirs = new Set(['', 'docs', 'blog']);
    const gc = (p: string): string[] => children[p] ?? [];
    const id = (p: string): boolean => dirs.has(p);
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open'], ['blog', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', gc, id);
    // i=1 (docs, depth 2): index.md (active) excluded → nothing.
    // i=0 (root, depth 1): docs (active ancestor) excluded → blog (depth 1),
    //   and blog is open so blog/index.md + blog/post.md recurse at depth 2.
    expect(paths(rows)).toEqual(['blog', 'blog/index.md', 'blog/post.md']);
    const d = depthOf(rows);
    expect(d['blog']).toBe(1);
    expect(d['blog/index.md']).toBe(2);
    expect(d['blog/post.md']).toBe(2);
  });

  // ── Row count drives panel height ──────────────────────────────────────────

  it('content row count = body row count (every row is one unit before home-inline merge)', () => {
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    // docs level: getting-started, api (index.md is active → excluded) = 2.
    // root level: AGENTS.md, assets (docs is active → excluded) = 2.
    expect(rows.length).toBe(4);
  });

  // ── Folder-valued leaf ────────────────────────────────
  // The active "file" is itself a directory: generated home (activePath=['']),
  // or dir mode (activePath=['', 'docs']). That folder is the deepest openable
  // level, so it must have at least one level to give a drag-fill gesture.

  it('a folder-valued leaf at the root (activePath=[\'\']) renders root\'s children when open', () => {
    const activePath = [''];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).toEqual(['AGENTS.md', 'docs', 'assets']);
    expect(rows.every((r) => r.depth === 1)).toBe(true);
  });

  it('a folder-valued leaf renders NOTHING when closed (no openable level implicitly open)', () => {
    const activePath = [''];
    const overrides: Map<string, 'open' | 'closed'> = new Map();
    expect(spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir)).toEqual([]);
  });

  it('a folder-valued leaf in dir mode (activePath=[\'\', \'docs\']) opens docs\'s children, deepest first, then root', () => {
    const activePath = ['', 'docs'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    // docs (the folder-valued leaf) is the deepest openable level → depth 2 (activePath.length).
    // root is the remaining ancestor → depth 1.
    expect(paths(rows)).toEqual([
      'docs/index.md',
      'docs/getting-started.md',
      'docs/api',
      'AGENTS.md',
      'assets',
    ]);
    const d = depthOf(rows);
    expect(d['docs/index.md']).toBe(2);
    expect(d['AGENTS.md']).toBe(1);
  });

  it('a folder-valued leaf excludes itself (never double-rendered — it is the breadcrumb, not a body row)', () => {
    const activePath = ['', 'docs'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(paths(rows)).not.toContain('docs');
  });

  // ── State rows: empty folder / degenerate index ──────────────────────────

  it('renders no synthetic child row for an open empty folder', () => {
    // 'assets' is a root-level, genuinely empty folder. Open root (so 'assets'
    // renders as a body row) AND 'assets' itself (so walkNested recurses into
    // it and finds nothing).
    const children: Record<string, string[]> = { '': ['assets'], assets: [] };
    const dirs = new Set(['', 'assets']);
    const gc = (p: string): string[] | null => children[p] ?? null;
    const id = (p: string): boolean => dirs.has(p);
    const rows = spineBody(['', 'other.md'], (p) => p === '' || p === 'assets', gc, id);
    expect(rows.map((r) => r.path)).toContain('assets');
    const stateRow = rows.find((r) => r.kind === 'empty');
    expect(stateRow).toBeUndefined();
  });

  it('renders an "unavailable" state row when the index has no entry for an open folder', () => {
    const children: Record<string, string[] | undefined> = { '': ['docs'] };
    const dirs = new Set(['', 'docs']);
    const gc = (p: string): string[] | null => (p in children ? (children[p] as string[]) : null);
    const id = (p: string): boolean => dirs.has(p);
    const activePath = ['', 'docs', 'docs/index.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', gc, id);
    const stateRow = rows.find((r) => r.kind === 'unavailable');
    expect(stateRow).toBeDefined();
    expect(stateRow?.depth).toBe(2);
  });

  it('does NOT render a state row when the only child is the active node (that content is in the breadcrumb)', () => {
    const activePath = ['', 'docs', 'docs/api', 'docs/api/ref.md'];
    const overrides: Map<string, 'open' | 'closed'> = new Map([['', 'open'], ['docs', 'open'], ['docs/api', 'open']]);
    const rows = spineBody(activePath, (p) => overrides.get(p) === 'open', getChildren, isDir);
    expect(rows.some((r) => r.kind)).toBe(false);
  });

});

// ── stateRowKind — the one decider for a state row's kind ───────────────────
//
// Both spineBody's own renderLevel (above) and render.ts's renderLevelBody
// call this single function instead of re-deriving the branch, so the two
// traversal sites can never drift into two copies of the same rule.
describe('stateRowKind', () => {
  it('null children (index has no entry) → unavailable, regardless of hidden', () => {
    expect(stateRowKind(null, 0)).toBe('unavailable');
    expect(stateRowKind(null, 5)).toBe('unavailable');
  });

  it('empty children, hidden = 0 → empty (genuinely nothing here)', () => {
    expect(stateRowKind([], 0)).toBe('empty');
  });

  it('empty children, hidden > 0 → hidden (moss filtered something)', () => {
    expect(stateRowKind([], 1)).toBe('hidden');
    expect(stateRowKind([], 2)).toBe('hidden');
  });
});

describe('spineBody: derivation hint row', () => {
  const open = (p: string): boolean => p === '' || p === 'docs';
  const body = (hasHint: (p: string) => boolean) =>
    spineBody([''], open, getChildren, isDir, () => 0, hasHint);

  it('adds one row, first under its folder, in front of the real children', () => {
    const rows = body((p) => p === 'docs');
    const at = rows.findIndex((r) => r.kind === 'hint');
    expect(rows[at - 1].path).toBe('docs');
    expect(rows[at].depth).toBe(2);
    expect(rows[at + 1].path).toBe('docs/index.md');
    expect(rows.length).toBe(body(() => false).length + 1);
  });

  it('adds nothing when no folder has a hint', () => {
    expect(body(() => false).some((r) => r.kind === 'hint')).toBe(false);
  });
});

describe('sash arithmetic', () => {
  describe('navH', () => {
    test('ROW_H matches segment CSS height (28px)', () => {
      expect(ROW_H).toBe(28);
    });

    test('NAV_PAD_BOTTOM is 0px', () => {
      expect(NAV_PAD_BOTTOM).toBe(0);
    });

    test('TITLEBAR_H matches --moss-titlebar-height (48px)', () => {
      expect(TITLEBAR_H).toBe(48);
    });

    test('breadcrumb only (0 extra rows)', () => {
      expect(navH(0)).toBe(TITLEBAR_H + NAV_PAD_BOTTOM);
    });

    test('with extra rows', () => {
      expect(navH(3)).toBe(TITLEBAR_H + 3 * (ROW_H + ROW_GAP) + NAV_PAD_BOTTOM);
    });
  });

  // The one height formula: content-fit, capped by the park, clamped to
  // [navH(0), floor]. The controller's wrapper height, its "is anything but
  // the breadcrumb showing" read and the sash's pointerdown baseline all come
  // from here, so none of them can restate the clamp differently.
  describe('desiredHeight', () => {
    const floor = 600;

    test('no park → content-fit', () => {
      expect(desiredHeight(3, null, floor)).toBe(navH(3));
    });

    test('a park caps content-fit (in-tree expand keeps the parked size)', () => {
      expect(desiredHeight(10, 100, floor)).toBe(100);
    });

    test('a park above content-fit does not stretch it (collapse shrinks below the park)', () => {
      expect(desiredHeight(1, 999, floor)).toBe(navH(1));
    });

    test('never below the breadcrumb-only minimum, even with a park under it', () => {
      expect(desiredHeight(0, null, floor)).toBe(navH(0));
      expect(desiredHeight(5, 10, floor)).toBe(navH(0));
    });

    test('never above the floor the editor leaves', () => {
      expect(desiredHeight(50, null, floor)).toBe(floor);
      expect(desiredHeight(50, 700, floor)).toBe(floor);
    });

    test('the minimum wins over a floor that has dropped beneath it', () => {
      expect(desiredHeight(2, null, 20)).toBe(navH(0));
    });
  });

  describe('maxTreeHeight', () => {
    // No chip-bar term: the chip bar is document content inside the CM
    // scroller (Amendment III), so only the divider sits between nav and
    // editor as budgeted chrome.
    test('normal case: 800px window minus divider and min editor', () => {
      // 800 - 1 - 144 = 655
      expect(maxTreeHeight(800, 1, MIN_EDITOR_H)).toBe(655);
    });

    test('returns negative when inputs exceed windowH', () => {
      // 140 - 1 - 144 = -5
      // Negative is fine — the caller clamps the floor against minH.
      expect(maxTreeHeight(140, 1, MIN_EDITOR_H)).toBe(-5);
    });
  });
});
