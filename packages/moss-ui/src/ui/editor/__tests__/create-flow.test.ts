/**
 * Untitled-first create flow (create-flow.ts) — the pure naming helpers, the
 * session state machine (rename-follows-title until the first body edit), and
 * Undo.
 */
import { describe, test, expect, vi, beforeEach } from 'vitest';

const { mockShowToast, mockMossConfirm } = vi.hoisted(() => ({ mockShowToast: vi.fn(), mockMossConfirm: vi.fn() }));

// Commands are INJECTED through CreateFlowDeps, so no command module is
// mocked; only the toast and native-confirm side effects are stubbed.
vi.mock("../../shared/ui/toast/toast-manager", () => ({ showToast: mockShowToast, showAck: vi.fn() }));
vi.mock("../../shared/ui/moss-alert", () => ({ mossConfirm: mockMossConfirm, mossAlert: vi.fn() }));

const mockListDirectory = vi.fn();
const mockCreateFile = vi.fn();
const mockInstantiateTemplate = vi.fn();
const mockDeleteEntry = vi.fn();
const mockListTemplates = vi.fn();
const mockRenameTemplate = vi.fn();
const mockDeleteTemplate = vi.fn();
const mockSetTemplateFolder = vi.fn();
const mockSiteLanguages = vi.fn();
const mockPageTranslations = vi.fn();
const mockCreateTranslation = vi.fn();

import {
  CreateFlow,
  revealAndOpenNewEntry,
  untitledBasename,
  hasDatePrefixConvention,
  slugifyTitle,
  generatedBasename,
  todayIso,
  type CreateFlowDeps,
  type CreateFlowTree,
} from '../create-flow';
import type { EntryId } from '../entry-registry';

// ── Pure naming helpers ────────────────────────────────────────────────────

describe('untitledBasename', () => {
  test('empty folder gets untitled.md', () => {
    expect(untitledBasename([])).toBe('untitled.md');
  });

  test('collision auto-suffixes with a counter, starting at 2', () => {
    expect(untitledBasename(['untitled.md'])).toBe('untitled 2.md');
    expect(untitledBasename(['untitled.md', 'untitled 2.md'])).toBe('untitled 3.md');
  });

  test('comparison is case-insensitive (APFS default)', () => {
    expect(untitledBasename(['Untitled.md'])).toBe('untitled 2.md');
  });

  test('a gap in the sequence is reused', () => {
    expect(untitledBasename(['untitled 2.md'])).toBe('untitled.md');
  });
});

describe('hasDatePrefixConvention', () => {
  test('two dated .md siblings establish the convention', () => {
    expect(hasDatePrefixConvention(['2026-08-01-a.md', '2026-08-02-b.md'])).toBe(true);
  });

  test('one dated sibling is not a convention', () => {
    expect(hasDatePrefixConvention(['2026-08-01-a.md', 'other.md'])).toBe(false);
  });

  test('dated non-md entries (folders, images) do not count', () => {
    expect(hasDatePrefixConvention(['2026-08-01-trip', '2026-08-02-trip.jpg'])).toBe(false);
  });
});

describe('slugifyTitle / generatedBasename', () => {
  test('slug lowercases and collapses punctuation to dashes', () => {
    expect(slugifyTitle('Winter light')).toBe('winter-light');
    expect(slugifyTitle('  Hello,   World!  ')).toBe('hello-world');
  });

  test('slug keeps CJK', () => {
    expect(slugifyTitle('冬日的光')).toBe('冬日的光');
  });

  test('empty title generates nothing (keep the untitled name)', () => {
    expect(generatedBasename('   ', { datePrefix: false, today: '2026-08-14' })).toBeNull();
  });

  test('without the date convention the title is used verbatim (round-trips in the H1)', () => {
    expect(generatedBasename('Winter light', { datePrefix: false, today: '2026-08-14' }))
      .toBe('Winter light.md');
  });

  test('with the date convention the name is date + slug', () => {
    expect(generatedBasename('Winter light', { datePrefix: true, today: '2026-08-14' }))
      .toBe('2026-08-14-winter-light.md');
  });

  test('a punctuation-only title falls back to untitled under the date convention', () => {
    expect(generatedBasename('!!!', { datePrefix: true, today: '2026-08-14' }))
      .toBe('2026-08-14-untitled.md');
  });

  test('todayIso is YYYY-MM-DD', () => {
    expect(todayIso(new Date(2026, 7, 14))).toBe('2026-08-14');
  });
});

