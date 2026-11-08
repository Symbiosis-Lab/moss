// cm-reference-resolver.ts — the ONE push-resolution adapter.
//
// Batch-resolves BOTH embed targets (image-extract → is_embed:true) and link
// targets (link-extract → is_embed:false) through the single shared Tauri
// command `editorResolveReferences`, which routes every reference through
// moss-core's `classify_reference` — the same kernel the build pipeline uses.
//
// One target string can appear as both an embed AND a link in the same
// document (e.g. `![[note]]` and `[[note]]`), and the SAME text classifies
// differently by context (an embed of a folder is a folder-index-iframe; a
// link to it is a Link). So the cache key is the (text, is_embed) PAIR, not
// the bare text — mirroring the `RefTarget { text, is_embed }` the command
// takes.
//
// Modeled on cm-asset-resolver.ts: a ViewPlugin (100ms debounce + batchResolve)
// populates a shared Map; a `refsResolvedEffect` is dispatched when a batch
// lands so decorations rebuild. It owns NO decorations of its own: the problem
// marks (cm-problems.ts), hover, cmd-click nav and image rendering all read
// its cache.

import { EditorView, ViewPlugin, ViewUpdate } from '@codemirror/view';
import { StateEffect, Extension } from '@codemirror/state';
import type { EditorState } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { extractImageTargets, extractLinkTargets } from '@symbiosis-lab/moss-syntax/cm6';
import type { EditorReferenceResolution, RefTarget } from '@symbiosis-lab/moss-ui';
import { onMossEvent } from "../utils/moss-events";
import { bumpAssetBust } from './asset-bust';
import type { FileEventsCoordinator } from './editor-file-events';
import { engine } from '../engine';

/** Batch resolver signature — overridable for tests. */
export type BatchResolveReferencesFn = (
  targets: RefTarget[],
  fromFile: string,
) => Promise<EditorReferenceResolution[]>;

export type { FileEventsCoordinator };

export interface ReferenceResolverOptions {
  /** Called at resolve time to get the current file path. Pulled live so
   *  resolution survives a cross-directory move without a page reload. */
  getFromFile: () => string;
  debounceMs?: number;
  /** Override for testing — uses the Tauri command by default. */
  batchResolve?: BatchResolveReferencesFn;
  /**
   * Injectable for tests (DI precedent: preview-follower.ts). Defaults to the
   * real cross-webview event bus.
   */
  onMossEvent?: typeof onMossEvent;
  /**
   * Optional EditorFileEvents coordinator. When provided, the resolver registers
   * its FileChanged, SitePromoted and SourceAssetChanged handlers synchronously via the coordinator
   * instead of self-subscribing to the cross-webview bus. This de-risks
   * fallback: if absent, the existing self-subscription path is used unchanged
   * so standalone usage and existing unit tests keep working.
   */
  fileEvents?: FileEventsCoordinator;
}

/** Effect dispatched after a batch lands — consumers rebuild decorations. */
export const refsResolvedEffect = StateEffect.define<void>();

export interface ReferenceResolver {
  extension: Extension[];
  /**
   * Read-only lookup over the (text, is_embed) cache. Returns the always-filled
   * envelope (kind/resolved/url/…) once a batch has resolved this target in
   * this context, else undefined.
   */
  resolvedCache: {
    get(text: string, isEmbed: boolean): EditorReferenceResolution | undefined;
    has(text: string, isEmbed: boolean): boolean;
  };
}

/** Cache key for one (text, is_embed) pair — one target can be both. */
function keyOf(text: string, isEmbed: boolean): string {
  return `${text} ${isEmbed}`;
}

/**
 * Collect the (text, is_embed) targets for the whole document: every image /
 * asset embed (is_embed:true) and every navigable link (is_embed:false).
 *
 * Pure over EditorState — exported for tests.
 */
export function extractReferenceTargets(state: EditorState): RefTarget[] {
  const out: RefTarget[] = [];
  for (const { target } of extractImageTargets(state)) {
    if (target) out.push({ text: target, is_embed: true });
  }
  for (const { target } of extractLinkTargets(state)) {
    if (target) out.push({ text: target, is_embed: false });
  }
  return out;
}

