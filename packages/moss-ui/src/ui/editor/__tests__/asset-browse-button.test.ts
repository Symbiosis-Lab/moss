/**
 * The Browse button — "pick it from my disk" beside the asset search.
 *
 * What these prove is the flow around the dialog, not the dialog: which
 * extensions it is asked for, whether a pick is copied, and whether anything
 * happens on a cancel. The reference form itself is decided in Rust and
 * tested there (`asset_ref_for_path`).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../os-drag-in', () => ({ importIntoVault: vi.fn() }));

import { host } from "../../host";
import { importIntoVault } from '../os-drag-in';
import { browseButton, dialogFilters, pickAssetRef, type BrowseContext } from '../asset-picker/browse-button';

const ROOT = '/vault';
const ITEM = { insert: '/img/photo.jpg', label: 'photo.jpg', detail: '/img/photo.jpg', kind: 'asset' };

function ctx(over: Partial<BrowseContext> = {}): BrowseContext {
  return {
    pagePath: () => 'posts/hello.md',
    projectRoot: () => ROOT,
    kinds: () => ['Image'],
    ...over,
  };
}

const openAssetDialog = vi.fn<typeof host.pickFiles>();
const assetRefForPath = vi.fn<NonNullable<typeof host.assetRefForPath>>();
host.pickFiles = openAssetDialog;
host.assetRefForPath = assetRefForPath;

beforeEach(() => {
  vi.mocked(openAssetDialog).mockReset();
  vi.mocked(importIntoVault).mockReset().mockResolvedValue([{ dir: 'assets', finalName: 'photo.jpg' }]);
  vi.mocked(assetRefForPath).mockReset().mockResolvedValue(ITEM as never);
});

describe('dialogFilters', () => {
  it('offers only the kinds the field accepts', () => {
    const [images] = dialogFilters(['Image']);
    expect(images.extensions).toContain('png');
    expect(images.extensions).not.toContain('mp4');
  });

  it('offers every embeddable asset when the field names no kinds', () => {
    const [all] = dialogFilters(null);
    expect(all.extensions).toEqual(expect.arrayContaining(['png', 'mp4']));
  });

  it('never offers something moss cannot embed', () => {
    // Derived from the generated registry, so the dialog cannot drift from
    // what the build actually handles.
    const [all] = dialogFilters(null);
    expect(all.extensions).not.toContain('exe');
  });
});

describe('pickAssetRef', () => {
  it('does nothing at all on a cancel', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue([]);
    expect(await pickAssetRef(ctx())).toBeNull();
    expect(importIntoVault).not.toHaveBeenCalled();
    expect(assetRefForPath).not.toHaveBeenCalled();
  });

  it('does not copy a file that is already in the vault', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue([`${ROOT}/img/photo.jpg`]);
    const afterImport = vi.fn(async () => {});
    expect(await pickAssetRef(ctx({ afterImport }))).toEqual(ITEM);
    expect(importIntoVault).not.toHaveBeenCalled();
    expect(afterImport).not.toHaveBeenCalled();
    expect(assetRefForPath).toHaveBeenCalledWith(`${ROOT}/img/photo.jpg`, 'posts/hello.md');
  });

  it('imports a file from outside the vault and refreshes the tree', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue(['/Users/me/Downloads/photo.jpg']);
    const afterImport = vi.fn(async () => {});
    expect(await pickAssetRef(ctx({ afterImport }))).toEqual(ITEM);
    expect(importIntoVault).toHaveBeenCalledWith(['/Users/me/Downloads/photo.jpg'], 'posts/hello.md', ROOT);
    // Resolved against where it LANDED, not where it came from.
    expect(assetRefForPath).toHaveBeenCalledWith(`${ROOT}/assets/photo.jpg`, 'posts/hello.md');
    expect(afterImport).toHaveBeenCalled();
  });

  it('recognises an in-vault file the OS dialog spelled differently', async () => {
    // macOS hands back NFD for composed characters; moss's own paths are NFC.
    // A raw prefix compare calls this file OUTSIDE the vault it is plainly
    // inside, and imports a duplicate of something the author can already see.
    //
    // Accented Latin, not CJK: Han ideographs have no NFD decomposition, so a
    // `照片` example would pass whether or not the code normalizes. Hangul and
    // accented Latin are what actually differ — the guard below is here so
    // this test cannot quietly become a tautology again.
    const nfcRoot = '/Users/me/Caf\u00e9'; // Café, precomposed
    const nfdPick = '/Users/me/Cafe\u0301/cover.png'; // Café, decomposed
    expect(nfdPick.startsWith(`${nfcRoot}/`)).toBe(false); // the raw compare is wrong
    vi.mocked(openAssetDialog).mockResolvedValue([nfdPick]);
    await pickAssetRef(ctx({ projectRoot: () => nfcRoot }));
    expect(importIntoVault).not.toHaveBeenCalled();
  });

  it('gives up quietly when the import fails', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue(['/elsewhere/photo.jpg']);
    vi.mocked(importIntoVault).mockResolvedValue(null);
    expect(await pickAssetRef(ctx())).toBeNull();
    expect(assetRefForPath).not.toHaveBeenCalled();
  });

  it('reads the page path AFTER the dialog closes', async () => {
    // A rename or a folder switch while the dialog is up must not resolve the
    // file against the page that was open when the button was pressed.
    let page = 'posts/old.md';
    vi.mocked(openAssetDialog).mockImplementation(async () => {
      page = 'posts/new.md';
      return [`${ROOT}/img/photo.jpg`];
    });
    await pickAssetRef(ctx({ pagePath: () => page }));
    expect(assetRefForPath).toHaveBeenCalledWith(`${ROOT}/img/photo.jpg`, 'posts/new.md');
  });
});

describe('browseButton', () => {
  it('stands the search down before the dialog opens, and restores it after', async () => {
    // An armed search debounce would otherwise fire a full uncached
    // project-tree walk WHILE the dialog is up.
    vi.mocked(openAssetDialog).mockResolvedValue([]);
    const order: string[] = [];
    const btn = browseButton({
      variant: 'inset',
      ctx: ctx(),
      onOpen: () => order.push('open'),
      onPicked: () => order.push('picked'),
      onSettled: () => order.push('settled'),
    });
    btn.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(order).toEqual(['open', 'settled']));
  });

  it('settles on a cancel too, so nothing stays held down', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue([]);
    const onSettled = vi.fn();
    const btn = browseButton({ variant: 'inset', ctx: ctx(), onPicked: vi.fn(), onSettled });
    btn.click();
    await vi.waitFor(() => expect(onSettled).toHaveBeenCalledTimes(1));
  });

  it('picks once for the mousedown+click pair a mouse press produces', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue([`${ROOT}/img/photo.jpg`]);
    const onPicked = vi.fn();
    const btn = browseButton({ variant: 'inset', ctx: ctx(), onPicked });
    btn.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }));
    btn.click();
    await vi.waitFor(() => expect(onPicked).toHaveBeenCalledTimes(1));
  });

  it('is reachable by keyboard, which dispatches click alone', async () => {
    vi.mocked(openAssetDialog).mockResolvedValue([`${ROOT}/img/photo.jpg`]);
    const onPicked = vi.fn();
    const btn = browseButton({ variant: 'inset', ctx: ctx(), onPicked });
    btn.click();
    await vi.waitFor(() => expect(onPicked).toHaveBeenCalledWith(ITEM));
  });

  it('keeps focus where it is when pressed by mouse', () => {
    // A chip popover that loses its input's focus closes itself.
    vi.mocked(openAssetDialog).mockResolvedValue([]);
    const btn = browseButton({ variant: 'inset', ctx: ctx(), onPicked: vi.fn() });
    const ev = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    btn.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });

  it('shows the word, not just the glyph', () => {
    // Both mounts are floating overlays, where data-tooltip is forbidden.
    const btn = browseButton({ variant: 'inset', ctx: ctx(), onPicked: vi.fn() });
    expect(btn.textContent?.trim()).not.toBe('');
    expect(btn.getAttribute('aria-label')).toBeTruthy();
  });
});
