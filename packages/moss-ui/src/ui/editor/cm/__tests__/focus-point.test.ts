import { afterEach, describe, expect, test } from 'vitest';
import { markdown } from '@codemirror/lang-markdown';
import { forceParsing } from '@codemirror/language';
import { EditorState, Transaction } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { history, undo } from '@codemirror/commands';
import { focusPointExtension } from '../focus-point';
import { blockEmbedsExtension } from '../../cm-block-embeds';
import { pictureAtCaretField } from '../picture-context';
import { MARKDOWN_EXTENSIONS } from '../source-extensions';
import { HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS } from '../../session/instrument-events';

const wire = { sent: [] as [string, unknown][], handlers: new Map<string, (e: { payload: unknown }) => void>() };
const host = {
  emit: (name: string, payload: unknown) => { wire.sent.push([name, payload]); },
  on: <T,>(name: string, handler: (payload: T) => void) => {
    wire.handlers.set(name, (e) => handler(e.payload as T));
    return () => { wire.handlers.delete(name); };
  },
};

const HERO = [':::hero', '![[photo.jpg|cover top]]', '# Title', ':::', '', '![[photo.jpg]]'].join('\n');
const IMAGE = { url: '/photo.jpg', width: 2000, height: 1000 };

let view: EditorView | null = null;
afterEach(() => { view?.destroy(); view = null; document.body.innerHTML = ''; wire.sent.length = 0; wire.handlers.clear(); });

function open(doc: string, caretLine: number, image: typeof IMAGE | null = IMAGE, parse = true) {
  const userEvents: (string | undefined)[] = [];
  const resolveImage = () => image ?? undefined;
  view = new EditorView({
    state: EditorState.create({ doc, extensions: [markdown({ extensions: MARKDOWN_EXTENSIONS }), history(), pictureAtCaretField, focusPointExtension({ path: () => 'p.md', resolveImage, host }),
      blockEmbedsExtension({ resolveImage }),
      EditorView.updateListener.of((u) => u.transactions.forEach((tr) => userEvents.push(tr.annotation(Transaction.userEvent))))] }),
    parent: document.body,
  });
  // The first parse runs on a time budget; a slow machine can place the caret before the hero is parsed.
  if (parse) forceParsing(view, doc.length, 10_000);
  const goto = (n: number) => view!.dispatch({ selection: { anchor: view!.state.doc.line(n).from } });
  goto(caretLine);
  const answer = (payload: unknown) => wire.handlers.get(HERO_FRAME)!({ payload });
  return { view, sent: wire.sent, answer, goto, userEvents };
}

/** The editor's measure pass lays the dot over the drawn picture. */
const settle = async () => {
  await new Promise((r) => setTimeout(r, 0));
  for (let i = 0; i < 2; i++) await new Promise<void>((r) => requestAnimationFrame(() => r()));
};
const picture = () => document.querySelector<HTMLElement>('.cm-lp-image-block > .cm-focus-point');
const dot = () => document.querySelector<HTMLElement>('.cm-focus-point-dot')!;
const frame = () => document.querySelector<HTMLElement>('.cm-focus-point-frame');
/** Line `n` as the editor shows it, which differs from the document while the dot moves. */
const shownLine = (n: number) => view!.contentDOM.querySelectorAll('.cm-line')[n - 1].textContent;
const focusWrites = (sent: [string, unknown][]) => sent.filter(([n]) => n === HERO_IMAGE_FOCUS).map(([, p]) => p);

/** Pointer events on the dot, over a picture with a 200x100 box at the origin. */
function pointer(type: string, xPct: number, yPct: number) {
  picture()!.getBoundingClientRect = () => ({ left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) });
  dot().dispatchEvent(new MouseEvent(type, { bubbles: true, button: 0, clientX: xPct * 2, clientY: yPct }));
}