export function createReferenceResolver(opts: ReferenceResolverOptions): ReferenceResolver {
  const batchResolve: BatchResolveReferencesFn =
    opts.batchResolve ??
    (async (targets, fromFile) => {
      const result = await engine.editorResolveReferences(targets, fromFile);
      if (result.status !== 'ok' || !Array.isArray(result.data)) return [];
      return result.data;
    });

  const debounceMs = opts.debounceMs ?? 100;
  const subscribe = opts.onMossEvent ?? onMossEvent;

  // Shared cache keyed by (text, is_embed).
  const internalCache = new Map<string, EditorReferenceResolution>();

  const resolvedCache = {
    get: (text: string, isEmbed: boolean): EditorReferenceResolution | undefined =>
      internalCache.get(keyOf(text, isEmbed)),
    has: (text: string, isEmbed: boolean): boolean =>
      internalCache.has(keyOf(text, isEmbed)),
  };

  // ── Batch-resolve ViewPlugin ───────────────────────────────────────────────
  let pendingTimer: ReturnType<typeof setTimeout> | null = null;
  let inFlight = false;
  // A revalidation (FileChanged / SitePromoted / SourceAssetChanged) resolves ALL current
  // targets and swaps the cache wholesale, vs a normal typing re-batch that only
  // fills UNCACHED targets. Set by the invalidation handlers, consumed by the
  // next runBatch. `revalidatePending` re-arms it if an invalidation lands while
  // a batch is already in flight, so an invalidation is never dropped.
  let revalidateAll = false;
  let revalidatePending = false;

  const plugin = ViewPlugin.fromClass(
    class {
      // FileChanged invalidation: when a file on disk changes (e.g. the user
      // creates/renames the target of a previously-broken reference), the cached
      // envelopes can go stale. Clear the whole cache and re-batch so EVERY
      // consumer reading this shared cache (image render, problem marks, hover,
      // cmd-click nav) sees fresh resolutions. The re-batch dispatches
      // `refsResolvedEffect`, which the problem field re-derives on.
      //
      // SourceAssetChanged: the watcher detected a real source-image change
      // (create/modify/delete/rename) BEFORE the rebuild-eligibility gate.
      // Bump the per-asset cache-bust token so the `?_t=<token>` in the resolved
      // URL changes → ImageBlockWidget.eq() sees a new widget → fresh <img> fetch.
      // Also clear the resolved cache so a deleted/renamed source re-resolves to
      // not-found instead of returning a stale envelope.
      //
      // SitePromoted: a new build is current, so an image's `rung_widths` can
      // have changed (a rung the server now vouches for, or none after an
      // edit). Earlier events cannot carry this: FileChanged diffs before the
      // background encoders finish, and AssetsSettled precedes the promotion
      // the server reads rungs from. An image whose rungs did not move keeps its
      // `srcset`, so its widget keeps its DOM.
      private unlistenFileChanged: (() => void) | null = null;
      private unlistenSitePromoted: (() => void) | null = null;
      private unlistenSourceAssetChanged: (() => void) | null = null;
      // Used by the fallback self-subscription path (no coordinator injected).
      private pending: Array<Promise<() => void>> = [];
      // True when this instance is wired through the coordinator (not self-subscribed).
      private usingCoordinator = false;

      constructor(view: EditorView) {
        this.scheduleBatch(view);

        if (opts.fileEvents) {
          // ── Coordinator path (synchronous registration) ──────────────────
          // All handlers are registered synchronously; unregister fns are stored
          // immediately. No promise-based teardown race to handle.
          this.usingCoordinator = true;
          this.unlistenFileChanged = opts.fileEvents.onFileChanged(() => {
            // Revalidate (resolve-all + synchronous swap), do NOT clear now — the
            // clear-then-async-refill window let a keystroke rebuild against an
            // empty cache and drop every image widget (the "CM6 shifted while
            // editing" churn from the editor's own auto-save FileChanged).
            revalidateAll = true;
            this.scheduleBatch(view);
          });
          this.unlistenSitePromoted = opts.fileEvents.onSitePromoted(() => {
            revalidateAll = true;
            this.scheduleBatch(view);
          });
          this.unlistenSourceAssetChanged = opts.fileEvents.onSourceAssetChanged((e) => {
            const path = e.path;
            if (!path) return;
            // Bump cache-bust so `?_t=<token>` changes → the changed image's URL
            // differs → ImageBlockWidget.eq() sees a new widget → fresh <img>.
            bumpAssetBust(path);
            // Revalidate-all + synchronous swap (not clear-then-async-refill): a
            // deleted/renamed source re-resolves to not-found and the modified
            // image re-renders with the bumped `?_t=`, while unchanged images keep
            // their URL and their widget DOM (no destroy/recreate flash).
            revalidateAll = true;
            this.scheduleBatch(view);
          });
        } else {
          // ── Self-subscription fallback path (promise-based) ───────────────
          // Kept exactly as before so standalone use + existing unit tests work.
          // FileChanged: a file appeared/renamed/deleted — cached envelopes can go
          // stale. Clear the whole cache and re-batch.
          const p1 = subscribe('FileChanged', () => {
            revalidateAll = true;
            this.scheduleBatch(view);
          });
          p1.then((u) => { this.unlistenFileChanged = u; });
          const p2 = subscribe('SitePromoted', () => {
            revalidateAll = true;
            this.scheduleBatch(view);
          });
          p2.then((u) => { this.unlistenSitePromoted = u; });
          // SourceAssetChanged: bump bust + clear cache + RE-RESOLVE (see the
          // coordinator path above — scheduleBatch, not a bare dispatch, so the
          // image repaints with the bumped `?_t=` instead of detaching).
          const p3 = subscribe('SourceAssetChanged', (msg: { payload?: { path?: string } }) => {
            const path = msg.payload?.path;
            if (!path) return;
            bumpAssetBust(path);
            revalidateAll = true;
            this.scheduleBatch(view);
          });
          p3.then((u) => { this.unlistenSourceAssetChanged = u; });
          this.pending = [p1, p2, p3];
        }
      }
      update(u: ViewUpdate) {
        // Re-batch on doc edits AND when the incremental parse ADVANCED. On a
        // large document the markdown parse is incremental: state creation
        // parses only a prefix and the parse worker extends the tree afterwards
        // via effect-only transactions (Language.setState — no docChanged). A
        // resolver gated on docChanged alone would scan an incomplete tree once
        // at startup and miss every embed/link beyond the parsed prefix, so a
        // trailing image stays raw until the user happens to type. The cheap
        // reference comparison picks up those parse-advance transactions — the
        // SAME guard imageBlockField/tableBlockField use to follow the parse.
        const treeAdvanced = syntaxTree(u.state) != syntaxTree(u.startState);
        if (u.docChanged || treeAdvanced) this.scheduleBatch(u.view);
      }
      destroy() {
        if (pendingTimer) { clearTimeout(pendingTimer); pendingTimer = null; }
        if (this.usingCoordinator) {
          // Coordinator path: unregister fns are synchronous, call directly.
          this.unlistenFileChanged?.();
          this.unlistenSitePromoted?.();
          this.unlistenSourceAssetChanged?.();
        } else {
          // Self-subscription fallback path: may race with a pending promise.
          this.unlistenFileChanged?.();
          this.unlistenSitePromoted?.();
          this.unlistenSourceAssetChanged?.();
          // If destroy runs before a subscribe() promise resolved, unlisten as
          // soon as it does — only for the slot that is still pending, so we never
          // call an already-resolved unlisten twice.
          if (!this.unlistenFileChanged) this.pending[0]?.then((u) => u());
          if (!this.unlistenSitePromoted) this.pending[1]?.then((u) => u());
          if (!this.unlistenSourceAssetChanged) this.pending[2]?.then((u) => u());
        }
        this.unlistenFileChanged = null;
        this.unlistenSitePromoted = null;
        this.unlistenSourceAssetChanged = null;
        this.pending = [];
      }
      scheduleBatch(view: EditorView) {
        if (pendingTimer) clearTimeout(pendingTimer);
        pendingTimer = setTimeout(() => {
          pendingTimer = null;
          void this.runBatch(view);
        }, debounceMs);
      }
      async runBatch(view: EditorView) {
        if (inFlight) {
          // An invalidation that lands mid-batch must not be dropped — re-arm it
          // so the cache still revalidates once the current batch settles.
          if (revalidateAll) revalidatePending = true;
          return;
        }
        const targets = extractReferenceTargets(view.state);
        const doRevalidate = revalidateAll;
        revalidateAll = false;
        // A normal typing re-batch resolves only UNCACHED pairs (fast). A
        // revalidation resolves ALL current pairs so a changed/deleted source
        // re-resolves correctly. The same pair can recur — dedup by key.
        const wanted = new Map<string, RefTarget>();
        for (const t of targets) {
          const k = keyOf(t.text, t.is_embed);
          if (doRevalidate || !internalCache.has(k)) wanted.set(k, t);
        }
        if (!doRevalidate && wanted.size === 0) return;
        inFlight = true;
        try {
          const list = Array.from(wanted.values());
          const results = list.length ? await batchResolve(list, opts.getFromFile()) : [];
          // Defensive: a non-array result (e.g. an unmocked Tauri invoke
          // resolving undefined in tests) must not crash the resolver — leave
          // the cache untouched and let the next batch retry.
          if (!Array.isArray(results)) return;
          if (doRevalidate) {
            // Build the fresh cache off to the side, then swap it in
            // SYNCHRONOUSLY — internalCache is NEVER observed empty. The old bug
            // cleared it BEFORE the async resolve, so a keystroke in the window
            // rebuilt against an empty cache and dropped every image widget.
            // Unchanged targets resolve to the same URL → their widget DOM is
            // reused (no reload); changed/deleted ones flip correctly.
            const next = new Map<string, EditorReferenceResolution>();
            list.forEach((t, i) => {
              const envelope = results[i];
              if (envelope) next.set(keyOf(t.text, t.is_embed), envelope);
            });
            internalCache.clear();
            for (const [k, v] of next) internalCache.set(k, v);
          } else {
            list.forEach((t, i) => {
              const envelope = results[i];
              if (envelope) internalCache.set(keyOf(t.text, t.is_embed), envelope);
            });
          }
          view.dispatch({ effects: refsResolvedEffect.of() });
        } finally {
          inFlight = false;
          if (revalidatePending) {
            revalidatePending = false;
            revalidateAll = true;
            this.scheduleBatch(view);
          }
        }
      }
    },
  );

  return {
    extension: [plugin] as Extension[],
    resolvedCache,
  };
}
