// One record per content problem: the mapping from resolver envelopes and
// build evidence to marks, the card that explains and fixes them, and the
// rules about when a mark hides.
import { describe, test, expect, vi } from 'vitest';
import { EditorSelection, EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { history, undo } from '@codemirror/commands';
import { forceParsing, syntaxTreeAvailable } from '@codemirror/language';
import { wikilinkConfig } from '@symbiosis-lab/moss-syntax';
import type { ReferenceCacheRead } from '@symbiosis-lab/moss-syntax/cm6';
import { goToNextProblem, problemCardAugmenter, problemsExtension, problemsField, readProblems, setEvidenceProblems, type ProblemInputs } from '../cm-problems';
import { refsResolvedEffect } from '../cm-reference-resolver';
import { embedAt } from '../cm-block-embeds';
import {
  assetCardProvider, cardAugmenters, cardProviders, cardSpecAt, linkCardProvider, openCardAt, referenceCardExtension, withLinkControls, type CardProvider,
} from '../cm-reference-card';

type Env = NonNullable<ReturnType<ReferenceCacheRead['get']>>;

function env(kind: string, extra: Partial<Env> = {}): Env {
  return {
    target: '', is_embed: false, kind: { kind }, resolved: null, url: null, anchor: null,
    resolved_path: null, message: null, candidates: [], folder: null, ...extra,
  };
}

/** A cache answering by (text, is_embed). */
function cacheOf(entries: Array<[string, boolean, Env]>): ReferenceCacheRead {
  const m = new Map(entries.map(([text, embed, e]) => [`${text} ${embed}`, e]));
  return { get: (text, isEmbed) => m.get(`${text} ${isEmbed}`) };
}

function inputs(cache: ReferenceCacheRead, over: Partial<ProblemInputs> = {}): ProblemInputs {
  return { resolvedCache: cache, refForPath: async () => null, projectRoot: () => '/site', ...over };
}

/** The state with its parse finished, unless `partial`: the first parse is time-boxed, so a slow runner stops short of the end. */
function stateWith(doc: string, cache: ReferenceCacheRead, opts: { anchor?: number; over?: Partial<ProblemInputs>; partial?: boolean } = {}): EditorState {
  const state = EditorState.create({
    doc,
    selection: { anchor: opts.anchor ?? doc.length },
    extensions: [markdown({ extensions: [wikilinkConfig] }), problemsExtension(inputs(cache, opts.over))],
  });
  if (opts.partial || syntaxTreeAvailable(state, doc.length)) return state;
  const view = new EditorView({ state, parent: document.body });
  for (let i = 0; i < 50 && !syntaxTreeAvailable(view.state, doc.length); i++) forceParsing(view, doc.length, 2000);
  const parsed = view.state;
  view.destroy();
  return parsed;
}

/** The marks the field draws: the marked text and its consequence. */
function marks(state: EditorState): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const source of state.facet(EditorView.decorations)) {
    if (typeof source === 'function') continue;
    source.between(0, state.doc.length, (from, to, deco) => {
      const cls = (deco.spec as { class?: string }).class ?? '';
      const m = /cm-problem--(\w+)/.exec(cls);
      if (m) out.push([state.doc.sliceString(from, to), m[1]]);
    });
  }
  return out;
}

const resolvedImage = (absolute_path: string, provenance: string) =>
  ({ provenance, absolute_path }) as Env['resolved'];