describe('focus point instrument', () => {
  test('caret on a hero picture asks for the hero frame and lays a dot over the drawn picture', async () => {
    const { sent } = open(HERO, 2);
    expect(sent).toContainEqual([HERO_FRAME_REQUEST, { path: 'p.md', line: 1 }]);
    await settle();
    expect(picture()!.parentElement!.querySelector('img')?.getAttribute('src')).toContain('/photo.jpg');
    expect([dot().style.left, dot().style.top]).toEqual(['50%', '0%']);
  });

  test('a hero parsed after the caret is placed is still asked for by its own line', () => {
    const above = 'A paragraph of prose.\n\n'.repeat(60_000);
    const first = above.split('\n').length;
    const { view, sent } = open(above + HERO, first + 1, IMAGE, false);
    expect(sent).not.toContainEqual([HERO_FRAME_REQUEST, { path: 'p.md', line: first }]);
    forceParsing(view, view.state.doc.length, 30_000);
    expect(sent).toContainEqual([HERO_FRAME_REQUEST, { path: 'p.md', line: first }]);
  }, 60_000);

  test('Escape on the dot writes nothing and puts the preview back', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', altKey: true, bubbles: true }));
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(view.state.doc.toString()).toBe(HERO);
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: null });
  });

  test('the instrument adds no row and no words to the document', async () => {
    const { goto } = open(HERO, 3);
    await settle();
    const rows = () => [...document.querySelectorAll('.cm-content > *')].map((el) => el.className);
    const before = rows();
    goto(2);
    await settle();
    expect(picture()).not.toBeNull();
    expect(rows()).toEqual(before);
    expect(picture()!.textContent).toBe('');
  });

  test('the picture of a hero opened with image= is drawn below its line wherever the caret is', async () => {
    const { goto } = open([':::hero {image=photo.jpg}', 'Words', ':::', '', 'after'].join('\n'), 5);
    await settle();
    const drawn = () => document.querySelectorAll('.cm-line:first-child + .cm-lp-image-row img').length;
    expect(drawn()).toBe(1);
    goto(1);
    await settle();
    expect(drawn()).toBe(1);
  });

  test('Alt-Enter puts the keyboard on the dot, and Mod-Enter is left to open the picture; arrows move the dot and leaving it writes once', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }));
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true }));
    expect(document.activeElement).not.toBe(dot());
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', altKey: true, bubbles: true }));
    expect(document.activeElement).toBe(dot());
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true }));
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: '49% 10%' });
    expect(view.state.doc.toString()).toBe(HERO);
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(view.state.doc.line(2).text).toBe('![[photo.jpg|cover focus:49%,10%]]');
  });

  test('a screen reader is given the dot as a named slider whose value text carries both axes after each key', async () => {
    open(HERO, 2);
    await settle();
    expect(dot().getAttribute('role')).toBe('slider');
    expect(dot().getAttribute('aria-label')).toBe('Focus point of the picture');
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true }));
    dot().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    // A slider has one number; the text is what carries the second axis.
    expect(dot().getAttribute('aria-valuetext')).toBe('49% 10%');
  });

  test('without an answer there is no outline; with one, the outline is the cover crop at the dot', async () => {
    const { answer } = open(HERO, 2);
    await settle();
    expect(frame()!.hidden).toBe(true);
    answer({ line: 1, width: 500, height: 500 });
    await settle();
    // picture 2:1, frame 1:1, dot at the top-centre: the left half-width strip centred on 50%.
    expect(frame()!.hidden).toBe(false);
    expect([frame()!.style.left, frame()!.style.top, frame()!.style.width, frame()!.style.height]).toEqual(['25%', '0%', '50%', '100%']);
  });

  test('an answer for another hero is ignored', async () => {
    const { answer } = open(HERO, 2);
    await settle();
    answer({ line: 9, width: 500, height: 500 });
    await settle();
    expect(frame()!.hidden).toBe(true);
  });

  test('dragging moves the preview live and shows the position in the line; letting go writes one undoable edit and holds the written position on the preview', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointermove', 20, 62);
    pointer('pointermove', 20, 70);
    expect(focusWrites(sent)).toEqual([{ path: 'p.md', line: 1, position: '20% 62%' }, { path: 'p.md', line: 1, position: '20% 70%' }]);
    expect(view.state.doc.toString()).toBe(HERO);
    expect(shownLine(2)).toBe('![[photo.jpg|cover focus:20%,70%]]');
    pointer('pointerup', 20, 70);
    expect(view.state.doc.line(2).text).toBe('![[photo.jpg|cover focus:20%,70%]]');
    expect(shownLine(2)).toBe(view.state.doc.line(2).text);
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: '20% 70%', held: true });
    undo(view);
    expect(view.state.doc.toString()).toBe(HERO);
  });

  test('letting go near a named position writes the name and holds the named point, not the dot', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 97, 3);
    pointer('pointerup', 97, 3);
    expect(view.state.doc.line(2).text).toBe('![[photo.jpg|cover top-right]]');
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: '100% 0%', held: true });
  });

  test('undo after a write lets the hold go, so the shell shows the page until the rebuild', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointerup', 20, 70);
    undo(view);
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: null });
  });

  test('an edit above the held picture lets it go; an edit below does not', async () => {
    const { view, sent } = open(['intro', '', ...HERO.split('\n')].join('\n'), 4);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointerup', 20, 70);
    const n = focusWrites(sent).length;
    view.dispatch({ changes: { from: view.state.doc.length, insert: '\nmore' } });
    expect(focusWrites(sent).length).toBe(n);
    view.dispatch({ changes: { from: 0, insert: 'x' } });
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 3, position: null });
  });

  test('the caret leaving after a write does not put the old position back', async () => {
    const { sent, goto } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointerup', 20, 70);
    goto(3);
    expect(focusWrites(sent).filter((w) => (w as { position: unknown }).position === null)).toEqual([]);
  });

  test('the write is tagged as an instrument edit', async () => {
    const { userEvents } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 10, 90);
    pointer('pointerup', 10, 90);
    expect(userEvents).toContain('input.instrument');
  });

  test('a drag that ends where the picture already is writes nothing and puts the preview back', async () => {
    const { view, sent } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointermove', 20, 62);
    expect(shownLine(2)).toBe('![[photo.jpg|cover focus:20%,62%]]');
    pointer('pointerup', 50, 0);
    expect(view.state.doc.toString()).toBe(HERO);
    expect(shownLine(2)).toBe(view.state.doc.line(2).text);
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: null });
  });

  test('the caret leaving the line removes the widget and puts the preview back, without writing', async () => {
    const { view, sent, goto } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointermove', 20, 62);
    goto(3);
    await settle();
    expect(picture()).toBeNull();
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: null });
    expect(view.state.doc.toString()).toBe(HERO);
  });

  test('Escape closes it the same way, and it stays closed until the caret comes back', async () => {
    const { view, sent, goto } = open(HERO, 2);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointermove', 20, 62);
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(picture()).toBeNull();
    expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 1, position: null });
    expect(view.state.doc.toString()).toBe(HERO);
    goto(3);
    goto(2);
    await settle();
    expect(picture()).not.toBeNull();
  });

  test('a hero opened with image= is edited on its opening line', async () => {
    const { view } = open([':::hero {image=photo.jpg align=end}', 'Words', ':::'].join('\n'), 1);
    await settle();
    pointer('pointerdown', 20, 62);
    pointer('pointerup', 20, 62);
    expect(view.state.doc.line(1).text).toBe(':::hero {image="photo.jpg|focus:20%,62%" align=end}');
  });

  test.each([
    ['the closing fence', HERO, 4],
    ['the opening line of a block that only starts with hero', ':::hero-banner {image=photo.jpg}\nwords\n:::', 1],
  ])('%s gets no instrument', async (_what, doc, line) => {
    const { sent } = open(doc, line);
    await settle();
    expect(sent).toEqual([]);
    expect(picture()).toBeNull();
  });

  describe('a picture outside any hero', () => {
    test('opens the instrument on its line, asks for the frame by that line, and writes the position into the embed', async () => {
      const { view, sent, answer } = open(HERO, 6);
      expect(sent).toContainEqual([HERO_FRAME_REQUEST, { path: 'p.md', line: 6 }]);
      await settle();
      answer({ line: 6, width: 500, height: 500 });
      await settle();
      expect(picture()).not.toBeNull();
      pointer('pointerdown', 20, 62);
      pointer('pointermove', 20, 62);
      expect(focusWrites(sent)).toEqual([{ path: 'p.md', line: 6, position: '20% 62%' }]);
      pointer('pointerup', 20, 62);
      expect(view.state.doc.line(6).text).toBe('![[photo.jpg|focus:20%,62%]]');
      expect(focusWrites(sent).at(-1)).toEqual({ path: 'p.md', line: 6, position: '20% 62%', held: true });
    });

    test('shows nothing until the page says the picture is cropped', async () => {
      const { answer } = open(HERO, 6);
      await settle();
      expect(picture()).toBeNull();
      answer({ line: 6, width: 500, height: 500 });
      await settle();
      expect(picture()).not.toBeNull();
    });

    test('a picture the page shows whole (frame aspect within 1% of its own) gets no widget', async () => {
      const { answer } = open(HERO, 6);
      await settle();
      answer({ line: 6, width: 400, height: 200 });
      await settle();
      expect(picture()).toBeNull();
      answer({ line: 6, width: 400, height: 201 });
      await settle();
      expect(picture()).toBeNull();
      answer({ line: 6, width: 400, height: 210 });
      await settle();
      expect(picture()).not.toBeNull();
    });
  });

  test('a hero whose frame matches the picture is shown whole and gets no widget', async () => {
    const { answer } = open(HERO, 2);
    await settle();
    answer({ line: 1, width: 400, height: 200 });
    await settle();
    expect(picture()).toBeNull();
  });

  test('something that is not an image gets no widget', async () => {
    open(HERO, 2, null);
    await settle();
    expect(picture()).toBeNull();
  });
});