// ── Session state machine ──────────────────────────────────────────────────

const ROOT = '/root';

interface Harness {
  flow: CreateFlow;
  tree: CreateFlowTree & { refreshAndRender: ReturnType<typeof vi.fn>; revealNewFile: ReturnType<typeof vi.fn>; startCreate: ReturnType<typeof vi.fn> };
  deps: CreateFlowDeps & { renameEntry: ReturnType<typeof vi.fn>; setTitleField: ReturnType<typeof vi.fn>; setHeadingSaved: ReturnType<typeof vi.fn>; createTranslation: ReturnType<typeof vi.fn> };
  setCurrentPath(p: string | null): void;
  statusEl(): HTMLElement;
  headingEl(): HTMLElement;
  selectedEvents: Array<{ filePath: string; isNewFile?: boolean }>;
  folderNavigateEvents: Array<{ folderPath: string; folderName: string }>;
}

function makeHarness(anchor: { path: string; isDir: boolean } = { path: 'posts', isDir: true }): Harness {
  document.body.innerHTML = `
    <button id="editor-create-btn"></button>
    <button id="editor-create-caret"></button>
    <div id="editor-heading" data-empty="false" tabindex="-1"></div>
    <div id="moss-create-status" hidden></div>
  `;
  // Registry-lite: EntryId identity survives renames (the map is updated by
  // the renameEntry stub the way the real tree lifecycle updates the registry).
  let nextId = 1;
  const byPath = new Map<string, number>();
  const byId = new Map<number, string>();
  const upsert = (rel: string): EntryId => {
    let id = byPath.get(rel);
    if (id === undefined) {
      id = nextId++;
      byPath.set(rel, id);
      byId.set(id, rel);
    }
    return id as EntryId;
  };
  let currentPath: string | null = null;

  const tree = {
    createAnchor: anchor,
    refreshAndRender: vi.fn(async () => {}),
    revealNewFile: vi.fn(),
    startCreate: vi.fn(async () => {}),
  };
  const renameEntry = vi.fn(async (oldAbs: string, newAbs: string) => {
    const oldRel = oldAbs.slice(ROOT.length + 1);
    const newRel = newAbs.slice(ROOT.length + 1);
    const id = byPath.get(oldRel);
    if (id !== undefined) {
      byPath.delete(oldRel);
      byPath.set(newRel, id);
      byId.set(id, newRel);
    }
    if (currentPath === oldRel) currentPath = newRel;
    return { status: 'ok' as const };
  });
  const selectedEvents: Harness['selectedEvents'] = [];
  const deps = {
    projectRoot: () => ROOT,
    fileTree: () => tree,
    openNewEntry: vi.fn(async (filePath: string) => { selectedEvents.push({ filePath, isNewFile: true }); }),
    currentFilePath: () => currentPath,
    upsert,
    pathOf: (id: EntryId) => byId.get(id as unknown as number) ?? null,
    registerPendingRename: vi.fn(),
    showInPreview: vi.fn(),
    renameEntry,
    setTitleField: vi.fn(),
    setHeadingSaved: vi.fn(),
    listDirectory: mockListDirectory,
    createFile: mockCreateFile,
    instantiateTemplate: mockInstantiateTemplate,
    deleteEntry: mockDeleteEntry,
    languages: { siteLanguages: mockSiteLanguages, pageTranslations: mockPageTranslations },
    createTranslation: mockCreateTranslation,
    templates: {
      listTemplates: mockListTemplates,
      renameTemplate: mockRenameTemplate,
      deleteTemplate: mockDeleteTemplate,
      setTemplateFolder: mockSetTemplateFolder,
    },
  };
  const flow = new CreateFlow(deps);
  document.addEventListener('editor:file-selected', ((e: CustomEvent) => { selectedEvents.push(e.detail); }) as EventListener);

  const folderNavigateEvents: Harness['folderNavigateEvents'] = [];
  document.addEventListener('editor:folder-navigate', ((e: CustomEvent) => {
    folderNavigateEvents.push(e.detail);
  }) as EventListener);

  return {
    flow,
    tree,
    deps,
    setCurrentPath: (p) => { currentPath = p; },
    statusEl: () => document.getElementById('moss-create-status')!,
    headingEl: () => document.getElementById('editor-heading')!,
    selectedEvents,
    folderNavigateEvents,
  };
}

