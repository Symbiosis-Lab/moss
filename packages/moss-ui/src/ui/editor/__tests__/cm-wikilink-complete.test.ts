import { describe, it, expect, vi } from 'vitest';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import {
  wikilinkBracketClose,
  wikilinkBracketCloseHandler,
  wikilinkCompleteExtension,
  isInCode,
  type WikilinkCompleteOptions,
  type WikilinkCompletionItem,
} from '../cm-wikilink-complete';
import { completionSource } from '../cm-completion';
import {
  CompletionContext,
  startCompletion,
  completionStatus,
  type Completion,
} from '@codemirror/autocomplete';
import type { LinkSyntax } from '@symbiosis-lab/moss-syntax';

describe('wikilinkBracketClose', () => {
  it('closes ]] when the second [ of a pair is typed', () => {
    expect(wikilinkBracketClose('see [', '')).toEqual({ insert: '[]]', caretOffset: 1 });
  });
  it('closes for the embed form (![ then second [)', () => {
    expect(wikilinkBracketClose('![', '')).toEqual({ insert: '[]]', caretOffset: 1 });
  });
  it('does not fire when the char before is not [', () => {
    expect(wikilinkBracketClose('hello', '')).toBeNull();
  });
  it('does not fire on a third [ (pair already open)', () => {
    expect(wikilinkBracketClose('[[', '')).toBeNull();
  });
  it('does not double the closer when ]] already follows', () => {
    expect(wikilinkBracketClose('[', ']]')).toEqual({ insert: '[', caretOffset: 1 });
  });
});

// ── Code-fence / inline-code guard (markdown syntax tree) ─────────────────

const mdState = (doc: string) =>
  EditorState.create({ doc, extensions: [markdown()] });

describe('isInCode', () => {
  it('is false in plain prose', () => {
    const doc = 'see [[';
    expect(isInCode(mdState(doc), doc.length)).toBe(false);
  });

  it('is true inside a fenced code block', () => {
    // Cursor right after `[[` typed INSIDE a ``` fence.
    const doc = '```\n[[';
    expect(isInCode(mdState(doc), doc.length)).toBe(true);
  });

  it('is true inside a tilde-fenced code block', () => {
    const doc = '~~~\ncode [[';
    expect(isInCode(mdState(doc), doc.length)).toBe(true);
  });

  it('is true inside CLOSED inline code', () => {
    // Cursor between the backticks of `[[code` — CommonMark only forms an
    // InlineCode node once the span is closed by a matching backtick.
    const doc = 'before `[[code` after';
    expect(isInCode(mdState(doc), 10)).toBe(true); // pos inside the span
  });

  it('is false for an UNTERMINATED backtick (per CommonMark)', () => {
    // A lone trailing backtick is not inline code until closed — the markdown
    // parser treats `[[ as plain paragraph text, so completion stays available.
    const doc = 'before `[[';
    expect(isInCode(mdState(doc), doc.length)).toBe(false);
  });

  it('is true inside an indented code block', () => {
    const doc = '    [[';
    expect(isInCode(mdState(doc), doc.length)).toBe(true);
  });

  it('is false again after a closed fence', () => {
    const doc = '```\ncode\n```\n[[';
    expect(isInCode(mdState(doc), doc.length)).toBe(false);
  });
});

// ── End-to-end: source() fence guard + apply via a real EditorView ────
//

// ── The completion source, driven directly ─────────────────────────────
// We build a markdown EditorState (so the syntax tree is present for the fence
// guard), construct a CompletionContext at the cursor, and call the source.
// Accepting a completion runs its real apply() against the live view. Rows
// come from a FAKE `complete` (the Rust command is exercised by its own Rust
// tests); the fake also captures the (prefix, fromFile, syntax, page) args so
// we can assert the source wires them.

const PAGE_AND_ASSET: WikilinkCompletionItem[] = [
  { insert: 'Notes', label: 'Notes', detail: null, kind: 'page' },
  { insert: 'photo.png', label: 'photo.png', detail: null, kind: 'asset' },
];

const HEADINGS: WikilinkCompletionItem[] = [
  { insert: 'Getting Started', label: 'Getting Started', detail: 'H2', kind: 'heading' },
];

interface CompleteCall {
  prefix: string;
  fromFile: string;
  syntax: LinkSyntax;
  page: string | null;
}

