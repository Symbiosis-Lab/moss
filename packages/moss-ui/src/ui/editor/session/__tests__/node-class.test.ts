// packages/moss-ui/src/ui/editor/session/__tests__/node-class.test.ts
import { describe, it, test, expect, expectTypeOf, vi } from 'vitest';
import type { TakeoverKind as GeneratedTakeoverKind } from '@symbiosis-lab/moss-ui';
import { folderHomeTakeover, type TakeoverKind } from '../node-class';
import {
  decideEditorSurface,
  shouldLoadContentPage,
  editorQueryFromNode,
  resolveNodeForEdit,
  readEditorNode,
  pendingOf,
} from '../node-class';
import type { ResolvedNode, EditSourceResolution } from '../node-class';

describe('decideEditorSurface', () => {
  it('content when a source file exists (regardless of isRoot/dir)', () => {
    expect(decideEditorSurface(
      { sourcePath: '/x/post.md', isRoot: false, takeover: null, pending: null },
    )).toEqual({ kind: 'content', sourcePath: '/x/post.md' });
  });

  it('empty-string source is treated as no source', () => {
    expect(decideEditorSurface(
      { sourcePath: '', isRoot: true, takeover: null, pending: null },
    )).toEqual({ kind: 'generated' });
  });

  it('unknown root does not claim the folder is empty', () => {
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: true, takeover: null, pending: null },
    )).toEqual({ kind: 'generated' });
  });

  it('root + non-empty folder, no index.md → the root-home takeover the resolver computed', () => {
    const takeover = folderHomeTakeover('/abs/site', 'My Site', true);
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: true, takeover, pending: null },
    )).toEqual({ kind: 'takeover', takeover });
  });

  it('an unclaimed term page carries its claim recipe; the surface is the same takeover kind', () => {
    const takeover = {
      kind: 'term-page' as const,
      display: '馬欣宜',
      field: 'author',
      files: [{ dir: '/abs/site/authors', name: '馬欣宜.md', frontmatter: { author_page: '馬欣宜' } }],
    };
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: false, takeover, pending: null },
    )).toEqual({ kind: 'takeover', takeover });
  });

  it('subfolder with a real dir, no home note → folder CTA', () => {
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: false, takeover: folderHomeTakeover('/abs/My Blog', 'My Blog'), pending: null },
    )).toEqual({ kind: 'takeover', takeover: folderHomeTakeover('/abs/My Blog', 'My Blog') });
  });

  it('no source, no dir (generated page) → no page to edit', () => {
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: false, takeover: null, pending: null },
    )).toEqual({ kind: 'generated' });
  });
});

describe('a source that is still arriving', () => {
  it('a known path whose bytes are pending waits on that file', () => {
    expect(decideEditorSurface({ sourcePath: '/x/post.md', isRoot: false, takeover: null, pending: 'download' }))
      .toEqual({ kind: 'pending', reason: 'download', sourcePath: '/x/post.md' });
  });

  // The reported failure: pressing Edit on the home page of a Google Drive
  // vault mid-materialization showed "No page to edit — This page is generated
  // by moss. There's nothing here to edit." over a file that plainly exists.
  // That sentence is a claim about the SITE; the truth was a claim about the
  // NETWORK, and the two surfaces must not be the same one.
  it('outranks every other surface — a real file is not a generated page', () => {
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: true, takeover: null, pending: 'download' },
    )).toEqual({ kind: 'pending', reason: 'download', sourcePath: null });

    // And not the folder CTA, which would offer to CREATE a page over one that
    // is on its way down.
    expect(decideEditorSurface(
      { sourcePath: null, isRoot: false, takeover: folderHomeTakeover('/abs/Blog', 'Blog'), pending: 'download' },
    )).toEqual({ kind: 'pending', reason: 'download', sourcePath: null });
  });

  it('rides the resolver, so the shell self-heal re-reads it on every retry', async () => {
    const waiting = vi.fn().mockResolvedValue({
      status: 'ok' as const,
      data: { source_path: null, source_pending: true },
    });
    const node = await resolveNodeForEdit('/', waiting);
    expect(decideEditorSurface(node)).toEqual({ kind: 'pending', reason: 'download', sourcePath: null });

    // …and the retry that finds the bytes landed returns the real surface with
    // no user action: same call, later answer.
    const arrived = vi.fn().mockResolvedValue({
      status: 'ok' as const,
      data: { source_path: '/vault/My Site.md', source_pending: false },
    });
    const healed = await resolveNodeForEdit('/', arrived);
    expect(decideEditorSurface(healed))
      .toEqual({ kind: 'content', sourcePath: '/vault/My Site.md' });
  });
});

