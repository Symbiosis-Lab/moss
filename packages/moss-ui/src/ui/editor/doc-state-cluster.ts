// The bottom band's left slot: undo, redo, and the save status badge.
//
// WHY IT IS HERE AND NOT SOMEWHERE MORE OBVIOUS
//
// Not the floating insert bar. That bar is context-adaptive — it hides when the
// editor is unfocused and swaps pages based on what the caret touches — while
// undo is a whole-document operation whose availability has nothing to do with
// the selection. A control that comes and goes with the caret cannot answer
// "is there anything left to undo".
//
// Not the titlebar. `.nav-right` shares its row with the breadcrumb, and the
// breadcrumb is not decoration: the spine allocator's offsets feed the file
// tree's row indentation, so every pixel reserved there is a pixel of legible
// path lost at the 390px floor. Two more buttons cost another rung of the tree.
//
// So: a stationary cluster at the editor's bottom-left, holding the three
// things that are document-level and stable across file switches.
import { t, onLocaleChange } from "../i18n/index";
import { kbdLabel, UNDO_KEY, REDO_KEY } from "../utils/kbd-label";
import { editorBand } from './editor-band';

/** Lucide `undo-2` / `redo-2`, at the insert bar's 16px chip size. */
const ICON_UNDO =
  '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5v0a5.5 5.5 0 0 1-5.5 5.5H11"/></svg>';
const ICON_REDO =
  '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5v0A5.5 5.5 0 0 0 9.5 20H13"/></svg>';

export interface DocStateClusterHooks {
  undo(): void;
  redo(): void;
  canUndo(): boolean;
  canRedo(): boolean;
  /**
   * Is a surface open that would COMMIT on an outside click, turning this
   * gesture into "flush, then undo the flush"? Only such a surface earns a
   * `stopPropagation` from these buttons — see `button()`.
   */
  holdsUncommittedOverlay(): boolean;
}

/**
 * Owns the cluster element. Mounted once, on the first edit of the session, and
 * then kept for the life of the editor webview.
 *
 * Not unmounted on file switch, deliberately. The save badge it hosts is the
 * one piece of editor chrome that must outlive a document: a failed save leaves
 * a badge that has to stay legible while the user goes and looks at something
 * else. That is the same reason the anchor was in the titlebar before — the
 * chip bar was rejected for it, being cleared per file — and a cluster that
 * came and went with the undo stack would have reintroduced exactly that.
 */
export class DocStateCluster {
  private el: HTMLElement | null = null;
  private host: HTMLElement | null = null;
  private undoBtn: HTMLButtonElement | null = null;
  private redoBtn: HTMLButtonElement | null = null;
  private localeUnsub: () => void = () => {};
  private observer: MutationObserver | null = null;

  constructor(private hooks: DocStateClusterHooks) {}

  get mounted(): boolean {
    return this.el !== null;
  }

