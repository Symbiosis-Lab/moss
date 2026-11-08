/**
 * Tests for the ONE push reference-resolution adapter (cm-reference-resolver.ts)
 * and the live-preview embed-widget routing by `kind` (cm-live-preview.ts).
 *
 * The resolver batches BOTH embed targets (image-extract → is_embed:true) and
 * link targets (link-extract → is_embed:false) through one mocked
 * editorResolveReferences call, caches by the (text, is_embed) PAIR, and the
 * live-preview routes the embed widget by the classified ReferenceKind.
 */

import { describe, test, expect, vi } from 'vitest';
import type { EditorReferenceResolution, ReferenceKind } from '@symbiosis-lab/moss-ui';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { forceParsing, syntaxTreeAvailable } from '@codemirror/language';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import {
  createReferenceResolver,
  extractReferenceTargets,
  type BatchResolveReferencesFn,
} from '../cm-reference-resolver';
import { assetBustQuery, __resetAssetBustForTests } from '../asset-bust';
import { problemsExtension } from '../cm-problems';

// ── Helpers ──────────────────────────────────────────────────────────

/** Build an EditorView with the markdown+wikilink language + the resolver. */
function makeView(doc: string, resolverExtension: unknown[], container: HTMLElement): EditorView {
  const state = EditorState.create({
    doc,
    extensions: [markdown({ extensions: [wikilinkConfig] }), ...(resolverExtension as any[])],
  });
  return new EditorView({ state, parent: container });
}

/** One always-filled envelope of the given kind (other fields nulled). */
function envelope(target: string, isEmbed: boolean, kind: ReferenceKind): EditorReferenceResolution {
  return {
    target,
    is_embed: isEmbed,
    kind,
    resolved: null,
    url: null,
    anchor: null,
    resolved_path: null,
    message: null,
    candidates: [],
    folder: null,
  };
}

// ── extractReferenceTargets (pure) ───────────────────────────────────

describe('extractReferenceTargets', () => {
  test('embeds get is_embed:true, links get is_embed:false', () => {
    const state = EditorState.create({
      doc: '![[pic.png]] and ![alt](b.jpg) and [text](page.md) and [[Wiki]]',
      extensions: [markdown({ extensions: [wikilinkConfig] })],
    });
    const targets = extractReferenceTargets(state);

    // Embeds (is_embed:true)
    expect(targets).toContainEqual({ text: 'pic.png', is_embed: true });
    expect(targets).toContainEqual({ text: 'b.jpg', is_embed: true });
    // Links (is_embed:false)
    expect(targets).toContainEqual({ text: 'page.md', is_embed: false });
    expect(targets).toContainEqual({ text: 'Wiki', is_embed: false });
    // ![[...]] must NOT also appear as a link target
    expect(targets).not.toContainEqual({ text: 'pic.png', is_embed: false });
  });
});

// ── createReferenceResolver (batch + cache) ──────────────────────────

