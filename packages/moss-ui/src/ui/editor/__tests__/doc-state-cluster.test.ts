import { describe, it, expect, vi, beforeEach } from 'vitest';
import { DocStateCluster } from '../doc-state-cluster';
import { setLocale } from "../../i18n/index";

/**
 * The bottom band's left slot. jsdom answers the questions that matter here —
 * what is in the DOM, what is disabled, where the save anchor ended up. Whether
 * it collides with the pill is a geometry question and lives in a render gate.
 */
describe('DocStateCluster', () => {
  let host: HTMLElement;
  let hooks: {
    undo: ReturnType<typeof vi.fn>;
    redo: ReturnType<typeof vi.fn>;
    canUndo: ReturnType<typeof vi.fn>;
    canRedo: ReturnType<typeof vi.fn>;
    holdsUncommittedOverlay: ReturnType<typeof vi.fn>;
  };
  let cluster: DocStateCluster;

  beforeEach(() => {
    document.body.innerHTML =
      '<div class="nav-right"><span id="moss-save-status" data-inline-task-anchor="save"></span></div>' +
      '<div id="moss-editor-cm"></div>';
    host = document.getElementById('moss-editor-cm')!;
    hooks = {
      undo: vi.fn(),
      redo: vi.fn(),
      canUndo: vi.fn().mockReturnValue(false),
      canRedo: vi.fn().mockReturnValue(false),
      holdsUncommittedOverlay: vi.fn().mockReturnValue(false),
    };
    cluster = new DocStateCluster(hooks);
  });

  const undoBtn = () => host.querySelectorAll('.doc-state-btn')[0] as HTMLButtonElement;
  const redoBtn = () => host.querySelectorAll('.doc-state-btn')[1] as HTMLButtonElement;

  it('does not exist until something is edited', () => {
    expect(host.querySelector('.doc-state-cluster')).toBeNull();
    expect(cluster.mounted).toBe(false);
  });

  it('mounts once; a second edit syncs rather than building a second cluster', () => {
    cluster.mount(host);
    cluster.mount(host);
    expect(host.querySelectorAll('.doc-state-cluster')).toHaveLength(1);
  });

  it('comes back after a file switch empties the pane under it', async () => {
    // EVERY surface mount does `#moss-editor-cm.innerHTML = ''` — it is the
    // documented contract for dismissing the cold-start skeleton, so the
    // cluster cannot opt out of it. Nobody is asked to put the cluster back;
    // the observer notices. No `mount()` call here, deliberately: the previous
    // version of this test called one, which is why it passed against a fix
    // that had no caller.
    const anchor = document.getElementById('moss-save-status')!;
    cluster.mount(host);

    host.innerHTML = ''; // editor-main.ts's CM6 mount path
    await Promise.resolve(); // the observer callback is a microtask

    expect(host.querySelector('.doc-state-cluster')).not.toBeNull();
    expect(host.querySelectorAll('.doc-state-btn')).toHaveLength(2);
    // And the anchor rides back in with it. It was MOVED here out of the
    // titlebar, so losing the cluster loses the save badge's home too, and
    // every later badge silently becomes the floating fallback.
    expect(document.getElementById('moss-save-status')).toBe(anchor);
    expect(anchor.parentElement).toBe(host.querySelector('.doc-state-cluster'));
  });

  it('survives the renderer that empties the pane AGAIN, one line later', async () => {
    // This is the sequence that defeated a re-attach written at the wipe:
    // `showFileViewer` and `mountFolderMode` both open by clearing the
    // container they are handed, so editor-main's wipe is not the last one —
    // the call placed after it was undone one statement later. That surface has
    // no CM6 view either, so no later update heals it and the save badge is
    // gone for as long as the user looks at the image they clicked.
    const anchor = document.getElementById('moss-save-status')!;
    cluster.mount(host);

    host.innerHTML = '';        // editor-main
    host.innerHTML = '';        // …and the renderer it calls next
    host.appendChild(document.createElement('img'));
    await Promise.resolve();

    expect(host.querySelector('.doc-state-cluster')).not.toBeNull();
    expect(document.getElementById('moss-save-status')).toBe(anchor);
  });

  it('stops watching once destroyed', () => {
    // Assert the DISCONNECT, not the outcome. `destroy()` also nulls `el`, and
    // ensureMounted early-returns on that — so a version that never disconnects
    // leaves a live observer on the pane for the rest of the session and still
    // passes an outcome-shaped test. (It did.)
    const spy = vi.spyOn(MutationObserver.prototype, 'disconnect');
    cluster.mount(host);
    cluster.destroy();
    expect(spy).toHaveBeenCalled();
    spy.mockRestore();
  });

  it('does not spin when the pane itself is detached', async () => {
    // A microtask loop is the worst shape of bug available here: re-appending
    // into a detached host can never make the cluster connected, so the
    // observer re-fires forever — and microtasks starve the timer queue, so no
    // timeout fires and the webview just freezes. This test cannot fail
    // gracefully; it hangs. That is the point.
    cluster.mount(host);
    host.remove();
    host.appendChild(document.createElement('div'));
    await Promise.resolve();
    // Reaching this line at all is the assertion. The rest just says the
    // cluster was left alone rather than re-appended in a circle.
    expect(host.querySelectorAll('.doc-state-cluster')).toHaveLength(1);
  });

  it('adopts the save anchor, and hands it back on destroy', () => {
    // The badge has to outlive a file switch, so the anchor cannot live
    // anywhere per-file. Moving the element (not re-creating it) is what lets a
    // badge already showing travel with it.
    const anchor = document.getElementById('moss-save-status')!;
    cluster.mount(host);
    expect(anchor.parentElement).toBe(host.querySelector('.doc-state-cluster'));

    cluster.destroy();
    expect(anchor.parentElement).toBe(document.querySelector('.nav-right'));
    expect(host.querySelector('.doc-state-cluster')).toBeNull();
  });

  it('hands the save anchor back even when destroyed while detached', () => {
    // Since the observer replaced the four synchronous re-attach call sites,
    // "detached" is a routine state — entered on every pane wipe and repaired
    // only a microtask later. A rescue that searched the DOCUMENT could not see
    // an anchor sitting inside the detached cluster, so it silently skipped,
    // and the subtree (anchor included) was dropped when `el` was nulled.
    // Every later badge would then fall through to the floating fallback: a
    // permanent consequence of a one-microtask window.
    const anchor = document.getElementById('moss-save-status')!;
    cluster.mount(host);
    host.innerHTML = ''; // detach, before the observer's microtask runs

    cluster.destroy();

    expect(anchor.parentElement).toBe(document.querySelector('.nav-right'));
    expect(anchor.isConnected).toBe(true);
  });

  it('watches the new pane when mounted into a different host', async () => {
    // Re-pointing `host` without re-pointing the observer leaves a half-live
    // cluster: the next edit re-appends it into the new pane (ensureMounted
    // reads `this.host`), but nothing watches that pane, so the first wipe
    // detaches it for good — silently, because the guard returns rather than
    // throwing. Unreachable today, and so is the microtask-loop guard beside
    // it; that one got a test, which is the whole argument for this one.
    cluster.mount(host);
    const second = document.createElement('div');
    document.body.appendChild(second);

    cluster.mount(second);
    expect(second.querySelector('.doc-state-cluster')).not.toBeNull();
    // Drain the OLD observer first. Moving the cluster out of `host` is itself
    // a childList mutation there, so a callback is already queued — and it
    // would re-append the cluster into the new pane on its own, letting this
    // test pass against a version that never re-points anything.
    await Promise.resolve();

    // The observer has to have MOVED, so drive it the way the app does — by
    // emptying the pane and waiting a microtask, not by calling ensureMounted
    // by hand (which would pass with no observer at all).
    second.innerHTML = '';
    await Promise.resolve();
    expect(second.querySelectorAll('.doc-state-cluster')).toHaveLength(1);
  });

  it('disables each side exactly when that side is empty', () => {
    // This is the payload: ⌘Z already works without any button, so what the
    // buttons are FOR is answering "is there anything left, and did my chip
    // edit land in the same stack as my typing".
    cluster.mount(host);
    expect(undoBtn().disabled).toBe(true);
    expect(redoBtn().disabled).toBe(true);

    hooks.canUndo.mockReturnValue(true);
    cluster.sync();
    expect(undoBtn().disabled).toBe(false);
    expect(redoBtn().disabled).toBe(true);

    hooks.canRedo.mockReturnValue(true);
    cluster.sync();
    expect(redoBtn().disabled).toBe(false);
  });

  it('runs the command and repaints in one click', () => {
    hooks.canUndo.mockReturnValue(true);
    cluster.mount(host);
    // The click empties the stack, the way a real undo of the only step would.
    hooks.undo.mockImplementation(() => hooks.canUndo.mockReturnValue(false));

    undoBtn().click();

    expect(hooks.undo).toHaveBeenCalledTimes(1);
    // Without the sync inside the handler the button stays enabled until some
    // unrelated update repaints it, so a second click hits an empty stack.
    expect(undoBtn().disabled).toBe(true);
  });

  it('does not steal focus from the editor', () => {
    // A click that blurs CM6 first lands the undo on a view with no selection,
    // and the caret ends up wherever the document happens to start.
    cluster.mount(host);
    const e = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    undoBtn().dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
  });

  describe('what the mousedown lets through to `document`', () => {
    // Four things listen for a document-level mousedown in this webview: the
    // unified ＋ dropdown's outside-dismiss, the context menu's, the chip bar's
    // provisional-ghost dismissal, and the file tree's (capture phase, so out of
    // reach either way). Stopping the event for all of them — which is what an
    // unconditional stopPropagation does — silently voids the first three.
    const reachesDocument = () => {
      const seen = vi.fn();
      document.addEventListener('mousedown', seen);
      try {
        undoBtn().dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
        return seen.mock.calls.length > 0;
      } finally {
        document.removeEventListener('mousedown', seen);
      }
    };

    it('stops it while a chip edit popover is open', () => {
      // Otherwise the popover dismisses (flushing its edit as one history entry)
      // and the click that follows immediately undoes that entry — while the
      // same gesture on the keyboard only closes the popover.
      hooks.holdsUncommittedOverlay.mockReturnValue(true);
      cluster.mount(host);
      expect(reachesDocument()).toBe(false);
    });

    it('lets it through when nothing holds an uncommitted edit', () => {
      // The sharpest of the three: the ＋ dropdown autofocuses its search box,
      // so surviving the click made the route `native` and Undo undid THE
      // TYPING IN THE FILTER BOX, leaving the document's history untouched.
      hooks.holdsUncommittedOverlay.mockReturnValue(false);
      cluster.mount(host);
      expect(reachesDocument()).toBe(true);
    });
  });

  it('names both buttons for a screen reader, and for the tooltip', () => {
    cluster.mount(host);
    for (const b of [undoBtn(), redoBtn()]) {
      expect(b.getAttribute('aria-label')).toMatch(/\S/);
      expect(b.dataset.tooltip).toMatch(/\S/);
    }
    // The hotkey rides the portal tooltip, per the project convention — never
    // the native `title` attribute.
    // The glyph is platform-dependent — jsdom's navigator.platform is not a
    // Mac, and on Linux/Windows the CM6 keymap is the ONLY path to undo (no
    // Edit menu), so a hard-coded ⌘ would misname a key that really works.
    expect(undoBtn().dataset.tooltipKbd).toBe('Ctrl+Z');
    expect(redoBtn().dataset.tooltipKbd).toBe('Ctrl+Shift+Z');
    expect(undoBtn().hasAttribute('title')).toBe(false);
  });

  it('relabels both buttons when the locale changes', () => {
    // The tooltip and the accessible name are the same string, so a missed
    // relabel leaves a screen reader naming the button in a language the user
    // has already left. The cluster outlives every locale switch — it is
    // mounted once for the session — so it cannot rely on a re-render.
    cluster.mount(host);
    const en = undoBtn().getAttribute('aria-label');
    expect(en).toBe(undoBtn().dataset.tooltip);

    setLocale('zh-hans');
    try {
      expect(undoBtn().getAttribute('aria-label')).not.toBe(en);
      expect(undoBtn().getAttribute('aria-label')).toBe(undoBtn().dataset.tooltip);
      expect(redoBtn().getAttribute('aria-label')).toBe(redoBtn().dataset.tooltip);
    } finally {
      setLocale('en');
    }
  });

});