// Build opts whose `complete` returns `items` and records each call's args.
function fakeOpts(
  items: WikilinkCompletionItem[],
  currentPath: string | null = 'index.md',
  resolveAssetPreview?: WikilinkCompleteOptions['resolveAssetPreview'],
): { opts: WikilinkCompleteOptions; calls: CompleteCall[] } {
  const calls: CompleteCall[] = [];
  const opts: WikilinkCompleteOptions = {
    complete: async (prefix, fromFile, syntax, page) => {
      calls.push({ prefix, fromFile, syntax, page });
      return items;
    },
    getCurrentPath: () => currentPath,
    resolveAssetPreview,
  };
  return { opts, calls };
}

// Build a real view at `cursor`, run the completion source, and return the
// view plus the result so a test can invoke apply() against the live document.
async function runSource(
  doc: string,
  cursor: number,
  items: WikilinkCompletionItem[] = PAGE_AND_ASSET,
  currentPath: string | null = 'index.md',
  resolveAssetPreview?: WikilinkCompleteOptions['resolveAssetPreview'],
) {
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: { anchor: cursor },
      extensions: [markdown()],
    }),
  });
  const { opts, calls } = fakeOpts(items, currentPath, resolveAssetPreview);
  const source = completionSource(opts);
  // `explicit: true` forces the source to run regardless of CM6's trigger
  // heuristics; the source's own guards (open-[[, open target) still decide.
  const ctx = new CompletionContext(view.state, cursor, true);
  const result = await source(ctx);
  return { view, result, calls };
}

// Find a completion by label in a result's options.
function pick(
  result: { options: readonly Completion[] } | null,
  label: string,
): Completion {
  const opt = result?.options.find((o) => o.label === label);
  if (!opt) throw new Error(`no completion labeled "${label}"`);
  return opt;
}

// Invoke a completion's function-form apply against the view.
function applyOpt(view: EditorView, opt: Completion, from: number, to: number) {
  const apply = opt.apply;
  if (typeof apply !== 'function') throw new Error('apply is not a function');
  apply(view, opt, from, to);
}

/** Run the source at the end of `doc`, accept the row labeled `label`, return the doc + cursor. */
async function accept(doc: string, label: string, items = PAGE_AND_ASSET, currentPath: string | null = 'index.md') {
  const { view, result } = await runSource(doc, doc.length, items, currentPath);
  applyOpt(view, pick(result, label), result!.from, doc.length);
  const out = { text: view.state.doc.toString(), cursor: view.state.selection.main.head };
  view.destroy();
  return out;
}

describe('completion source — re-query on every keystroke', () => {
  // The Rust ranker owns matching + ranking (substring, language, proximity),
  // so CM6 must re-query it for each new query — never freeze on the first
  // result. A `validFor` regexp tells CM6 to KEEP the current result while the
  // typed text still matches and, with `filter: false`, to NOT re-filter it —
  // which froze the dropdown on the empty-prefix (whole-vault) list and made
  // typing a substring appear to match nothing. Omitting `validFor` makes CM6
  // re-run the source per keystroke so the backend ranking actually applies.
  it('omits validFor for page completion so CM6 re-queries the backend', async () => {
    const { result } = await runSource('see [[No', 'see [[No'.length);
    expect(result?.validFor).toBeUndefined();
    expect(result?.filter).toBe(false);
  });

  it('omits validFor for heading completion', async () => {
    const { result } = await runSource('[[Notes#Get', '[[Notes#Get'.length, HEADINGS);
    expect(result?.validFor).toBeUndefined();
  });
});