  /** Idempotent: the first edit mounts it, every later one just syncs. */
  mount(host: HTMLElement): void {
    const hostChanged = this.host !== host;
    this.host = host;
    if (this.el) {
      // A new pane needs the cluster AND a new watcher, and both halves are
      // easy to miss. `ensureMounted` early-returns while `el.isConnected` is
      // true, so it cannot perform the move — the cluster would sit in the old
      // pane forever. And re-pointing `this.host` without re-pointing the
      // observer leaves the move working once and never again: the first wipe
      // of the new pane detaches the cluster for good, silently, because the
      // guard in ensureMounted returns rather than throwing. Unreachable
      // today, for the same reason and with the same standing as that guard.
      if (hostChanged) {
        editorBand(host).appendChild(this.el);
        this.watch(host);
      }
      this.ensureMounted();
      this.sync();
      return;
    }
    const el = document.createElement('div');
    el.className = 'doc-state-cluster';
    el.id = 'doc-state-cluster';

    // The SAME spec strings the CM6 keymap is registered from, rendered for the
    // platform: on Linux and Windows the keymap is the only path to undo (no
    // Edit menu), so a hard-coded ⌘ would misname a key that really works.
    this.undoBtn = this.button(ICON_UNDO, 'editor.undo', kbdLabel(UNDO_KEY), () =>
      this.hooks.undo(),
    );
    this.redoBtn = this.button(ICON_REDO, 'editor.redo', kbdLabel(REDO_KEY), () =>
      this.hooks.redo(),
    );
    // The two buttons share a wrapper so the dim can fade THEM without fading
    // the save badge beside them — see `.doc-state-controls` in editor.css for
    // why the fade cannot go on the cluster or on each button.
    const controls = document.createElement('div');
    controls.className = 'doc-state-controls';
    controls.appendChild(this.undoBtn);
    controls.appendChild(this.redoBtn);
    el.appendChild(controls);

    // The save badge moves here from the titlebar. inline-status.ts finds it by
    // attribute and falls back to a floating badge if it is absent, so moving
    // the anchor is the whole change — nothing else needs to know.
    const status = document.getElementById('moss-save-status');
    if (status) el.appendChild(status);

    editorBand(host).appendChild(el);
    this.el = el;
    this.watch(host);
    this.localeUnsub = onLocaleChange(() => this.relabel());
    this.relabel();
    this.sync();
  }

  /**
   * Watch `host` for anything that empties it — see ensureMounted. The callback
   * runs as a microtask, so the cluster is back before the frame is painted and
   * nothing flickers. Re-appending re-triggers the observer, but by then
   * `isConnected` is true and ensureMounted returns without acting.
   */
  private watch(host: HTMLElement): void {
    this.observer?.disconnect();
    this.observer = new MutationObserver(() => this.ensureMounted());
    this.observer.observe(host, { childList: true });
  }

  private button(icon: string, key: 'editor.undo' | 'editor.redo', kbd: string, run: () => void) {
    const b = document.createElement('button');
    b.className = 'doc-state-btn';
    b.dataset.i18nTooltip = key;
    b.dataset.tooltipKbd = kbd;
    b.innerHTML = icon;
    // Without preventDefault the click blurs CM6 before the command runs, so
    // the undo lands on a view with no selection and the caret is stranded
    // wherever the document happens to start.
    //
    // stopPropagation is the other half, and it is about the chip EDIT popover
    // specifically: overlay.ts dismisses on a document-level `mousedown`, so
    // without this the popover would close (flushing its edit as a history
    // entry) and the click that follows would immediately undo that entry —
    // while the same gesture on the keyboard only closes the popover.
    // preventDefault does not stop the dismissal; only not reaching `document`
    // does.
    //
    // CONDITIONALLY, though, because `document` is where four other things
    // listen: the unified ＋ dropdown's outside-dismiss, the context menu's, the
    // chip bar's provisional-ghost dismissal, and the file tree's (capture, so
    // unaffected). The first three share `dismiss-outside.ts`, whose ordinary
    // outside-click listener stays in the BUBBLE phase precisely so this
    // stopPropagation keeps meaning what it says; its capture-phase listener
    // fires only for drag-region clicks, which never reach these buttons.
    //
    // Stopping unconditionally would void them: the ＋ dropdown's autofocused
    // search box would stay open across the click, making the route `native`
    // so Undo undoes THE TYPING IN THE FILTER BOX, and a context menu would
    // survive an undo that could delete the very chip it was anchored to.
    b.addEventListener('mousedown', (e) => {
      e.preventDefault();
      if (this.hooks.holdsUncommittedOverlay()) e.stopPropagation();
    });
    b.addEventListener('click', () => {
      run();
      this.sync();
    });
    return b;
  }

  private relabel(): void {
    for (const b of [this.undoBtn, this.redoBtn]) {
      if (!b) continue;
      const label = t(b.dataset.i18nTooltip as 'editor.undo');
      b.dataset.tooltip = label;
      // The tooltip is a hover affordance; the accessible name has to be on the
      // element itself or the button reads as "button" to a screen reader.
      b.setAttribute('aria-label', label);
    }
  }

