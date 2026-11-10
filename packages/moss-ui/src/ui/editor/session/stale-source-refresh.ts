// packages/moss-ui/src/ui/editor/session/stale-source-refresh.ts
//
// Asking again when the answer the editor was given was "this file is still
// downloading": `armForArrival` + `onFilesArrived` retry once bytes land.
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
// and re-asking the page's lookup is the right retry: the arming ends the
// moment a fresh answer is anything other than "still arriving", and the
// lookup's owner delivers that answer to the editor, "this page really is
// generated" included.
//
// A pane the editor raised by opening a PATH is not about the preview's page at
// all, and after a file-tree click the two name different documents. Asking the
// URL there answers about the wrong file twice over: it can push some other
// page's node into the editor, and a torn `article-map.json` can answer
// "generated" for a file that is merely downloading — printing the same
// wrong answer over the pane that exists to prevent it. So a named pane
// re-offers its own file instead, and asks the backend nothing. Who stops it
// is the editor: it is the only half that can see whether the pane is still up.

import type { PendingReason, EditorNodeWire } from './node-class';

export interface StaleSourceRefreshDeps {
  /**
   * Ask the page's lookup again and settle it for the editor; null when the
   * URL is no longer the previewed page or a newer lookup replaced this one.
   */
  refreshPage: (urlPath: string) => Promise<{ pending: PendingReason | null } | null>;
  /** The URL path the preview is showing RIGHT NOW — the currency guard's input. */
  liveUrlPath: () => string;
  /** Is the editor panel the view actually on screen? */
  editorIsActive: () => boolean;
  /** Re-offer a named file to the editor (the `editor:load-source` channel). */
  applyNode: (node: EditorNodeWire) => void;
}

export class StaleSourceRefresh {
  /** The URL path whose pane announced itself pending, or null when disarmed. */
  private armedUrlPath: string | null = null;
  /** The file that pane named, when the editor knew it. See the header. */
  private armedSourcePath: string | null = null;
  /** One resolve at a time: a burst of arrivals must not stack requests. */
  private inFlight = false;

  constructor(private readonly deps: StaleSourceRefreshDeps) {}

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

    if (this.deps.liveUrlPath() !== urlPath) { this.disarm(); return; }
    this.inFlight = true;
    const answer = await this.deps.refreshPage(urlPath);
    this.inFlight = false;
    // Any pending verdict keeps the arming: a failed lookup is not evidence
    // about the file, and treating it as one would strand the pane for good.
    if (!answer?.pending) this.disarm();
  }
}