describe('pendingOf: what an answer leaves unknown', () => {
  it('a page the build has not mapped is a lookup, not a download', () => {
    expect(pendingOf({ source_path: null, source_known: false })).toBe('lookup');
  });
  it('an older backend that never says is read as knowing', () => {
    expect(pendingOf({ source_path: null })).toBeNull();
  });
  it('a home whose folder cannot be listed yet is still coming down', () => {
    const takeover = { ...folderHomeTakeover('/abs/site/posts', 'Posts'), folder_empty: null };
    expect(pendingOf({ source_path: null, takeover })).toBe('download');
  });
});

describe('readEditorNode: one reading of a node off the wire', () => {
  it("keeps the shell's verdict", () => {
    expect(readEditorNode({ sourcePath: null, isRoot: false, takeover: null, pending: 'lookup' }))
      .toEqual({ node: { sourcePath: null, isRoot: false, takeover: null, pending: 'lookup' }, homeFromDisk: false });
  });
  it('a takeover the native open baked is an answer, as on a warm open', () => {
    const takeover = folderHomeTakeover('/abs/Notes', 'Notes');
    expect(readEditorNode({ sourcePath: null, isRoot: false, takeover }).node.pending).toBeNull();
  });
  it('a baked home whose folder is still arriving waits for the download', () => {
    const takeover = { ...folderHomeTakeover('/abs/site', 'Site', true), folder_empty: null };
    expect(readEditorNode({ sourcePath: null, isRoot: true, takeover }).node.pending).toBe('download');
  });
  it('a bare root finds its home from disk; any other bare node waits for the shell', () => {
    expect(readEditorNode({ sourcePath: null, isRoot: true, takeover: null }).homeFromDisk).toBe(true);
    expect(readEditorNode({ sourcePath: null, isRoot: false, takeover: null }))
      .toEqual({ node: { sourcePath: null, isRoot: false, takeover: null, pending: 'lookup' }, homeFromDisk: false });
  });
});

describe('editorQueryFromNode', () => {
  it('serializes the full node in build_editor_url param order', () => {
    const takeover = folderHomeTakeover('/abs/My Blog', 'My Blog');
    expect(editorQueryFromNode({ sourcePath: 'works/a b.md', isRoot: true, takeover }))
      .toBe(`?source=works%2Fa%20b.md&is_root=1&takeover=${encodeURIComponent(JSON.stringify(takeover))}`);
  });
  it('omits absent params and returns empty string for the bare node', () => {
    expect(editorQueryFromNode({ sourcePath: null, isRoot: false, takeover: null })).toBe('');
  });
  it('source-only node', () => {
    expect(editorQueryFromNode({ sourcePath: 'alpha.md', isRoot: false, takeover: null })).toBe('?source=alpha.md');
  });
});

describe('shouldLoadContentPage (dedup must not strand a pane-state)', () => {
  it('skips reload only when same file AND CM6 still mounted', () => {
    expect(shouldLoadContentPage(true, true)).toBe(false);
  });
  it('forces reload when same file but CM6 was torn down by a pane-state', () => {
    expect(shouldLoadContentPage(true, false)).toBe(true);
  });
  it('loads when a different file is selected (CM6 mounted or not)', () => {
    expect(shouldLoadContentPage(false, true)).toBe(true);
    expect(shouldLoadContentPage(false, false)).toBe(true);
  });
});

