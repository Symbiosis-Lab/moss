import type { EntryRegistry } from './entry-registry';
import type { SourceRole } from '@symbiosis-lab/moss-ui';
import { toAbsolute } from './path-domain';
import type { EngineResult } from '@symbiosis-lab/moss-ui';
import {
  previewProspect,
  isStructurallyPermanentMiss,
  type PreviewNoteReason,
} from './preview-prospect';
import { createGenerationToken, type GenerationToken } from "../shared/utils/generation-token";
import { engine } from '../engine';

/**
 * Editor→Preview navigation subscriber.
 *
 * Single owner of the decision "should the preview iframe navigate?". Sources:
 *   1. Registry's active-document-path signal (subscribeActivePath).
 *      Fires on tree click (loadFile → setActive) and on rename of active.
 *   2. Public navigate(path) method. Called by editor-main's
 *      editor:file-selected listener so that re-clicking the same file
 *      still re-emits navigate-preview (the 404-escape).
 *
 * Action: subscribe to FileChanged FIRST (unless the path is a structurally
 * permanent miss — see preview-follower-permanence.ts), THEN resolve URL via
 * engine.resolveUrlForFile.
 *   - Ok(Some(url)) → unsubscribe (no longer needed) and emit
 *     editor:navigate-preview { urlPath }.
 *   - Ok(None) → leave the subscription in place; retry on every FileChanged
 *     until URL resolves OR active path supersedes (generation increments).
 *     Never falls back to '/' — that fallback creates a feedback loop
 *     through the Preview→Editor sync that re-loads the home file.
 * Subscribing BEFORE the resolve attempt (rather than only after it comes
 * back empty) closes a race: a brand-new file's creation triggers exactly
 * one rebuild independently of the frontend, and that rebuild's FileChanged
 * can arrive while the initial resolve call is still in flight. A
 * register-after-check subscription would miss that one event and — absent
 * a second edit to trigger another rebuild — never recover.
 *
 * Retry on FileChanged, NOT BuildComplete: BuildComplete is emitted ONLY by
 * the video-conversion pipeline (crates/moss-build/src/build/media/video.rs); a plain
 * content rebuild (markdown rename, no videos) never emits it, so waiting
 * on it would leave the preview at the old 404 URL forever.
 * FileChanged is emitted once per rebuild cycle AFTER output is written
 * (crates/moss-build/src/ops/watch.rs, via the AppHandle so it reaches the
 * action-panel webview). Retrying on EVERY FileChanged (not one-shot)
 * tolerates intermediate rebuilds whose output does not yet contain the URL.
 *
 * Pattern: Hugo `hugo server --navigateToChanged` (post-build resolution +
 * livereload navigate).
 */

type ResolveUrlForFile = (filePath: string) => Promise<EngineResult<string | null>>;
/** Resolves what a source file becomes on the built site. The generated
 *  `SourceRole` is used verbatim — a hand-restated shape here silently lost the
 *  field that a later backend answer added. */
type DescribeSourceRole = (filePath: string) => Promise<EngineResult<SourceRole>>;
type EmitFn = (event: string, payload: unknown) => Promise<void>;
type RebuildHandler = (msg: { kind: 'FileChanged' }) => void;
type OnMossEvent = (kind: 'FileChanged', handler: RebuildHandler) => Promise<() => void>;

export interface PreviewFollowerOptions {
  registry: EntryRegistry;
  projectRoot: string;
  resolveUrlForFile: ResolveUrlForFile;
  emit: EmitFn;
  onMossEvent: OnMossEvent;
  /** Optional: when omitted, slot files are treated as ordinary transient
   *  misses (the pre-slot-awareness behavior). Injected so tests can drive it. */
  describeSourceRole?: DescribeSourceRole;
  /** Writes the line explaining why the preview stayed put. The follower is
   *  its only writer — see `preview-note.ts`. */
  setNote?: (reason: PreviewNoteReason | null) => void;
}

export class PreviewFollower {
  private readonly registry: EntryRegistry;
  private readonly projectRoot: string;
  private readonly resolveUrlForFile: ResolveUrlForFile;
  private readonly emit: EmitFn;
  private readonly onMossEvent: OnMossEvent;
  private readonly describeSourceRole: DescribeSourceRole | null;
  private readonly setNote: (reason: PreviewNoteReason | null) => void;

  private unsubRegistry: (() => void) | null = null;

  /** Pending FileChanged callbacks capture this and compare on return, to
   *  short-circuit when a newer path change has superseded them — prevents
   *  stale callbacks from emitting old URLs. See generation-token.ts. */
  private readonly gen: GenerationToken = createGenerationToken();
  /** Cleanup for any in-flight FileChanged subscription. */
  private pendingBuildUnsub: (() => void) | null = null;

  constructor(opts: PreviewFollowerOptions) {
    this.registry = opts.registry;
    this.projectRoot = opts.projectRoot;
    this.resolveUrlForFile = opts.resolveUrlForFile;
    this.emit = opts.emit;
    this.onMossEvent = opts.onMossEvent;
    this.describeSourceRole = opts.describeSourceRole ?? null;
    this.setNote = opts.setNote ?? (() => {});
  }

