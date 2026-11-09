// packages/moss-ui/src/ui/editor/editor-heading.ts
//
// The heading TEXT, its visibility and its SOURCE are owned by moss-core
// (`moss_core::heading::compute`, surfaced via the `compute_heading_state`
// Tauri command). The editor renders `compute().text` verbatim — there is no
// TS-side filename derivation. This module wires the contenteditable heading
// element's commit/keyboard behavior, and owns the one record that says what
// the heading currently means.
//
// WHY THERE IS A RECORD RATHER THAN A HANDFUL OF FLAGS
//
// The title is the only editing surface in the editor with no document model:
// it is a contenteditable that commits on blur, so between the first keystroke
// and leaving it the typed text exists nowhere but the DOM. Spreading that
// state across separate variables repeatedly produced the same bug shape: TWO
// GUARDS WRITTEN IN TERMS OF DIFFERENT PREDICATES — e.g. guarding a write on
// `activeElement` while guarding routing on "is there a pending edit" let the
// H1 go stale after an undo and stay stale.
//
// So the two facts that kept disagreeing are now ONE assignment each, and every
// predicate is derived from them:
//
//   `record`      what moss-core resolved, and WHICH DOCUMENT that answer is
//                 about. `resolved` is the same fact as `headingStateLoaded`,
//                 so the text and the permission to commit it can no longer
//                 drift.
//   `editingFor`  which document the text now in the element was typed AGAINST.
//
// Two things about the record earn their keep by being awkward:
//
//   - `resolved` is a field rather than the record being nulled, because there
//     are THREE states and nulling collapses two of them. A host with no
//     compute behind it commits whatever is typed; a host whose compute has
//     simply not answered yet must commit NOTHING, because everything it could
//     measure against belongs to the document being left. Nulling the record
//     said the first when it meant the second.
//   - identity is `getCurrentId()` (the EntryId), not the path, per the
//     project-wide rule: a path is a display label that moves on rename, and
//     the whole point of `editingFor` is to survive the document changing
//     underneath. Comparing paths made an in-place rename of the OPEN file
//     read as a different document, which discarded the title the user was
//     typing at the moment they were most likely to be typing one.
//
// `composing` survives as a term, but it is no longer an independent guess.
// Every predicate here is now defined against ONE question — "would flushing
// this produce a commit?" — and composition, orphaning and an unchanged string
// are three ways the answer is no. That question has its own field now.

import type { HeadingSavedState } from './heading-routing';

export type { HeadingSavedState };

/** Re-exported shape of the routing discriminant; see `HeadingSavedState`. */
type HeadingSource = HeadingSavedState['source'];

export type HeadingCommitResult =
  | { ok: true }
  | { ok: false; error: string };

/**
 * Stable identity of the open document. The EntryId, when the host has a
 * registry; the path when it does not (the email composer, tests). Compared
 * only for equality — this module never interprets it.
 */
export type DocumentId = string | number | null;

export interface HeadingContext {
  /** Returns the currently open file path, or null. */
  getCurrentPath(): string | null;
  /**
   * Identity of the open document — stable across a rename, unlike its path.
   * Optional: a host without a registry falls back to the path, which is
   * correct for a host that cannot rename underneath the heading anyway.
   */
  getCurrentId?(): DocumentId;
  /**
   * Called with the trimmed new heading text on blur (when text actually
   * changed and contains no slashes). Empty string is passed through to
   * the host — the host decides per HeadingSource what empty means
   * (rejected in filename-mode, written as `title: ""` in title-mode).
   */
  onCommit(newText: string): Promise<HeadingCommitResult>;
  /**
   * Move focus DOWN into the editor body with the caret at document start
   * (Obsidian inline-title parity). Called on Enter, and on ArrowDown when the
   * caret sits at the very end of the title. The host performs a FOCUS +
   * SELECTION move only (never a document change). Optional — when omitted,
   * Enter simply commits + blurs the title (legacy behavior; e.g. the email
   * composer has no CM6 body to descend into).
   */
  focusBody?(): void;
}

