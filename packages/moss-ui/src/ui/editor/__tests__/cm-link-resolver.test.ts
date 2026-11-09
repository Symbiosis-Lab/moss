import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { host } from "../../host";
import type { EditorReferenceResolution } from '@symbiosis-lab/moss-ui';

// ---------------------------------------------------------------------------
// Hoisted mocks — vi.mock factories are hoisted above imports, so we must
// use vi.hoisted() for any variables referenced inside them.
// ---------------------------------------------------------------------------
const { mockListen, mockUnlisten, listenCallbacks, fireMossEvent } = vi.hoisted(() => {
  // Fan-out registry: the reference resolver registers TWO 'moss-event'
  // subscriptions (FileChanged + AssetReady), each a kind-filtering wrapper from
  // onMossEvent. Store an ARRAY per event and fire all of them, so the wrapper
  // whose kind matches the payload runs (the others ignore it).
  const listenCallbacks = new Map<string, Function[]>();
  const mockUnlisten = vi.fn();
  const mockListen = vi.fn().mockImplementation((event: string, cb: Function) => {
    const arr = listenCallbacks.get(event) ?? [];
    arr.push(cb);
    listenCallbacks.set(event, arr);
    return Promise.resolve(mockUnlisten);
  });
  const fireMossEvent = (payload: unknown) => {
    for (const cb of listenCallbacks.get('moss-event') ?? []) {
      cb(payload);
    }
  };
  return { mockListen, mockUnlisten, listenCallbacks, fireMossEvent };
});

import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import {
  createReferenceResolver,
  type BatchResolveReferencesFn,
} from '../cm-reference-resolver';

// ---------------------------------------------------------------------------
// FileChanged invalidation moved to cm-reference-resolver: it is the
// SINGLE owner of the shared (text, is_embed) cache that the link/asset lint,
// hover, image-render, and cmd-click nav all read. On a FileChanged event it
// clears the cache and re-batches, so a previously-broken reference whose target
// is now created re-resolves. These tests pin that behaviour.
// ---------------------------------------------------------------------------

function makeView(doc: string, resolverExtension: unknown[], container: HTMLElement): EditorView {
  const state = EditorState.create({
    doc,
    extensions: [markdown({ extensions: [wikilinkConfig] }), ...(resolverExtension as any[])],
  });
  return new EditorView({ state, parent: container });
}

/** A link envelope, with kind switching on whether the target now resolves. */
function linkEnv(target: string, found: boolean): EditorReferenceResolution {
  return {
    target,
    is_embed: false,
    kind: found ? { kind: 'link', data: { anchor: null } } : { kind: 'not-found' },
    resolved: null,
    url: found ? `/${target}` : null,
    anchor: null,
    resolved_path: found ? `/project/${target}.md` : null,
    message: null,
    candidates: [],
    folder: null,
  };
}

describe('reference resolver FileChanged invalidation', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    mockListen.mockClear();
    mockUnlisten.mockClear();
    listenCallbacks.clear();
    // The engine's event channel: the subscription hands each event's payload.
    host.engine.on = mockListen;
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('registers a FileChanged listener on construction', async () => {
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({ batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 5 });
    const view = makeView('See [[My Page]] for details.', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 0));
    expect(mockListen).toHaveBeenCalledWith('moss-event', expect.any(Function));
    view.destroy();
  });

  test('FileChanged clears the cache and re-batches so a fixed link re-resolves', async () => {
    let call = 0;
    const batch: BatchResolveReferencesFn = vi.fn().mockImplementation(async (targets) => {
      call++;
      // First batch: not found. Subsequent batches: found.
      return (targets as { text: string; is_embed: boolean }[]).map((t) =>
        linkEnv(t.text, call > 1),
      );
    });
    const resolver = createReferenceResolver({ batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 5 });
    const view = makeView('[[My Page]]', resolver.extension, container);

    // Initial batch lands → not-found.
    await new Promise((r) => setTimeout(r, 30));
    expect(batch).toHaveBeenCalledTimes(1);
    expect(resolver.resolvedCache.get('My Page', false)?.kind.kind).toBe('not-found');

    // Fire a FileChanged event — clears the cache + re-schedules a batch.
    expect(listenCallbacks.get('moss-event')?.length ?? 0).toBeGreaterThan(0);
    fireMossEvent({ kind: 'FileChanged', payload: { changed_output_files: null, deleted_paths: null, moved_output_paths: null, modified_paths: null } });

    await new Promise((r) => setTimeout(r, 30));
    expect(batch).toHaveBeenCalledTimes(2);
    // Re-resolved: the link is now found.
    expect(resolver.resolvedCache.get('My Page', false)?.kind.kind).toBe('link');

    view.destroy();
  });

  test('destroy() calls the unlisten function', async () => {
    const batch: BatchResolveReferencesFn = vi.fn().mockResolvedValue([]);
    const resolver = createReferenceResolver({ batchResolve: batch, getFromFile: () => '/x/p.md', debounceMs: 5 });
    const view = makeView('[[My Page]]', resolver.extension, container);

    await new Promise((r) => setTimeout(r, 0));
    expect(mockUnlisten).not.toHaveBeenCalled();
    view.destroy();
    // Three subscriptions (FileChanged + AssetReady + SourceAssetChanged) → three unlisten calls.
    // Note: the resolver uses the self-subscription fallback path (no fileEvents coordinator),
    // which has 3 direct subscriptions. The EditorFileEvents coordinator itself has 4
    // (adding RawFileCreated), but that is tested separately through EditorFileEvents.start/stop.
    expect(mockUnlisten).toHaveBeenCalledTimes(3);
  });
});