describe('createReferenceResolver', () => {
  test('batches embed + link targets with correct is_embed', async () => {
    const batch: BatchResolveReferencesFn = vi
      .fn()
      .mockResolvedValue([] as EditorReferenceResolution[]);
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[pic.png]] and [text](page.md)', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 40));
    expect(batch).toHaveBeenCalled();
    const sent = (batch as any).mock.calls.flatMap((c: any[]) => c[0]);
    expect(sent).toContainEqual({ text: 'pic.png', is_embed: true });
    expect(sent).toContainEqual({ text: 'page.md', is_embed: false });
    view.destroy();
    container.remove();
  });

  test('FileChanged revalidates WITHOUT emptying the cache (no destroy/recreate window)', async () => {
    // Root cause of the "CM6 layout shifted while editing" churn:
    // the FileChanged handler called internalCache.clear() immediately, so any
    // rebuild in the ~100ms debounce window (a keystroke) resolved every image
    // to undefined → all image widgets dropped → .cm-content collapsed → then
    // recreated + reloaded when the batch refilled. The editor's OWN 2s auto-save
    // fires FileChanged, so this happened constantly while typing. Fix: keep the
    // old cache live, resolve all targets, then swap synchronously — the cache is
    // never observed empty, so unchanged images keep resolving and their widgets
    // are reused (no collapse/reload).
    const batch: BatchResolveReferencesFn = vi
      .fn()
      .mockImplementation(async (targets) =>
        (targets as { text: string; is_embed: boolean }[]).map((t) =>
          envelope(t.text, t.is_embed, { kind: 'image' }),
        ),
      );
    let fileChangedHandler: () => void = () => {};
    const fileEvents = {
      onFileChanged: (h: () => void) => {
        fileChangedHandler = h;
        return () => {};
      },
      onSitePromoted: () => () => {},
      onSourceAssetChanged: () => () => {},
    };
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[pic.png]]', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('pic.png', true)).toBeDefined(); // resolved + cached

    // FileChanged (e.g. the editor's own auto-save). The cache must STAY
    // populated (not cleared) so a rebuild in the debounce window still sees it.
    fileChangedHandler();
    expect(resolver.resolvedCache.get('pic.png', true)).toBeDefined(); // was undefined pre-fix

    // After the revalidation batch settles, the target is still resolved.
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('pic.png', true)).toBeDefined();
    view.destroy();
    container.remove();
  });

  test('cache is keyed by (text, is_embed) — same text differs by context', async () => {
    // `note` appears as BOTH an embed (![[note]]) and a link ([[note]]) — they
    // classify differently and must occupy DISTINCT cache slots.
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets) =>
      (targets as { text: string; is_embed: boolean }[]).map((t) =>
        t.is_embed
          ? envelope(t.text, true, { kind: 'folder-index-iframe' })
          : envelope(t.text, false, { kind: 'link', data: { anchor: null } }),
      ),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[note]] and [[note]]', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 40));
    const embedEnv = resolver.resolvedCache.get('note', true);
    const linkEnv = resolver.resolvedCache.get('note', false);
    expect(embedEnv?.kind.kind).toBe('folder-index-iframe');
    expect(linkEnv?.kind.kind).toBe('link');
    expect(resolver.resolvedCache.has('note', true)).toBe(true);
    expect(resolver.resolvedCache.has('note', false)).toBe(true);
    view.destroy();
    container.remove();
  });
});

// ── unresolved decoration reads the unified cache ──────────

/** Collect the class names of every decoration mark in the view, in order. */
function decoClasses(view: EditorView): string[] {
  const out: string[] = [];
  const set = view.state.facet(EditorView.decorations);
  for (const provider of set) {
    const decos = typeof provider === 'function' ? provider(view) : provider;
    const it = decos.iter();
    while (it.value) {
      const cls = (it.value.spec as any)?.class;
      if (typeof cls === 'string') out.push(cls);
      it.next();
    }
  }
  return out;
}

/** Build an envelope carrying a resolved ResolvedAsset (image kind). */
function resolvedImageEnvelope(target: string): EditorReferenceResolution {
  return {
    target,
    is_embed: true,
    kind: { kind: 'image' },
    resolved: {
      absolute_path: `/site/${target}`,
      request_url: `/${target}`,
      mime_type: 'image/png',
      width: 800,
      height: 600,
      size_bytes: 204800,
      provenance: 'literal' as any,
      rung_widths: [],
      offers_original: false,
    },
    url: null,
    anchor: null,
    resolved_path: null,
    message: null,
    candidates: [],
    folder: null,
  };
}

// The resolver owns no marks; the problem field reads its cache and re-derives
// when a batch lands. End to end: a real batch, then the mark.
describe('a landed batch marks a missing file through the problem field', () => {
  function decoFor(doc: string, batch: BatchResolveReferencesFn): Promise<{ view: EditorView; container: HTMLElement }> {
    const resolver = createReferenceResolver({ batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 10 });
    const problems = problemsExtension({ resolvedCache: resolver.resolvedCache, refForPath: async () => null, projectRoot: () => null });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView(doc, [...resolver.extension, problems], container);
    return new Promise((r) => setTimeout(() => r({ view, container }), 40));
  }

  test('not-found embed away from the cursor → red problem mark, and no words on the mark itself', async () => {
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets) =>
      (targets as { text: string; is_embed: boolean }[]).map((t) =>
        envelope(t.text, t.is_embed, { kind: 'not-found' }),
      ),
    );
    const { view, container } = await decoFor('prose\n![[missing.png]]', batch);
    expect(decoClasses(view)).toContain('cm-problem cm-problem--blocks');
    // The card is the one place with words; a tooltip on the mark would be a second.
    expect(container.querySelector('.cm-problem')?.hasAttribute('data-tooltip')).toBe(false);
    view.destroy();
    container.remove();
  });

  test('resolved image → no problem mark', async () => {
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets) =>
      (targets as { text: string; is_embed: boolean }[]).map((t) => resolvedImageEnvelope(t.text)),
    );
    const { view, container } = await decoFor('![alt](photo.png)', batch);
    expect(decoClasses(view).some((c) => c.includes('cm-problem'))).toBe(false);
    view.destroy();
    container.remove();
  });
});