describe('resolveNodeForEdit (Edit-open must mount the on-disk backing .md, not fall to "generated")', () => {
  // The stale cache the Edit button used to bake straight into the editor while a
  // large folder was still rendering its preview: constructor defaults, before
  // PREVIEW_URL_CHANGED had a chance to populate them.
  const STALE_DEFAULT_CACHE: ResolvedNode = {
    sourcePath: null,
    isRoot: false,
    takeover: null,
    pending: null,
  };

  const ok = (data: EditSourceResolution) =>
    vi.fn().mockResolvedValue({ status: 'ok' as const, data });

  it('BUG REPRODUCTION: the OLD behavior (baking the stale cache) routes the site root to "generated"', () => {
    // This is what develop did: setNode(cachedDefaults) → decideEditorSurface →
    // the residual "No page to edit / generated by moss" fallthrough. Pinned here
    // so the fix below is provably a change from this broken outcome.
    expect(decideEditorSurface(STALE_DEFAULT_CACHE))
      .toEqual({ kind: 'generated' });
  });

  it('mid-build root (resolver returns no source yet) → isRoot=true, NEVER the stale generated default', async () => {
    // While the big folder is still building, resolve_page_source('/') can return
    // source_path:null (article-map not yet written). The fix must still bake
    // is_root=1 from the LIVE url, so the editor takes its disk-truth root path.
    // The resolver still answers the root's takeover: it needs no build record,
    // only the vault's name (takeover.rs), so a mid-build root is never bare.
    const takeover = folderHomeTakeover('/abs/site', 'My Site', true);
    const resolvePageSource = ok({ source_path: null, takeover });

    const node = await resolveNodeForEdit('/', resolvePageSource);

    expect(node.isRoot).toBe(true); // <-- develop baked false here; that was the bug
    // With isRoot true and a non-empty folder, the surface is the home CTA / (post
    // disk-truth resolve) content — CRUCIALLY not 'generated'.
    expect(decideEditorSurface(node))
      .toEqual({ kind: 'takeover', takeover });
    expect(decideEditorSurface(node).kind).not.toBe('generated');
  });

  it('root with a backing self-named note (William Blake.md) → content mounting that exact file', async () => {
    const wbPath = '/Users/liuguo/repo/Symbiosis-Lab/moss-recordings/William Blake/William Blake.md';
    const resolvePageSource = ok({ source_path: wbPath });

    const node = await resolveNodeForEdit('/', resolvePageSource);

    expect(node).toEqual({ sourcePath: wbPath, isRoot: true, takeover: null, pending: null });
    expect(decideEditorSurface(node))
      .toEqual({ kind: 'content', sourcePath: wbPath });
  });

  it('does NOT invent a source for a genuinely generated page (tag/feed): stays "generated"', async () => {
    // The resolver is the single source of truth: a generated URL returns
    // source_path:null / takeover:null, and a non-root URL derives isRoot=false.
    const resolvePageSource = ok({ source_path: null, takeover: null });

    const node = await resolveNodeForEdit('/tags/rust/', resolvePageSource);

    expect(node).toEqual({ sourcePath: null, isRoot: false, takeover: null, pending: null });
    expect(decideEditorSurface(node)).toEqual({ kind: 'generated' });
  });

  it('no-home subfolder with a real dir → its folder-home takeover, carried through', async () => {
    const resolvePageSource = ok({
      source_path: null,
      takeover: folderHomeTakeover('/abs/Illuminated Books', 'Illuminated Books'),
    });

    const node = await resolveNodeForEdit('/illuminated-books/', resolvePageSource);

    expect(node.isRoot).toBe(false);
    expect(decideEditorSurface(node))
      .toEqual({ kind: 'takeover', takeover: folderHomeTakeover('/abs/Illuminated Books', 'Illuminated Books') });
  });

  it('treats several root url spellings as root ("", "/", "///") — build-independent rule', async () => {
    const resolvePageSource = ok({ source_path: null });
    for (const p of ['', '/', '///']) {
      const node = await resolveNodeForEdit(p, resolvePageSource);
      expect(node.isRoot, `urlPath=${JSON.stringify(p)}`).toBe(true);
    }
  });

  it('resolve FAILURE is a lookup verdict, never a claim about the page', async () => {
    const failing = vi.fn().mockResolvedValue({ status: 'error' as const, error: 'boom' });
    const node = await resolveNodeForEdit('/', failing);
    // isRoot still comes from the live URL.
    expect(node).toEqual({ sourcePath: null, isRoot: true, takeover: null, pending: 'lookup' });
    expect(decideEditorSurface(node)).toEqual({ kind: 'pending', reason: 'lookup', sourcePath: null });
  });

  it('resolve THROWS → the same verdict (no unhandled rejection)', async () => {
    const throwing = vi.fn().mockRejectedValue(new Error('ipc down'));
    const node = await resolveNodeForEdit('/tags/rust/', throwing);
    expect(node).toEqual({ sourcePath: null, isRoot: false, takeover: null, pending: 'lookup' });
  });
});

describe('canonical folder emptiness', () => {
  it('shows existing cards for a proven empty nested folder and retains its target', () => {
    const takeover = { ...folderHomeTakeover('/abs/site/posts', 'Posts'), folder_empty: true };
    expect(decideEditorSurface({ sourcePath: null, isRoot: false, takeover, pending: null }))
      .toEqual({ kind: 'root-empty', folder: takeover });
  });
  it('never promotes missing editable files to a claim that a folder is empty', () => {
    const takeover = { ...folderHomeTakeover('/abs/site', 'Site', true), folder_empty: false };
    expect(decideEditorSurface({ sourcePath: null, isRoot: true, takeover, pending: null }))
      .toEqual({ kind: 'takeover', takeover });
  });
});

test('a folder whose enumeration is unavailable waits without showing starter cards', () => {
  const takeover = { ...folderHomeTakeover('/abs/site/posts', 'Posts'), folder_empty: null };
  expect(decideEditorSurface(readEditorNode({ sourcePath: null, isRoot: false, takeover }).node))
    .toEqual({ kind: 'pending', reason: 'download', sourcePath: null });
});

test('TAKEOVER_KINDS names the same kinds as the generated TakeoverKind', () => {
  expectTypeOf<TakeoverKind>().toEqualTypeOf<GeneratedTakeoverKind>();
});