describe('completion source — wiring', () => {
  it('passes the typed query, syntax=wikilink, and page=null for [[ page completion', async () => {
    const { calls } = await runSource('see [[No', 'see [[No'.length);
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ prefix: 'No', fromFile: 'index.md', syntax: 'wikilink', page: null });
  });

  it('passes syntax=embed for the ![[ trigger', async () => {
    const { calls } = await runSource('![[ph', '![[ph'.length);
    expect(calls[0]).toMatchObject({ prefix: 'ph', syntax: 'embed', page: null });
  });

  it('passes syntax=inline for a [text](… target, whether or not it opens with /', async () => {
    const a = await runSource('see [here](no', 'see [here](no'.length);
    expect(a.calls[0]).toMatchObject({ prefix: 'no', syntax: 'inline', page: null });
    const b = await runSource('see [here](/ab', 'see [here](/ab'.length);
    expect(b.calls[0]).toMatchObject({ prefix: '/ab', syntax: 'inline' });
  });

  it('passes the resolved page target for [[Page#Heading completion', async () => {
    const { calls } = await runSource('[[Notes#Get', '[[Notes#Get'.length, HEADINGS);
    expect(calls[0]).toMatchObject({ prefix: 'Get', page: 'Notes', syntax: 'wikilink' });
  });

  it('uses the current file as the page target for same-page [[# and [](#', async () => {
    const wiki = await runSource('[[#Int', '[[#Int'.length, HEADINGS, 'guide/index.md');
    expect(wiki.calls[0]).toMatchObject({ prefix: 'Int', page: 'guide/index.md' });
    const inline = await runSource('[t](#Int', '[t](#Int'.length, HEADINGS, 'guide/index.md');
    expect(inline.calls[0]).toMatchObject({ prefix: 'Int', page: 'guide/index.md', syntax: 'inline' });
  });

  it('offers nothing for same-page [[# when no file is open', async () => {
    const { result, calls } = await runSource('[[#Int', '[[#Int'.length, HEADINGS, null);
    expect(result).toBeNull();
    expect(calls).toEqual([]);
  });

  it('renders label, detail and the kind as the icon type', async () => {
    const items: WikilinkCompletionItem[] = [
      { insert: 'about', label: '關於我們', detail: 'about', kind: 'page' },
      { insert: 'notes/', label: 'notes/', detail: null, kind: 'folder' },
      { insert: '/tags/design/', label: 'Design', detail: '/tags/design/', kind: 'generated' },
    ];
    const { result } = await runSource('[[', 2, items);
    const page = pick(result, '關於我們');
    expect(page.detail).toBe('about');
    expect(page.type).toBe('page');
    expect(pick(result, 'notes/').type).toBe('folder');
    expect(pick(result, 'notes/').detail).toBeUndefined();
    // A ring alone means "pending" elsewhere in moss; a generated row says so.
    const gen = pick(result, 'Design');
    expect(gen.type).toBe('generated');
    expect(gen.detail).toBe('/tags/design/ · generated');
  });

  it('returns null when the backend yields no items', async () => {
    const { result } = await runSource('[[zzz', '[[zzz'.length, []);
    expect(result).toBeNull();
  });
});

// The list stays plain text; only the HIGHLIGHTED row grows a preview card,
// via CM6's Completion.info. Rendering of the card itself is asset-preview's
// test — here we only assert which options carry `info` and that building one
// does not fetch anything until the row is rested on.
describe('completion source — asset preview panel (Completion.info)', () => {
  const resolveAssetPreview = async () => null;

  it('attaches info to asset options only — pages and headings have none', async () => {
    const { result } = await runSource('[[', 2, PAGE_AND_ASSET, 'index.md', resolveAssetPreview);
    expect(typeof pick(result, 'photo.png').info).toBe('function');
    expect(pick(result, 'Notes').info).toBeUndefined();
    const headings = await runSource('[[Notes#', 8, HEADINGS, 'index.md', resolveAssetPreview);
    expect(pick(headings.result, 'Getting Started').info).toBeUndefined();
  });

  it('attaches nothing when no resolver is wired (an editor without the seam is unchanged)', async () => {
    const { result } = await runSource('[[', 2);
    expect(pick(result, 'photo.png').info).toBeUndefined();
  });

  it('builds the card lazily: info() returns a handle showing the insert value, having fetched nothing', async () => {
    const resolve = vi.fn(async () => null);
    const { result } = await runSource('[[', 2, PAGE_AND_ASSET, 'index.md', resolve);
    const info = pick(result, 'photo.png').info as () => { dom: HTMLElement; destroy(): void };
    const handle = info();
    expect(handle.dom.textContent).toContain('photo.png');
    expect(resolve).not.toHaveBeenCalled();
    handle.destroy();
  });
});