describe('createReferenceResolver — SourceAssetChanged → image repaint', () => {
  test('SourceAssetChanged bumps the per-asset bust for that path', async () => {
    __resetAssetBustForTests();
    const cbs: Record<string, Array<(m: any) => void>> = {};
    const onMossEvent = vi.fn(async (kind: string, cb: (m: any) => void) => {
      (cbs[kind] ??= []).push(cb);
      return () => {};
    });
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      onMossEvent: onMossEvent as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    expect(assetBustQuery('/assets/x.jpg')).toBe('');
    expect(cbs['SourceAssetChanged']?.length ?? 0).toBeGreaterThan(0);
    cbs['SourceAssetChanged'][0]({ payload: { path: '/assets/x.jpg' } });

    expect(assetBustQuery('/assets/x.jpg')).toMatch(/^\?_t=\d+$/);
    view.destroy();
    container.remove();
  });

  test('SourceAssetChanged with an empty path is ignored (no spurious bust)', async () => {
    __resetAssetBustForTests();
    const cbs: Record<string, Array<(m: any) => void>> = {};
    const onMossEvent = vi.fn(async (kind: string, cb: (m: any) => void) => {
      (cbs[kind] ??= []).push(cb);
      return () => {};
    });
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({
      batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 10,
      onMossEvent: onMossEvent as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    cbs['SourceAssetChanged'][0]({ payload: { path: '' } });
    expect(assetBustQuery('')).toBe('');
    view.destroy();
    container.remove();
  });

  test('without a coordinator, a promoted build re-resolves too', async () => {
    const cbs: Record<string, Array<(m: any) => void>> = {};
    const onMossEvent = vi.fn(async (kind: string, cb: (m: any) => void) => {
      (cbs[kind] ??= []).push(cb);
      return () => {};
    });
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({
      batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 10,
      onMossEvent: onMossEvent as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);
    await new Promise((r) => setTimeout(r, 40));
    const calls = (batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length;

    cbs['SitePromoted'][0]({ payload: { generation_id: 'g2' } });
    await new Promise((r) => setTimeout(r, 40));

    expect((batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length).toBeGreaterThan(calls);
    view.destroy();
    container.remove();
  });
});

// ── coordinator wiring ────────────────────────────────────

describe('createReferenceResolver — coordinator wiring', () => {
  test('registers FileChanged + SitePromoted + SourceAssetChanged on the injected coordinator', async () => {
    const fc: any[] = [];
    const sp: any[] = [];
    const sac: any[] = [];
    const fileEvents = {
      onFileChanged: (h: any) => { fc.push(h); return () => {}; },
      onSitePromoted: (h: any) => { sp.push(h); return () => {}; },
      onSourceAssetChanged: (h: any) => { sac.push(h); return () => {}; },
    };
    const mockOnMossEvent = vi.fn(); // must NOT be called when fileEvents is provided
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
      onMossEvent: mockOnMossEvent as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    expect(fc.length).toBe(1);
    expect(sp.length).toBe(1);
    expect(sac.length).toBe(1);
    expect(mockOnMossEvent).not.toHaveBeenCalled();

    view.destroy();
    container.remove();
  });

  test('coordinator FileChanged handler revalidates WITHOUT emptying the cache', async () => {
    __resetAssetBustForTests();
    const fc: Array<() => void> = [];
    const fileEvents = {
      onFileChanged: (h: any) => { fc.push(h); return () => {}; },
      onSitePromoted: (_h: any) => () => {},
      onSourceAssetChanged: (_h: any) => () => {},
    };
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets: any[]) =>
      targets.map((t) => envelope(t.text, t.is_embed, { kind: 'image' })),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    // Wait for the initial batch to settle
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);

    // Fire the FileChanged handler — the cache must STAY populated (revalidate,
    // NOT clear), so a rebuild in the debounce window never drops the widget.
    const callsBefore = (batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length;
    fc[0]();
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);
    // ...and it still re-resolves (a fresh batch runs) so changed refs update.
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);
    expect((batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length).toBeGreaterThan(callsBefore);

    view.destroy();
    container.remove();
  });

  test('coordinator SourceAssetChanged handler bumps bust token, revalidates (no empty window), then re-resolves', async () => {
    __resetAssetBustForTests();
    const sac: Array<(e: any) => void> = [];
    const fileEvents = {
      onFileChanged: (_h: any) => () => {},
      onSitePromoted: (_h: any) => () => {},
      onSourceAssetChanged: (h: any) => { sac.push(h); return () => {}; },
    };
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets: any[]) =>
      targets.map((t) => envelope(t.text, t.is_embed, { kind: 'image' })),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    // Wait for the initial batch to settle so cache is populated.
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);

    expect(assetBustQuery('/图片/摄影/X.jpeg')).toBe('');
    // Coordinator passes payload directly (no .payload wrapper)
    sac[0]({ path: '/图片/摄影/X.jpeg' });
    // Bust token must be set for the path.
    expect(assetBustQuery('/图片/摄影/X.jpeg')).toMatch(/^\?_t=\d+$/);
    // The cache must STAY populated (revalidate-and-swap — no empty window).
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);
    // ...then RE-RESOLVED — the handler reschedules a batch (scheduleBatch), NOT
    // a bare refsResolvedEffect dispatch. Without the re-resolve the image would
    // rebuild from the emptied cache and stay detached; the re-batch repopulates
    // the cache so it repaints with the bumped `?_t=`.
    const callsAfterClear = (batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length;
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.has('x.jpg', true)).toBe(true);
    expect((batch as unknown as { mock: { calls: unknown[] } }).mock.calls.length).toBeGreaterThan(callsAfterClear);

    view.destroy();
    container.remove();
  });

  test('external asset move: a FOLDER-path SourceAssetChanged (no image extension) re-resolves a stale envelope', async () => {
    // Moving a folder of assets in the file manager delivers watcher events
    // whose paths are the DIRECTORY, and the watcher's source-asset pass emits
    // SourceAssetChanged for those too
    // (see watch.rs source_asset_request_paths). The resolver must treat that
    // exactly like an image-path event: revalidate-all and re-resolve, so a
    // `![[photo.png]]` that went not-found (or kept a dead moss-source:// URL)
    // recovers without waiting for a rebuild-driven FileChanged that a
    // suppressed output diff may never send.
    __resetAssetBustForTests();
    const sac: Array<(e: any) => void> = [];
    const fileEvents = {
      onFileChanged: (_h: any) => () => {},
      onSitePromoted: (_h: any) => () => {},
      onSourceAssetChanged: (h: any) => { sac.push(h); return () => {}; },
    };
    // Backend state machine: before the "move" the target is broken; after it,
    // the live FS index finds the file at its new location.
    let moved = false;
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets: any[]) =>
      targets.map((t) =>
        envelope(t.text, t.is_embed, moved ? { kind: 'image' } : { kind: 'not-found' }),
      ),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[photo.png]]', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('photo.png', true)?.kind.kind).toBe('not-found');

    // The Finder move lands: the watcher reports the folder rename.
    moved = true;
    sac[0]({ path: '/pics' }); // directory request path — no image extension
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('photo.png', true)?.kind.kind).toBe('image');

    view.destroy();
    container.remove();
  });

  test('a promoted build re-resolves, so an image picks up the rungs it can now be served from', async () => {
    __resetAssetBustForTests();
    const promoted: Array<() => void> = [];
    const fileEvents = {
      onFileChanged: (_h: any) => () => {},
      onSitePromoted: (h: any) => { promoted.push(h); return () => {}; },
      onSourceAssetChanged: (_h: any) => () => {},
    };
    let rungs: number[] = [];
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets: any[]) =>
      targets.map((t) => ({
        ...envelope(t.text, t.is_embed, { kind: 'image' }),
        resolved: { request_url: '/x.jpg', mime_type: 'image/jpeg', width: 4000, height: 3000, rung_widths: rungs },
      })),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('x.jpg', true)?.resolved?.rung_widths).toEqual([]);

    rungs = [800, 1600];
    promoted[0]();
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.get('x.jpg', true)?.resolved?.rung_widths).toEqual([800, 1600]);
    // New rung URLs are new identities already; the source's bust is untouched.
    expect(assetBustQuery('/x.jpg')).toBe('');

    view.destroy();
    container.remove();
  });

  test('destroy() calls coordinator unregister fns', async () => {
    const unregisterFC = vi.fn();
    const unregisterSP = vi.fn();
    const unregisterSAC = vi.fn();
    const fileEvents = {
      onFileChanged: (_h: any) => unregisterFC,
      onSitePromoted: (_h: any) => unregisterSP,
      onSourceAssetChanged: (_h: any) => unregisterSAC,
    };
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
      fileEvents: fileEvents as any,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView('![[x.jpg]]', resolver.extension, container);

    expect(unregisterFC).not.toHaveBeenCalled();
    expect(unregisterSP).not.toHaveBeenCalled();
    expect(unregisterSAC).not.toHaveBeenCalled();
    view.destroy();
    expect(unregisterFC).toHaveBeenCalledTimes(1);
    expect(unregisterSP).toHaveBeenCalledTimes(1);
    expect(unregisterSAC).toHaveBeenCalledTimes(1);

    container.remove();
  });
});

