// packages/moss-ui/src/ui/editor/__tests__/cm-comment.test.ts
import { describe, it, expect, afterEach } from 'vitest';
import { EditorState, EditorSelection } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { parseComments, commentDecorations, commentExtension, commentGutterMarkers, insertCommentAndEdit, resolveCommentChange, sanitizeNote, parsePercentComments, toggleObsidianComment } from '../cm-comment';

function state(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown()] });
}

/** Run the Obsidian %% toggle over (doc, selection); return the resulting doc + main selection. */
function runToggle(doc: string, anchor: number, head = anchor): { doc: string; from: number; to: number } {
  const view = new EditorView({
    state: EditorState.create({ doc, selection: EditorSelection.single(anchor, head), extensions: [markdown()] }),
  });
  toggleObsidianComment(view);
  const r = view.state.selection.main;
  const out = { doc: view.state.doc.toString(), from: r.from, to: r.to };
  view.destroy();
  return out;
}

describe('toggleObsidianComment (⌘/)', () => {
  it('wraps a selection in %%…%%', () => {
    const r = runToggle('hello world', 6, 11); // select "world"
    expect(r.doc).toBe('hello %%world%%');
  });

  it('unwraps a selected %%…%% comment', () => {
    const r = runToggle('hello %%world%%', 6, 15); // select "%%world%%"
    expect(r.doc).toBe('hello world');
  });

  it('with no selection, comments the current line', () => {
    const r = runToggle('hello', 2); // cursor inside "hello"
    expect(r.doc).toBe('%%hello%%');
  });

  it('with no selection, uncomments a %%…%% line', () => {
    const r = runToggle('%%hello%%', 4); // cursor inside the comment
    expect(r.doc).toBe('hello');
  });

  it('is a round-trip (comment then uncomment restores the text)', () => {
    const once = runToggle('draft note', 0, 10);
    expect(once.doc).toBe('%%draft note%%');
    const twice = runToggle(once.doc, once.from, once.to);
    expect(twice.doc).toBe('draft note');
  });

  it('does not insert an HTML comment', () => {
    const r = runToggle('para', 0, 4);
    expect(r.doc).not.toContain('<!--');
    expect(r.doc).toContain('%%');
  });
});

describe('parseComments', () => {
  it('pairs an adjacent highlight+comment into a span comment', () => {
    const doc = 'a {==brown fox==}{>>too vague<<} z';
    const cs = parseComments(state(doc));
    expect(cs).toHaveLength(1);
    const c = cs[0];
    expect(doc.slice(c.from, c.to)).toBe('{==brown fox==}{>>too vague<<}');
    expect(doc.slice(c.spanFrom!, c.spanTo!)).toBe('brown fox');
    expect(doc.slice(c.noteFrom, c.noteTo)).toBe('too vague');
    expect(c.line).toBe(1);
  });

  it('treats a lone comment as a point comment (no span)', () => {
    const doc = 'first line\nsecond {>>note here<<} line';
    const cs = parseComments(state(doc));
    expect(cs).toHaveLength(1);
    expect(cs[0].spanFrom).toBeUndefined();
    expect(doc.slice(cs[0].noteFrom, cs[0].noteTo)).toBe('note here');
    expect(cs[0].line).toBe(2);
  });

  it('does NOT pair when whitespace separates highlight and comment', () => {
    const doc = '{==text==} {>>note<<}';
    const cs = parseComments(state(doc));
    // highlight is unpaired (ignored); the comment is a point comment
    expect(cs).toHaveLength(1);
    expect(cs[0].spanFrom).toBeUndefined();
  });

  it('finds multiple comments on one line in document order', () => {
    const doc = '{>>one<<} mid {==x==}{>>two<<}';
    const cs = parseComments(state(doc));
    expect(cs.map(c => doc.slice(c.noteFrom, c.noteTo))).toEqual(['one', 'two']);
  });

  it('ignores CriticMarkup inside code spans (parseMarks masks code)', () => {
    const doc = 'text `{>>not a comment<<}` more';
    expect(parseComments(state(doc))).toHaveLength(0);
  });

  it('greedy-left pairing: comment pairs IMMEDIATELY-preceding highlight only', () => {
    // {==a==} is left unpaired; {==b==}{>>c<<} is paired as a span comment
    const doc = '{==a==}{==b==}{>>c<<}';
    const cs = parseComments(state(doc));
    expect(cs).toHaveLength(1);
    expect(cs[0].spanFrom).toBeDefined();
    expect(doc.slice(cs[0].noteFrom, cs[0].noteTo)).toBe('c');
    expect(doc.slice(cs[0].spanFrom!, cs[0].spanTo!)).toBe('b');
  });
});