describe('completion apply — wikilinks close with ]]', () => {
  it('inserts the row text + ]] and lands the cursor after it', async () => {
    expect(await accept('[[No', 'Notes')).toEqual({ text: '[[Notes]]', cursor: 9 });
  });

  it('inserts exactly the backend text — the label is not the insert', async () => {
    const items: WikilinkCompletionItem[] = [{ insert: 'notes/ideas', label: 'ideas', detail: 'notes/ideas', kind: 'page' }];
    expect((await accept('[[notes/id', 'ideas', items)).text).toBe('[[notes/ideas]]');
  });

  it('does NOT double ]] when ]] already follows the cursor', async () => {
    // "[[No]]" with cursor at offset 4 (between "No" and "]]").
    const { view, result } = await runSource('[[No]]', 4);
    applyOpt(view, pick(result, 'Notes'), result!.from, 4);
    expect(view.state.doc.toString()).toBe('[[Notes]]'); // not [[Notes]]]]
    expect(view.state.selection.main.head).toBe(9); // after the existing ]]
    view.destroy();
  });

  it('replaces the whole target when accepting mid-token, not just the typed prefix', async () => {
    const { view, result } = await runSource('[[Nozzz]] tail', 4);
    applyOpt(view, pick(result, 'Notes'), result!.from, 4);
    expect(view.state.doc.toString()).toBe('[[Notes]] tail');
    view.destroy();
  });

  it('leaves the link open when # or | follows the target', async () => {
    const { view, result } = await runSource('[[No|alias]]', 4);
    applyOpt(view, pick(result, 'Notes'), result!.from, 4);
    expect(view.state.doc.toString()).toBe('[[Notes|alias]]');
    expect(view.state.selection.main.head).toBe('[[Notes'.length);
    view.destroy();
  });

  it('closes after a heading completion', async () => {
    expect(await accept('[[Notes#Get', 'Getting Started', HEADINGS))
      .toEqual({ text: '[[Notes#Getting Started]]', cursor: 25 });
  });

  it('a folder row descends: no closer, cursor at the end of the path, list reopened', async () => {
    const items: WikilinkCompletionItem[] = [{ insert: 'notes/', label: 'notes/', detail: null, kind: 'folder' }];
    const { view, result } = await runSource('[[no]]', 4, items);
    applyOpt(view, pick(result, 'notes/'), result!.from, 4);
    expect(view.state.doc.toString()).toBe('[[notes/]]');
    expect(view.state.selection.main.head).toBe('[[notes/'.length);
    view.destroy();
  });
});

describe('completion apply — inline links replace the target verbatim', () => {
  it('fills a [text](… target up to the closing paren', async () => {
    const items: WikilinkCompletionItem[] = [{ insert: '/about/', label: 'About', detail: '/about/', kind: 'page' }];
    const { view, result } = await runSource('see [here](/abzz) now', 'see [here](/ab'.length, items);
    applyOpt(view, pick(result, 'About'), result!.from, 'see [here](/ab'.length);
    expect(view.state.doc.toString()).toBe('see [here](/about/) now');
    expect(view.state.selection.main.head).toBe('see [here](/about/'.length);
    // A page's URL ends in `/` too; only a FOLDER row reopens the list.
    expect(completionStatus(view.state)).toBeNull();
    view.destroy();
  });

  it('a heading in an inline link inserts after the #', async () => {
    const items: WikilinkCompletionItem[] = [{ insert: 'getting-started', label: 'Getting Started', detail: 'getting-started', kind: 'heading' }];
    const { text } = await accept('[t](notes#get', 'Getting Started', items);
    expect(text).toBe('[t](notes#getting-started');
  });
});

// ── Bracket auto-close — driven through the real input handler ───────────
// EditorView.inputHandler is what fires on a real keystroke. We invoke it the
// way CM6's input pipeline does (the registered handler facet), proving the
// `[` → `[]]` close happens end-to-end in a live view.

function typeChar(view: EditorView, ch: string): void {
  const pos = view.state.selection.main.head;
  const handlers = view.state.facet(EditorView.inputHandler);
  for (const h of handlers) {
    if (h(view, pos, pos, ch, () => view.state.update({ changes: { from: pos, insert: ch } }))) {
      return; // handled (e.g. our bracket close)
    }
  }
  // No handler claimed it: insert the char plainly.
  view.dispatch({ changes: { from: pos, insert: ch }, selection: { anchor: pos + ch.length } });
}

