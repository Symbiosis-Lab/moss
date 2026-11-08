// packages/moss-ui/src/ui/editor/session/stale-source-refresh.ts
//
// Re-resolving the editor's surface when the answer it was given has gone
// stale — and the answer that goes stale fastest is "this file is still
// downloading".
//
// Two callers, one mechanism, because they are the same bug seen twice: the
// editor mounted a surface from an answer that was true when it was asked and
// false a moment later, and nothing asked again. `refreshStale` covers the
// bake race (the cached source/dir are null because the resolve had not landed
// when the panel opened); `armForArrival` + `onFilesArrived` cover the case
// where the file existed all along but its bytes were still coming down from
// the cloud. The stale-cache half already self-healed; the pending half did
// not.
//
// # Why the arrival signal is injected
//
// The retry has to be driven by something that knows bytes landed, and that is
// the cloud stream — which lives on the shell side (`preview/cloud/
// cloud-sync-store.ts`, "one listener, one state"). `interim-editor-no-shell-
// reach-in` forbids this directory from importing it, and rightly: this module
// should not care WHERE the news comes from. So the shell subscribes and calls
// `onFilesArrived`, and everything here is a fold over injected facts — no
// Tauri import, no store import, testable without a mock of either.
//
// # Why this is not a poll
//
// Nothing here holds a timer. The retry is armed by the pane announcing itself
// and fires only on an edge the cloud actually reported. A file that is
// genuinely absent produces no arrivals of its own, so it is never retried into
// eternity; it costs at most one re-resolve per arrival edge in the episode
// around it, and the in-flight guard collapses a burst into one — for the URL
// arm that means one resolve, and for the named arm one open, which is the
// expensive one.
//
// # Two panes, two identities
//
// A pending pane raised by resolving a preview URL is a claim about THAT URL,
// and re-resolving it is the right retry: the arming ends the moment a fresh
// answer is anything other than "still arriving" — including "this page really
// is generated", which is pushed rather than swallowed, because a pending pane
// left standing over that answer is the inverse of the bug this fixes.
//
// A pane the editor raised by opening a PATH is not about the preview's page at
// all, and after a file-tree click the two name different documents. Asking the
// URL there answers about the wrong file twice over: it can push some other
// page's node into the editor, and a torn `article-map.json` can answer
// "generated" for a file that is merely downloading — printing the same
// wrong answer over the pane that exists to prevent it. So a named pane
// re-offers its own file instead, and asks the backend nothing. Who stops it
// is the editor: it is the only half that can see whether the pane is still up.

import { isRootUrlPath } from "../../utils/url-path";
import type { EditSourceResolution, ResolvedNode } from './node-class';

export interface StaleSourceRefreshDeps {
  /** Fresh disk-truth resolve of a preview URL path (`resolve_page_source`). */
  resolvePageSource: (
    urlPath: string,
  ) => Promise<{ status: 'ok'; data: EditSourceResolution } | { status: 'error'; error: string }>;
  /** The URL path the preview is showing RIGHT NOW — the currency guard's input. */
  liveUrlPath: () => string;
  /** Is the editor panel the view actually on screen? */
  editorIsActive: () => boolean;
  /** Push a corrected node to the editor (the `editor:load-source` channel). */
  applyNode: (node: ResolvedNode) => void;
  warn: (message: string) => void;
}

/** What a re-resolve concluded. Only `pending` keeps the arrival retry armed. */
type Outcome = 'applied' | 'pending' | 'nothing-to-correct' | 'superseded' | 'failed';

export class StaleSourceRefresh {
  /** The URL path whose pane announced itself pending, or null when disarmed. */
  private armedUrlPath: string | null = null;
  /** The file that pane named, when the editor knew it. See the header. */
  private armedSourcePath: string | null = null;
  /** One resolve at a time: a burst of arrivals must not stack requests. */
  private inFlight = false;

  constructor(private readonly deps: StaleSourceRefreshDeps) {}

  /**
   * The stale-cache self-heal, unchanged in behaviour: a background re-resolve
   * for a panel that opened on an ambiguous bake. A confirmed-generated answer
   * (both null) is deliberately NOT pushed here — for a non-root page that is
   * also what a racy echo looks like, and forwarding it would downgrade a live
   * takeover pane to the buttonless generated surface. The pending pane is the one
   * case where the empty answer must be forwarded, and that caller says so.
   */
  async refreshStale(urlPath: string): Promise<void> {
    await this.reresolve(urlPath, { pushEmptyAnswer: false });
  }