describe('what each envelope becomes', () => {
  test('a missing image blocks publishing and says what moss looked for', () => {
    const cache = cacheOf([['gone.png', true, env('not-found')], ['img/far.jpg', true, env('not-found')], ['slide.jpg', true, env('not-found')]]);
    const state = stateWith('x\n\n![](gone.png)\n\n![](img/far.jpg)\n\n![[slide.jpg|800x450]]\n', cache, { anchor: 0 });
    // The mark is the path alone, never an embed's brackets or width.
    expect(marks(state)).toEqual([['gone.png', 'blocks'], ['img/far.jpg', 'blocks'], ['slide.jpg', 'blocks']]);
    const [bare, path, wiki] = readProblems(state);
    expect(bare.sentence).toBe("No file named gone.png in your folder. The site can't be published without it.");
    expect(path.sentence).toBe("No file at img/far.jpg in your folder. The site can't be published without it.");
    expect(wiki.sentence).toBe("No file named slide.jpg in your folder. The site can't be published without it.");
    expect(bare.fixes).toEqual([]);
    // A card opened with the caret on the embed's brackets still carries the reason.
    const probe = embedAt(state, state.doc.toString().indexOf('![[') + 1)!.parts.targetFrom;
    expect(problemCardAugmenter(null, state, probe)?.body.textContent).toBe(wiki.sentence);
  });

  test('a missing image offers the same picture under another extension', async () => {
    const cache = cacheOf([['slide.jpg', true, env('not-found', { candidates: ['img/slide.png', 'a.jpeg', 'b.jpeg', 'c.jpeg'] })]]);
    const doc = '![[slide.jpg|800x450]]\n\nx';
    const refForPath = vi.fn(async () => 'slide.png');
    const view = new EditorView({ state: stateWith(doc, cache, { over: { refForPath } }), parent: document.body });
    const problem = view.state.field(problemsField).all[0];
    expect(problem.fixes.map((f) => f.label)).toEqual(['Use img/slide.png', 'Use a.jpeg', 'Use b.jpeg']);
    await problem.fixes[0].run(view);
    expect(refForPath).toHaveBeenCalledWith('/site/img/slide.png');
    expect(view.state.doc.toString()).toBe('![[slide.png|800x450]]\n\nx');
    view.destroy();
  });

  test('an ambiguous image warns, names the file moss uses, and offers each candidate', () => {
    const cache = cacheOf([['cat.png', true, env('ambiguous', { candidates: ['a/cat.png', 'b/cat.png', 'c/cat.png', 'd/cat.png'] })]]);
    const [p] = readProblems(stateWith('![](cat.png)\n\nx', cache));
    expect(p.consequence).toBe('warns');
    expect(p.sentence).toBe('Several files are named cat.png. moss uses a/cat.png.');
    expect(p.fixes).toEqual(['Use a/cat.png', 'Use b/cat.png', 'Use c/cat.png']);
  });

  test('a wrong-case path warns and offers the real file, named by its place in the folder', () => {
    const cache = cacheOf([['Cat.PNG', true, env('image', { resolved: resolvedImage('/site/img/cat.png', 'case-mismatch') })]]);
    const [p] = readProblems(stateWith('![](Cat.PNG)\n\nx', cache));
    expect([p.consequence, p.sentence, p.fixes]).toEqual(['warns', 'In your folder this file is spelled cat.png.', ['Use img/cat.png']]);
  });

  test('in a host that cannot write a reference form, the image warns with no fix to offer', () => {
    const cache = cacheOf([['Cat.PNG', true, env('image', { resolved: resolvedImage('/site/img/cat.png', 'case-mismatch') })]]);
    const [p] = readProblems(stateWith('![](Cat.PNG)\n\nx', cache, { over: { refForPath: undefined } }));
    expect([p.consequence, p.fixes]).toEqual(['warns', []]);
  });

  test('a backslash path warns and offers the forward-slash form', () => {
    const cache = cacheOf([['img\\cat.png', true, env('image', { resolved: resolvedImage('/site/img/cat.png', 'separator-fallback') })]]);
    const [p] = readProblems(stateWith('![](img\\cat.png)\n\nx', cache));
    expect([p.consequence, p.fixes]).toEqual(['warns', ['Use img/cat.png']]);
  });

  test('a link to a moved page warns, says the old address forwards, and offers the update', () => {
    const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
    const [p] = readProblems(stateWith('[Ma](/authors/ma/)\n\nx', cache));
    expect(p.consequence).toBe('warns');
    expect(p.sentence).toBe('This page moved to /people/ma/. The old address forwards there.');
    expect(p.fixes).toEqual(['Update to /people/ma/']);
  });

  test('a case or slug mismatch warns and offers the page’s own address', () => {
    const cache = cacheOf([['/About/', false, env('link', { url: '/about/', message: 'resolves to canonical URL /about/' })]]);
    const [p] = readProblems(stateWith('[a](/About/)\n\nx', cache));
    expect([p.consequence, p.fixes]).toEqual(['warns', ['Use /about/']]);
  });

  test('a link to a page not written yet is dimmed, never flagged', () => {
    const state = stateWith('[[someday]]\n\nx', cacheOf([['someday', false, env('not-found')]]));
    expect(marks(state)).toEqual([['someday', 'draft']]);
  });

  test('a resolved link, an external link, an anchor and an unresolved-yet target are no problem', () => {
    const cache = cacheOf([
      ['/ok/', false, env('link', { url: '/ok/' })],
      ['https://example.com', false, env('external')],
      ['#top', false, env('anchor')],
    ]);
    expect(readProblems(stateWith('[a](/ok/) [b](https://example.com) [c](#top) [d](/pending/)\n\nx', cache))).toEqual([]);
  });
});

