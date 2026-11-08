// The open document when its file leaves outside moss: the text is never lost,
// whether the author stays, moves on, saves it back, or the file turns out to
// have been renamed.
import { describe, test, expect, vi, afterEach } from 'vitest';
import { createOutsideFile, type DocumentText, type OutsideFileHost } from '../outside-file';

// The toasts as data: what was said, and the actions offered (the last toast wins).
type Action = { label: string; onClick?: () => void };
const live = new Map<string, { what: string; actions: Action[] }>();
const said: string[] = [];
vi.mock("../../../shared/ui/toast/toast-manager", () => ({
  showToast: (advisory: { what: string }, options: { id?: string; actions?: Action[] } = {}) => {
    said.push(advisory.what);
    if (options.id) live.set(options.id, { what: advisory.what, actions: options.actions ?? [] });
  },
  removeToastById: (id: string) => { live.delete(id); },
}));

const typed: DocumentText = { frontmatter: { title: 'Note' }, body: 'typed before it left' };

function host(over: Partial<OutsideFileHost> = {}) {
  const h = {
    hasUnsavedText: vi.fn(() => true),
    cancelPendingSave: vi.fn(),
    liveText: vi.fn(() => typed),
    createFile: vi.fn(async () => ({ ok: true as const })),
    writeText: vi.fn(async () => true),
    reopen: vi.fn(async () => {}),
    markClean: vi.fn(),
    showFolderOf: vi.fn(async () => {}),
    ...over,
  };
  return h;
}

/** Every action on the held-text toasts still up. */
const toastActions = () => [...live.values()].flatMap((t) => t.actions);
const press = async (label: string) => {
  toastActions().find((a) => a.label === label)!.onClick!();
  await new Promise((r) => setTimeout(r, 0));
};
const toastTexts = () => said;

afterEach(() => { live.clear(); said.length = 0; });

describe('a deleted document with unsaved text', () => {
  test('stays open; Save it back writes the live text, then reopens the file', async () => {
    const h = host();
    const f = createOutsideFile(h);
    expect(f.deletedOutside('note.md')).toBe(true);
    expect(f.holdsDocument()).toBe(true);
    await press('Save it back');
    expect(h.createFile).toHaveBeenCalledWith('note.md');
    expect(h.writeText).toHaveBeenCalledWith('note.md', typed, false);
    expect(h.reopen).toHaveBeenCalledWith('note.md');
    expect(f.holdsDocument()).toBe(false);
  });

  test('a refused save-back keeps the text held and the choice offered', async () => {
    const h = host({ createFile: vi.fn(async () => ({ ok: false as const, error: 'a file with that name already exists' })) });
    const f = createOutsideFile(h);
    f.deletedOutside('note.md');
    await press('Save it back');
    expect(h.writeText).not.toHaveBeenCalled();
    expect(f.holdsDocument()).toBe(true);
    expect(toastTexts().some((t) => t?.includes('already exists'))).toBe(true);
    expect(toastActions().some((a) => a.label.includes('Save it back'))).toBe(true);
  });

  test('after the page was re-created but the text could not be written, a retry writes without creating again', async () => {
    const writes = [false, true];
    const h = host({ writeText: vi.fn(async () => writes.shift()!) });
    const f = createOutsideFile(h);
    f.deletedOutside('note.md');
    await press('Save it back');
    expect(f.holdsDocument()).toBe(true);
    await press('Save it back');
    expect(h.createFile).toHaveBeenCalledTimes(1);
    expect(h.writeText).toHaveBeenCalledTimes(2);
    expect(f.holdsDocument()).toBe(false);
  });

  test('Discard lets it close to its folder, writing nothing', async () => {
    const h = host();
    const f = createOutsideFile(h);
    f.deletedOutside('note.md');
    await press('Discard');
    expect(h.writeText).not.toHaveBeenCalled();
    expect(h.showFolderOf).toHaveBeenCalledWith('note.md');
    expect(f.holdsDocument()).toBe(false);
  });

  test('with nothing unsaved it closes, and only says why', () => {
    const f = createOutsideFile(host({ hasUnsavedText: vi.fn(() => false) }));
    expect(f.deletedOutside('note.md')).toBe(false);
    expect(toastActions()).toHaveLength(0);
  });
});

describe('moving on from a document whose file left', () => {
  test('while a save waits on a gone file, the text is kept and offered back', async () => {
    const h = host();
    const f = createOutsideFile(h);
    f.noteGone('note.md');
    f.leaving();
    expect(h.liveText).toHaveBeenCalled();
    await press('Save it back');
    expect(h.writeText).toHaveBeenCalledWith('note.md', typed, false);
    expect(h.reopen).not.toHaveBeenCalled();
  });

  test('a held document keeps the text it had when the author moved on', async () => {
    let live = typed;
    const h = host({ liveText: vi.fn(() => live) });
    const f = createOutsideFile(h);
    f.deletedOutside('note.md');
    f.leaving();
    live = { frontmatter: {}, body: 'the next file' };
    await press('Save it back');
    expect(h.writeText).toHaveBeenCalledWith('note.md', typed, false);
  });

  test('a rename seen after the author moved on re-points the offer, writing nothing on its own', async () => {
    const h = host();
    const f = createOutsideFile(h);
    f.noteGone('note.md');
    f.leaving();
    expect(f.followRenames([['note.md', 'field-notes.md']])).toBe(false);
    expect(h.writeText).not.toHaveBeenCalled();
    expect(toastTexts()).toContain('"note.md" was renamed to "field-notes.md" outside moss. Your unsaved text is still here.');
    await press('Save into field-notes.md');
    expect(h.createFile).not.toHaveBeenCalled();
    expect(h.writeText).toHaveBeenCalledWith('field-notes.md', typed, true);
  });

  test('a second document whose file leaves does not cost the first its text', async () => {
    let live2 = typed;
    const h = host({ liveText: vi.fn(() => live2) });
    const f = createOutsideFile(h);
    f.noteGone('a.md');
    f.leaving();
    live2 = { frontmatter: {}, body: 'typed in b' };
    f.deletedOutside('b.md');
    await press('Save it back');
    expect(h.writeText).toHaveBeenCalledWith('a.md', typed, false);
    await press('Save it back');
    expect(h.writeText).toHaveBeenCalledWith('b.md', { frontmatter: {}, body: 'typed in b' }, false);
  });

  test('a second gone document is kept too when the author moves on from it', async () => {
    let live2 = typed;
    const h = host({ liveText: vi.fn(() => live2) });
    const f = createOutsideFile(h);
    f.noteGone('a.md');
    f.leaving();
    live2 = { frontmatter: {}, body: 'typed in b' };
    f.noteGone('b.md');
    expect(f.isGoneAt('b.md')).toBe(true);
    f.leaving();
    expect([...live.keys()].sort()).toEqual(['editor-held-document:a.md', 'editor-held-document:b.md']);
  });

  test('a rename while the document is still open resumes saving at the new path', async () => {
    const f = createOutsideFile(host());
    f.noteGone('note.md');
    expect(f.isGoneAt('note.md')).toBe(true);
    expect(f.followRenames([['note.md', 'field-notes.md']])).toBe(true);
    expect(f.isGoneAt('note.md')).toBe(false);
  });
});
