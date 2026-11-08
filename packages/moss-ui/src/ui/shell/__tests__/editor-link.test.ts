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
  return { l, host, preview };
}

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
});
afterEach(() => {
  for (const [type, cb] of windowListeners.splice(0)) window.removeEventListener(type, cb);
  vi.restoreAllMocks();
});

describe('EditorLink', () => {
  test('a previewed page is recorded always, and forwarded only to an open editor', () => {
    let open = false;
    const { l } = link({ editorIsOpen: () => open });
    eventBus.emit(Events.PREVIEW_URL_CHANGED, { urlPath: '/a/', sourcePath: '/site/a.md', takeover: null, isReload: false });
    expect(l.page).toEqual({ sourcePath: '/site/a.md', takeover: null });
    expect(emitted).toEqual([]);

    open = true;
    eventBus.emit(Events.PREVIEW_URL_CHANGED, { urlPath: '/', sourcePath: null, takeover: null, isReload: false });
    expect(emitted).toEqual([['editor:load-source', { sourcePath: null, isRoot: true, takeover: null }]]);
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
