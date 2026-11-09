/**
 * The template prompts (save-as, rename, delete) driven directly through the
 * real input-popover DOM and the stubbed moss-alert confirm, asserting what
 * reaches the injected commands. The menu that leads to them is the ＋▾
 * menu's — its composition is create-flow.test.ts's, the levels ctx-menu.test.ts's.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const { mockMossConfirm, mockShowToast } = vi.hoisted(() => ({ mockMossConfirm: vi.fn(), mockShowToast: vi.fn() }));
vi.mock("../../shared/ui/moss-alert", () => ({
  mossConfirm: mockMossConfirm,
  mossAlert: vi.fn(),
}));
vi.mock("../../shared/ui/toast/toast-manager", () => ({
  showToast: mockShowToast,
  showAck: vi.fn(),
}));

import {
  deleteTemplate,
  promptRenameTemplate,
  promptSaveAsTemplate,
  clearTemplateFolder,
  promptTemplateFolder,
  templateRowLabel,
  type TemplateCommands,
  type TemplateMeta,
} from '../template-menu';
import { closeActiveInputPopover } from "../../shared/ui/input-popover";
import type { LanguageSource } from '../languages';

const PAGE: TemplateMeta = { id: 'aaa', name: 'Blog post', kind: 'page', created_at: '2026-09-03', folder: null, langs: [] };
const FOLDER: TemplateMeta = { id: 'bbb', name: 'Section', kind: 'folder', created_at: '2026-09-03', folder: null, langs: [] };

function makeCommands(): TemplateCommands & LanguageSource {
  return {
    createTemplate: vi.fn(async () => ({ status: 'ok' as const, data: 'new-id' })),
    listTemplates: vi.fn(async () => ({ status: 'ok' as const, data: [] })),
    renameTemplate: vi.fn(async () => ({ status: 'ok' as const, data: null })),
    setTemplateFolder: vi.fn(async () => ({ status: 'ok' as const, data: null })),
    deleteTemplate: vi.fn(async () => ({ status: 'ok' as const, data: null })),
    siteLanguages: vi.fn(async () => ({ status: 'ok' as const, data: { default_lang: 'en', trees: [] } })),
    listDirectory: vi.fn(async () => ({ status: 'ok' as const, data: [] })),
  };
}

const at = () => new MouseEvent('click', { clientX: 20, clientY: 30 });

function popoverInput(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>('.input-popover input.pop-input');
  if (!el) throw new Error('popover input not mounted');
  return el;
}

const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  closeActiveInputPopover();
  document.body.innerHTML = '';
});

describe('promptRenameTemplate', () => {
  it('opens the popover prefilled and submits renameTemplate; a failure toasts', async () => {
    const commands = makeCommands();
    promptRenameTemplate(commands, PAGE, at());
    expect(popoverInput().value).toBe('Blog post');
    popoverInput().value = 'Essay';
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(commands.renameTemplate).toHaveBeenCalledWith('aaa', 'Essay');
    expect(mockShowToast).not.toHaveBeenCalled();

    vi.mocked(commands.renameTemplate).mockResolvedValueOnce({ status: 'error', error: 'boom' });
    promptRenameTemplate(commands, PAGE, at());
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(mockShowToast).toHaveBeenCalledWith(expect.objectContaining({ what: expect.stringContaining('boom') }));
  });
});

describe('deleteTemplate', () => {
  it('asks first and only deletes on confirm', async () => {
    const commands = makeCommands();
    mockMossConfirm.mockResolvedValueOnce(false);
    await deleteTemplate(commands, PAGE);
    expect(mockMossConfirm).toHaveBeenCalledWith(
      'Delete template “Blog post”?',
      expect.objectContaining({ confirmLabel: 'Delete', destructive: true }),
    );
    expect(commands.deleteTemplate).not.toHaveBeenCalled();

    mockMossConfirm.mockResolvedValueOnce(true);
    await deleteTemplate(commands, PAGE);
    expect(commands.deleteTemplate).toHaveBeenCalledWith('aaa');
  });
});

describe('promptSaveAsTemplate', () => {
  it('names the template in the popover and calls createTemplate with the source path', async () => {
    const commands = makeCommands();
    promptSaveAsTemplate(commands, { absPath: '/site/news/news.md', projectRoot: '/site', isHome: true }, at());
    popoverInput().value = 'Section shape';
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(commands.createTemplate).toHaveBeenCalledWith('/site/news/news.md', 'Section shape', true, null);
  });

  it('offers to save a translated page with the template, and saves it when the author agrees', async () => {
    const commands = makeCommands();
    vi.mocked(commands.siteLanguages).mockResolvedValue({ status: 'ok', data: { default_lang: 'en', trees: ['zh-hans'] } });
    vi.mocked(commands.listDirectory).mockImplementation(async (abs: string) => (
      { status: 'ok', data: abs === '/site/zh-hans/work' ? [{ name: 'paper.md' }] : [] }
    ));
    mockMossConfirm.mockResolvedValueOnce(true);
    promptSaveAsTemplate(commands, { absPath: '/site/work/paper.md', projectRoot: '/site', isHome: false }, at());
    popoverInput().value = 'Essay';
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(mockMossConfirm).toHaveBeenCalledWith(
      'Also save the 简体中文 page as its template?',
      expect.objectContaining({ confirmLabel: 'Save both' }),
    );
    expect(commands.createTemplate).toHaveBeenCalledWith('/site/work/paper.md', 'Essay', false, [['zh-hans', '/site/zh-hans/work/paper.md']]);
  });

  it('saves only the page when the author declines the translations', async () => {
    const commands = makeCommands();
    vi.mocked(commands.siteLanguages).mockResolvedValue({ status: 'ok', data: { default_lang: 'en', trees: ['zh-hans'] } });
    vi.mocked(commands.listDirectory).mockImplementation(async (abs: string) => (
      { status: 'ok', data: abs === '/site/zh-hans/work' ? [{ name: 'paper.md' }] : [] }
    ));
    mockMossConfirm.mockResolvedValueOnce(false);
    promptSaveAsTemplate(commands, { absPath: '/site/work/paper.md', projectRoot: '/site', isHome: false }, at());
    popoverInput().value = 'Essay';
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(commands.createTemplate).toHaveBeenCalledWith('/site/work/paper.md', 'Essay', false, null);
  });
});

describe('promptTemplateFolder', () => {
  it('prefills the folder and submits it trimmed', async () => {
    const commands = makeCommands();
    promptTemplateFolder(commands, { ...PAGE, folder: 'work/' }, at());
    expect(popoverInput().value).toBe('work/');
    popoverInput().value = '  work  ';
    popoverInput().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush();
    expect(commands.setTemplateFolder).toHaveBeenCalledWith('aaa', 'work');
  });

  it('clears the folder with null', async () => {
    const commands = makeCommands();
    clearTemplateFolder(commands, { ...PAGE, folder: 'work/' });
    await flush();
    expect(commands.setTemplateFolder).toHaveBeenCalledWith('aaa', null);
  });
});

describe('templateRowLabel', () => {
  it('names the folder a template goes to, and leaves a folderless template alone', () => {
    expect(templateRowLabel({ ...PAGE, folder: 'work/' })).toBe('Blog post — in work');
    expect(templateRowLabel(PAGE)).toBe('Blog post');
  });
});