/**
 * True when the collapsed caret sits at the very end of the title's text — the
 * ArrowDown "descend into body" trigger. The title is a single-line
 * `contenteditable="plaintext-only"` (Enter never inserts a newline), so its
 * content is at most one text node. Defensive against jsdom's partial Selection
 * support: any missing/odd selection state returns false, so ArrowDown falls
 * through to its default (move within the title) rather than mis-firing.
 */
function caretAtEnd(el: HTMLElement): boolean {
  const sel = el.ownerDocument.getSelection?.();
  if (!sel || sel.rangeCount === 0 || !sel.isCollapsed) return false;
  const { focusNode, focusOffset } = sel;
  if (!focusNode || !el.contains(focusNode)) return false;
  // Caret anchored on the element itself: at end when past the last child node
  // (an empty title has 0 children, so offset 0 is both start and end).
  if (focusNode === el) return focusOffset === el.childNodes.length;
  // Caret inside the single text node: at end when the offset is its length.
  return focusOffset === (focusNode.textContent?.length ?? 0);
}

export interface HeadingController {
  /**
   * Record what moss-core resolved for the file that is open now, and show it.
   * ONE call raises "state loaded", captures the routing source and updates the
   * text — they described the same event and used to be three statements.
   *
   * Showing it is conditional, and decides for itself: it will not stomp an
   * edit in progress, will not replace the node an IME is composing into
   * (the text is deferred to `compositionend` instead), and will not touch the
   * DOM at all when the string is unchanged, because `textContent =` destroys
   * the caret even then.
   */
  setSaved(text: string, source: HeadingSource): void;
  /**
   * Correct the saved TEXT without re-deciding anything else, and show it on
   * the same terms `setSaved` does. For a host that knows the title moved but
   * is not the authority on how it routes — the untitled-first create flow,
   * which names the file through its own rename and uses this to blank the H1
   * so the "Untitled" ghost shows. No-op before the first compute lands.
   */
  noteSavedText(text: string): void;
  /**
   * Give back the answer `clear()` withdrew, unchanged. For the load that never
   * happened: the document the answer was about is still the open one, so the
   * withdrawal was a prediction that turned out false. No-op if a new answer has
   * since landed, or if there was never one.
   */
  restore(): void;
  /**
   * Withdraw what moss-core said: the open file is changing, or its compute
   * failed. `saved()` returns null again, which is what the host's commit
   * routing reads to refuse a commit whose source it cannot trust yet. The
   * text stays as the display baseline, so an untouched heading still knows it
   * is untouched — see `resolved` in the file header.
   */
  clear(): void;
  /** What moss-core last resolved, or null before the first compute lands. */
  saved(): HeadingSavedState | null;
  /**
   * Move focus UP into the title with the caret collapsed at the END of its
   * text (Obsidian inline-title parity — the mirror of `focusBody`'s descend).
   * Called by CM6's ArrowUp handler when the caret is an empty cursor on line 1.
   *
   * Returns `false` WITHOUT focusing when the title is hidden — the empty-title
   * state collapses it to `display:none` via `data-empty="true"` (editor.css)
   * and we must never focus an invisible element, so the caller lets CM6's
   * default ArrowUp run. Returns `true` once focus + caret placement succeed.
   */
  focusEnd(): boolean;
  /**
   * Is there text in the heading that the store has not been told about, for
   * THIS file?
   *
   * The heading commits on BLUR, so between the first keystroke and leaving it
   * the typed title exists nowhere but the DOM. ⌘Z must not walk the document
   * log while that is true — the log's top entry would be whatever the user did
   * BEFORE they started typing here, so the key would revert an older change
   * while the title they were editing sat untouchable and then landed anyway on
   * the next blur.
   *
   * Focus is not the same question: a focused but UNCHANGED heading has nothing
   * pending, and treating it as pending costs the user a dead keypress. Neither
   * is composition — a key event never reaches here mid-conversion. But it is a
   * necessary CONDITION, because `flushPendingEdit()` is a blur and a blur can
   * only resolve an edit the element is still holding.
   */
  hasPendingEdit(): boolean;
  /** Commit a pending edit now (via blur, the one commit path). */
  flushPendingEdit(): void;
}