/** Create + activate a session for posts/untitled.md in one step. */
async function createAndOpen(h: Harness): Promise<void> {
  await h.flow.newPage();
  const rel = h.selectedEvents[0].filePath;
  h.setCurrentPath(rel);
  h.flow.onFileLoaded(rel);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockListDirectory.mockResolvedValue({ status: 'ok', data: [] });
  mockCreateFile.mockImplementation(async (_dir: string, name: string) => (
    { status: 'ok', data: `${ROOT}/posts/${name}` }
  ));
  mockDeleteEntry.mockResolvedValue({ status: 'ok', data: null });
  mockInstantiateTemplate.mockImplementation(async (_id: string, _dir: string, name: string) => (
    { status: 'ok', data: `${ROOT}/docs/${name}` }
  ));
  mockListTemplates.mockResolvedValue({ status: 'ok', data: [] });
  mockRenameTemplate.mockResolvedValue({ status: 'ok', data: null });
  mockDeleteTemplate.mockResolvedValue({ status: 'ok', data: null });
  mockSetTemplateFolder.mockResolvedValue({ status: 'ok', data: null });
  mockSiteLanguages.mockResolvedValue({ status: 'ok', data: { default_lang: 'en', trees: [] } });
  mockPageTranslations.mockResolvedValue({ status: 'ok', data: [] });
  mockCreateTranslation.mockImplementation(async (sourceAbs: string, lang: string) => (
    { status: 'ok', data: `${ROOT}/${lang}/${sourceAbs.slice(ROOT.length + 1)}` }
  ));
});

describe('CreateFlow.newPage', () => {
  test('creates untitled.md in the browsed folder and opens it as a new file', async () => {
    const h = makeHarness();
    await h.flow.newPage();

    expect(mockCreateFile).toHaveBeenCalledWith(`${ROOT}/posts`, 'untitled.md');
    expect(h.tree.refreshAndRender).toHaveBeenCalled();
    expect(h.tree.revealNewFile).toHaveBeenCalledWith('posts/untitled.md');
    expect(h.selectedEvents).toEqual([{ filePath: 'posts/untitled.md', isNewFile: true }]);
  });

  test('a page template seeds the same untitled session, into the menu\'s folder rather than the anchor', async () => {
    const h = makeHarness();
    await h.flow.newPage({ dir: 'docs', templateId: 'tpl-1' });

    expect(mockCreateFile).not.toHaveBeenCalled();
    expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-1', `${ROOT}/docs`, 'untitled.md');
    expect(h.tree.revealNewFile).toHaveBeenCalledWith('docs/untitled.md');
    expect(h.selectedEvents).toEqual([{ filePath: 'docs/untitled.md', isNewFile: true }]);
    // The H1-commit rename session is live, same as a blank page.
    h.setCurrentPath('docs/untitled.md');
    h.flow.onFileLoaded('docs/untitled.md');
    expect(h.statusEl().hidden).toBe(false);
  });

  test('a file anchor creates in its PARENT folder', async () => {
    const h = makeHarness({ path: 'posts/2026-08-02-on-gardens.md', isDir: false });
    mockCreateFile.mockResolvedValueOnce({ status: 'ok', data: `${ROOT}/posts/untitled.md` });
    await h.flow.newPage();
    expect(mockCreateFile).toHaveBeenCalledWith(`${ROOT}/posts`, 'untitled.md');
  });

  test('on-disk collision auto-suffixes instead of blocking', async () => {
    const h = makeHarness();
    mockCreateFile
      .mockResolvedValueOnce({ status: 'error', error: "'untitled.md' already exists" })
      .mockResolvedValueOnce({ status: 'ok', data: `${ROOT}/posts/untitled 2.md` });
    await h.flow.newPage();
    expect(mockCreateFile).toHaveBeenNthCalledWith(2, `${ROOT}/posts`, 'untitled 2.md');
    expect(h.selectedEvents[0].filePath).toBe('posts/untitled 2.md');
  });

  test('listing-known siblings pre-seed the suffix', async () => {
    const h = makeHarness();
    mockListDirectory.mockResolvedValue({
      status: 'ok',
      data: [{ name: 'untitled.md' }, { name: 'Untitled 2.md' }],
    });
    await h.flow.newPage();
    expect(mockCreateFile).toHaveBeenCalledWith(`${ROOT}/posts`, 'untitled 3.md');
  });

  test('a non-collision create failure surfaces a toast and opens nothing', async () => {
    const h = makeHarness();
    mockCreateFile.mockResolvedValue({ status: 'error', error: 'disk full' });
    await h.flow.newPage();
    expect(mockShowToast).toHaveBeenCalled();
    expect(h.selectedEvents).toEqual([]);
  });
});

