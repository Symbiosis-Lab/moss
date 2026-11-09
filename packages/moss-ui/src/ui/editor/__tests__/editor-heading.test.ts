import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { wireEditorHeading } from '../editor-heading';
import { CmEditor } from '../cm-editor';

// wireEditorHeading only reads `engine` transitively through nothing here, but
// CmEditor reaches the engine via applyEmbedWidth. Mock the one command it can
// touch so construction stays offline (mirrors cm-editor.test.ts).
vi.mock('../../engine', () => ({
  engine: { setImageWidth: vi.fn() },
}));

function makeHeading(text = ''): HTMLElement {
  const el = document.createElement('div');
  el.id = 'editor-heading';
  el.setAttribute('contenteditable', 'plaintext-only');
  el.textContent = text;
  document.body.appendChild(el);
  return el;
}

function enter(el: HTMLElement): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
  el.dispatchEvent(e);
  return e;
}

describe('wireEditorHeading — title Enter descends into the CM6 body (Obsidian parity)', () => {
  beforeEach(() => { document.body.innerHTML = ''; });
  afterEach(() => { document.body.innerHTML = ''; });

  test('Enter in the title focuses CM6 with the caret at document start (position 0)', () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const editor = new CmEditor({ content: 'First line\nSecond line', container, onChange: () => {} });
    const view = editor.getView();
    // Prove the descend RESETS the caret: park the selection mid-document first.
    view.dispatch({ selection: { anchor: 8 } });
    expect(view.state.selection.main.head).toBe(8);

    const heading = makeHeading('My Title');
    wireEditorHeading({
      getCurrentPath: () => 'note.md',
      onCommit: async () => ({ ok: true }),
      // Same closure editor-main wires: focus + selection move only.
      focusBody: () => { view.focus(); view.dispatch({ selection: { anchor: 0 } }); },
    });

    heading.focus();
    const e = enter(heading);

    expect(e.defaultPrevented).toBe(true);          // no inserted newline / default submit
    expect(view.state.selection.main.head).toBe(0); // caret at document start
    expect(view.hasFocus).toBe(true);               // focus actually moved into CM6
    editor.destroy();
  });

  test('Enter invokes focusBody exactly once (the wiring the bug was missing)', () => {
    const heading = makeHeading('Some Title');
    const focusBody = vi.fn();
    wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }), focusBody });

    heading.focus();
    const e = enter(heading);

    expect(e.defaultPrevented).toBe(true);
    expect(focusBody).toHaveBeenCalledTimes(1);
  });

  test('Enter without a wired body still commits + blurs (legacy behavior preserved)', () => {
    const heading = makeHeading('New Title');
    const onCommit = vi.fn(async () => ({ ok: true as const }));
    // No focusBody (email-composer style host).
    wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit });

    heading.focus();
    const e = enter(heading);

    expect(e.defaultPrevented).toBe(true);
    expect(document.activeElement).not.toBe(heading); // blurred
    // blur → commit() fires (text changed from '' → 'New Title').
    expect(onCommit).toHaveBeenCalledWith('New Title');
  });

  test('Enter is NOT hijacked mid-IME-composition (isComposing)', () => {
    const heading = makeHeading('拼');
    const focusBody = vi.fn();
    wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }), focusBody });

    heading.focus();
    const e = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, isComposing: true });
    heading.dispatchEvent(e);

    expect(e.defaultPrevented).toBe(false); // left for the IME to confirm the candidate
    expect(focusBody).not.toHaveBeenCalled();
  });

  test('ArrowDown at the end of the title descends into the body; mid-title does not', () => {
    const heading = makeHeading('Title');
    const focusBody = vi.fn();
    wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }), focusBody });
    heading.focus();

    const sel = document.getSelection()!;
    const textNode = heading.firstChild!;

    // Caret at end of "Title" (offset 5) → descend.
    sel.removeAllRanges();
    const atEnd = document.createRange();
    atEnd.setStart(textNode, 5);
    atEnd.collapse(true);
    sel.addRange(atEnd);
    const down1 = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true });
    heading.dispatchEvent(down1);
    expect(down1.defaultPrevented).toBe(true);
    expect(focusBody).toHaveBeenCalledTimes(1);

    // Caret mid-title (offset 2) → fall through (no descend, no preventDefault).
    sel.removeAllRanges();
    const mid = document.createRange();
    mid.setStart(textNode, 2);
    mid.collapse(true);
    sel.addRange(mid);
    const down2 = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true });
    heading.dispatchEvent(down2);
    expect(down2.defaultPrevented).toBe(false);
    expect(focusBody).toHaveBeenCalledTimes(1); // unchanged
  });
});

