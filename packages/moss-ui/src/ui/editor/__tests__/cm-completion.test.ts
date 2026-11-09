// jsdom-only tests for the shortcode completion source: everything here needs
// a real `EditorView` (snippet expansion, accept-path dispatch). The pure line
// parsers this file used to test moved to `@symbiosis-lab/moss-syntax`
// (packages/moss-syntax/src/__tests__/completion-core.test.ts); the
// `parseCompletionContext` dispatcher is tested app-side in
// `cm-completion-context.test.ts` (node lane).

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';

// ── blockAlreadyClosed (name re-completion must not corrupt a block) ──────

import { blockAlreadyClosed, attrNameApplyText } from '../cm-completion';
import { SHORTCODE_CATALOG } from '../cm-shortcode-catalog';
import { SHORTCODES, type TargetKind } from '@symbiosis-lab/moss-syntax';

describe('blockAlreadyClosed', () => {
  function stateOf(doc: string) {
    return EditorState.create({ doc });
  }

  test('open fence with a close fence below → true (existing block)', () => {
    const s = stateOf(':::gr\ncell one\n:::');
    expect(blockAlreadyClosed(s, 1)).toBe(true);
  });

  test('open fence with nothing below → false (fresh block, snippet ok)', () => {
    const s = stateOf(':::gr\n');
    expect(blockAlreadyClosed(s, 1)).toBe(false);
  });

  test('another block OPENING before any close → false', () => {
    // The close fence below belongs to the OTHER block.
    const s = stateOf(':::gr\n\n:::hero\nx\n:::');
    expect(blockAlreadyClosed(s, 1)).toBe(false);
  });
});

// ── Name completion apply — ONE snippet path with tab stops ───────────────

import { EditorView } from '@codemirror/view';
import { CompletionContext } from '@codemirror/autocomplete';
import { completionSource } from '../cm-completion';
import type { WikilinkCompleteOptions } from '../cm-wikilink-complete';

// Name / attr-name phases never touch `opts` — this stub exists only to
// satisfy the factory's parameter; asset-search phases are covered
// separately (they'd throw if this stub were actually invoked).
const unusedOpts: WikilinkCompleteOptions = {
  complete: () => Promise.reject(new Error('not expected to be called')),
  getCurrentPath: () => null,
};

describe('name completion apply (snippet with tab stops)', () => {
  function applyName(doc: string, pos: number, label: string): EditorView {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc, selection: { anchor: pos } }),
      parent: container,
    });
    const ctx = new CompletionContext(view.state, pos, true);
    const result = completionSource(unusedOpts)(ctx);
    if (!result || result instanceof Promise) throw new Error('no completion result');
    const completion = result.options.find((o) => o.label === label);
    if (!completion) throw new Error(`no option ${label}`);
    (completion.apply as (v: EditorView, c: unknown, f: number, t: number) => void)(
      view, completion, result.from, pos,
    );
    return view;
  }

  test('fresh block expands the template with the FIRST FIELD selected', () => {
    const view = applyName(':::gr', 5, 'grid');
    const doc = view.state.doc.toString();
    expect(doc).toContain(':::grid 2');
    expect(doc).toContain('cell one');
    expect(doc).toContain('+++');
    expect(doc.trimEnd().endsWith(':::')).toBe(true);
    // Tab-stop contract: the first placeholder is SELECTED — typing replaces
    // it, Tab moves to the next field. No more cursor in a dead spot.
    const sel = view.state.selection.main;
    expect(view.state.doc.sliceString(sel.from, sel.to)).toBe('2');
    view.destroy();
  });

  test('existing block: name-only insert, no snippet corruption', () => {
    const view = applyName(':::gr\nbody\n:::', 5, 'grid');
    expect(view.state.doc.toString()).toBe(':::grid\nbody\n:::');
    view.destroy();
  });
});

// ── attr-name completion insert text ───────────────────────────────────────

describe('attrNameApplyText', () => {
  test('an ordinary attr inserts name= (caret ready for a value)', () => {
    expect(attrNameApplyText({ name: 'wide' })).toBe('wide=');
  });

  test('a flag attr inserts the bare name — no = the parser never reads', () => {
    expect(attrNameApplyText({ name: 'scroll', flag: true })).toBe('scroll');
  });
});

// ── attr-value / body-line phases dispatch the RPC asset search ───────────