describe('session activation', () => {
  test('opening the created file shows the status line and heading placeholder', async () => {
    const h = makeHarness();
    await createAndOpen(h);

    expect(h.statusEl().hidden).toBe(false);
    expect(h.statusEl().textContent).toContain('posts');
    expect(h.statusEl().textContent).toContain('untitled.md');
    expect(h.statusEl().textContent).toContain('Undo');
    expect(h.headingEl().dataset.createPlaceholder).toBeTruthy();
    expect(h.deps.setHeadingSaved).toHaveBeenCalledWith('');
    expect(h.flow.isActiveForCurrent()).toBe(true);
  });

  test('the heading takes the new-file focus for a session file', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    expect(h.flow.consumeHeadingFocus('posts/untitled.md')).toBe(true);
    expect(document.activeElement).toBe(h.headingEl());
  });

  test('opening a DIFFERENT file ends the session', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    h.setCurrentPath('other.md');
    h.headingEl().textContent = 'Other page'; // the next loadFile repaints the heading
    h.flow.onFileLoaded('other.md');
    expect(h.statusEl().hidden).toBe(true);
    expect(h.headingEl().dataset.createPlaceholder).toBeUndefined();
    expect(h.flow.isActiveForCurrent()).toBe(false);
  });

  test('the Untitled ghost survives session end while the heading is still empty', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    h.flow.onBodyEdited(); // ends the session with the heading untouched (empty)
    expect(h.headingEl().dataset.createPlaceholder).toBeTruthy();
  });
});

describe('rename-follows-title', () => {
  test('committing the title renames through the tree machinery (verbatim, no date convention)', async () => {
    const h = makeHarness();
    await createAndOpen(h);

    const result = await h.flow.commitTitle('Winter light');
    expect(result.ok).toBe(true);
    expect(h.deps.registerPendingRename).toHaveBeenCalledWith('posts/untitled.md', 'posts/Winter light.md');
    expect(h.deps.renameEntry).toHaveBeenCalledWith(
      `${ROOT}/posts/untitled.md`,
      `${ROOT}/posts/Winter light.md`,
    );
    // No slug drift → the filename carries the title; no frontmatter write.
    expect(h.deps.setTitleField).not.toHaveBeenCalled();
    expect(h.statusEl().textContent).toContain('Winter light.md');
  });

  test('with the date convention the rename is date+slug AND title: is written', async () => {
    const h = makeHarness();
    mockListDirectory.mockResolvedValue({
      status: 'ok',
      data: [{ name: '2026-08-01-a.md' }, { name: '2026-08-02-b.md' }],
    });
    await createAndOpen(h);

    const today = todayIso();
    await h.flow.commitTitle('Winter light');
    expect(h.deps.renameEntry).toHaveBeenCalledWith(
      `${ROOT}/posts/untitled.md`,
      `${ROOT}/posts/${today}-winter-light.md`,
    );
    // The slugged filename no longer carries the author's words — title: does.
    expect(h.deps.setTitleField).toHaveBeenCalledWith('Winter light');
    expect(h.statusEl().textContent).toContain(`${today}-winter-light.md`);
  });

  test('the preview is pointed at the renamed page, which no build may have seen under its first name', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    await h.flow.commitTitle('Winter light');
    expect(h.deps.showInPreview).toHaveBeenCalledWith('posts/Winter light.md');
  });

  test('retitling re-renames while still title-only', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    await h.flow.commitTitle('Winter light');
    await h.flow.commitTitle('Spring rain');
    expect(h.deps.renameEntry).toHaveBeenLastCalledWith(
      `${ROOT}/posts/Winter light.md`,
      `${ROOT}/posts/Spring rain.md`,
    );
  });

  test('an empty title keeps the untitled name (never an error)', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    const result = await h.flow.commitTitle('   ');
    expect(result.ok).toBe(true);
    expect(h.deps.renameEntry).not.toHaveBeenCalled();
  });

  test('a rename failure propagates its error to the heading commit', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    h.deps.renameEntry.mockResolvedValueOnce({ status: 'error', error: 'target exists' });
    const result = await h.flow.commitTitle('Winter light');
    expect(result).toEqual({ ok: false, error: 'target exists' });
    expect(h.deps.showInPreview).not.toHaveBeenCalled();
  });

  test('the first body edit ends the session — the filename stops following', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    h.flow.onBodyEdited();
    expect(h.statusEl().hidden).toBe(true);
    expect(h.flow.isActiveForCurrent()).toBe(false);
    // Later commits are the caller's (routeHeadingCommit) business.
    await h.flow.commitTitle('Winter light');
    expect(h.deps.renameEntry).not.toHaveBeenCalled();
  });

  test('a body edit in a DIFFERENT file does not end a pending session', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    h.setCurrentPath('other.md'); // simulate a stale notification path
    h.flow.onBodyEdited();
    h.setCurrentPath('posts/untitled.md');
    expect(h.flow.isActiveForCurrent()).toBe(true);
  });
});