  /**
   * The pending pane is up; retry when the cloud reports an arrival. Re-arming
   * is harmless and expected — the editor re-announces on every surface it
   * applies, and the last announcement is the one on screen.
   *
   * `sourcePath` is the file the pane named, when the editor knew it; without
   * one the retry falls back to re-resolving `urlPath`. They are not
   * interchangeable — see "Two panes, two identities" above.
   */
  armForArrival(urlPath: string, sourcePath: string | null = null): void {
    this.armedUrlPath = urlPath;
    this.armedSourcePath = sourcePath;
    // An announcement is the editor answering the offer in flight, whatever it
    // says. Clearing here is what makes the next arrival edge count.
    this.inFlight = false;
  }

  /** The pane is gone (closed, navigated away, replaced). Stop retrying. */
  disarm(): void {
    this.armedUrlPath = null;
    this.armedSourcePath = null;
    this.inFlight = false;
  }

  /**
   * Bytes landed somewhere in the folder. If a pending pane is armed, ask again
   * — the file it is waiting for may be among them.
   *
   * Dropping the edge while a resolve is in flight is the coalescing rule: the
   * answer in flight is at least as fresh as this edge, and if it still says
   * "arriving" the NEXT edge asks again.
   */
  async onFilesArrived(): Promise<void> {
    const urlPath = this.armedUrlPath;
    if (urlPath === null || this.inFlight) return;
    // Asked once, for both branches. The named one used to ask and the URL one
    // did not, so a pending pane whose panel the user closed kept re-resolving
    // on every arrival edge for the rest of the download, with nowhere to put
    // the answer — `pending` returns before the guard further down.
    if (!this.deps.editorIsActive()) { this.disarm(); return; }

    // A pane that named its file re-offers that file and asks nothing: the
    // answer it needs is not "what is at this URL" but "can this file be read
    // yet", and only the editor's own open can settle that. One offer per
    // arrival edge; the editor's same-file guard makes a redundant one a no-op,
    // and a re-open that lands back on the pending pane re-arms this by
    // re-announcing.
    if (this.armedSourcePath !== null) {
      // In flight until the editor ANSWERS — and the answer is the next
      // announcement, which `armForArrival` clears this on. An offer is a full
      // open: `read_to_string_with_materialize_wait` blocks up to
      // INTERACTIVE_DEADLINE (15s) against a 2s arrival tick, so without this
      // seven blocking waits stack up on one file and six get discarded by
      // `loadGeneration`. It is also the terminating condition for an open that
      // fails without reaching a surface — a file deleted or renamed mid-wait
      // announces nothing, and would otherwise draw a "could not open" toast
      // every two seconds for the rest of the sync.
      this.inFlight = true;
      this.deps.applyNode({
        // `isRoot` false is inert for a node with a source path: the surface
        // decision and the root re-resolve both short-circuit on the path, and
        // `editorQueryFromNode` does bake it, but the boot path prefers
        // `source=` over `is_root=` whenever both could be read.
        sourcePath: this.armedSourcePath,
        isRoot: false,
        takeover: null,
      });
      return;
    }

    const outcome = await this.reresolve(urlPath, { pushEmptyAnswer: true });
    // `failed` keeps the arming: a resolve that threw is not evidence about the
    // file, and treating it as one is how a transient backend hiccup would
    // strand the pane for good.
    if (outcome !== 'pending' && outcome !== 'failed') this.disarm();
  }

  private async reresolve(
    urlPath: string,
    opts: { pushEmptyAnswer: boolean },
  ): Promise<Outcome> {
    this.inFlight = true;
    try {
      let result;
      try {
        result = await this.deps.resolvePageSource(urlPath);
      } catch (err) {
        this.deps.warn(
          `[StaleSourceRefresh] resolvePageSource threw: ${err instanceof Error ? err.message : String(err)}`,
        );
        return 'failed';
      }
      if (result.status !== 'ok') return 'failed';
      const { source_path, takeover, source_pending } = result.data;

      // Still coming down: leave the pane exactly as it is. Pushing a node with
      // no source would be read as "nothing to edit here" — the very claim the
      // pending pane exists to stop moss from making.
      if (source_pending || (takeover?.folder_empty === null && (takeover.kind === 'root-home' || takeover.kind === 'folder-home'))) return 'pending';
      if (!source_path && !takeover && !opts.pushEmptyAnswer) return 'nothing-to-correct';

      // Currency guard: the user may have navigated the preview while this was
      // in flight, and a stale answer must never clobber a fresher surface a
      // genuine navigation already landed.
      if (this.deps.liveUrlPath() !== urlPath) return 'superseded';
      // The panel may have been closed or switched away from in the same window.
      if (!this.deps.editorIsActive()) return 'superseded';

      this.deps.applyNode({
        sourcePath: source_path,
        isRoot: isRootUrlPath(urlPath),
        takeover: takeover ?? null,
      });
      return 'applied';
    } finally {
      this.inFlight = false;
    }
  }
}