describe('completionSource — asset-search dispatch', () => {
  function contextAt(doc: string, pos: number): CompletionContext {
    const state = EditorState.create({ doc, selection: { anchor: pos } });
    return new CompletionContext(state, pos, true);
  }

  test('hero image= attr-value calls opts.complete with allowedKinds [Image, Video]', async () => {
    const calls: unknown[] = [];
    const opts: WikilinkCompleteOptions = {
      complete: async (...args) => {
        calls.push(args);
        return [{ insert: 'photo.jpg', label: 'photo', detail: '.jpg', kind: 'asset' }];
      },
      getCurrentPath: () => 'index.md',
    };
    const doc = ':::hero {image=photo';
    const result = await completionSource(opts)(contextAt(doc, doc.length));
    expect(calls).toEqual([['photo', 'index.md', 'asset-path', null, ['Image', 'Video']]]);
    expect(result?.options.map((o) => o.label)).toEqual(['photo']);
  });

  test('gallery body-line calls opts.complete with allowedKinds [Image]', async () => {
    const calls: unknown[] = [];
    const opts: WikilinkCompleteOptions = {
      complete: async (...args) => {
        calls.push(args);
        return [];
      },
      getCurrentPath: () => 'index.md',
    };
    const doc = ':::gallery {cols=3}\nphoto';
    const result = await completionSource(opts)(contextAt(doc, doc.length));
    expect(calls).toEqual([['photo', 'index.md', 'asset-path', null, ['Image']]]);
    expect(result).toBeNull(); // empty RPC result → no completions
  });

  test('gallery body-line asset options carry the preview panel — every link phase funnels through one row builder', async () => {
    const opts: WikilinkCompleteOptions = {
      complete: async () => [{ insert: 'photo.jpg', label: 'photo', detail: '.jpg', kind: 'asset' }],
      getCurrentPath: () => 'index.md',
      resolveAssetPreview: async () => null,
    };
    const doc = ':::gallery {cols=3}\nphoto';
    const result = await completionSource(opts)(contextAt(doc, doc.length));
    expect(typeof result?.options[0].info).toBe('function');
  });

  test('grid cols= attr-value has no assetKinds → no RPC dispatch, no completion', () => {
    const opts: WikilinkCompleteOptions = {
      complete: () => Promise.reject(new Error('should not be called')),
      getCurrentPath: () => 'index.md',
    };
    const doc = ':::grid {cols=3';
    const result = completionSource(opts)(contextAt(doc, doc.length));
    expect(result).toBeNull();
  });
});

// ── The accept path: replace the whole TOKEN, render it parseably ─────────

describe('asset-completion apply', () => {
  /** Run the source at `pos`, accept the single returned option, return the doc. */
  async function accept(
    doc: string,
    pos: number,
    item: { insert: string; kind?: TargetKind },
  ): Promise<{ text: string; cursor: number }> {
    const opts: WikilinkCompleteOptions = {
      complete: async () => [{ label: 'x', detail: null, kind: 'asset' as const, ...item }],
      getCurrentPath: () => 'index.md',
    };
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc, selection: { anchor: pos } }),
      parent: container,
    });
    const result = await completionSource(opts)(
      new CompletionContext(view.state, pos, true),
    );
    if (!result) throw new Error('no completion result');
    const apply = result.options[0].apply as
      (v: EditorView, c: unknown, f: number, t: number) => void;
    apply(view, result.options[0], result.from, pos);
    const out = { text: view.state.doc.toString(), cursor: view.state.selection.main.head };
    view.destroy();
    return out;
  }

  test('a CJK attr value is QUOTED so parse_attrs accepts it', async () => {
    const doc = ':::hero {image=關於';
    const { text } = await accept(doc, doc.length, { insert: '關於/頭像.png' });
    expect(text).toBe(':::hero {image="關於/頭像.png"');
  });

  test('an ASCII path stays a bareword', async () => {
    const doc = ':::hero {image=pho';
    const { text, cursor } = await accept(doc, doc.length, { insert: 'photo.jpg' });
    expect(text).toBe(':::hero {image=photo.jpg');
    expect(cursor).toBe(text.length);
  });

  test('an existing quoted value is replaced INCLUDING its quotes', async () => {
    const doc = ':::hero {image="old.jpg" wide=true}';
    const { text } = await accept(doc, doc.indexOf('old') + 3, { insert: 'new.jpg' });
    expect(text).toBe(':::hero {image=new.jpg wide=true}');
  });

  test("the author's |attrs suffix survives the accept", async () => {
    const doc = ':::hero {image="old.jpg|caption=Foo"}';
    const { text } = await accept(doc, doc.indexOf('old') + 3, { insert: '圖.png' });
    expect(text).toBe(':::hero {image="圖.png|caption=Foo"}');
  });

  test('a gallery body path replaces the whole token, not just the typed prefix', async () => {
    const doc = ':::gallery {cols=3}\npho.jpg';
    const { text } = await accept(doc, doc.indexOf('pho') + 3, { insert: 'photo.jpg' });
    expect(text).toBe(':::gallery {cols=3}\nphoto.jpg');
  });

  test('a linked image inside a grid cell completes its target (the corpus bug)', async () => {
    const doc = ':::grid {cols=2}\n[![](card)](/awards/x/)';
    const { text } = await accept(doc, doc.indexOf('card') + 4, { insert: '/awards/得獎卡.png' });
    expect(text).toBe(':::grid {cols=2}\n[![](/awards/得獎卡.png)](/awards/x/)');
  });

  test('a folder row descends: the value is left open for the reopened list', async () => {
    const doc = ':::hero {image=pho';
    const { text, cursor } = await accept(doc, doc.length, { insert: 'photos/' });
    expect(text).toBe(':::hero {image=photos/');
    expect(cursor).toBe(text.length);
  });

  test('a folder row into a quoted value keeps the cursor inside the quotes', async () => {
    const doc = ':::hero {image=關';
    const { text, cursor } = await accept(doc, doc.length, { insert: '關於/', kind: 'folder' });
    expect(text).toBe(':::hero {image="關於/"');
    expect(cursor).toBe(text.length - 1);
  });
});