describe('undo', () => {
  test('returns to the previously open file and trashes the created page', async () => {
    const h = makeHarness();
    h.setCurrentPath('posts/2026-08-02-on-gardens.md');
    await h.flow.newPage();
    h.setCurrentPath('posts/untitled.md');
    h.flow.onFileLoaded('posts/untitled.md');

    await h.flow.undo();

    expect(h.selectedEvents.map((e) => e.filePath)).toEqual([
      'posts/untitled.md',
      'posts/2026-08-02-on-gardens.md',
    ]);
    expect(mockDeleteEntry).toHaveBeenCalledWith(`${ROOT}/posts/untitled.md`);
    expect(h.statusEl().hidden).toBe(true);
    expect(h.flow.isActiveForCurrent()).toBe(false);
  });

  test('undo after a rename deletes the file at its RENAMED path (EntryId identity)', async () => {
    const h = makeHarness();
    h.setCurrentPath('home.md');
    await h.flow.newPage();
    h.setCurrentPath('posts/untitled.md');
    h.flow.onFileLoaded('posts/untitled.md');
    await h.flow.commitTitle('Winter light');

    await h.flow.undo();
    expect(mockDeleteEntry).toHaveBeenCalledWith(`${ROOT}/posts/Winter light.md`);
  });

  test('with no previous file, undo lands on the destination folder surface', async () => {
    const h = makeHarness();
    await createAndOpen(h);
    await h.flow.undo();
    expect(mockDeleteEntry).toHaveBeenCalledWith(`${ROOT}/posts/untitled.md`);
    expect(h.folderNavigateEvents).toEqual([
      { folderPath: `${ROOT}/posts`, folderName: 'posts' },
    ]);
  });

  test('clicking the status line Undo button runs the undo', async () => {
    const h = makeHarness();
    h.setCurrentPath('home.md');
    await h.flow.newPage();
    h.setCurrentPath('posts/untitled.md');
    h.flow.onFileLoaded('posts/untitled.md');

    (h.statusEl().querySelector('button.moss-link') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(mockDeleteEntry).toHaveBeenCalled());
  });
});

// ── The ＋▾ menu ──
// Composition only: which rows exist and what each dispatches. The primitive's
// level mechanics are ctx-menu.test.ts's; geometry is the render gate's.

const ESSAY = { id: 'tpl-essay', name: 'Essay', kind: 'page' as const, created_at: '2026-09-05', folder: null, langs: [] as string[] };
const SECTION = { id: 'tpl-section', name: 'Section', kind: 'folder' as const, created_at: '2026-09-05', folder: null, langs: [] as string[] };