describe('when a mark hides', () => {
  const cache = cacheOf([['gone', false, env('moved', { url: '/new/' })], ['lost', false, env('moved', { url: '/new2/' })]]);
  const doc = '[a](gone)  [b](lost)';

  test('only the construct the cursor is in hides; its sibling stays marked', () => {
    expect(marks(stateWith(doc, cache, { anchor: doc.indexOf('gone') + 1 }))).toEqual([['lost', 'warns']]);
    expect(marks(stateWith(doc, cache, { anchor: 10 }))).toEqual([['gone', 'warns'], ['lost', 'warns']]);
  });

  test('a selection change changes the marks, not the records', () => {
    let state = stateWith(doc, cache, { anchor: 10 });
    const before = readProblems(state);
    state = state.update({ selection: EditorSelection.cursor(doc.indexOf('gone') + 1) }).state;
    expect(readProblems(state)).toEqual(before);
    expect(marks(state)).toEqual([['lost', 'warns']]);
  });

  test("a picture's construct is its whole embed, in either form", () => {
    const pictures = cacheOf([['gone.png', true, env('not-found')]]);
    for (const [text, inside] of [['![alt](gone.png)', 'alt'], ['![[gone.png|200]]', '200']]) {
      const page = `${text}\n\nx`;
      expect(marks(stateWith(page, pictures))).toEqual([['gone.png', 'blocks']]);
      for (const anchor of [page.indexOf(inside) + 1, text.length]) expect(marks(stateWith(page, pictures, { anchor }))).toEqual([]);
    }
  });
});

describe('a large document', () => {
  test('a cached reference past the first parsed stretch is marked once the parse reaches the end', () => {
    // Far past the initial time-budgeted parse; asserted so a faster machine cannot mask the premise.
    const doc = 'lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n'.repeat(600) + '[tail](/authors/ma/)\n';
    const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
    const view = new EditorView({ state: stateWith(doc, cache, { anchor: 0, partial: true }), parent: document.body });
    expect(syntaxTreeAvailable(view.state, doc.length)).toBe(false);
    expect(marks(view.state)).toEqual([]);
    for (let i = 0; i < 50 && !syntaxTreeAvailable(view.state, doc.length); i++) forceParsing(view, doc.length, 2000);
    expect(marks(view.state)).toEqual([['/authors/ma/', 'warns']]);
    view.destroy();
  });
});

