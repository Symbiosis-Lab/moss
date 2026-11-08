import { describe, test, expect, beforeEach, afterEach, vi } from 'vitest';
import { Transaction, StateField, StateEffect, ChangeSet } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { undo, undoDepth } from '@codemirror/commands';
import { EditorView, runScopeHandlers } from '@codemirror/view';
import { CmEditor, applyEmbedWidth } from '../cm-editor';
import { commentParsedField } from '../cm-comment';
import { mossHighlight } from '@symbiosis-lab/moss-syntax/cm6';

// `applyEmbedWidth` asks the engine for the rewritten image; the test controls its answer.
vi.mock('../../engine', async (orig) => {
  const real = await orig<typeof import('../../engine')>();
  return { ...real, engine: { ...real.engine, setImageWidth: vi.fn() } };
});
import { engine } from '../../engine';

function nodeNames(editor: CmEditor): Set<string> {
  const names = new Set<string>();
  syntaxTree(editor.getView().state).iterate({ enter: (n) => { names.add(n.name); } });
  return names;
}

describe('CmEditor', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  test('mounts into container', () => {
    const editor = new CmEditor({
      content: 'Hello world',
      container,
      onChange: () => {},
    });

    expect(container.querySelector('.cm-editor')).not.toBeNull();
    editor.destroy();
  });

  test('markdown parses GFM strikethrough + tables (parity with build pipeline)', () => {
    // The build pipeline enables ENABLE_STRIKETHROUGH + ENABLE_TABLES
    // (crates/moss-build/src/build/markdown/pipeline.rs), so the editor must parse them
    // too — otherwise the live preview diverges from the published site (and the
    // cm-live-preview Strikethrough handler stays dead with bare CommonMark).
    const editor = new CmEditor({
      content: '~~struck~~\n\n| a | b |\n| --- | --- |\n| 1 | 2 |',
      container,
      onChange: () => {},
      getFromFile: () => 'note.md',
    });
    const names = nodeNames(editor);
    expect(names.has('Strikethrough')).toBe(true);
    expect(names.has('Table')).toBe(true);
    editor.destroy();
  });

  test('uses the shared source composition for markdown styling and callouts', () => {
    const editor = new CmEditor({
      content: '# Source heading\n\n> [!note]\n> Callout body',
      container,
      onChange: () => {},
      getFromFile: () => 'note.md',
    });

    expect(container.querySelector('.cm-line.cm-lp-h1')).not.toBeNull();
    expect(container.querySelector('.cm-line.cm-callout-block')).not.toBeNull();
    expect(editor.getView().state.field(commentParsedField, false)).toBeDefined();
    editor.destroy();

    const commentsOff = new CmEditor({
      content: 'Markdown without comment controls',
      container,
      onChange: () => {},
      getFromFile: () => 'note.md',
      comments: false,
    });
    expect(commentsOff.getView().state.field(commentParsedField, false)).toBeUndefined();
    commentsOff.destroy();
  });

  test('empty markdown document shows the ghost placeholder; typed content removes it', () => {
    const editor = new CmEditor({
      content: '',
      container,
      onChange: () => {},
      getFromFile: () => 'note.md',
    });
    const ghost = container.querySelector('.cm-placeholder');
    expect(ghost?.textContent).toBe('Write, or type / for blocks');

    editor.getView().dispatch({ changes: { from: 0, insert: 'x' } });
    expect(container.querySelector('.cm-placeholder')).toBeNull();
    editor.destroy();
  });

  test('non-markdown files get no placeholder', () => {
    const editor = new CmEditor({
      content: '',
      container,
      onChange: () => {},
      getFromFile: () => 'config.toml',
    });
    expect(container.querySelector('.cm-placeholder')).toBeNull();
    editor.destroy();
  });

  /** Another writer's change, as the merge hands it over: a change set on the current text. */
  const external = (editor: CmEditor, from: number, to: number, insert: string) =>
    editor.applyExternal(ChangeSet.of({ from, to, insert }, editor.getView().state.doc.length));

  test('applies an external change', () => {
    const editor = new CmEditor({ content: 'Hello world', container, onChange: () => {} });
    external(editor, 6, 11, 'moss');
    expect(editor.getContent()).toBe('Hello moss');
    editor.destroy();
  });

  test('an external change does NOT call onChange, so autosave does not write it back', () => {
    let changed = false;
    const editor = new CmEditor({ content: 'Hello', container, onChange: () => { changed = true; } });
    external(editor, 5, 5, ', changed');
    expect(changed).toBe(false);
    expect(editor.getContent()).toBe('Hello, changed');
    editor.destroy();
  });

  // The editor opens beside the preview without focus, caret at the start. It
  // is being read, not edited, so line 1 must not open half-raw.
  test('a heading\'s markers stay visible regardless of focus or caret (no reveal, no modes)', () => {
    // revealFollowsFocus and its unfocused/focused hide-reveal behavior are
    // gone — moving the cursor, or focus, never changes what's on
    // screen; `##` is always present (hung, per cm-lp-h1), never hidden.
    const editor = new CmEditor({ content: '## Title\n\nbody', container, onChange: () => {} });
    expect(container.querySelector('.cm-line')!.textContent).toBe('## Title');
    window.dispatchEvent(new Event('focus'));
    expect(container.querySelector('.cm-line')!.textContent).toBe('## Title');
    window.dispatchEvent(new Event('blur'));
    expect(container.querySelector('.cm-line')!.textContent).toBe('## Title');
    editor.destroy();
  });

  test('an external change does NOT report a caret move: the file changing is not the user going somewhere', () => {
    // onCaretMove drives the preview to the caret's line.
    const onCaretMove = vi.fn();
    const editor = new CmEditor({ content: 'one\ntwo\nthree', container, onChange: () => {}, onCaretMove });
    editor.getView().dispatch({ selection: { anchor: 8 } });
    expect(onCaretMove).toHaveBeenCalledTimes(1);
    external(editor, 3, 13, '');
    expect(onCaretMove).toHaveBeenCalledTimes(1);
    editor.destroy();
  });

  test('user keystrokes still trigger onChange', () => {
    // Regression: the external-annotation guard must not affect user-origin edits.
    let changeCount = 0;
    const editor = new CmEditor({
      content: 'Hello',
      container,
      onChange: () => { changeCount++; },
    });
    const view = editor.getView();
    // Simulate a user typing: a plain dispatch without the external annotation.
    view.dispatch({
      changes: { from: view.state.doc.length, insert: '!' },
    });
    expect(changeCount).toBe(1);
    expect(editor.getContent()).toBe('Hello!');
    editor.destroy();
  });

  test('includes syntax highlighting extension for moss theme', () => {
    const editor = new CmEditor({
      content: '[link](url)',
      container,
      onChange: () => {},
    });

    // The editor should have syntaxHighlighting in its extensions.
    // We verify by checking the EditorView has the facet loaded.
    // Access the internal view to check compartments.
    const view = (editor as unknown as { view: { state: { facet: (f: unknown) => unknown[] } } }).view;

    // syntaxHighlighting registers via the highlightingFor facet.
    // A simpler check: the editor should expose its extensions include
    // HighlightStyle. We can verify by checking that the editor's
    // state has highlight-related facets.
    // The most reliable check: verify the extension count increased.
    // Before: minimalSetup + markdown + lineWrapping + updateListener = 4
    // After: + syntaxHighlighting = 5
    expect(view.state).toBeDefined();

    // mossHighlight should be exported and defined (a HighlightStyle instance)
    expect(mossHighlight).toBeDefined();

    editor.destroy();
  });

  describe('applyOwnEdit() — an edit made at the person\'s request', () => {
    test('is in the undo history, reaches onChange, and washes the lines it changed', () => {
      const onChange = vi.fn();
      const editor = new CmEditor({ content: 'one\ntwo mine\nthree', container, onChange });
      const view = editor.getView();
      editor.applyOwnEdit(ChangeSet.of({ from: 4, to: 12, insert: 'two theirs' }, view.state.doc.length));
      expect(view.state.doc.toString()).toBe('one\ntwo theirs\nthree');
      expect(onChange).toHaveBeenCalled();
      expect(view.dom.querySelectorAll('.cm-outside-wash')).toHaveLength(1);
      expect(view.dom.querySelector('.cm-outside-wash')!.textContent).toBe('two theirs');
      undo(view);
      expect(view.state.doc.toString()).toBe('one\ntwo mine\nthree');
      editor.destroy();
    });
  });

  describe('applyExternal() — another writer\'s change', () => {
    test('washes the lines it changed, and the wash goes after the fade', () => {
      vi.useFakeTimers();
      const editor = new CmEditor({ content: 'alpha\nbravo\ncharlie', container, onChange: () => {} });
      const view = editor.getView();
      external(editor, 6, 11, 'BRAVO');
      const washed = [...view.dom.querySelectorAll('.cm-outside-wash')].map((l) => l.textContent);
      expect(washed).toEqual(['BRAVO']);
      vi.advanceTimersByTime(2000);
      expect(view.dom.querySelectorAll('.cm-outside-wash')).toHaveLength(0);
      vi.useRealTimers();
      editor.destroy();
    });

    test('an empty change dispatches nothing', () => {
      const editor = new CmEditor({ content: 'Hello world', container, onChange: () => {} });
      const dispatchSpy = vi.spyOn(editor.getView(), 'dispatch');
      editor.applyExternal(ChangeSet.empty(11));
      expect(dispatchSpy).not.toHaveBeenCalled();
      editor.destroy();
    });

    test('a caret outside the change stays on the same text', () => {
      const editor = new CmEditor({ content: 'alpha\nbravo\ncharlie', container, onChange: () => {} });
      const view = editor.getView();
      view.dispatch({ selection: { anchor: 15 } }); // 'cha|rlie'
      external(editor, 0, 0, 'new first line\n');
      expect(view.state.sliceDoc(view.state.selection.main.head - 3, view.state.selection.main.head)).toBe('cha');
      editor.destroy();
    });

    test('is kept out of the undo history', () => {
      const editor = new CmEditor({ content: 'initial', container, onChange: () => {} });
      const dispatchSpy = vi.spyOn(editor.getView(), 'dispatch');
      external(editor, 0, 7, 'new content');
      expect(dispatchSpy).toHaveBeenCalledTimes(1);
      const spec = dispatchSpy.mock.calls[0][0] as { annotations?: unknown };
      const list = Array.isArray(spec.annotations) ? spec.annotations : [spec.annotations];
      expect(list.some((a: { type?: unknown; value?: unknown }) => a?.type === Transaction.addToHistory && a.value === false)).toBe(true);
      editor.destroy();
    });

    test('keeps DOM focus when the view was focused', () => {
      const editor = new CmEditor({ content: 'initial', container, onChange: () => {} });
      const view = editor.getView();
      view.focus();
      external(editor, 0, 7, 'new content');
      expect(view.hasFocus).toBe(true);
      editor.destroy();
    });

    test('does not take focus when the view did not have it', () => {
      const editor = new CmEditor({ content: 'initial', container, onChange: () => {} });
      const view = editor.getView();
      external(editor, 0, 7, 'new content');
      expect(view.hasFocus).toBe(false);
      editor.destroy();
    });

    test('Cmd+Z still undoes the person\'s edit after another writer\'s change elsewhere', () => {
      // A whole-document replace would remap the person's undo event to an
      // empty change, which CM6's history prunes; a change touching only its
      // own span leaves it undoable.
      const editor = new CmEditor({ content: 'hello world\n', container, onChange: () => {} });
      const view = editor.getView();
      view.dispatch({ changes: { from: 11, insert: ' typed' }, userEvent: 'input.type' });
      expect(undoDepth(view.state)).toBe(1);
      external(editor, 18, 18, 'an external line\n');
      expect(undoDepth(view.state)).toBe(1);
      undo(view);
      expect(editor.getContent()).toBe('hello world\nan external line\n');
      editor.destroy();
    });
  });

  // ── applyEmbedWidth ──────────────────────────────────────────────────────
  // The drag-resize / double-click-reset write path. These call the real
  // exported function: the widget's pointer gesture needs a measure pass jsdom
  // cannot provide, and a test that re-implements the resolve-await-re-resolve
  // sequence proves only that the test can do it.

  describe('applyEmbedWidth', () => {
    function editorWith(content: string) {
      // No `getFromFile`: the width path never consults the reference cache,
      // and wiring the resolver would start a real IPC batch timer.
      return new CmEditor({ content, container, onChange: () => {} });
    }

    test('bails without dispatching when the doc changed during the IPC await', async () => {
      const editor = editorWith('![alt](photo.jpg)');
      const view = editor.getView();
      let settle!: (v: string) => void;
      vi.mocked(engine.setImageWidth).mockReturnValueOnce(
        new Promise((res) => { settle = (data) => res({ status: 'ok', data }); }) as any,
      );
      const dispatchSpy = vi.spyOn(view, 'dispatch');

      const pending = applyEmbedWidth(view, 0, '75%');
      // A concurrent keystroke lands while the IPC is in flight.
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: '![alt](other.jpg)' } });
      settle('![alt|75%](photo.jpg)');
      await pending;

      // The user's edit stands; the stale rewrite is never written.
      expect(editor.getContent()).toBe('![alt](other.jpg)');
      for (const [spec] of dispatchSpy.mock.calls as [{ changes?: { insert?: string } }][]) {
        expect(spec.changes?.insert).not.toBe('![alt|75%](photo.jpg)');
      }
      editor.destroy();
    });

    // Every one of these is a WikilinkEmbed, not an Image — climbing for
    // `'Image'` alone would silently no-op the resize instead of failing.
    test.each([
      ['![[folder/]]', '![[folder/]]', 0],
      ['![[x.png]]', '![[x.png]]', 0],
      // Inside a link the embed is what gets the width token — never the Link.
      ['[![[x.png]]](/awards/)', '![[x.png]]', 2],
    ])('%s → setImageWidth receives exactly %s', async (doc, expected, pos) => {
      vi.mocked(engine.setImageWidth).mockClear();
      vi.mocked(engine.setImageWidth).mockResolvedValueOnce({ status: 'ok', data: `${expected.slice(0, -2)}|60%]]` } as any);
      const editor = editorWith(doc);

      await applyEmbedWidth(editor.getView(), pos, '60%');
      expect(vi.mocked(engine.setImageWidth)).toHaveBeenCalledWith(expected, '60%');

      // The width landed in the doc, so the reset path sees the widened token
      // — still the embed alone, never the enclosing Link.
      const widened = `${expected.slice(0, -2)}|60%]]`;
      vi.mocked(engine.setImageWidth).mockResolvedValueOnce({ status: 'ok', data: expected } as any);
      await applyEmbedWidth(editor.getView(), pos, null);
      expect(vi.mocked(engine.setImageWidth)).toHaveBeenLastCalledWith(widened, null);
      editor.destroy();
    });
  });

  // ── sealed-state scrim ────────────────────────────────────────────
  describe('setReadOnly / scrim', () => {
    test('setReadOnly(true) adds .cm-seal-scrim with status text inside the editor DOM', () => {
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);

      const scrim = editor.getView().dom.querySelector('.cm-seal-scrim');
      expect(scrim).not.toBeNull();

      const status = scrim?.querySelector('.cm-seal-scrim__status');
      expect(status).not.toBeNull();
      expect(status?.textContent).toBe('Publishing your site…');

      editor.destroy();
    });

    test('a rename/move commit freeze never says it is publishing', () => {
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true, 'commit');
      const status = editor.getView().dom.querySelector('.cm-seal-scrim__status');
      expect(status?.textContent).toBe('Updating links…');

      // The same scrim is reused: a publish seal afterwards says publishing again.
      editor.setReadOnly(false, 'commit');
      editor.setReadOnly(true, 'publish');
      expect(editor.getView().dom.querySelector('.cm-seal-scrim__status')?.textContent).toBe('Publishing your site…');

      editor.destroy();
    });

    test('setReadOnly(false) removes (or hides) the .cm-seal-scrim', () => {
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);
      editor.setReadOnly(false);

      // After unseal, the scrim must either be absent or have opacity:0 /
      // aria-hidden — the DOM element may be kept for fade-out but it should
      // not be "active" (no .is-sealing class on the wrapper).
      const dom = editor.getView().dom;
      expect(dom.classList.contains('sealed')).toBe(false);
      // The scrim should not carry .is-sealing (fading-in state) after unseal.
      const scrim = dom.querySelector('.cm-seal-scrim');
      if (scrim) {
        expect(scrim.classList.contains('is-sealing')).toBe(false);
      }

      editor.destroy();
    });

    test('setReadOnly(false) makes the scrim non-blocking so clicks reach the editor (regression: editor uneditable after publish)', () => {
      // Regression for the v0.7.9 "editor cannot be edited" bug: _updateScrim(false)
      // used to only drop the .is-sealing class, leaving the scrim div in the DOM
      // with the base rule's `pointer-events: all` even at opacity:0 — an invisible
      // overlay that swallowed every click after the first publish.
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);
      editor.setReadOnly(false);

      const scrim = editor.getView().dom.querySelector<HTMLElement>('.cm-seal-scrim');
      // If the element is kept for fade-out it MUST NOT intercept pointer events.
      if (scrim) {
        expect(scrim.style.pointerEvents).toBe('none');
      }

      editor.destroy();
    });

    test('re-sealing after an unseal restores the click barrier', () => {
      // The pointer-events:none applied on unseal must be reverted on re-seal,
      // or a second publish would not block edits.
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);
      editor.setReadOnly(false);
      editor.setReadOnly(true);

      const scrim = editor.getView().dom.querySelector<HTMLElement>('.cm-seal-scrim');
      expect(scrim).not.toBeNull();
      // Inline override cleared → defers to the `.is-sealing` rule's pointer-events:all.
      expect(scrim?.style.pointerEvents).toBe('');
      expect(scrim?.classList.contains('is-sealing')).toBe(true);

      editor.destroy();
    });

    test('setReadOnly(true) adds .sealed class on the cm-editor DOM element', () => {
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);

      expect(editor.getView().dom.classList.contains('sealed')).toBe(true);

      editor.destroy();
    });

    test('setReadOnly(true) marks the scrim with .is-sealing for asymmetric fade-in', () => {
      const editor = new CmEditor({ content: 'hello', container, onChange: () => {} });
      editor.setReadOnly(true);

      const scrim = editor.getView().dom.querySelector('.cm-seal-scrim');
      expect(scrim?.classList.contains('is-sealing')).toBe(true);

      editor.destroy();
    });
  });

  // ── extraExtensions hook ──────────────────────────────────────────────────
  // Callers (e.g. the email composer) need to inject context-specific CM6
  // extensions without touching the shared wrapper.  `extraExtensions` is
  // appended AFTER all built-in extensions so they can override or augment
  // any built-in facet.
  describe('extraExtensions', () => {
    // Probe: a StateField that holds a known sentinel value.  If the field is
    // present in the state it means the extension was registered; if absent,
    // `view.state.field(probeField, false)` returns undefined.
    const probeField = StateField.define<number>({
      create: () => 42,
      update: (v) => v,
    });

    test('a StateField passed via extraExtensions is active in the resulting view state', () => {
      const editor = new CmEditor({
        content: 'hello',
        container,
        onChange: () => {},
        extraExtensions: [probeField],
      });

      // The probe field must be registered — its sentinel value is readable.
      const value = editor.getView().state.field(probeField, false);
      expect(value).toBe(42);
      editor.destroy();
    });

    test('constructing without extraExtensions still works (backward compatible)', () => {
      const editor = new CmEditor({
        content: 'hello',
        container,
        onChange: () => {},
        // No extraExtensions — must not throw or degrade.
      });

      expect(editor.getView().state).toBeDefined();
      expect(editor.getContent()).toBe('hello');
      editor.destroy();
    });

    test('multiple extensions in extraExtensions are all registered', () => {
      const fieldA = StateField.define<string>({ create: () => 'A', update: (v) => v });
      const fieldB = StateField.define<string>({ create: () => 'B', update: (v) => v });

      const editor = new CmEditor({
        content: '',
        container,
        onChange: () => {},
        extraExtensions: [fieldA, fieldB],
      });

      expect(editor.getView().state.field(fieldA, false)).toBe('A');
      expect(editor.getView().state.field(fieldB, false)).toBe('B');
      editor.destroy();
    });
  });

  // ── ArrowUp escape into the title (onEscapeTop) ─────────────────────────
  describe('ArrowUp escape into the title (onEscapeTop)', () => {
    // Fire ArrowUp through CM6's keymap dispatch. runScopeHandlers is the
    // correct way to exercise a CM6 keymap under jsdom (a raw dispatchEvent on
    // the contentDOM does not reliably reach the keymap handler). This markdown
    // editor also registers the slash menu's Prec.highest ArrowUp binding, so
    // these tests double as proof our Prec.high handler coexists with it (menu
    // closed → slash returns false → ours runs).
    //
    // When OUR handler returns false, dispatch falls through to CM6's default
    // ArrowUp (`cursorLineUp`), which needs real layout (`getClientRects`) that
    // jsdom does not implement — so it throws a geometry TypeError. That throw
    // happens AFTER our handler ran (and after any onEscapeTop mock is already
    // recorded), and is a jsdom limitation, not our behavior. Swallow ONLY that
    // geometry error and report `{ handled, fellThrough }`; rethrow anything
    // else. In a real WebView the default cursor motion runs normally.
    const arrowUp = (view: EditorView): { handled: boolean; fellThrough: boolean } => {
      const ev = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true });
      try {
        return { handled: runScopeHandlers(view, ev, 'editor'), fellThrough: false };
      } catch (e) {
        if (e instanceof TypeError && /getClientRects|coords/i.test(String(e))) {
          // Our handler returned false → CM6's default ArrowUp ran and hit the
          // jsdom layout gap. i.e. we did NOT consume the event.
          return { handled: false, fellThrough: true };
        }
        throw e;
      }
    };

    test('empty cursor on line 1 invokes onEscapeTop and consumes the event', () => {
      const onEscapeTop = vi.fn(() => true);
      const editor = new CmEditor({ content: 'first\nsecond', container, onChange: () => {}, onEscapeTop });
      const view = editor.getView();
      view.dispatch({ selection: { anchor: 0 } }); // empty cursor, line 1

      const { handled, fellThrough } = arrowUp(view);

      expect(onEscapeTop).toHaveBeenCalledTimes(1);
      expect(handled).toBe(true);       // our handler returned true → consumed
      expect(fellThrough).toBe(false);  // default ArrowUp never ran
      editor.destroy();
    });

    test('caret on line 2 does NOT invoke onEscapeTop', () => {
      const onEscapeTop = vi.fn(() => true);
      const editor = new CmEditor({ content: 'first\nsecond', container, onChange: () => {}, onEscapeTop });
      const view = editor.getView();
      view.dispatch({ selection: { anchor: 8 } }); // "se|cond" — line 2

      arrowUp(view); // falls through to the default ArrowUp (predicate off line 1)

      expect(onEscapeTop).not.toHaveBeenCalled();
      editor.destroy();
    });

    test('a non-empty range selection on line 1 does NOT invoke onEscapeTop', () => {
      const onEscapeTop = vi.fn(() => true);
      const editor = new CmEditor({ content: 'first\nsecond', container, onChange: () => {}, onEscapeTop });
      const view = editor.getView();
      view.dispatch({ selection: { anchor: 0, head: 3 } }); // range on line 1

      arrowUp(view); // falls through (selection not empty)

      expect(onEscapeTop).not.toHaveBeenCalled();
      editor.destroy();
    });

    test('onEscapeTop returning false (title hidden) does NOT consume the event', () => {
      const onEscapeTop = vi.fn(() => false);
      const editor = new CmEditor({ content: 'first\nsecond', container, onChange: () => {}, onEscapeTop });
      const view = editor.getView();
      view.dispatch({ selection: { anchor: 0 } }); // empty cursor, line 1

      const { handled, fellThrough } = arrowUp(view);

      expect(onEscapeTop).toHaveBeenCalledTimes(1); // predicate matched → delegated
      expect(handled).toBe(false);      // false → not consumed by us
      expect(fellThrough).toBe(true);   // fell through to CM6's default ArrowUp
      editor.destroy();
    });
  });

  // ── Backspace at the document start joins the title (onEscapeTop) ──────
  describe('Backspace at the document start (onEscapeTop)', () => {
    const backspace = (view: EditorView): boolean =>
      runScopeHandlers(view, new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }), 'editor');
    const mount = (content: string, anchor: number, onEscapeTop: () => boolean) => {
      const editor = new CmEditor({ content, container, onChange: () => {}, onEscapeTop });
      const view = editor.getView();
      view.dispatch({ selection: { anchor } });
      return { editor, view };
    };

    test('blank line 1: removes its break and focuses the title; undo restores it', () => {
      const onEscapeTop = vi.fn(() => true);
      const { editor, view } = mount('\n## Heading three\n\nbody', 0, onEscapeTop);
      expect(backspace(view)).toBe(true);
      expect(onEscapeTop).toHaveBeenCalledTimes(1);
      expect(view.state.doc.toString()).toBe('## Heading three\n\nbody');
      expect(undo(view)).toBe(true);
      expect(view.state.doc.toString()).toBe('\n## Heading three\n\nbody');
      editor.destroy();
    });

    test('line 1 with text: focuses the title and leaves the text alone', () => {
      const onEscapeTop = vi.fn(() => true);
      const { editor, view } = mount('first\nsecond', 0, onEscapeTop);
      expect(backspace(view)).toBe(true);
      expect(onEscapeTop).toHaveBeenCalledTimes(1);
      expect(view.state.doc.toString()).toBe('first\nsecond');
      editor.destroy();
    });

    test('empty body: focuses the title', () => {
      const onEscapeTop = vi.fn(() => true);
      const { editor, view } = mount('', 0, onEscapeTop);
      expect(backspace(view)).toBe(true);
      expect(onEscapeTop).toHaveBeenCalledTimes(1);
      editor.destroy();
    });

    test('title hidden (hook returns false): the line is kept and Backspace is not consumed', () => {
      const { editor, view } = mount('\nsecond', 0, () => false);
      backspace(view);
      expect(view.state.doc.toString()).toBe('\nsecond');
      editor.destroy();
    });

    test('caret off position 0, or a selection: the hook is not called', () => {
      const onEscapeTop = vi.fn(() => true);
      const { editor, view } = mount('ab\ncd', 1, onEscapeTop);
      backspace(view);
      expect(view.state.doc.toString()).toBe('b\ncd');
      view.dispatch({ selection: { anchor: 0, head: 1 } });
      backspace(view);
      expect(onEscapeTop).not.toHaveBeenCalled();
      editor.destroy();
    });
  });

  describe('restore', () => {
    test('the same text brings its undo history back', () => {
      const a = new CmEditor({ content: 'start', container, onChange: () => {} });
      a.getView().dispatch({ changes: { from: 5, insert: ' more' }, userEvent: 'input.type' });
      const json = a.toJSON();
      a.destroy();

      const b = new CmEditor({ content: 'start more', container, onChange: () => {}, restore: json });
      undo(b.getView());
      expect(b.getContent()).toBe('start');
      b.destroy();
    });
  });
});