function ranges(doc: string) {
  const set = commentDecorations(state(doc));
  const r: { from: number; to: number; spec: any }[] = [];
  const it = set.iter();
  while (it.value) { r.push({ from: it.from, to: it.to, spec: it.value.spec }); it.next(); }
  return r;
}

describe('commentDecorations — every character stays on screen', () => {
  const classOf = (doc: string, from: number) => ranges(doc).find((x) => x.from === from)?.spec?.class;
  it('a span comment: markup faint, the text highlighted, the note faint and smaller', () => {
    const doc = 'a {==fox==}{>>vague<<} z';
    const r = ranges(doc);
    const slices = r.map((x) => [doc.slice(x.from, x.to), x.spec.class] as const);
    expect(slices).toEqual([
      ['{==', 'cm-criticmarkup-delim cm-comment-trigger'],
      ['fox', 'cm-criticmarkup-highlight'],
      ['==}{>>', 'cm-criticmarkup-delim cm-comment-trigger'],
      ['vague', 'cm-comment-note-text cm-comment-trigger'],
      ['<<}', 'cm-criticmarkup-delim cm-comment-trigger'],
    ]);
  });
  it('a lone note: markup faint, the note visible', () => {
    const doc = 'note {>>todo<<} here';
    expect(ranges(doc).map((x) => doc.slice(x.from, x.to))).toEqual(['{>>', 'todo', '<<}']);
    expect(classOf(doc, doc.indexOf('todo'))).toContain('cm-comment-note-text');
  });
  it('an empty note leaves no empty mark', () => {
    const doc = '{==a==}{>><<}';
    expect(ranges(doc).map((x) => doc.slice(x.from, x.to))).toEqual(['{==', 'a', '==}{>>', '<<}']);
  });
  it('nothing is replaced and no widget is drawn: no decoration hides or stands in for a character', () => {
    for (const doc of ['a {==fox==}{>>vague<<} z', 'note {>>todo<<} here']) {
      for (const x of ranges(doc)) {
        expect(x.spec.widget).toBeUndefined();
        expect(x.spec.class).toBeDefined(); // a bare replace has no class
      }
    }
  });
  it('contributes no atomic ranges: the caret can step through every character', () => {
    const view = new EditorView({ state: EditorState.create({ doc: 'a {==fox==}{>>vague<<} z', extensions: [markdown(), commentExtension()] }) });
    expect(view.state.facet(EditorView.atomicRanges)).toHaveLength(0);
    view.destroy();
  });
});

describe('the comment popover opens from the visible markup and note', () => {
  const doc = 'a {==fox==}{>>vague<<} z';
  const mount = () => {
    const view = new EditorView({ state: EditorState.create({ doc, extensions: [markdown(), commentExtension()] }), parent: document.body });
    return view;
  };
  const press = (el: Element) => {
    const ev = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    el.dispatchEvent(ev);
    return ev;
  };
  afterEach(() => { document.body.innerHTML = ''; });

  it.each([['{==', 0], ['==}{>>', 1], ['vague', 2], ['<<}', 3]])('a press on %s opens it', (_text, idx) => {
    const view = mount();
    const triggers = [...view.dom.querySelectorAll('.cm-comment-trigger')];
    expect(triggers).toHaveLength(4);
    const ev = press(triggers[idx]);
    expect(ev.defaultPrevented).toBe(true);
    expect(document.querySelector('.cm-comment-popover')).not.toBeNull();
    expect((document.querySelector('.cm-comment-note') as HTMLTextAreaElement).value).toBe('vague');
    view.destroy();
  });

  it('a press on the highlighted text does not: the caret goes there', () => {
    const view = mount();
    press(view.dom.querySelector('.cm-criticmarkup-highlight')!);
    expect(document.querySelector('.cm-comment-popover')).toBeNull();
    view.destroy();
  });
});

describe('commentGutterMarkers', () => {
  it('places one marker per comment, at the comment start', () => {
    const doc = 'x {>>a<<}\ny {==z==}{>>b<<}';
    const markers = commentGutterMarkers(state(doc)); // returns array of {from}
    expect(markers.map(m => m.from)).toEqual([2, 12]);
  });
});

function view(doc: string, sel?: { anchor: number; head: number }) {
  return new EditorView({ state: EditorState.create({ doc, selection: sel, extensions: [markdown()] }) });
}