describe('build evidence', () => {
  const evidence = (from: number, to: number) => setEvidenceProblems.of([{ from, to, reference: 'hero.png' }]);
  // A Hero block the live resolver does not see: the build's span is the only answer.
  const doc = 'intro\n:::hero {image="hero.png"}\n:::\n';
  const at = doc.indexOf('hero.png');

  test('becomes a blocking mark, and moves with an edit before it', () => {
    let state = stateWith(doc, cacheOf([]), { anchor: 0 });
    state = state.update({ effects: evidence(at, at + 8) }).state;
    expect(marks(state)).toEqual([['hero.png', 'blocks']]);
    state = state.update({ changes: { from: 0, insert: 'new ' } }).state;
    expect(marks(state)).toEqual([['hero.png', 'blocks']]);
  });

  test('stays marked while the selection is on it, because Open at line selects exactly it', () => {
    let state = stateWith(doc, cacheOf([]), { anchor: 0 });
    state = state.update({ effects: evidence(at, at + 8), selection: EditorSelection.range(at, at + 8) }).state;
    expect(marks(state)).toEqual([['hero.png', 'blocks']]);
  });

  test('is dropped the moment an edit touches its text, and cleared by empty evidence', () => {
    let state = stateWith(doc, cacheOf([]), { anchor: 0 }).update({ effects: evidence(at, at + 8) }).state;
    state = state.update({ changes: { from: at + 4, to: at + 8, insert: '.jpg' } }).state;
    expect(marks(state)).toEqual([]);
    state = state.update({ effects: evidence(at, at + 8) }).state;
    state = state.update({ effects: setEvidenceProblems.of([]) }).state;
    expect(marks(state)).toEqual([]);
  });

  test('gives way where the live resolver has an answer, even a clean one', () => {
    // The file was added after the build: the resolver says the image is fine.
    const md = 'x\n![a](lost.png)\n';
    const live = cacheOf([['lost.png', true, env('image', { resolved: resolvedImage('/site/lost.png', 'literal') })]]);
    let state = stateWith(md, live, { anchor: 0 });
    state = state.update({ effects: setEvidenceProblems.of([{ from: 2, to: 15, reference: 'lost.png' }]) }).state;
    expect(marks(state)).toEqual([]);
  });

  test('a missing image the build also reported is marked once', () => {
    const md = 'x\n![a](lost.png)\n';
    let state = stateWith(md, cacheOf([['lost.png', true, env('not-found')]]), { anchor: 0 });
    state = state.update({ effects: setEvidenceProblems.of([{ from: 2, to: 15, reference: 'lost.png' }]) }).state;
    expect(marks(state)).toEqual([['lost.png', 'blocks']]);
  });

  test('a resolved batch re-derives the live records', () => {
    const store = new Map<string, Env>();
    const cache: ReferenceCacheRead = { get: (text, embed) => store.get(`${text} ${embed}`) };
    let state = stateWith('[a](gone)\n\nx', cache);
    expect(marks(state)).toEqual([]);
    store.set('gone false', env('moved', { url: '/new/' }));
    state = state.update({ effects: refsResolvedEffect.of() }).state;
    expect(marks(state)).toEqual([['gone', 'warns']]);
  });
});

describe('stepping through a page’s problems', () => {
  test('the caret goes to the next problem that stops publishing, and wraps', () => {
    const cache = cacheOf([['a.png', true, env('not-found')], ['b.png', true, env('not-found')], ['someday', false, env('not-found')]]);
    const doc = 'x\n\n![](a.png)\n\n[[someday]]\n\n![](b.png)\n';
    const view = new EditorView({ state: stateWith(doc, cache, { anchor: 0 }), parent: document.body });
    const head = () => view.state.selection.main.head;
    expect(goToNextProblem(view)).toBe(true);
    expect(head()).toBe(doc.indexOf('a.png'));
    goToNextProblem(view);
    expect(head()).toBe(doc.indexOf('b.png'));
    goToNextProblem(view);
    expect(head()).toBe(doc.indexOf('a.png'));
    view.destroy();
  });

  test('a page with none is left alone', () => {
    const view = new EditorView({ state: stateWith('plain', cacheOf([]), { anchor: 2 }), parent: document.body });
    expect(goToNextProblem(view)).toBe(false);
    expect(view.state.selection.main.head).toBe(2);
    view.destroy();
  });
});