  start(): void {
    if (this.unsubRegistry !== null) return;
    this.unsubRegistry = this.registry.subscribeActivePath((newPath) => {
      this.onPathChanged(newPath);
    });
    // Initial sync: if the registry already has an active path when we
    // start, resolve+emit for it. Without this, the follower would silently
    // wait for the next setActive/rename even though the editor already
    // has a file open. subscribeActivePath does not replay current state
    // on subscribe (it only fires on change), so we must read it explicitly.
    //
    // The initial-sync branch (firing onPathChanged for the already-active
    // entry) is dead code in editor-main's current boot path (start() runs
    // before any setActive). It exists for future re-mount scenarios and is
    // pinned by `start() emits for an already-active entry` in
    // preview-follower.test.ts.
    const activeId = this.registry.activeEntryId;
    if (activeId !== null) {
      const record = this.registry.getById(activeId);
      const currentPath = record?.path ?? null;
      if (currentPath !== null) {
        this.onPathChanged(currentPath);
      }
    }
  }

  stop(): void {
    this.unsubRegistry?.();
    this.unsubRegistry = null;
    this.pendingBuildUnsub?.();
    this.pendingBuildUnsub = null;
  }

  /**
   * Force re-resolution + navigate for `path`. Used by the editor:file-selected
   * handler so re-clicking the active file in the tree still re-emits
   * navigate-preview (404-escape behavior).
   *
   * `path` must be a file the editor has actually opened. This method takes the
   * caller's word for that — it has no view of the editor's document — so the
   * caller is responsible for not asking it to follow somewhere the editor did
   * not go. editor-main gates the call on `loadFile()` returning `'opened'`.
   */
  async navigate(path: string, expectedUrlForFile?: ResolveUrlForFile): Promise<void> {
    const gen = this.bumpGeneration(path);
    await this.attempt('navigate', () => this.resolveAndEmit(gen, path, expectedUrlForFile));
  }

  /**
   * Run one navigation step so that it can never reject.
   *
   * Nobody is waiting for the answer. The registry subscriber below, the
   * FileChanged listener, and editor-main's own `void previewFollower?.navigate(…)`
   * all start these and walk away — so a throw from anywhere inside has no
   * caller left to catch it and becomes an unhandled rejection: invisible in
   * the app, and in tests an error charged to whichever unrelated test happened
   * to be running when the promise finally settled.
   *
   * Following the preview is best-effort by design — every ordinary dead end in
   * here (no URL yet, resolve failed, superseded) already ends by leaving the
   * preview where it is. An unexpected throw is the same outcome, so it is
   * reported the same way the expected ones are: a warning, and no navigation.
   */
  private async attempt(what: string, step: () => Promise<void>): Promise<void> {
    try {
      await step();
    } catch (e) {
      console.warn(`[PreviewFollower] ${what} aborted:`, e);
    }
  }

  private onPathChanged(newPath: string | null): void {
    const gen = this.bumpGeneration(newPath);
    if (newPath === null) return;
    void this.attempt('path change', () => this.resolveAndEmit(gen, newPath));
  }

  /** Advance the generation and clear any in-flight build wait. Called from
   *  both the subscriber and the public navigate(). Returns the new token,
   *  captured immediately by the caller's async step. */
  private bumpGeneration(_newPath: string | null): number {
    const gen = this.gen.bump();
    this.pendingBuildUnsub?.();
    this.pendingBuildUnsub = null;
    return gen;
  }