// ── Defects an adversarial review found in the accept path ────────────────

describe('accept-path regressions', () => {
  async function acceptInto(
    doc: string,
    pos: number,
    item: { insert: string; kind?: TargetKind },
  ): Promise<string> {
    const opts: WikilinkCompleteOptions = {
      complete: async () => [{ label: 'x', detail: null, kind: 'asset' as const, ...item }],
      getCurrentPath: () => 'index.md',
    };
    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({
      state: EditorState.create({ doc, selection: { anchor: pos } }),
      parent: container,
    });
    const result = await completionSource(opts)(
      new CompletionContext(view.state, pos, true),
    );
    if (!result) throw new Error('no completion result');
    const apply = result.options[0].apply as
      (v: EditorView, c: unknown, f: number, t: number) => void;
    apply(view, result.options[0], result.from, pos);
    const text = view.state.doc.toString();
    view.destroy();
    return text;
  }

  test('an unterminated quote does not swallow the closing brace', async () => {
    // `:::hero {image="關}` is one keystroke off the shipped hero snippet: its
    // `photo.jpg` tab-stop is selected, so typing `"` then a CJK prefix
    // produces it. Accepting used to delete the `}`.
    const doc = ':::hero {image="關}\n# Title\n:::';
    const text = await acceptInto(doc, doc.indexOf('}'), { insert: '關於/頭像.png' });
    expect(text).toBe(':::hero {image="關於/頭像.png"}\n# Title\n:::');
  });

  test('an unterminated quote does not swallow trailing attrs', async () => {
    const doc = ':::hero {image="pho} wide=page';
    const text = await acceptInto(doc, doc.indexOf('}'), { insert: 'photo.jpg' });
    expect(text).toBe(':::hero {image=photo.jpg} wide=page');
  });

  test("the author's escaped |attrs suffix round-trips instead of double-escaping", async () => {
    const doc = String.raw`:::hero {image="a.png|caption=\"Hi\""}`;
    const text = await acceptInto(doc, doc.indexOf('a.png') + 3, { insert: 'new.png' });
    expect(text).toBe(String.raw`:::hero {image="new.png|caption=\"Hi\""}`);
  });
});

describe('completionSource — width phase', () => {
  const unusedWikilinkOpts: WikilinkCompleteOptions = {
    complete: () => Promise.reject(new Error('width phase never dispatches an RPC')),
    getCurrentPath: () => null,
  };

  test('the width phase offers percent presets and writes |NN%, closing the brackets', () => {
    const doc = '![[cover.png|5';
    const state = EditorState.create({ doc, selection: { anchor: doc.length } });
    const context = new CompletionContext(state, doc.length, true);
    const result = completionSource(unusedWikilinkOpts)(context);
    if (!result || result instanceof Promise) throw new Error('no synchronous completion result');
    expect(result.options.map((o) => o.label)).toEqual(['50%']); // the one preset starting with "5"

    const container = document.createElement('div');
    document.body.appendChild(container);
    const view = new EditorView({ state, parent: container });
    const apply = result.options[0].apply as (v: EditorView, c: unknown, f: number, t: number) => void;
    apply(view, result.options[0], result.from, doc.length);
    expect(view.state.doc.toString()).toBe('![[cover.png|50%]]');
    view.destroy();
    container.remove();
  });
});

describe('presentation overlay covers the generated catalog', () => {
  // SHORTCODE_CATALOG is built by overlaying app i18n on the generated
  // artifact (Rust SSOT). These are the totality guards: add an authorable
  // shortcode or an attr in `contract/shortcodes.rs`, regenerate, and the
  // suite is red until the overlay presents it.
  test('every authorable shortcode is presented, hidden ones are not', () => {
    const presented = new Set(SHORTCODE_CATALOG.map((s) => s.name));
    for (const info of SHORTCODES) {
      expect(
        presented.has(info.name),
        `${info.name}: authorable=${info.authorable} but presented=${presented.has(info.name)}`,
      ).toBe(info.authorable);
    }
  });

  test('every generated attr carries an i18n hint key', () => {
    for (const spec of SHORTCODE_CATALOG) {
      for (const attr of spec.attrs) {
        expect(attr.hintKey, `${spec.name}.${attr.name} has no hint key`).toBeTruthy();
      }
    }
  });

  test('templates come from the artifact', () => {
    for (const spec of SHORTCODE_CATALOG) {
      const info = SHORTCODES.find((s) => s.name === spec.name)!;
      expect(spec.template).toBe(info.canonicalTemplate);
    }
  });
});