describe('the card', () => {
  function cardState(doc: string, cache: ReferenceCacheRead, providers: CardProvider[], over: Partial<ProblemInputs> = {}): EditorState {
    return EditorState.create({
      doc,
      extensions: [
        markdown({ extensions: [wikilinkConfig] }),
        problemsExtension(inputs(cache, over)),
        ...providers.map((p) => cardProviders.of(p)),
        cardAugmenters.of(problemCardAugmenter),
      ],
    });
  }
  const withControls = (cache: ReferenceCacheRead) => (base: CardProvider): CardProvider =>
    (state, pos) => { const spec = base(state, pos); return spec && withLinkControls(spec, state, pos, cache); };

  test('a missing image’s card leads with the sentence and offers choosing a file or removing the line', () => {
    const cache = cacheOf([['gone.png', true, env('not-found')]]);
    const doc = '![](gone.png)';
    const state = cardState(doc, cache, [assetCardProvider(cache as never, { pagePath: () => '/site/p.md', projectRoot: () => '/site', applyWidth: vi.fn() })]);
    const spec = cardSpecAt(state, doc.indexOf('gone'))!;
    expect(spec.body.firstElementChild?.textContent).toBe("No file named gone.png in your folder. The site can't be published without it.");
    expect(spec.actions?.map((a) => a.label)).toEqual(['Choose a file…', 'Remove']);
  });

  test('a picture that is there keeps Replace… and its widths', () => {
    const cache = cacheOf([['here.png', true, env('image', { resolved: resolvedImage('/site/here.png', 'literal') })]]);
    const doc = '![](here.png)';
    const state = cardState(doc, cache, [assetCardProvider(cache as never, { pagePath: () => '/site/p.md', projectRoot: () => '/site', applyWidth: vi.fn() })]);
    expect(cardSpecAt(state, doc.indexOf('here'))!.actions?.map((a) => a.label)).toEqual(['Replace…', '25%', '50%', '75%', '100%', 'Reset']);
  });

  test('Remove takes the embed, and its line when alone on it, in one undo step', () => {
    const cache = cacheOf([['gone.png', true, env('not-found')]]);
    const provider = assetCardProvider(cache as never, { pagePath: () => '/site/p.md', projectRoot: () => '/site', applyWidth: vi.fn() });
    const remove = (doc: string) => {
      const view = new EditorView({
        state: EditorState.create({ doc, extensions: [markdown({ extensions: [wikilinkConfig] }), history(), problemsExtension(inputs(cache)), cardProviders.of(provider), cardAugmenters.of(problemCardAugmenter)] }),
        parent: document.body,
      });
      void cardSpecAt(view.state, doc.indexOf('gone'))!.actions!.find((a) => a.label === 'Remove')!.run(view);
      const after = view.state.doc.toString();
      undo(view);
      expect(view.state.doc.toString()).toBe(doc);
      view.destroy();
      return after;
    };
    expect(remove('before\n\n![](gone.png)\n\nafter')).toBe('before\n\nafter');
    expect(remove('before\n![](gone.png)\nafter')).toBe('before\nafter');
    expect(remove('before\n\n![](gone.png)')).toBe('before\n');
    expect(remove('see ![](gone.png) here')).toBe('see  here');
  });

  test('a link to a page that does not exist still has its URL field and Unlink', () => {
    const cache = cacheOf([['nowhere.md', false, env('not-found')]]);
    const doc = '[a](nowhere.md)';
    const state = cardState(doc, cache, [withControls(cache)(linkCardProvider)]);
    const spec = cardSpecAt(state, doc.indexOf('nowhere'))!;
    expect(spec.title).toBe('nowhere.md');
    expect(spec.body.textContent).toBe('No page named nowhere.md yet.');
    expect(spec.fields?.map((f) => f.label)).toEqual(['URL']);
    expect(spec.actions?.map((a) => a.label)).toEqual(['Open', 'Unlink']);
  });

  test('the fix comes first, and the provider’s body is wrapped, never written into', () => {
    const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
    const doc = '[Ma](/authors/ma/)';
    const providerBody = document.createElement('div');
    const provider: CardProvider = () => ({ title: 'Ma', body: providerBody, actions: [{ label: 'Open', run: () => {} }] });
    const spec = cardSpecAt(cardState(doc, cache, [provider]), doc.indexOf('/authors'))!;
    expect(spec.actions?.map((a) => a.label)).toEqual(['Update to /people/ma/', 'Open']);
    expect(spec.body).not.toBe(providerBody);
    expect(spec.body.lastElementChild).toBe(providerBody);
    // A provider filling its body later (the page card's RPC) cannot erase the sentence.
    providerBody.textContent = 'The page about Ma.';
    expect(spec.body.firstElementChild?.textContent).toContain('moved to /people/ma/');
  });

  test('with no problem the card is unchanged', () => {
    const cache = cacheOf([['/ok/', false, env('link', { url: '/ok/' })]]);
    const body = document.createElement('div');
    const spec = cardSpecAt(cardState('[a](/ok/)', cache, [() => ({ title: 'a', body })]), 5)!;
    expect(spec.body).toBe(body);
  });
});