  /**
   * Put the cluster back if the pane was emptied under it.
   *
   * WHY THIS IS DRIVEN BY AN OBSERVER AND NOT BY CALL SITES
   *
   * Every surface mount does `#moss-editor-cm.innerHTML = ''` — the documented
   * contract for dismissing the cold-start skeleton — so the cluster cannot opt
   * out of being detached. Detaching destroys nothing: `this.el` still holds the
   * subtree, `#moss-save-status` included, so re-appending restores a badge that
   * was already showing.
   *
   * A re-attach after each wipe in editor-main cannot work: the wipe is not the
   * last thing that empties the pane — the renderer invoked on the NEXT line
   * empties it again (`file-viewer.ts` and `folder-mode.ts` both open by
   * clearing the container they are handed).
   *
   * "Anyone who mounts a surface must remember to put the cluster back" is a
   * rule with an unbounded number of call sites and no way to check it locally. Watching the pane needs no cooperation: whoever
   * empties it, however they empty it, in whatever module, the cluster returns
   * on the same microtask.
   *
   * The one surface that wants the pane to STAY empty is the email composer,
   * and it is safe only because it is entered from `init()` before any file is
   * loaded — so no edit has happened, the cluster has never mounted, and there
   * is no observer. A composer reachable from an open document would need an
   * explicit opt-out.
   */
  ensureMounted(): void {
    // `host.isConnected` is the loop guard, not a nicety. If the pane itself
    // were ever detached, re-appending could never make the cluster connected,
    // so each append would re-trigger the observer forever — and a microtask
    // loop starves the timer queue, so nothing times out and the webview simply
    // freezes. Unreachable today (#moss-editor-cm is static in editor.html and
    // only its children are replaced), which is exactly why it needs the clause
    // rather than a comment.
    if (!this.el || this.el.isConnected || !this.host?.isConnected) return;
    // Into the BAND, not the pane: the band is what divides the row between
    // this and the insert-bar pill, and `editorBand()` re-creates it when the
    // pane was emptied under both of them.
    editorBand(this.host).appendChild(this.el);
    this.sync();
  }

  /** Repaint the enabled/disabled state. Cheap; call it after anything. */
  sync(): void {
    if (!this.el) return;
    this.ensureMounted();
    if (this.undoBtn) this.undoBtn.disabled = !this.hooks.canUndo();
    if (this.redoBtn) this.redoBtn.disabled = !this.hooks.canRedo();
  }

  /**
   * No production path tears the editor webview down today — the cluster is
   * mounted on the first edit and kept — so this is exercised by the tests and
   * the render-gate harness. It stays because the save anchor's rescue has to
   * be written wherever the removal is: an `el.remove()` added later without it
   * would take `#moss-save-status` out of the document permanently.
   */
  destroy(): void {
    this.observer?.disconnect();
    this.observer = null;
    this.localeUnsub();
    this.localeUnsub = () => {};
    // Hand the save anchor back so a badge outlives the editor's teardown the
    // way it did when it lived in the titlebar. `.nav-right` missing is not a
    // reason to skip it — that would let `el.remove()` below delete the anchor
    // outright, and every later badge would fall through to the floating
    // fallback. Anywhere in the document beats nowhere.
    //
    // Search THIS SUBTREE first, not the document. Since the observer replaced
    // the four synchronous re-attach call sites, "detached" is a routine state
    // entered on every pane wipe and repaired only a microtask later — and a
    // `getElementById` walks the DOCUMENT, so during that window it returns
    // null, the rescue is skipped, and `this.el = null` drops the anchor with
    // the subtree. Permanently: every later findAnchor('save') falls through to
    // the floating fallback.
    const status =
      this.el?.querySelector('#moss-save-status') ?? document.getElementById('moss-save-status');
    if (status) (document.querySelector('.nav-right') ?? document.body).appendChild(status);
    this.el?.remove();
    this.el = null;
    this.host = null;
    this.undoBtn = null;
    this.redoBtn = null;
  }
}