// ── follows incremental parse progress ───────────────────────────────
//
// On a large document the markdown parse is incremental: state creation parses
// only a prefix (Work.InitViewport = 3000 chars), and the parse worker extends
// the tree afterwards via transactions that change neither doc nor selection
// (Language.setState effects). The resolver's startup batch therefore scans an
// INCOMPLETE syntax tree and misses any embed/link beyond the parsed prefix.
// If the resolver only re-batches on docChanged, those trailing targets never
// resolve — so imageBlockField (which DOES rebuild on parse-advance) finds the
// Image node but reads an empty cache and leaves the embed raw until the user
// happens to type. The resolver must re-batch on parse-advance too, the same
// `syntaxTree(state) != syntaxTree(startState)` guard the block fields use.
//
// `forceParsing(view, …)` drives that parse-advance path synchronously — it
// dispatches exactly the effect-only transactions the worker would.
describe('createReferenceResolver — follows incremental parse progress', () => {
  // Far past Work.InitViewport (3000) so the initial time-budgeted parse cannot
  // reach the tail (asserted below so a faster machine can't mask the premise).
  const FILLER = 'lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n'.repeat(600);
  const DOC = FILLER + '![[tail.jpg]]\n';

  test('trailing embed beyond the initial parse resolves after the parse advances — no edit', async () => {
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets: any[]) =>
      targets.map((t) => envelope(t.text, t.is_embed, { kind: 'image' })),
    );
    const resolver = createReferenceResolver({
      batchResolve: batch,
      getFromFile: () => '/x/p.md',
      debounceMs: 10,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = makeView(DOC, resolver.extension, container);

    // Premise: the tail embed is NOT in the initial (time-budgeted) parse.
    expect(syntaxTreeAvailable(view.state, DOC.length)).toBe(false);

    // Let the initial debounced batch fire against the partial tree. The tail
    // embed isn't in the tree yet, so it must NOT be resolved.
    await new Promise((r) => setTimeout(r, 40));
    expect(resolver.resolvedCache.has('tail.jpg', true)).toBe(false);

    // Advance the parse to the end — effect-only transactions, NO doc/selection
    // change (exactly what the background parse worker dispatches on open).
    for (let i = 0; i < 50 && !syntaxTreeAvailable(view.state, DOC.length); i++) {
      forceParsing(view, DOC.length, 2000);
    }
    expect(syntaxTreeAvailable(view.state, DOC.length)).toBe(true);

    // Give the (debounced) re-batch + async resolve a tick to settle.
    await new Promise((r) => setTimeout(r, 40));

    // The resolver re-batched on parse-advance and resolved the now-parsed tail
    // embed WITHOUT any edit. Before the fix this stays false (the resolver
    // ignored the parse-advance transactions).
    expect(resolver.resolvedCache.has('tail.jpg', true)).toBe(true);

    view.destroy();
    container.remove();
  });
});