export function wireEditorHeading(ctx: HeadingContext): HeadingController {
  const headingEl = document.getElementById('editor-heading') as HTMLElement | null;
  if (!headingEl)
    return {
      setSaved: () => {},
      noteSavedText: () => {},
      restore: () => {},
      clear: () => {},
      saved: () => null,
      focusEnd: () => false,
      hasPendingEdit: () => false,
      flushPendingEdit: () => {},
    };
  const el = headingEl;

  /**
   * What moss-core resolved, plus whether that answer is still current.
   *
   * `resolved: false` (after `clear()`) says a load has begun and the answer for
   * the document now arriving has not landed. `text` is then the PREVIOUS
   * document's title — still the right thing to leave on screen, since it is
   * what the user is looking at, and wrong to measure anything against. So an
   * unresolved record answers "no" to every question about the text: nothing is
   * pending, nothing commits, and nothing is repainted from it until the new
   * answer replaces it.
   */
  interface HeadingRecord extends HeadingSavedState {
    resolved: boolean;
  }

  let record: HeadingRecord | null = null;
  /**
   * Which document the text currently in the element is addressed to. Set when
   * the user takes the element, cleared when the edit resolves. Wrapped in an
   * object rather than held as a bare id, because `null` is a legitimate
   * IDENTITY (focus can land while no file is open, or while the registry is
   * still warming) and must not be confused with "nobody is editing" — read as
   * the latter it made the orphan check answer "not orphaned" for the one text
   * that has no destination at all.
   */
  let editingFor: { id: DocumentId } | null = null;
  /** IME transport only. Never a term in a predicate — see the file header. */
  let composing = false;
  /**
   * A `setSaved` whose paint could not happen yet — the IME held the node, or
   * the answer had lapsed. Stays raised until something can discharge it.
   */
  let deferred = false;
  /**
   * A `blur` that arrived mid-composition. The blur is the ONLY commit path, and
   * committing from inside a composition would write a half-converted candidate,
   * so it is refused there — but refusing it dropped the commit on the floor:
   * the element ends up unfocused with text nobody will ever write, which then
   * reads as a pending edit that `flushPendingEdit()` cannot flush (it blurs an
   * element that is not focused). Recorded here and discharged at
   * `compositionend`, which is the moment the refusal stops applying.
   */
  let commitOwed = false;

  const shownText = (): string => (el.textContent ?? '').trim();
  /** Identity of the open document; the path when the host has no registry. */
  const docId = (): DocumentId =>
    ctx.getCurrentId ? ctx.getCurrentId() : ctx.getCurrentPath();
  /** The element holds text addressed to a document that is no longer open. */
  const orphaned = (): boolean => editingFor !== null && editingFor.id !== docId();
  /**
   * Is there an answer to compare the element's text against? A record whose
   * answer has lapsed has a `text`, but it describes the document being left,
   * so measuring against it says nothing about this one. No record at all is a
   * different case — a host with no compute behind it commits normally, which
   * is what the empty-string baseline below is for.
   */
  const answered = (): boolean => record === null || record.resolved;
  /**
   * Would flushing commit something? Composition says no because `blur` returns
   * early during one; orphaning says no because `commit` refuses to write to a
   * file the text was not typed for; a lapsed answer says no because there is
   * nothing to compare against; an unchanged string says no because there is
   * nothing to write. Answering yes to any of them spends the user's ⌘Z on a
   * flush that does nothing.
   *
   * `editingFor !== null` leads, and it is the term that makes this balanced.
   * The other four are facts about STRINGS, and the only thing that lowers them
   * is the user retyping what was there before — while the only way to flush is
   * `blur`, a fact about FOCUS. So a `commit` that deliberately leaves text on
   * screen (a load in flight, an orphan) used to raise a pending edit that
   * nothing could lower: `flushPendingEdit()` blurs an element that is not
   * focused, which does nothing, and ⌘Z routed to that flush for the rest of the
   * session — every undo in the editor, including in the body, silently eaten.
   * Asking whether the element actually HOLDS the edit is the same question the
   * orphan check asks, so raise and lower now move together.
   */
  const pendingEdit = (): boolean =>
    editingFor !== null &&
    !composing &&
    !orphaned() &&
    answered() &&
    shownText() !== (record?.text ?? '');

  el.addEventListener('focus', () => { editingFor = { id: docId() }; });
  el.addEventListener('compositionstart', () => { composing = true; });
  el.addEventListener('compositionend', () => {
    composing = false;
    // Apply whatever arrived while the IME held the node. Dropping it was not
    // merely a lost update: `saved` had already advanced, so the next blur saw
    // "no change" and wrote the STALE text back over the value that had
    // arrived — an external edit silently reverted by finishing a conversion.
    // Re-reading `saved` rather than a captured string keeps the last writer
    // the winner when several arrive during one composition.
    // The blur this composition swallowed, first: what the user just finished
    // typing is the newest thing anyone has said about this title, so it wins
    // over an update that arrived mid-conversion.
    if (commitOwed) {
      // `deferred` is deliberately left alone. `commit()` settles the screen on
      // every path where it actually writes, but it also has paths where it
      // bails without painting — a load in flight, most of all — and lowering
      // the held paint on its behalf would strand it exactly as dropping it did.
      void commit();
      return;
    }
    if (!deferred) return;
    // …but not from an answer that has lapsed: during a load the record's text
    // is the previous document's title, and painting it here is the same wrong
    // -title flash `commit` and `Escape` refuse. HELD rather than dropped —
    // dropping it meant a load that then FAILED left the record holding a title
    // the screen never showed, and the next blur wrote the visible one back over
    // it: an external edit reverted by finishing a conversion, which is the very
    // thing this replay exists to prevent.
    if (!answered()) return;
    deferred = false;
    if (record) show(record.text);
  });

  /**
   * Put a newly-arrived saved text on screen — unless the element is busy with
   * it. `stale` and `pending` are read by the CALLER, before the record moves:
   * afterwards the comparison is against the new value and every edit looks
   * pending. Shared by the two ways a saved text arrives (`setSaved`,
   * `noteSavedText`) so neither can grow its own opinion about the write.
   */
  function reveal(text: string, stale: boolean, pending: boolean): void {
    // A pending edit addressed to a document that is no longer open is not an
    // edit to protect — it is text with no destination — so the incoming title
    // takes the element back. Asked FIRST, because both guards below would
    // otherwise preserve it.
    if (stale) {
      // Re-pointed, not cleared, when the element still has focus: `show()`
      // re-focuses an element that is already focused, which fires no `focus`
      // event, so nothing would ever raise this again — and everything the user
      // typed next would be addressed to no document at all, then committed
      // onto whichever one was open at the blur.
      editingFor = document.activeElement === el ? { id: docId() } : null;
      deferred = false;
      show(text);
      return;
    }
    if (pending) return;
    // DEFERRED, not dropped, while an IME composes: writing now would replace
    // the node the IME is composing into and abort the conversion, and
    // discarding is the silent revert described at `compositionend`.
    if (composing) {
      deferred = true;
      return;
    }
    deferred = false;
    show(text);
  }

  /** Put `text` on screen without disturbing a caret that is already in it. */
  function show(text: string): void {
    // `textContent =` is replace-all: it destroys the existing text node and
    // inserts a new one EVEN WHEN THE STRING IS IDENTICAL, and a live range
    // inside a removed node collapses to (parent, 0). Since every frontmatter
    // change notifies this module, an unconditional write sent the caret to the
    // start of the title on any chip edit, any external file change, and on the
    // very ⌘Z the update exists to reflect.
    if ((el.textContent ?? '') === text) return;
    const hadFocus = document.activeElement === el;
    el.textContent = text;
    // A genuinely different title replaced under the caret still has to leave
    // the caret somewhere sensible. End of the new text is the same place
    // `focusEnd` puts it for ArrowUp, and the same place typing would.
    if (hadFocus) focusEnd();
  }

  const commit = async (): Promise<void> => {
    const newDisplay = shownText();
    // Before the first compute lands there is no saved text, and the empty
    // string is the right stand-in: the commit still goes out and the host's
    // own `headingStateLoaded` guard is what refuses it, with a message. A host
    // with no compute at all (the email composer) commits normally.
    commitOwed = false;
    const wasEditing = editingFor;
    // Normally a commit means the user left, so nobody is editing. But an owed
    // commit is discharged at `compositionend`, which can arrive after the user
    // has clicked back IN — and `focus` will not fire again for an element that
    // never lost focus, so clearing this would leave everything typed next
    // addressed to no document, and ⌘Z walking the file's log while the title
    // holds an unwritten edit. Same rule as the orphan takeback in `reveal`.
    editingFor = document.activeElement === el ? { id: docId() } : null;
    // A load has begun and moss-core has not answered for the document arriving.
    // There is nothing to compare against — the record's text belongs to the
    // document being left — so emptying the heading in this window would read as
    // a deliberate change and go out as one, be refused for the same reason, and
    // repaint the OLD file's title into the new file's H1 behind an error
    // banner. Leaving the element exactly as the user left it is right: the
    // incoming `setSaved` paints it a moment later.
    if (record !== null && !record.resolved) return;
    const savedText = record?.text ?? '';
    // The document this text was addressed to is not the one open. There is
    // nowhere to put it: committing would write someone else's title into this
    // file's frontmatter, or rename this file to it. Restoring the open file's
    // own title is the only outcome that is not a write to the wrong document.
    if (wasEditing !== null && wasEditing.id !== docId()) {
      show(savedText);
      return;
    }
    if (newDisplay === savedText || newDisplay.includes('/') || !ctx.getCurrentPath()) {
      show(savedText);
      return;
    }
    // Which document this commit is FOR, captured before the await. A
    // Filename-mode commit renames the file and rewrites wikilinks across the
    // project, so it easily outlives a load — and everything below writes to the
    // heading or to the record, both of which by then describe whatever document
    // arrived in the meantime. (A rename does not change this token: identity is
    // the EntryId, which is the whole reason it is the EntryId.)
    // Deliberately not applyHeadingState's docGeneration: this commit IS the rename, so it must survive by identity, not by a counter scoped to loadFile's document-replacement lifecycle.
    const committingFor = docId();
    const result = await ctx.onCommit(newDisplay);
    if (docId() !== committingFor) return;
    if (!result.ok) {
      // Not while the user is back in the element typing something newer — the
      // repaint would wipe it, and because the text would then match the record
      // it would not even survive as a pending edit.
      if (editingFor === null) show(savedText);
      const err = document.createElement('div');
      err.className = 'editor-heading-error';
      err.textContent = result.error;
      el.parentElement?.insertBefore(err, el.nextSibling);
      setTimeout(() => err.remove(), 3000);
      return;
    }
    if (record) record = { ...record, text: newDisplay };
  };

  // Move the caret DOWN into the editor body (Obsidian inline-title parity):
  // commit the title first, then descend. Blurring the heading fires its blur
  // handler → commit(); we blur EXPLICITLY (rather than relying on the CM6
  // focus stealing it) so the commit is ordered before focus leaves and so this
  // still commits when no body is wired (focusBody omitted → legacy blur-only).
  const descendToBody = (): void => {
    el.blur();
    ctx.focusBody?.();
  };

  el.addEventListener('keydown', (e) => {
    // Respect IME composition: never hijack the Enter/Arrow that confirms an
    // in-progress candidate. The local `composing` flag can lag compositionend
    // on some IMEs, so `e.isComposing` is the belt-and-suspenders check.
    if (composing || e.isComposing) return;
    if (e.key === 'Enter') {
      // The title is single-line — Enter never inserts a newline. With a body
      // wired it moves the caret into CM6 at document start (position 0);
      // without one it just commits + blurs (unchanged legacy behavior).
      e.preventDefault();
      descendToBody();
    } else if (e.key === 'ArrowDown' && ctx.focusBody && caretAtEnd(el)) {
      // Caret at the end of the title + ArrowDown → descend into the body, same
      // as Enter. A mid-title ArrowDown falls through to move within the text.
      e.preventDefault();
      descendToBody();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      editingFor = null;
      // Same reason as `commit`: with no current answer there is no text to
      // restore, only the previous document's.
      if (answered()) show(record?.text ?? '');
      el.blur();
    }
  });

  el.addEventListener('blur', () => {
    if (composing) {
      commitOwed = true;
      return;
    }
    void commit();
  });

  // Move focus UP into the title with the caret collapsed at its END — the
  // mirror of `descendToBody`, triggered from CM6's ArrowUp on line 1. The
  // empty-title state hides the heading (`data-empty="true"` → `display:none`),
  // so refuse to focus it in that case and report failure; the CM6 caller then
  // lets its default ArrowUp run. Uses `selectNodeContents` + `collapse(false)`
  // so the caret lands past the last node regardless of whether the title holds
  // a text node (non-empty) or none (empty but visible). Defensive against
  // jsdom's partial Selection/Range support: if either is unavailable the focus
  // half still lands (the visible behavior) and we report success.
  const focusEnd = (): boolean => {
    if (el.dataset.empty === 'true') return false;
    el.focus();
    const doc = el.ownerDocument;
    const sel = doc.getSelection?.();
    const range = doc.createRange?.();
    if (sel && range) {
      range.selectNodeContents(el);
      range.collapse(false); // false → collapse to END of the contents
      sel.removeAllRanges();
      sel.addRange(range);
    }
    return true;
  };

  return {
    setSaved: (text: string, source: HeadingSource) => {
      // No open file: there is nothing for an answer to be about. Withdrawn,
      // not destroyed — nulling the record also threw away the display baseline
      // and the source, which `clear()`/`restore()` could then never bring back,
      // so an untouched heading read as an edit and blanked itself behind an
      // error banner.
      if (ctx.getCurrentPath() === null) {
        if (record) record = { ...record, resolved: false };
        return;
      }
      const stale = orphaned();
      // Read before the record moves — see `reveal`.
      //
      // The guard used to be `activeElement !== el`, which is a stricter
      // question than the one that matters and dropped a real update on the
      // floor: with the caret parked in an UNCHANGED title, ⌘Z routes to the
      // document log (correctly — there is nothing to flush), the undo lands in
      // the file, the preview and the title chip, and this line then refused to
      // move the H1. Blurring did not repair it either, because `commit()` sees
      // no change and writes the stale text back. The heading kept showing a
      // title the file no longer had until some unrelated frontmatter edit
      // happened to arrive while it was unfocused, at which point it silently
      // snapped.
      const pending = pendingEdit();
      record = { text, source, resolved: true };
      reveal(text, stale, pending);
    },
    noteSavedText: (text: string) => {
      if (!record) return;
      const stale = orphaned();
      const pending = pendingEdit();
      record = { ...record, text };
      // Shows it, like `setSaved` does. This seam is how the untitled-first
      // flow BLANKS the H1 so the "Untitled" ghost appears, and a version of
      // it that only moved the number left the previous file's title on screen
      // over an empty saved text — which then read as a pending edit and
      // committed that title, renaming the new file to it.
      reveal(text, stale, pending);
    },
    clear: () => {
      // Withdraw the answer, keep the string. See `resolved` in the header.
      if (record) record = { ...record, resolved: false };
    },
    restore: () => {
      // The load this was clearing for never happened, and the document the
      // answer describes is still open — so the answer is still true. Without
      // this, a read that fails partway leaves the heading of a document the
      // user is still looking at permanently uncommittable, with nothing on
      // screen to say so and nothing able to raise the bit again.
      if (!record) return;
      record = { ...record, resolved: true };
      // A paint held back because the answer had lapsed can happen now.
      if (deferred && !composing) {
        deferred = false;
        show(record.text);
      }
    },
    // Narrowed to the public shape on the way out: `resolved` is how this
    // module keeps its own promises, and a caller that could read it would be
    // back to deciding for itself what the heading means.
    saved: () =>
      record?.resolved
        ? { text: record.text, source: record.source }
        : null,
    focusEnd,
    hasPendingEdit: pendingEdit,
    // Blur, not `commit()` directly: blur is the one commit path, and routing
    // through it keeps a flush indistinguishable from the user clicking away.
    flushPendingEdit: () => el.blur(),
  };
}