describe('wireEditorHeading — focusEnd moves focus UP into the title (ArrowUp parity)', () => {
  beforeEach(() => { document.body.innerHTML = ''; });
  afterEach(() => { document.body.innerHTML = ''; });

  test('focusEnd() focuses the title and collapses the caret at the very end', () => {
    const heading = makeHeading('My Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });

    // Precondition: focus is NOT on the title yet — focusEnd must move it there.
    expect(document.activeElement).not.toBe(heading);

    const moved = ctrl.focusEnd();

    expect(moved).toBe(true);
    expect(document.activeElement).toBe(heading); // focus actually moved into the title
    const sel = document.getSelection()!;
    expect(sel.isCollapsed).toBe(true); // a caret, not a range
    // Caret at the very end of "My Title": either offset 8 into the single text
    // node, or anchored on the element past its last child (empty/odd cases).
    const focusNode = sel.focusNode!;
    if (focusNode === heading) {
      expect(sel.focusOffset).toBe(heading.childNodes.length);
    } else {
      expect(sel.focusOffset).toBe('My Title'.length);
    }
  });

  test('focusEnd() returns false and does NOT focus a hidden (data-empty) title', () => {
    const heading = makeHeading('');
    // The empty-title state collapses the heading to display:none via
    // `.editor-heading[data-empty="true"]` — never focus an invisible element.
    heading.dataset.empty = 'true';
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });

    const moved = ctrl.focusEnd();

    expect(moved).toBe(false);
    expect(document.activeElement).not.toBe(heading);
  });

  // ── What ⌘Z has to ask before it walks the log ────────────────────────
  //
  // The heading commits on BLUR, so between the first keystroke and leaving it
  // the typed title exists nowhere but the DOM. The router used to infer that
  // from FOCUS, which is a different question — and wrong in both directions:
  // a focused-and-changed heading had its ⌘Z revert the edit made BEFORE the
  // title was touched (while the title itself landed anyway on the next blur),
  // and after the first fix a focused-but-unchanged heading ate the keypress.
  // Only this module knows what was last saved, so only it can answer.

  test('hasPendingEdit() is false for a focused but unchanged heading', () => {
    const heading = makeHeading('Saved Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Saved Title', 'Filename');
    heading.focus();

    // The dead-keypress case: focus alone must not read as a pending edit.
    expect(ctrl.hasPendingEdit()).toBe(false);
  });

  test('hasPendingEdit() is true once the text diverges from what was saved', () => {
    const heading = makeHeading('Saved Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Saved Title', 'Filename');
    heading.focus();
    heading.textContent = 'Saved Title, edited';

    expect(ctrl.hasPendingEdit()).toBe(true);
  });

  test('hasPendingEdit() ignores surrounding whitespace, as the commit does', () => {
    // commit() trims before comparing, so an untrimmed compare here would report
    // a pending edit that the flush then declines to make — a keypress spent on
    // nothing.
    const heading = makeHeading('Saved Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Saved Title', 'Filename');
    heading.textContent = '  Saved Title  ';

    expect(ctrl.hasPendingEdit()).toBe(false);
  });

  test('flushPendingEdit() commits through blur, the one commit path', async () => {
    const heading = makeHeading('Old');
    const onCommit = vi.fn().mockResolvedValue({ ok: true });
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit });
    ctrl.setSaved('Old', 'Filename');
    heading.focus();
    heading.textContent = 'New';

    ctrl.flushPendingEdit();
    await Promise.resolve();

    // Routed through blur rather than calling commit() directly, so a flush is
    // indistinguishable from the user clicking away — one commit path, not two.
    expect(document.activeElement).not.toBe(heading);
    expect(onCommit).toHaveBeenCalledWith('New');
    expect(ctrl.hasPendingEdit()).toBe(false);
  });

  test('hasPendingEdit() is false while an IME is still composing', () => {
    // An IME's in-flight candidate text is not yet an edit the user has made,
    // and the blur handler declines to commit during composition — so reporting
    // it as pending would route ⌘Z to a flush that does nothing at all.
    const heading = makeHeading('Saved Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Saved Title', 'Filename');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'Saved Title 拼';

    expect(ctrl.hasPendingEdit()).toBe(false);

    heading.dispatchEvent(new CompositionEvent('compositionend'));
    expect(ctrl.hasPendingEdit()).toBe(true);
  });

  test('setSaved() updates a FOCUSED heading that holds no pending edit', async () => {
    // The undo-strands-the-title bug. With the caret parked in an unchanged
    // title, ⌘Z routes to the document log — correctly, there is nothing to
    // flush — the undo lands in the file and the title chip, and this is the
    // call that has to move the H1 with them. Guarding on FOCUS instead of on a
    // pending edit dropped it, and blurring did not repair it either: commit()
    // sees no change and writes the stale saved value back, so the heading kept
    // showing a title the file no longer had.
    const heading = makeHeading('Current Title');
    const onCommit = vi.fn().mockResolvedValue({ ok: true });
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit });
    ctrl.setSaved('Current Title', 'Filename');
    heading.focus();

    ctrl.setSaved('Reverted Title', 'Filename');

    expect(heading.textContent).toBe('Reverted Title');
    // …and the heading now agrees with the store, so leaving it commits nothing.
    heading.blur();
    await Promise.resolve();
    expect(onCommit).not.toHaveBeenCalled();
  });

  test('setSaved() leaves the caret alone when the text has not changed', () => {
    // `textContent =` is replace-all: it destroys the existing text node and
    // inserts a new one EVEN FOR AN IDENTICAL STRING, and a live range inside a
    // removed node collapses to (parent, 0). Every frontmatter change notifies
    // this, so an unconditional write sent the caret to the start of the title
    // on any chip edit, any external file change, and on the very ⌘Z the write
    // was added for — repairing the text and destroying the caret instead.
    const heading = makeHeading('Same Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Same Title', 'Filename');
    heading.focus();
    const textNode = heading.firstChild!;
    const range = document.createRange();
    range.setStart(textNode, 5);
    range.collapse(true);
    const sel = document.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    ctrl.setSaved('Same Title', 'Filename');

    expect(document.getSelection()!.focusNode).toBe(textNode);
    expect(document.getSelection()!.focusOffset).toBe(5);
  });

  test('setSaved() puts the caret at the end when it DOES replace focused text', () => {
    // A genuinely different title still has to leave the caret somewhere
    // sensible — offset 0 of a title the user did not type is not it.
    const heading = makeHeading('Old Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Old Title', 'Filename');
    heading.focus();

    ctrl.setSaved('A Different Title', 'Filename');

    expect(heading.textContent).toBe('A Different Title');
    // "At the end" has two spellings — anchored on the element past its last
    // child (what `focusEnd`'s selectNodeContents+collapse produces) or inside
    // the text node at its length. `caretAtEnd` accepts both; so does this.
    const sel = document.getSelection()!;
    const atEnd =
      sel.focusNode === heading
        ? sel.focusOffset === heading.childNodes.length
        : sel.focusOffset === (sel.focusNode?.textContent?.length ?? -1);
    expect(atEnd).toBe(true);
    expect(sel.focusOffset).not.toBe(0);
  });

  test('setSaved() does not write through an IME composition', () => {
    // `hasPendingEdit()` is deliberately false while composing — an in-flight
    // candidate is not yet an edit the user has made — which is right for the
    // reader asking "is there something to flush" and wrong for the writer:
    // writing here replaces the node the IME is composing into and aborts the
    // conversion. The two questions need two terms.
    const heading = makeHeading('Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Title', 'Filename');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));

    ctrl.setSaved('Something Arriving Mid-Composition', 'Filename');

    expect(heading.textContent).toBe('Title');
  });

  test('setSaved() still refuses to stomp typing in progress', () => {
    // The other half, and the reason the old focus guard existed: an external
    // update arriving mid-edit must not delete what the user is typing.
    const heading = makeHeading('Current Title');
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    ctrl.setSaved('Current Title', 'Filename');
    heading.focus();
    heading.textContent = 'Half-typed new ti';

    ctrl.setSaved('Something Else', 'Filename');

    expect(heading.textContent).toBe('Half-typed new ti');
    // And it is still pending against the NEW saved value, so blurring commits.
    expect(ctrl.hasPendingEdit()).toBe(true);
  });

  test('the absent-heading stub answers both questions safely', () => {
    // No #editor-heading in the DOM → the no-op stub. Reporting a pending edit
    // it could never flush would make ⌘Z do nothing, forever.
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    expect(ctrl.hasPendingEdit()).toBe(false);
    expect(() => ctrl.flushPendingEdit()).not.toThrow();
  });

  test('focusEnd() returns false when the heading element is absent', () => {
    // No #editor-heading in the DOM → wireEditorHeading returns the no-op stub.
    const ctrl = wireEditorHeading({ getCurrentPath: () => 'note.md', onCommit: async () => ({ ok: true }) });
    expect(ctrl.focusEnd()).toBe(false);
  });
});