describe('bracket auto-close (real EditorView inputHandler)', () => {
  function viewWithHandler(doc: string): EditorView {
    const container = document.createElement('div');
    document.body.appendChild(container);
    return new EditorView({
      state: EditorState.create({ doc, extensions: [wikilinkBracketCloseHandler()] }),
      parent: container,
    });
  }

  it('typing the second [ produces [[]] with the cursor between', () => {
    const view = viewWithHandler('['); // one bracket already present
    view.dispatch({ selection: { anchor: 1 } }); // cursor after the `[`
    typeChar(view, '[');
    expect(view.state.doc.toString()).toBe('[[]]');
    expect(view.state.selection.main.head).toBe(2); // between [[ and ]]
    view.destroy();
  });

  it('embed: typing the second [ after ![ produces ![[]]', () => {
    const view = viewWithHandler('![');
    view.dispatch({ selection: { anchor: 2 } });
    typeChar(view, '[');
    expect(view.state.doc.toString()).toBe('![[]]');
    expect(view.state.selection.main.head).toBe(3);
    view.destroy();
  });

  it('a lone [ (no preceding bracket) is inserted normally, no close', () => {
    const view = viewWithHandler('word ');
    view.dispatch({ selection: { anchor: 5 } });
    typeChar(view, '[');
    expect(view.state.doc.toString()).toBe('word [');
    view.destroy();
  });

  it('does not double ]] when a stray closer already follows (e2e through handler)', () => {
    // Reachable case: doc is `[]]`, cursor after the first `[` (offset 1),
    // user types the second `[`. The handler must insert only `[` (not `[]]`),
    // yielding `[[]]`, cursor between the pairs — not `[[]]]]`.
    const view = viewWithHandler('[]]');
    view.dispatch({ selection: { anchor: 1 } });
    typeChar(view, '[');
    expect(view.state.doc.toString()).toBe('[[]]');
    expect(view.state.selection.main.head).toBe(2);
    view.destroy();
  });
});

// ── The code guard covers BOTH sources, not just the wikilink one ─────────

describe('wikilinkCompleteExtension — the source stays silent inside code', () => {
  /** Poll a real-time condition; throws rather than letting a stall read as a pass. */
  async function pollUntil(done: () => boolean, deadlineMs = 5000): Promise<void> {
    for (let waited = 0; waited < deadlineMs; waited += 5) {
      if (done()) return;
      await new Promise((r) => setTimeout(r, 5));
    }
    throw new Error('completion never settled');
  }

  /**
   * Mount the ASSEMBLED extension and ask CM6 to complete, so the guard is
   * tested where it actually lives (the composition point) rather than on one
   * source in isolation.
   */
  async function completionsAt(doc: string, pos: number): Promise<unknown[]> {
    const calls: unknown[] = [];
    const opts: WikilinkCompleteOptions = {
      complete: async (...args) => {
        calls.push(args);
        return [{ insert: 'photo.jpg', label: 'photo.jpg', detail: null, kind: 'asset' }];
      },
      getCurrentPath: () => 'index.md',
    };
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({
        doc,
        selection: { anchor: pos },
        extensions: [markdown(), wikilinkCompleteExtension(opts)],
      }),
      parent: container,
    });
    startCompletion(view);
    // Wait for CM6's OWN settle signal, not a wall-clock guess. `startCompletion`
    // moves the status to 'pending' synchronously; it leaves 'pending' only once
    // every source has resolved — 'active' if one produced options, null if none
    // did. Polling that is exact for both the positive and the negative cases.
    //
    // A fixed 100ms sleep stood here and was load-dependent: CM6 debounces the
    // query 50ms before even calling the sources, so on a loaded box the sleep
    // expired first and the positive case saw zero calls. It failed under 12
    // spinners on this 4-core machine, and passed alone every time.
    await pollUntil(() => completionStatus(view.state) !== 'pending');
    view.destroy();
    container.remove();
    return calls;
  }

  it('fires for a bare ![](…) in prose', async () => {
    const doc = 'See ![](pho';
    expect(await completionsAt(doc, doc.length)).toHaveLength(1);
  });

  it('fires for [[ in prose and stays silent for [[ inside a fenced code block', async () => {
    expect(await completionsAt('see [[No', 'see [[No'.length)).toHaveLength(1);
    const doc = 'Docs:\n\n```markdown\n[[No\n```\n';
    expect(await completionsAt(doc, doc.indexOf('[[No') + 4)).toEqual([]);
  });

  it('does NOT fire inside a fenced code block', async () => {
    // A moss doc that documents moss markdown had its fenced examples
    // silently rewritten on accept: the `image` phase fires on any `![](…)`
    // anywhere, and only the wikilink source carried the code guard.
    const doc = 'Docs:\n\n```markdown\n![](pho\n```\n';
    expect(await completionsAt(doc, doc.indexOf('![](pho') + 7)).toEqual([]);
  });

  it('does NOT fire inside an inline code span', async () => {
    const doc = 'Write `![](pho` to embed.';
    expect(await completionsAt(doc, doc.indexOf('pho') + 3)).toEqual([]);
  });

  it('does NOT fire for a shortcode fence inside a code block', async () => {
    const doc = 'Docs:\n\n```markdown\n:::hero {image=pho\n```\n';
    expect(await completionsAt(doc, doc.indexOf('=pho') + 4)).toEqual([]);
  });
});
