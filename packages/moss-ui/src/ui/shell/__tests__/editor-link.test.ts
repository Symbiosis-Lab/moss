/**
 * The link between the preview and the editor, driven through its host
 * contract alone: what any host must supply, with no desktop app around it.
 */
import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import type { PreviewManager } from '../../preview/preview-manager';

const { mossEvents } = vi.hoisted(() => ({
  mossEvents: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("../../utils/moss-events", () => ({
  onMossEvent: vi.fn((kind: string, cb: (event: { payload: unknown }) => void) => {
    mossEvents.set(kind, cb);
    return Promise.resolve(() => {});
  }),
}));
vi.mock('../../engine', () => ({ engine: { resolvePageSource: vi.fn(), describeSourceRole: vi.fn() } }));

import { EditorLink, type EditorLinkHost } from '../editor-link';
import { host, inPageBus } from "../../host";
import { eventBus, Events } from '../../context/event-bus';
import { engine } from '../../engine';
import { pageSource } from '../../preview/page-source';

const answer = (data: Record<string, unknown>) =>
  ({ status: 'ok', data: { is_dir: false, is_article: false, takeover: null, ...data } }) as never;

function deferred() {
  let resolve!: (value: unknown) => void;
  const promise = new Promise((r) => { resolve = r; });
  return { promise, resolve };
}

const flush = () => new Promise((r) => setTimeout(r, 0));
/** Bytes landed in the open folder. */
const arrived = () => mossEvents.get('SourceStructureSettled')!({ payload: { folder_path: '/site', outcome: 'Ready' } });

function fakePreview() {
  return {
    getActivePage: vi.fn(() => ({ url: 'http://x/a/', sourceFile: 'a.md' })),
    getCurrentUrl: vi.fn(() => 'http://x/a/'),
    navigateToPage: vi.fn(),
    markPendingEcho: vi.fn(),
    armNewPageWait: vi.fn(),
    claimScrollForEditor: vi.fn(),
    scrollToSourceLine: vi.fn(),
    setLastEditorPosition: vi.fn(),
  };
}

function link(over: Partial<EditorLinkHost> = {}) {
  const preview = fakePreview();
  const host: EditorLinkHost = {
    preview: () => preview as unknown as PreviewManager,
    editorIsOpen: () => true,
    editorIsFullWidth: () => false,
    folderPath: () => '/site',
    openTarget: vi.fn(),
    openContextMenu: vi.fn(),
    ...over,
  };
  const l = new EditorLink(host);
  l.connect();
  links.push(l);
  return { l, host, preview };
}

// The page source is shared; a past test's link would otherwise forward into this test's bus.
const links: EditorLink[] = [];

const emitted: Array<[string, unknown]> = [];
const post = (data: unknown) => window.dispatchEvent(new MessageEvent('message', { data }));

// Each link's window listener would otherwise answer the next test's messages.
const windowListeners: Array<[string, EventListenerOrEventListenerObject]> = [];
const addWindowListener = window.addEventListener.bind(window);
beforeEach(() => {
  vi.spyOn(window, 'addEventListener').mockImplementation((type: string, cb: EventListenerOrEventListenerObject) => {
    windowListeners.push([type, cb]);
    addWindowListener(type, cb);
  });
  // A page's own bus, as a browser host has: what the link says, the test hears.
  host.bus = inPageBus();
  emitted.length = 0;
  for (const name of ['editor:load-source', 'editor:reveal-line', 'editor:select-text', 'editor:reveal-field']) {
    host.bus.on(name, (payload) => emitted.push([name, payload]));
  }
  mossEvents.clear();
  eventBus.clear();
  pageSource.invalidate();
  vi.mocked(engine.resolvePageSource).mockReset();
});
afterEach(() => {
  for (const l of links.splice(0)) l.disconnect();
  for (const [type, cb] of windowListeners.splice(0)) window.removeEventListener(type, cb);
  vi.restoreAllMocks();
});

describe('EditorLink', () => {
  test('only a settled answer reaches the editor, and only an open one', async () => {
    let open = false;
    const { preview } = link({ editorIsOpen: () => open });
    vi.mocked(engine.resolvePageSource).mockResolvedValueOnce(answer({ source_path: '/site/a.md', is_article: true }));
    await pageSource.navigate('/a/', false);
    expect(emitted).toEqual([]);

    open = true;
    preview.getCurrentUrl.mockReturnValue('http://x/');
    const lookup = deferred();
    vi.mocked(engine.resolvePageSource).mockReturnValueOnce(lookup.promise as never);
    const settled = pageSource.navigate('/', false);
    await Promise.resolve();
    // Retaining while the lookup is in flight means saying nothing at all.
    expect(emitted).toEqual([]);
    lookup.resolve(answer({ source_path: null, is_dir: true }));
    await settled;
    expect(emitted).toEqual([['editor:load-source', { sourcePath: null, isRoot: true, takeover: null, pending: null }]]);
  });

  test("an arrival re-ask of the page on screen reaches the editor, and the bus only when the answer changed", async () => {
    link();
    const heard = vi.fn();
    eventBus.on(Events.PREVIEW_URL_CHANGED, heard);
    vi.mocked(engine.resolvePageSource).mockResolvedValue(answer({ source_path: null, source_pending: true }));
    await pageSource.navigate('/a/', false);
    expect(heard).toHaveBeenCalledOnce();
    host.bus.emit('editor:source-pending', { pending: true, sourcePath: null });

    arrived();
    await vi.waitFor(() => expect(engine.resolvePageSource).toHaveBeenCalledTimes(2));
    await flush();
    expect(heard).toHaveBeenCalledOnce();

    vi.mocked(engine.resolvePageSource).mockResolvedValue(answer({ source_path: '/site/a.md', is_article: true }));
    arrived();
    await vi.waitFor(() => expect(emitted.at(-1)).toEqual(['editor:load-source', expect.objectContaining({ sourcePath: '/site/a.md', pending: null })]));
    expect(heard).toHaveBeenCalledTimes(2);
    expect(heard).toHaveBeenLastCalledWith(expect.objectContaining({ sourcePath: '/site/a.md', isArticle: true, isReload: true }));
    // Exactly one load per answer: the re-ask does not push its own copy.
    expect(emitted.filter(([, node]) => (node as { sourcePath?: string }).sourcePath === '/site/a.md')).toHaveLength(1);
  });

  test('a reload of the page on screen reaches the editor only while it waits on that page', async () => {
    link();
    vi.mocked(engine.resolvePageSource).mockResolvedValue(answer({ source_path: null, source_known: false }));
    await pageSource.navigate('/a/', false);
    expect(emitted.at(-1)).toEqual(['editor:load-source', expect.objectContaining({ pending: 'lookup' })]);

    vi.mocked(engine.resolvePageSource).mockResolvedValue(answer({ source_path: '/site/a.md' }));
    await pageSource.navigate('/a/', true);
    expect(emitted.at(-1)).toEqual(['editor:load-source', expect.objectContaining({ sourcePath: '/site/a.md', pending: null })]);

    // The user opened another file; a rebuild of the page must not take the editor back.
    host.bus.emit('editor:source-pending', { pending: false, sourcePath: null });
    emitted.length = 0;
    await pageSource.navigate('/a/', true);
    expect(emitted).toEqual([]);
  });

  test('opening the editor bakes the answer in hand, or a bare page whose answer follows on request', async () => {
    const { l } = link();
    const lookup = deferred();
    vi.mocked(engine.resolvePageSource).mockReturnValueOnce(lookup.promise as never);
    expect(l.openNode()).toEqual({ sourcePath: null, isRoot: false, takeover: null });
    expect(engine.resolvePageSource).toHaveBeenCalledWith('/a/');

    host.bus.emit('editor:request-page', {});
    lookup.resolve(answer({ source_path: '/site/a.md' }));
    await vi.waitFor(() => expect(emitted).toEqual([['editor:load-source', { sourcePath: '/site/a.md', isRoot: false, takeover: null, pending: null }]]));
    expect(l.openNode()).toEqual({ sourcePath: '/site/a.md', isRoot: false, takeover: null });
    expect(engine.resolvePageSource).toHaveBeenCalledOnce();
  });

  test('a double click on another file opens it even with the editor closed', () => {
    const { host } = link({ editorIsOpen: () => false });
    const target = { kind: 'body-line', path: 'b.md', line: 3 };
    post({ type: 'moss-source-target', target, interaction: 'click' });
    expect(host.openTarget).not.toHaveBeenCalled();
    post({ type: 'moss-source-target', target, interaction: 'dblclick' });
    expect(host.openTarget).toHaveBeenCalledWith(target);
  });

  test('a click in the open file reveals its lines; a closed editor hears nothing', () => {
    let open = false;
    link({ editorIsOpen: () => open });
    post({ type: 'moss-source-target', target: { kind: 'body-range', startLine: 4, endLine: 6 }, interaction: 'click' });
    expect(emitted).toEqual([]);
    open = true;
    post({ type: 'moss-source-target', target: { kind: 'body-range', startLine: 4, endLine: 6 }, interaction: 'click' });
    expect(emitted).toEqual([['editor:reveal-line', { startLine: 4, endLine: 6 }]]);
  });

  test('the context menu goes to the host whether or not the editor is open', () => {
    const { host } = link({ editorIsOpen: () => false });
    const ctx = { type: 'moss-context-menu', x: 1, y: 2, page: '/a/', targets: [], styles: [], link: null, image: null, selection: null };
    post(ctx);
    expect(host.openContextMenu).toHaveBeenCalledWith(ctx);
  });

  test('a full-width editor does not scroll the preview', async () => {
    let full = true;
    const { preview } = link({ editorIsFullWidth: () => full });
    host.bus.emit('scroll-sync:editor-to-preview', { type: 'source-line', line: 9 });
    expect(preview.scrollToSourceLine).not.toHaveBeenCalled();
    full = false;
    host.bus.emit('scroll-sync:editor-to-preview', { type: 'source-line', line: 9 });
    expect(preview.scrollToSourceLine).toHaveBeenCalledWith(9, null, false, false, false);
  });

  test("a folder's listing settling retries the pending pane only for the open folder", () => {
    const { l } = link();
    const retry = vi.spyOn(l.staleSource, 'onFilesArrived').mockResolvedValue(undefined);
    mossEvents.get('SourceStructureSettled')!({ payload: { folder_path: '/other', outcome: 'Ready' } });
    expect(retry).not.toHaveBeenCalled();
    mossEvents.get('SourceStructureSettled')!({ payload: { folder_path: '/site', outcome: 'Ready' } });
    expect(retry).toHaveBeenCalledOnce();
  });
});

describe('a page gesture while the editor shows another file', () => {
  const paragraph = { kind: 'body-line', line: 5 };
  const gesture = (interaction: string) => post({ type: 'moss-source-target', target: paragraph, interaction });
  const revealed = () => emitted.filter(([name]) => name === 'editor:reveal-line');

  async function onPageA() {
    const made = link();
    vi.mocked(engine.resolvePageSource).mockResolvedValueOnce(answer({ source_path: '/site/a.md', is_article: true }));
    await pageSource.navigate('/a/', false);
    // Past the settle after a page switch.
    const now = Date.now();
    vi.spyOn(Date, 'now').mockReturnValue(now + 1000);
    return made;
  }

  test('a click reveals nothing there, and a double click opens the page at the place', async () => {
    const { host: h } = await onPageA();
    host.bus.emit('editor:file-ready', { path: 'footer.md' });
    gesture('click');
    expect(revealed()).toEqual([]);
    gesture('dblclick');
    expect(h.openTarget).toHaveBeenCalledWith({ ...paragraph, path: 'a.md' });

    host.bus.emit('editor:file-ready', { path: 'a.md' });
    gesture('click');
    expect(revealed()).toEqual([['editor:reveal-line', { startLine: 5, endLine: 5 }]]);
    gesture('dblclick');
    expect(h.openTarget).toHaveBeenLastCalledWith(paragraph);
  });

  test('a menu row for the page names its file, and "edit this page" becomes that page', async () => {
    const { l } = await onPageA();
    expect(l.located(paragraph)).toEqual(paragraph);
    expect(l.located(null)).toBeNull();
    host.bus.emit('editor:file-ready', { path: 'footer.md' });
    expect(l.located(paragraph)).toEqual({ ...paragraph, path: 'a.md' });
    expect(l.located(null)).toEqual({ kind: 'page', path: 'a.md' });
    expect(l.located({ kind: 'page', path: 'b.md' })).toEqual({ kind: 'page', path: 'b.md' });
    // Template chrome has no source in any file.
    expect(l.located({ kind: 'none' })).toEqual({ kind: 'none' });
  });

  test('the same file under another Unicode spelling, or a folder just switched to, is not another file', async () => {
    const made = link();
    vi.mocked(engine.resolvePageSource).mockResolvedValueOnce(answer({ source_path: '/site/B\u00fcndnis.md', is_article: true }));
    await pageSource.navigate('/a/', false);
    host.bus.emit('editor:file-ready', { path: 'Bu\u0308ndnis.md' });
    expect(made.l.located(paragraph)).toEqual(paragraph);
    host.bus.emit('editor:file-ready', { path: 'footer.md' });
    made.l.folderChanged();
    expect(made.l.located(paragraph)).toEqual(paragraph);
  });

  test('a closed editor is on no file: the page opens as it always did', async () => {
    let open = true;
    const made = link({ editorIsOpen: () => open });
    vi.mocked(engine.resolvePageSource).mockResolvedValueOnce(answer({ source_path: '/site/a.md', is_article: true }));
    await pageSource.navigate('/a/', false);
    host.bus.emit('editor:file-ready', { path: 'footer.md' });
    open = false;
    expect(made.l.located(null)).toBeNull();
  });
});

describe('inPageBus', () => {
  test('a message reaches every listener on the same page until it unsubscribes', () => {
    const bus = inPageBus();
    const heard: unknown[] = [];
    const off = bus.on('instrument:hero-frame', (p) => heard.push(p));
    bus.on('instrument:hero-frame', (p) => heard.push(['second', p]));
    bus.emit('instrument:hero-frame', 1);
    off();
    bus.emit('instrument:hero-frame', 2);
    bus.emit('another', 3);
    expect(heard).toEqual([1, ['second', 1], ['second', 2]]);
  });
});

describe("the editor's side of the link, in one page", () => {
  test('the layout button asks the shell over the bus, and the shell answers it there', async () => {
    host.bus = inPageBus();
    document.body.innerHTML = '<button id="editor-layout-btn"></button>';
    const { wireLayoutButton } = await import('../../editor/editor-layout-btn');
    wireLayoutButton();
    const asked: string[] = [];
    host.bus.on('editor:request-full-width', () => asked.push('full'));
    host.bus.on('editor:request-split', () => asked.push('split'));

    document.getElementById('editor-layout-btn')!.click();
    host.bus.emit('editor:full-width-changed', { fullWidth: true });
    document.getElementById('editor-layout-btn')!.click();
    expect(asked).toEqual(['full', 'split']);
  });
});