describe('hover explains, ⌘K fixes', () => {
  test('the hover card shows the sentence and no buttons; the card opened with ⌘K offers the fix', () => {
    vi.useFakeTimers();
    try {
      const doc = '[Ma](/authors/ma/)';
      const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
      const view = new EditorView({
        state: EditorState.create({
          doc,
          selection: { anchor: 0 },
          extensions: [
            markdown({ extensions: [wikilinkConfig] }), problemsExtension(inputs(cache)), referenceCardExtension(),
            cardProviders.of(linkCardProvider), cardAugmenters.of(problemCardAugmenter),
          ],
        }),
        parent: document.body,
      });
      const at = doc.indexOf('/authors') + 2;
      view.posAtCoords = () => at;
      view.dom.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
      vi.advanceTimersByTime(200);
      const hover = view.dom.querySelector('.cm-reference-card')!;
      expect(hover.textContent).toContain('moved to /people/ma/');
      expect(hover.querySelectorAll('button')).toHaveLength(0);

      openCardAt(view, at);
      const opened = view.dom.querySelector('.cm-reference-card')!;
      expect([...opened.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Update to /people/ma/']);
      view.destroy();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('a fix applies', () => {
  function viewOf(state: EditorState): EditorView {
    return new EditorView({ state, parent: document.body });
  }

  test('Update to … rewrites only the link’s address', () => {
    const doc = '[/authors/ma/](/authors/ma/) after';
    const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
    const view = viewOf(stateWith(doc, cache));
    const card = problemCardAugmenter(null, view.state, doc.lastIndexOf('/authors'))!;
    card.actions![0].run(view);
    expect(view.state.doc.toString()).toBe('[/authors/ma/](/people/ma/) after');
    view.destroy();
  });

  test('a link fix does nothing once the address no longer reads what the card showed', () => {
    const doc = '[a](/authors/ma/)';
    const cache = cacheOf([['/authors/ma/', false, env('moved', { url: '/people/ma/' })]]);
    const view = viewOf(stateWith(doc, cache));
    const card = problemCardAugmenter(null, view.state, doc.indexOf('/authors'))!;
    view.dispatch({ changes: { from: doc.indexOf('ma/'), to: doc.indexOf('ma/') + 2, insert: 'li' } });
    card.actions![0].run(view);
    expect(view.state.doc.toString()).toBe('[a](/authors/li/)');
    view.destroy();
  });

  test('an image fix does nothing if the author typed while moss fetched the reference form', async () => {
    const doc = '![](Cat.PNG)\n\nx';
    const cache = cacheOf([['Cat.PNG', true, env('image', { resolved: resolvedImage('/site/img/cat.png', 'case-mismatch') })]]);
    let finish!: (ref: string) => void;
    const view = viewOf(stateWith(doc, cache, { over: { refForPath: () => new Promise((r) => { finish = r; }) } }));
    const card = problemCardAugmenter(null, view.state, 5)!;
    const running = card.actions![0].run(view) as unknown as Promise<void>;
    view.dispatch({ changes: { from: 4, insert: 'big-' } });
    finish('img/cat.png');
    await running;
    expect(view.state.doc.toString()).toBe('![](big-Cat.PNG)\n\nx');
    view.destroy();
  });

  test('Use … writes the reference form moss computes for the real file, inside a wikilink embed', async () => {
    const doc = '![[Cat.PNG|50%]]\n\nx';
    const cache = cacheOf([['Cat.PNG', true, env('image', { resolved: resolvedImage('/site/img/cat.png', 'case-mismatch') })]]);
    const refForPath = vi.fn(async () => 'img/cat.png');
    const view = viewOf(stateWith(doc, cache, { over: { refForPath } }));
    const card = problemCardAugmenter(null, view.state, 4)!;
    await card.actions![0].run(view);
    expect(refForPath).toHaveBeenCalledWith('/site/img/cat.png');
    expect(view.state.doc.toString()).toBe('![[img/cat.png|50%]]\n\nx');
    view.destroy();
  });
});