describe('the ＋▾ menu', () => {
  const rows = () => Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu .ctx-item'));
  const rowLabels = () => rows().map((r) => r.querySelector('.ctx-label')?.textContent);
  const notes = () => Array.from(document.querySelectorAll('.ctx-menu .ctx-hint'), (n) => n.textContent);
  const row = (label: string): HTMLElement => {
    const el = rows().find((r) => r.querySelector('.ctx-label')?.textContent === label);
    if (!el) throw new Error(`no row "${label}" among ${rowLabels().join(' | ')}`);
    return el;
  };
  async function openMenu(h: Harness, templates: Array<typeof ESSAY | typeof SECTION>): Promise<void> {
    mockListTemplates.mockResolvedValue({ status: 'ok', data: templates });
    h.flow.wire();
    document.getElementById('editor-create-caret')!.click();
    await vi.waitFor(() => expect(document.querySelector('.ctx-menu')).not.toBeNull());
  }

  test('lists create rows, the template group under its note, Manage Templates…, and the destination; no Import from URL', async () => {
    const h = makeHarness();
    await openMenu(h, [ESSAY, SECTION]);
    expect(rowLabels()).toEqual(['New Page', 'New Folder', 'Essay', 'Section (folder)', 'Manage Templates…']);
    expect(notes()).toEqual(['From template', 'Creates in posts']);
    expect(row('Manage Templates…').getAttribute('aria-haspopup')).toBe('menu');
    h.flow.destroy();
  });

  test('a page template row seeds the untitled-first page into the menu\'s folder', async () => {
    const h = makeHarness();
    await openMenu(h, [ESSAY]);
    row('Essay').click();
    await vi.waitFor(() => expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-essay', `${ROOT}/posts`, 'untitled.md'));
    expect(document.querySelector('.ctx-menu')).toBeNull();
    h.flow.destroy();
  });

  test('a folder template row starts the inline-name folder create with the template', async () => {
    const h = makeHarness();
    await openMenu(h, [SECTION]);
    row('Section (folder)').click();
    expect(h.tree.startCreate).toHaveBeenCalledWith('posts', 'folder', { templateId: 'tpl-section' });
    expect(mockInstantiateTemplate).not.toHaveBeenCalled();
    h.flow.destroy();
  });

  test('an empty store adds no template group and nothing to manage', async () => {
    const h = makeHarness();
    await openMenu(h, []);
    expect(rowLabels()).toEqual(['New Page', 'New Folder']);
    expect(notes()).toEqual(['Creates in posts']);
    h.flow.destroy();
  });

  test('a failed listing opens the empty shape and toasts', async () => {
    const h = makeHarness();
    mockListTemplates.mockResolvedValue({ status: 'error', error: 'boom' });
    h.flow.wire();
    document.getElementById('editor-create-caret')!.click();
    await vi.waitFor(() => expect(document.querySelector('.ctx-menu')).not.toBeNull());
    expect(rowLabels()).toEqual(['New Page', 'New Folder']);
    expect(mockShowToast).toHaveBeenCalledWith(expect.objectContaining({ what: expect.stringContaining('boom') }));
    h.flow.destroy();
  });

  test('a loose document\'s folder has no site, so no template read, rows or toast', async () => {
    const h = makeHarness();
    mockListTemplates.mockResolvedValue({ status: 'error', error: 'No project open' });
    (h.deps as { templates: unknown }).templates = null;
    h.flow.wire();
    document.getElementById('editor-create-caret')!.click();
    await vi.waitFor(() => expect(document.querySelector('.ctx-menu')).not.toBeNull());
    expect(rowLabels()).toEqual(['New Page', 'New Folder']);
    expect(mockListTemplates).not.toHaveBeenCalled();
    expect(mockShowToast).not.toHaveBeenCalled();
    h.flow.destroy();
  });

  test('Manage Templates… → template → Rename closes the menu and opens the popover prefilled', async () => {
    const h = makeHarness();
    await openMenu(h, [ESSAY]);
    row('Manage Templates…').click();
    expect(rowLabels()).toEqual(['Manage Templates…', 'Essay']);
    row('Essay').click();
    expect(rowLabels()).toEqual(['Essay', 'Rename', 'Folder…', 'Delete']);
    expect(row('Delete').classList.contains('danger')).toBe(true);
    row('Rename').click();
    expect(document.querySelector('.ctx-menu[role="menu"]')).toBeNull(); // the popover borrows the class
    expect(document.querySelector<HTMLInputElement>('.input-popover input.pop-input')?.value).toBe('Essay');
    h.flow.destroy();
  });

  test('Manage Templates… → template → Delete asks, then deletes', async () => {
    const h = makeHarness();
    mockMossConfirm.mockResolvedValueOnce(true);
    await openMenu(h, [ESSAY]);
    row('Manage Templates…').click();
    row('Essay').click();
    row('Delete').click();
    await vi.waitFor(() => expect(mockDeleteTemplate).toHaveBeenCalledWith('tpl-essay'));
    h.flow.destroy();
  });
});

// The create latch stays held through the actual open, rather than an event dispatch.
describe('revealAndOpenNewEntry', () => {
  test('refreshes, reveals and awaits the editor open in order', async () => {
    const order: string[] = [];
    let settle!: () => void;
    const tree = { refreshAndRender: async () => { order.push('refresh'); }, revealNewFile: () => { order.push('reveal'); } };
    const pending = revealAndOpenNewEntry(tree, 'proj.md', async () => {
      order.push('open');
      await new Promise<void>((resolve) => { settle = resolve; });
      order.push('opened');
    });
    await vi.waitFor(() => expect(order).toEqual(['refresh', 'reveal', 'open']));
    settle();
    await pending;
    expect(order).toEqual(['refresh', 'reveal', 'open', 'opened']);
  });
});

// ── Folder templates, the create menu's destination, and a page's other language ──

const PRODUCTION = { id: 'tpl-prod', name: 'Production', kind: 'page' as const, created_at: '2026-10-18', folder: 'work/', langs: [] as string[] };
const TWO_WORK = { id: 'tpl-two', name: 'Essay', kind: 'page' as const, created_at: '2026-10-18', folder: 'work/', langs: [] as string[] };
const BILINGUAL = { default_lang: 'en', trees: ['zh-hans'] };

describe('folder templates', () => {
  const rows = () => Array.from(document.querySelectorAll<HTMLElement>('.ctx-menu .ctx-item'));
  const rowLabels = () => rows().map((r) => r.querySelector('.ctx-label')?.textContent);
  const row = (label: string): HTMLElement => {
    const el = rows().find((r) => r.querySelector('.ctx-label')?.textContent === label);
    if (!el) throw new Error(`no row "${label}" among ${rowLabels().join(' | ')}`);
    return el;
  };
  async function openMenu(h: Harness, templates: Array<{ id: string; name: string; kind: 'page' | 'folder'; created_at: string; folder: string | null; langs: string[] }>): Promise<void> {
    mockListTemplates.mockResolvedValue({ status: 'ok', data: templates });
    h.flow.wire();
    document.getElementById('editor-create-caret')!.click();
    await vi.waitFor(() => expect(document.querySelector('.ctx-menu')).not.toBeNull());
  }

  test('template row creates in its folder with About selected', async () => {
    const h = makeHarness({ path: 'about', isDir: true });
    mockInstantiateTemplate.mockResolvedValueOnce({ status: 'ok', data: `${ROOT}/work/untitled.md` });
    await openMenu(h, [PRODUCTION]);
    row('Production — in work').click();
    await vi.waitFor(() => expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-prod', `${ROOT}/work`, 'untitled.md'));
    expect(h.tree.revealNewFile).toHaveBeenCalledWith('work/untitled.md');
    h.flow.destroy();
  });

  test('a folder template in a language tree lands in the same folder of that tree', async () => {
    mockSiteLanguages.mockResolvedValue({ status: 'ok', data: BILINGUAL });
    const h = makeHarness({ path: 'zh-hans/about', isDir: true });
    await openMenu(h, [PRODUCTION]);
    row('Production — in work').click();
    await vi.waitFor(() => expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-prod', `${ROOT}/zh-hans/work`, 'untitled.md'));
    h.flow.destroy();
  });

  test('a template whose folder is gone creates nothing and says why', async () => {
    const h = makeHarness({ path: 'about', isDir: true });
    await openMenu(h, [PRODUCTION]);
    mockListDirectory.mockResolvedValueOnce({ status: 'error', error: 'no such folder' });
    row('Production — in work').click();
    await vi.waitFor(() => expect(mockShowToast).toHaveBeenCalledWith(expect.objectContaining({
      what: 'The work/ folder this template belongs to is gone. Choose a folder or delete the template.',
    })));
    expect(mockInstantiateTemplate).not.toHaveBeenCalled();
    expect(mockCreateFile).not.toHaveBeenCalled();
    h.flow.destroy();
  });

  test('cmd-N in a folder with one template uses it, and New Page reads Blank page there', async () => {
    const h = makeHarness({ path: 'work', isDir: true });
    mockListTemplates.mockResolvedValue({ status: 'ok', data: [PRODUCTION] });
    h.flow.wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', metaKey: true }));
    await vi.waitFor(() => expect(mockInstantiateTemplate).toHaveBeenCalledWith('tpl-prod', `${ROOT}/work`, 'untitled.md'));
    expect(mockCreateFile).not.toHaveBeenCalled();
    h.flow.destroy();

    const h2 = makeHarness({ path: 'work', isDir: true });
    await openMenu(h2, [PRODUCTION]);
    expect(rowLabels().slice(0, 3)).toEqual(['Production — in work', 'Blank page', 'New Folder']);
    row('Blank page').click();
    await vi.waitFor(() => expect(mockCreateFile).toHaveBeenCalledWith(`${ROOT}/work`, 'untitled.md'));
    expect(mockInstantiateTemplate).toHaveBeenCalledTimes(1);
    h2.flow.destroy();
  });

  test('cmd-N with two templates in the folder stays blank and lists them first', async () => {
    const h = makeHarness({ path: 'work', isDir: true });
    mockListTemplates.mockResolvedValue({ status: 'ok', data: [PRODUCTION, TWO_WORK] });
    h.flow.wire();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', metaKey: true }));
    await vi.waitFor(() => expect(mockCreateFile).toHaveBeenCalledWith(`${ROOT}/work`, 'untitled.md'));
    expect(mockInstantiateTemplate).not.toHaveBeenCalled();
    h.flow.destroy();

    const h2 = makeHarness({ path: 'work', isDir: true });
    await openMenu(h2, [PRODUCTION, TWO_WORK]);
    expect(rowLabels().slice(0, 3)).toEqual(['Production — in work', 'Essay — in work', 'New Page']);
    h2.flow.destroy();
  });

  test('translation rows list only the languages the page lacks', async () => {
    mockSiteLanguages.mockResolvedValue({ status: 'ok', data: { default_lang: 'en', trees: ['zh-hans', 'zh-hant'] } });
    mockPageTranslations.mockResolvedValue({ status: 'ok', data: [
      { lang: 'zh-hans', rel: 'zh-hans/work/纸.md', exists: true, folder_missing: false },
      { lang: 'zh-hant', rel: 'zh-hant/work/paper.md', exists: false, folder_missing: false },
    ] });
    const h = makeHarness({ path: 'work', isDir: true });
    h.setCurrentPath('work/paper.md');
    await openMenu(h, []);
    expect(rowLabels()).toContain('繁體中文 version of this page');
    expect(rowLabels()).not.toContain('简体中文 version of this page');
    h.flow.destroy();
  });

  test('no translation rows on a one-language site', async () => {
    const h = makeHarness({ path: 'work', isDir: true });
    h.setCurrentPath('work/paper.md');
    await openMenu(h, []);
    expect(rowLabels().some((l) => l?.includes('version of this'))).toBe(false);
    h.flow.destroy();
  });

  test('a folder home page missing in the other tree offers the folder version', async () => {
    mockSiteLanguages.mockResolvedValue({ status: 'ok', data: BILINGUAL });
    mockPageTranslations.mockResolvedValue({ status: 'ok', data: [{ lang: 'zh-hans', rel: 'zh-hans/work/work.md', exists: false, folder_missing: true }] });
    const h = makeHarness({ path: 'work', isDir: true });
    h.setCurrentPath('work/work.md');
    await openMenu(h, []);
    expect(rowLabels()).toContain('简体中文 version of this folder');
    h.flow.destroy();
  });

  test('a translation row creates the mirrored page, and its title is not a rename', async () => {
    mockSiteLanguages.mockResolvedValue({ status: 'ok', data: BILINGUAL });
    mockPageTranslations.mockResolvedValue({ status: 'ok', data: [{ lang: 'zh-hans', rel: 'zh-hans/work/paper.md', exists: false, folder_missing: false }] });
    const h = makeHarness({ path: 'work', isDir: true });
    h.setCurrentPath('work/paper.md');
    await openMenu(h, []);
    row('简体中文 version of this page').click();
    await vi.waitFor(() => expect(mockCreateTranslation).toHaveBeenCalledWith(`${ROOT}/work/paper.md`, 'zh-hans'));
    await vi.waitFor(() => expect(h.selectedEvents).toHaveLength(1));
    expect(h.selectedEvents[0].filePath).toBe('zh-hans/work/paper.md');
    h.setCurrentPath('zh-hans/work/paper.md');
    h.flow.onFileLoaded('zh-hans/work/paper.md');
    const result = await h.flow.commitTitle('Winter light');
    expect(result.ok).toBe(true);
    expect(h.deps.renameEntry).not.toHaveBeenCalled();
    expect(h.deps.setTitleField).toHaveBeenCalledWith('Winter light');
    h.flow.destroy();
  });

  test('Manage Templates… offers Folder… between Rename and Delete', async () => {
    const h = makeHarness({ path: 'work', isDir: true });
    await openMenu(h, [PRODUCTION]);
    row('Manage Templates…').click();
    row('Production — in work').click();
    expect(rowLabels()).toEqual(['Production — in work', 'Rename', 'Folder…', 'Clear folder', 'Delete']);
    h.flow.destroy();
  });
});