  private async resolveAndEmit(gen: number, path: string, expectedUrlForFile?: ResolveUrlForFile): Promise<void> {
    // Distinguish a *transient* miss (new/renamed content file, not built
    // yet) from a *structurally-permanent* one that can never produce a page
    // URL. Permanent misses must NOT spin the FileChanged retry loop forever, and
    // they are the ones that owe the author an on-screen explanation.
    //
    // Two of the four answers are path facts. The other two are Rust's: a slot
    // file (`footer.md`) depends on moss_core's language-tree list, and
    // "is this extension a page at all" is `scan::classify::is_page_source`. Both ride on
    // `describe_source_role` rather than being restated here — that copy is the
    // Rust↔TS parity trap. The probe is skipped entirely when the path alone
    // settles it, so no round trip is added for assets or `.moss/` inputs.
    let prospect = previewProspect(path, null);
    if (prospect === 'page' && this.describeSourceRole) {
      try {
        const role = await this.describeSourceRole(toAbsolute(path, this.projectRoot));
        if (this.gen.isStale(gen)) return;
        if (role.status === 'ok') prospect = previewProspect(path, role.data);
        if (role.status === 'ok' && role.data.slot) {
          // No URL to navigate to, so ask the shell to reveal the footer on
          // whatever page it can. The shell owns this decision because only it
          // knows what the preview is currently showing. Fire-and-forget: a
          // reveal that fails must not stop the navigation logic below (which
          // correctly resolves to nothing and leaves the preview alone).
          await this.emit('editor:reveal-slot', { language: role.data.language });
          if (this.gen.isStale(gen)) return;
        }
      } catch (e) {
        // Treat an unavailable answer as "not a slot file" — the pre-existing
        // retry behavior. A failed probe must never suppress navigation.
        console.warn('[PreviewFollower] describeSourceRole threw:', e);
      }
    }

    // Subscribe to FileChanged BEFORE attempting the resolve (not after a
    // failed attempt). A brand-new file's creation triggers exactly one
    // rebuild independently of the frontend; that rebuild's FileChanged can
    // arrive while THIS resolve call (or, for the caller that kicked off
    // this whole path change, the several awaited RPCs before it) is still
    // in flight. A register-after-check subscription misses that one event
    // and — if the user hasn't edited anything else yet to trigger a second
    // rebuild — never recovers. Subscribing first closes the gap: any
    // FileChanged from this point on is heard, whether or not this resolve
    // attempt itself succeeds.
    // The note is a pure function of the prospect: `page` and `asset` both
    // mean "a build can still put this on screen", and neither earns a line.
    this.setNote(
      prospect === 'site-wide' || prospect === 'not-a-page' ? prospect : null,
    );

    const permanentMiss = isStructurallyPermanentMiss(prospect);
    let unsub: (() => void) | null = null;
    if (!permanentMiss) {
      unsub = await this.onMossEvent('FileChanged', () => {
        if (this.gen.isStale(gen)) return;
        void this.attempt('build retry', () => this.retryAfterBuild(gen, path));
      });
      if (this.gen.isStale(gen)) {
        unsub();
        return;
      }
      this.pendingBuildUnsub?.();
      this.pendingBuildUnsub = unsub;
    }

    let result: EngineResult<string | null>;
    try {
      result = await this.resolveUrlForFile(toAbsolute(path, this.projectRoot));
    } catch (e) {
      console.warn('[PreviewFollower] resolveUrlForFile threw:', e);
      if (unsub && this.pendingBuildUnsub === unsub) {
        unsub();
        this.pendingBuildUnsub = null;
      }
      return;
    }
    if (this.gen.isStale(gen)) return;
    if (result.status !== 'ok') {
      console.warn('[PreviewFollower] resolveUrlForFile failed:', result.error);
      if (unsub && this.pendingBuildUnsub === unsub) {
        unsub();
        this.pendingBuildUnsub = null;
      }
      return;
    }
    if (result.data) {
      // Resolved on this attempt. If a subscription was registered above, a
      // FileChanged that arrived while this call was in flight may have
      // ALREADY fired retryAfterBuild concurrently, which resolves, unsubs,
      // and emits on its own. `pendingBuildUnsub` is the single source of
      // truth for "who owns this subscription right now": retryAfterBuild's
      // success path clears it unconditionally before emitting, so if it won
      // the race, this call sees it no longer pointing at `unsub` and must
      // NOT emit a second time for the same navigation. (No subscription —
      // permanentMiss was true — means no concurrent retry was possible;
      // always emit in that case, matching the pre-existing behavior.)
      if (unsub) {
        if (this.pendingBuildUnsub !== unsub) {
          return; // lost the race — the concurrent retry already emitted
        }
        unsub();
        this.pendingBuildUnsub = null;
      }
      await this.emit('editor:navigate-preview', { urlPath: result.data });
      return;
    }
    // New pages may navigate before a build, but the canonical answer owns
    // this subscription. If a build resolved while the expected-URL RPC was
    // in flight, never overwrite its navigation with the guessed route.
    if (expectedUrlForFile && unsub) {
      const expected = await expectedUrlForFile(toAbsolute(path, this.projectRoot));
      if (this.gen.isStale(gen) || this.pendingBuildUnsub !== unsub) return;
      if (expected.status === 'ok' && expected.data) {
        await this.emit('editor:navigate-preview', {
          urlPath: expected.data, awaitingBuild: true,
        });
      }
    }
    // Not yet resolvable. Permanent misses never subscribed (matches prior
    // behavior — no retry loop for assets/.moss paths). Transient misses are
    // already subscribed from above; retryAfterBuild fires from that
    // listener on the next FileChanged.
    console.debug(
      `[PreviewFollower] no preview URL yet for ${path} — ` +
        (permanentMiss ? `reason: ${prospect} (not retrying)` : 'reason: not built yet (retrying on FileChanged)'),
    );
  }

  private async retryAfterBuild(gen: number, path: string): Promise<void> {
    let result: EngineResult<string | null>;
    try {
      result = await this.resolveUrlForFile(toAbsolute(path, this.projectRoot));
    } catch (e) {
      console.warn('[PreviewFollower] resolveUrlForFile threw on retry:', e);
      return;
    }
    if (this.gen.isStale(gen)) return;
    if (result.status !== 'ok' || !result.data) return;
    // Got URL. Unsubscribe from FileChanged (we're done) and emit.
    this.pendingBuildUnsub?.();
    this.pendingBuildUnsub = null;
    await this.emit('editor:navigate-preview', { urlPath: result.data });
  }
}