describe('wireEditorHeading — the text belongs to a FILE, not just to the element', () => {
  beforeEach(() => { document.body.innerHTML = ''; });
  afterEach(() => { document.body.innerHTML = ''; });

  /** A host whose open file can be swapped out from under the heading. */
  /**
   * `id` opts the host into EntryId identity, the way the editor wires it.
   * Omitted, identity falls back to the path — which is what a host without a
   * registry gets, and what `rename` below exists to tell apart.
   */
  function host(initial: string | null, id?: number) {
    let path = initial;
    let entryId = id ?? null;
    const onCommit = vi.fn(async () => ({ ok: true as const }));
    const ctrl = wireEditorHeading({
      getCurrentPath: () => path,
      ...(id === undefined ? {} : { getCurrentId: () => entryId }),
      onCommit,
    });
    return {
      ctrl,
      onCommit,
      /** A different document. */
      open: (p: string, newId?: number) => { path = p; if (newId !== undefined) entryId = newId; },
      /** The SAME document, at a new path. */
      rename: (p: string) => { path = p; },
    };
  }

  test('a title typed for one file is never committed onto another', async () => {
    // Nothing blurs the heading on a load — a click in the PREVIEW pane is a
    // different webview, so DOM focus stays on the H1. Committing the leftover
    // text writes someone else's title into b's frontmatter, and in Filename
    // mode RENAMES b, rewriting wikilinks project-wide.
    const heading = makeHeading('Title A');
    const h = host('a.md');
    h.ctrl.setSaved('Title A', 'Filename');
    heading.focus();
    heading.textContent = 'Half-typed for A';

    h.open('b.md');
    heading.blur();
    await Promise.resolve();

    expect(h.onCommit).not.toHaveBeenCalled();
    // …and the element is handed back to the file that is actually open.
    expect(heading.textContent).toBe('Title A');
  });

  test('holding an IME open does not smuggle the old file’s title onto the new one', async () => {
    // The predicate that used to guard this reports FALSE during a composition,
    // on the grounds that an in-flight candidate is not yet an edit. That is
    // the right answer to "is there something to flush" and the wrong answer to
    // "which file is this text for", and reusing it here let a held conversion
    // walk straight through the guard.
    const heading = makeHeading('Title A');
    const h = host('a.md');
    h.ctrl.setSaved('Title A', 'Filename');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'Title Aにほ';

    h.open('b.md');
    h.ctrl.setSaved('Title B', 'Filename');

    // The incoming title takes the element back: the composition was addressed
    // to a file that is no longer open, so there is nothing to protect.
    expect(heading.textContent).toBe('Title B');
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();
  });

  test('the new file’s title stays editable after a switch left an edit behind', async () => {
    // The flag this replaces was lowered by a pending edit and had nothing to
    // raise it again, so b's title was refused on every later commit and its
    // ⌘Z fell through to the native contenteditable undo.
    const heading = makeHeading('Title A');
    const h = host('a.md');
    h.ctrl.setSaved('Title A', 'Filename');
    heading.focus();
    heading.textContent = 'Half-typed for A';

    h.open('b.md');
    h.ctrl.setSaved('Title B', 'Title');
    expect(h.ctrl.saved()).toEqual({ text: 'Title B', source: 'Title' });

    // A genuine edit to b's title now commits, like any other.
    heading.focus();
    heading.textContent = 'Title B revised';
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledWith('Title B revised');
  });

  test('clear() withdraws the source the host routes on', () => {
    const heading = makeHeading('Title A');
    const h = host('a.md');
    expect(h.ctrl.saved()).toBeNull(); // before the first compute lands
    h.ctrl.setSaved('Title A', 'Title');
    expect(h.ctrl.saved()?.source).toBe('Title');
    h.ctrl.clear();
    // The host reads this to refuse a commit whose routing it cannot trust —
    // the text, the source and the permission move as one value now.
    expect(h.ctrl.saved()).toBeNull();
    expect(heading.textContent).toBe('Title A'); // display is untouched
  });

  test('an update that arrives mid-composition lands, rather than being reverted', () => {
    // Dropping it was worse than losing it: the saved value had already
    // advanced, so the next blur saw "no change" and wrote the STALE text back
    // over the value that had arrived. Finishing a conversion silently
    // reverted somebody else's edit.
    const heading = makeHeading('Old');
    const h = host('a.md');
    h.ctrl.setSaved('Old', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));

    h.ctrl.setSaved('External', 'Title');
    expect(heading.textContent).toBe('Old'); // not written through the IME

    heading.dispatchEvent(new CompositionEvent('compositionend'));
    expect(heading.textContent).toBe('External');
  });

  test('the LAST update to arrive during a composition is the one that lands', () => {
    const heading = makeHeading('Old');
    const h = host('a.md');
    h.ctrl.setSaved('Old', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));

    h.ctrl.setSaved('First', 'Title');
    h.ctrl.setSaved('Second', 'Title');
    heading.dispatchEvent(new CompositionEvent('compositionend'));

    expect(heading.textContent).toBe('Second');
  });

  test('noteSavedText corrects the text without touching the routing source', () => {
    // The untitled-first create flow names the file through its own rename, so
    // it knows the title moved but is not the authority on how it routes.
    const heading = makeHeading('Untitled');
    const h = host('a.md');
    h.ctrl.setSaved('Untitled', 'Filename');
    h.ctrl.noteSavedText('Named By Flow');
    expect(h.ctrl.saved()).toEqual({ text: 'Named By Flow', source: 'Filename' });
  });

  test('noteSavedText shows the text, which is how a new page gets its Untitled ghost', async () => {
    // The create flow's seam blanks the H1 so the `:empty` ghost appears, and
    // it runs AFTER the first compute has already painted the generated
    // filename. A version that moved only the saved string left `untitled` on
    // screen against an empty baseline — so an untouched new page read as a
    // pending edit, and blurring it committed `untitled` as the user's title
    // (renaming the file, under the date-prefix convention).
    const heading = makeHeading('untitled');
    const h = host('a.md');
    h.ctrl.setSaved('untitled', 'Filename');
    h.ctrl.noteSavedText('');

    expect(heading.textContent).toBe('');
    expect(h.ctrl.hasPendingEdit()).toBe(false);

    heading.focus();
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();
  });

  test('a load beginning under a parked caret commits nothing and keeps the title', async () => {
    // `clear()` is called at the START of a load, while the previous file's
    // title is still on screen. Forgetting the text along with the answer left
    // `commit()` comparing an untouched heading against the empty string: it
    // committed, the host refused it as not-loaded-yet, and the failure path
    // blanked the H1 and posted an error banner — all from parking the caret
    // in a title and reloading the file.
    const heading = makeHeading('Title A');
    const h = host('a.md');
    h.ctrl.setSaved('Title A', 'Filename');
    heading.focus();

    h.ctrl.clear();
    heading.blur();
    await Promise.resolve();

    expect(h.onCommit).not.toHaveBeenCalled();
    expect(heading.textContent).toBe('Title A');
    expect(heading.parentElement?.querySelector('.editor-heading-error')).toBeNull();
  });

  test('emptying the title during a load commits nothing and paints nothing', async () => {
    // The sharper form of the case above. `clear()` keeps the previous title as
    // the text on screen, so anything measured against it during the load window
    // is measuring the wrong document: emptying the heading read as a deliberate
    // change, went out as one, was refused as not-loaded-yet, and the failure
    // path painted file A's title into file B's H1 behind an error banner. An
    // answer that has lapsed answers "no" to every question instead.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');

    h.ctrl.clear();
    h.open('b.md', 2);
    heading.focus();
    heading.textContent = '';
    expect(h.ctrl.hasPendingEdit()).toBe(false);
    heading.blur();
    await Promise.resolve();

    expect(h.onCommit).not.toHaveBeenCalled();
    expect(heading.textContent).toBe(''); // the user's own edit, left alone
    expect(heading.parentElement?.querySelector('.editor-heading-error')).toBeNull();

    // …and the incoming answer paints B's title over it, rather than treating
    // the empty element as an edit worth protecting.
    h.ctrl.setSaved('Title B', 'Title');
    expect(heading.textContent).toBe('Title B');
  });

  test('a load that fails gives the still-open document its title back', async () => {
    // `clear()` is a PREDICTION that a load is about to replace the document.
    // When the read fails, the previous document stays mounted and no heading
    // state is recomputed — so a withdrawal left standing makes the title of a
    // file the user is still looking at uncommittable, with nothing on screen
    // saying so and nothing able to raise the bit again.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');

    h.ctrl.clear();   // loadFile begins
    h.ctrl.restore(); // …and the read fails; a.md is still open

    expect(h.ctrl.saved()?.source).toBe('Title');
    heading.focus();
    heading.textContent = 'Title A revised';
    expect(h.ctrl.hasPendingEdit()).toBe(true);
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledWith('Title A revised');
  });

  test('Escape during a load leaves the element alone rather than repainting', async () => {
    // The repaint would come from the record's text, which during a load window
    // is the title of the document being LEFT — so cancelling an edit would
    // paint the previous file's title into the new file's heading.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');

    h.ctrl.clear();
    h.open('b.md', 2);
    heading.focus();
    heading.textContent = 'half-typed';
    heading.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await Promise.resolve();

    expect(heading.textContent).not.toBe('Title A');
    h.ctrl.setSaved('Title B', 'Title');
    expect(heading.textContent).toBe('Title B');
  });

  test('a host with no compute behind it still holds its own edits', async () => {
    // "No record at all" and "a record whose answer has lapsed" are different
    // states, and only the second one means the text cannot be trusted. Reading
    // them as one leaves a host that never computes permanently unable to say it
    // is holding an edit — and its first-ever saved text would stomp what the
    // user had already typed.
    const heading = makeHeading('');
    const h = host('note.md');
    heading.focus();
    heading.textContent = 'Typed before anything resolved';

    expect(h.ctrl.saved()).toBeNull();
    expect(h.ctrl.hasPendingEdit()).toBe(true);

    h.ctrl.setSaved('Computed Later', 'Title');
    expect(heading.textContent).toBe('Typed before anything resolved');
  });

  test('a commit that outlives its document does not rewrite the next one', async () => {
    // A Filename-mode commit renames the file and rewrites wikilinks across the
    // project, so it easily outlives a load. Everything after the await writes
    // to the heading or the record — both of which by then describe a different
    // document — so an unguarded write-back left the record claiming a title the
    // H1 was not showing, for a file that never had it.
    const heading = makeHeading('Title A');
    let resolveCommit: (v: { ok: true }) => void = () => {};
    const onCommit = vi.fn(() => new Promise<{ ok: true }>((r) => { resolveCommit = r; }));
    let path: string | null = 'a.md';
    let id = 1;
    const ctrl = wireEditorHeading({
      getCurrentPath: () => path,
      getCurrentId: () => id,
      onCommit,
    });
    ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.textContent = 'A revised';
    heading.blur();
    await Promise.resolve();
    expect(onCommit).toHaveBeenCalledWith('A revised');

    // The document changes while the rename is still in flight.
    ctrl.clear();
    path = 'b.md';
    id = 2;
    ctrl.setSaved('Title B', 'Title');
    resolveCommit({ ok: true });
    await Promise.resolve();
    await Promise.resolve();

    expect(ctrl.saved()).toEqual({ text: 'Title B', source: 'Title' });
    expect(heading.textContent).toBe('Title B');
  });

  test('a pending edit the element no longer holds cannot eat every later undo', async () => {
    // `hasPendingEdit()` raises on "the text differs", a fact about strings; the
    // only way to flush it is `blur`, a fact about focus. A commit that
    // deliberately leaves text on screen — here, one refused because a load was
    // in flight — used to raise it with nothing able to lower it: flushing blurs
    // an element that is not focused, so ⌘Z routed to that no-op flush for the
    // rest of the session, in the body as well as the title.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');

    h.ctrl.clear();                       // a load begins
    heading.focus();
    heading.textContent = 'Half-typed';
    heading.blur();                       // commit refuses, text left standing
    await Promise.resolve();
    h.ctrl.restore();                     // …and the load fails; a.md is still open

    expect(h.ctrl.hasPendingEdit()).toBe(false);
    h.ctrl.flushPendingEdit();
    expect(h.ctrl.hasPendingEdit()).toBe(false);

    // And the document's own updates reach the H1 again, rather than being
    // preserved against an edit nobody is making.
    h.ctrl.setSaved('Title A external', 'Title');
    expect(heading.textContent).toBe('Title A external');
  });

  test('an orphan takeback keeps the element pointed at the document on screen', async () => {
    // `show()` re-focuses an element that is already focused, which fires no
    // `focus` event — so clearing the edit's destination here left nothing able
    // to set it again, and everything typed afterwards was addressed to no
    // document at all, then committed onto whichever one was open at the blur.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Filename');
    heading.focus();
    heading.textContent = 'Half-typed for A';

    h.open('b.md', 2);
    h.ctrl.setSaved('Title B', 'Filename'); // takes the element back
    expect(heading.textContent).toBe('Title B');
    expect(document.activeElement).toBe(heading); // …and the user is still in it

    // What the user types NOW is a real edit to b.md, and the editor has to know
    // it: otherwise ⌘Z walks the document log while the title holds text that
    // has not been written, reverting an older change and then landing this one
    // anyway on the next blur.
    heading.textContent = 'Typed for B';
    expect(h.ctrl.hasPendingEdit()).toBe(true);
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledWith('Typed for B');
  });

  test('a composition ending during a load does not flash the previous title', async () => {
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    h.ctrl.setSaved('Title A external', 'Title'); // deferred behind the IME

    h.ctrl.clear();
    h.open('b.md', 2);
    heading.dispatchEvent(new CompositionEvent('compositionend'));

    expect(heading.textContent).not.toBe('Title A external');
    h.ctrl.setSaved('Title B', 'Title');
    expect(heading.textContent).toBe('Title B');
  });

  test('losing the open file withdraws the answer without destroying the title', async () => {
    // Nulling the record threw away the display baseline and the routing source
    // too, and neither clear() nor restore() could bring them back — so an
    // untouched heading read as an edit, was refused, and blanked itself behind
    // an error banner.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');

    h.open(null as unknown as string);
    h.ctrl.setSaved('ignored', 'Filename'); // no path: nothing to answer about
    expect(h.ctrl.saved()).toBeNull();

    heading.focus();
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();
    expect(heading.textContent).toBe('Title A');
    expect(heading.parentElement?.querySelector('.editor-heading-error')).toBeNull();
  });

  test('a blur swallowed by a composition still commits when the composition ends', async () => {
    // Committing mid-conversion would write a half-converted candidate, so the
    // blur is refused there — but refusing it dropped the commit entirely. The
    // element then sat unfocused holding text nobody would ever write, which
    // read as a pending edit that could not be flushed (flushing blurs an
    // element that is not focused), so every later undo in the editor routed to
    // that no-op and did nothing.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'タイトル';
    heading.blur(); // the IME has not finished; the commit is refused
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();

    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();

    expect(h.onCommit).toHaveBeenCalledWith('タイトル');
    expect(h.ctrl.hasPendingEdit()).toBe(false);
  });

  test('a load that fails during a composition does not revert the update it held', async () => {
    // The held paint is skipped while the answer has lapsed, on the promise that
    // the incoming setSaved will paint instead. A load that FAILS never sends
    // one — so dropping the paint left the record holding a title the screen had
    // never shown, and the next blur wrote the visible one back over it. In
    // Filename mode that is a rename, undone by finishing a conversion.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    h.ctrl.setSaved('Title A external', 'Title'); // held behind the IME

    h.ctrl.clear();                                // a load begins
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    expect(heading.textContent).toBe('Title A');   // nothing painted yet
    h.ctrl.restore();                              // …and the read fails

    expect(heading.textContent).toBe('Title A external');
    expect(h.ctrl.hasPendingEdit()).toBe(false);
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();
  });

  test('a failed commit does not wipe what the user typed while it was in flight', async () => {
    const heading = makeHeading('Title A');
    let reject: (v: { ok: false; error: string }) => void = () => {};
    const onCommit = vi.fn(
      () => new Promise<{ ok: false; error: string }>((r) => { reject = r; }),
    );
    const ctrl = wireEditorHeading({
      getCurrentPath: () => 'a.md',
      getCurrentId: () => 1,
      onCommit,
    });
    ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.textContent = 'Revised';
    heading.blur();
    await Promise.resolve();

    // Back in, typing something newer, while the first commit is still in flight.
    heading.focus();
    heading.textContent = 'Revised more';
    reject({ ok: false, error: 'nope' });
    await Promise.resolve();
    await Promise.resolve();

    expect(heading.textContent).toBe('Revised more');
    expect(ctrl.hasPendingEdit()).toBe(true);
  });

  test('an owed commit during a failed load still does not revert the held update', async () => {
    // The blur-during-composition case of the test above. Discharging the owed
    // commit must not also discharge the HELD PAINT: `commit()` bails without
    // painting while a load is in flight, so lowering the paint on its behalf
    // stranded it exactly as dropping it did, and the next blur wrote the
    // visible title back over the external one.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    h.ctrl.setSaved('Title A external', 'Title'); // held behind the IME
    heading.textContent = 'タイトル';

    h.ctrl.clear();   // a load begins
    heading.blur();   // the commit is owed, not run
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();
    h.ctrl.restore(); // …and the read fails

    expect(heading.textContent).toBe('Title A external');
    expect(h.ctrl.hasPendingEdit()).toBe(false);
  });

  test('an owed commit is discharged once, not on every later composition', async () => {
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'B';
    heading.blur();
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledTimes(1);

    // A later, unrelated composition must not re-fire the commit that was
    // already discharged — it would run mid-typing, against whatever is on
    // screen by then.
    heading.focus();
    heading.textContent = 'C';
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledTimes(1);
  });

  test('a failed load never repaints into a live composition', async () => {
    // `restore()` discharges a held paint, and a paint replaces the node the IME
    // is composing into — which aborts the conversion, the thing the whole
    // deferral mechanism exists to avoid.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'にほ';
    h.ctrl.setSaved('Title A external', 'Title'); // held

    h.ctrl.clear();
    h.ctrl.restore(); // the read fails, while the IME is STILL composing

    expect(heading.textContent).toBe('にほ');
    // …and it lands the moment the conversion finishes.
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();
    expect(heading.textContent).toBe('Title A external');
  });

  test('a commit discharged under a live caret keeps the element pointed at its document', async () => {
    // An owed commit is discharged at `compositionend`, which can arrive after
    // the user has clicked back in — and `focus` does not fire again for an
    // element that never lost focus, so clearing the destination here left
    // everything typed next addressed to no document.
    const heading = makeHeading('Title A');
    const h = host('a.md', 1);
    h.ctrl.setSaved('Title A', 'Title');
    heading.focus();
    heading.dispatchEvent(new CompositionEvent('compositionstart'));
    heading.textContent = 'B';
    heading.blur();
    heading.focus(); // back in before the conversion finishes
    heading.dispatchEvent(new CompositionEvent('compositionend'));
    await Promise.resolve();

    heading.textContent = 'B and more';
    expect(h.ctrl.hasPendingEdit()).toBe(true);
  });

  test('renaming the open file does not orphan the title being typed into it', async () => {
    // Identity is the EntryId; the path is a display label that moves. Compared
    // by path, an in-place rename of the OPEN file read as a different
    // document — so the title the user was typing was discarded at the one
    // moment they were most likely to be typing one.
    const heading = makeHeading('Old Name');
    const h = host('old.md', 7);
    h.ctrl.setSaved('Old Name', 'Filename');
    heading.focus();
    heading.textContent = 'My New Title';

    h.rename('renamed.md'); // same document, new path

    expect(h.ctrl.hasPendingEdit()).toBe(true);
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).toHaveBeenCalledWith('My New Title');
  });

  test('text typed while no file is open never lands on the file that opens next', async () => {
    // `getCurrentPath()` returns null whenever the registry has not resolved an
    // active entry, and focus can land in that window. Recording the identity
    // as a bare null made it indistinguishable from "nobody is editing", so the
    // orphan check answered "not orphaned" for the one text that has no
    // destination at all — and it committed onto whichever file opened next.
    const heading = makeHeading('');
    const h = host(null);
    heading.focus();
    heading.textContent = 'typed with nothing open';

    h.open('b.md');
    h.ctrl.setSaved('Title B', 'Filename');

    expect(heading.textContent).toBe('Title B');
    heading.blur();
    await Promise.resolve();
    expect(h.onCommit).not.toHaveBeenCalled();
  });
});