describe('insertCommentAndEdit', () => {
  it('wraps a selection as {==sel==}{>><<} and lands the cursor in the note', () => {
    const v = view('the quick fox', { anchor: 4, head: 9 }); // "quick"
    insertCommentAndEdit(v);
    expect(v.state.doc.toString()).toBe('the {==quick==}{>><<} fox');
    // cursor between {>> and <<}
    const head = v.state.selection.main.head;
    expect(v.state.doc.sliceString(head - 3, head)).toBe('{>>');
    v.destroy();
  });

  it('inserts a point comment {>><<} at the cursor when there is no selection', () => {
    const v = view('hello world', { anchor: 5, head: 5 });
    insertCommentAndEdit(v);
    expect(v.state.doc.toString()).toBe('hello{>><<} world');
    v.destroy();
  });

  it('collapses a cross-block selection (spanning \\n\\n) to a point comment at from', () => {
    const v = view('first para\n\nsecond para', { anchor: 6, head: 14 }); // spans blank line
    insertCommentAndEdit(v);
    // Should insert point comment at from=6, NOT wrap with {==…==}
    const doc = v.state.doc.toString();
    expect(doc).toBe('first {>><<}para\n\nsecond para');
    expect(doc).not.toContain('{==');
    v.destroy();
  });
});

describe('resolveCommentChange', () => {
  it('span comment resolves to its highlighted text (annotation removed, prose kept)', () => {
    const doc = 'a {==fox==}{>>vague<<} z';
    const st = state(doc);
    const c = parseComments(st)[0];
    const ch = resolveCommentChange(c, st);
    const next = doc.slice(0, ch.from) + ch.insert + doc.slice(ch.to);
    expect(next).toBe('a fox z');
  });
  it('point comment resolves to nothing', () => {
    const doc = 'hello {>>todo<<} world';
    const st = state(doc);
    const c = parseComments(st)[0];
    const ch = resolveCommentChange(c, st);
    const next = doc.slice(0, ch.from) + ch.insert + doc.slice(ch.to);
    expect(next).toBe('hello  world'); // the comment text removed; spaces around remain
  });
});

describe('sanitizeNote', () => {
  it('replaces <<} with << } to prevent closing delimiter from corrupting the doc', () => {
    expect(sanitizeNote('safe text')).toBe('safe text');
    expect(sanitizeNote('tricky <<} end')).toBe('tricky << } end');
    expect(sanitizeNote('multiple <<} times <<}')).toBe('multiple << } times << }');
    expect(sanitizeNote('<<}<<}')).toBe('<< }<< }');
  });

  it('a note containing <<} is committed such that parseComments still finds exactly one well-formed comment', () => {
    // Build a doc with one span comment, then simulate what commit() does:
    // sanitize the user's note and write it back between {>> and <<}.
    const baseDoc = '{>><<}';
    const maliciousNote = 'bad <<} text';
    const safeNote = sanitizeNote(maliciousNote);
    // safeNote must not contain <<}
    expect(safeNote).not.toContain('<<}');
    // Construct the committed doc as commit() would
    const committedDoc = `{>>${safeNote}<<}`;
    const cs = parseComments(state(committedDoc));
    expect(cs).toHaveLength(1);
    expect(cs[0].spanFrom).toBeUndefined(); // still a point comment
    expect(committedDoc.slice(cs[0].noteFrom, cs[0].noteTo)).toBe(safeNote);
  });
});

// ── parsePercentComments ─────────────────────────────────────────────────────

describe('parsePercentComments', () => {
  it('detects an inline %%...%% pair on one line', () => {
    const doc = 'before %%hidden note%% after';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(1);
    expect(cs[0].kind).toBe('inline');
    expect(doc.slice(cs[0].from, cs[0].to)).toBe('%%hidden note%%');
  });

  it('detects multiple inline pairs on the same line', () => {
    const doc = 'a %%one%% b %%two%% c';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(2);
    expect(doc.slice(cs[0].from, cs[0].to)).toBe('%%one%%');
    expect(doc.slice(cs[1].from, cs[1].to)).toBe('%%two%%');
  });

  it('does NOT flag a lone %% with no closing pair on the same line', () => {
    const doc = 'only one %% marker here';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(0);
  });

  it('detects a block %% comment spanning multiple lines', () => {
    const doc = 'before\n%%\nhidden text\n%%\nafter';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(1);
    expect(cs[0].kind).toBe('block');
    // The entire block from the opening %% line through the closing %% line
    // (including any trailing newline) is the comment range.
    expect(doc.slice(cs[0].from, cs[0].to)).toContain('hidden text');
    expect(doc.slice(cs[0].from, cs[0].to)).toContain('%%');
  });

  it('does not detect %% inside inline code as a comment', () => {
    const doc = 'Use `%%note%%` syntax in Obsidian.';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(0);
  });

  it('does not detect %% inside fenced code as a comment', () => {
    const doc = '```\n%%not a comment%%\n```';
    const cs = parsePercentComments(state(doc));
    expect(cs).toHaveLength(0);
  });

  it('50% off prose (single percent) is never matched', () => {
    const doc = '50% off and 80% done';
    const cs = parsePercentComments(state(doc));
    // No `%%` sequences present at all
    expect(cs).toHaveLength(0);
  });
});
