/**
 * "Pick it from my disk" — the escape hatch beside every asset search.
 *
 * Every surface that asks the author for an asset path offers one way to
 * answer: type, and pick from a search dropdown. That is fast when you
 * remember the filename and useless when you don't — and a photo still sitting
 * in ~/Downloads cannot be named at all, because it isn't in the vault yet.
 * This is the one component both surfaces mount: the chip bar's cover/logo
 * field (inset on the input's trailing edge) and the CM6 completion tooltip
 * (a header strip).
 *
 * Two rules hold it together:
 *
 *  - **The pick returns a search-shaped item.** `pickAssetRef` hands back the
 *    same `WikilinkCompletion` the Rust ranker returns, so a picked file
 *    travels the accept path the search already has — no second insert format
 *    to keep in step. The reference form itself is decided in Rust
 *    (`asset_ref_for_path`), by the same helper the ranker uses.
 *  - **The word is visible, not hovered.** Both mounts are floating overlays,
 *    where `data-tooltip=` is forbidden (CLAUDE.md), and a control that needs
 *    a hint to be understood should be fixed rather than annotated.
 *    So the glyph never carries the
 *    meaning alone.
 *
 * The dialog and the reference form are the host's (`host.pickFiles`,
 * `host.assetRefForPath`).
 */

import type { WikilinkCompletion } from '@symbiosis-lab/moss-ui';
import { host, type FileFilter } from "../../host";
import { ASSET_REGISTRY } from '../asset-registry.generated';
import { importDirFor, importIntoVault } from '../os-drag-in';
import { toAbsolute, toRelative, childPath, relativeInside } from '../path-domain';
import { t } from "../../i18n/index";

/** A folder, open on its trailing edge — "from my disk", not "an image". */
const SVG_FOLDER =
  '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
  'stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>';

/** What the surface knows, read at PICK TIME — a rename or a folder switch
 *  during the dialog must not resolve the file against the old page. */
export interface BrowseContext {
  /** Project-root-relative path of the page being edited. */
  pagePath(): string;
  /** Absolute project root, or `null` when no folder is open. */
  projectRoot(): string | null;
  /** Schema `file_kinds` for the field; `null` means every embeddable asset. */
  kinds(): string[] | null;
  /**
   * Run after a file was copied INTO the vault. Every path that lands a copy
   * owes the file tree a refresh, or the import is invisible there until some
   * unrelated event — the drift `afterVaultImport` exists to prevent
   * (`os-drag-in.ts`). Not called when the pick was already in the vault.
   */
  afterImport?(): Promise<void>;
}

/**
 * The OS dialog filter for `kinds`, derived from the generated asset registry
 * so the dialog offers exactly what moss can embed — never a hand-kept list.
 */
export function dialogFilters(kinds: string[] | null): FileFilter[] {
  const extensions = ASSET_REGISTRY
    .filter((a) => a.canEmbed && (kinds === null || kinds.includes(a.kind)))
    .map((a) => a.ext);
  const name =
    kinds === null ? t('asset.filter_media')
    : kinds.length === 1 && kinds[0] === 'Image' ? t('asset.filter_images')
    : t('asset.filter_media');
  return [{ name, extensions }];
}

/**
 * Open the dialog, import the file if it came from outside the vault, and
 * return it in the shape the search returns. `null` for a cancel, and for a
 * pick the backend refuses (a folder, an extension moss cannot classify).
 *
 * A file ALREADY inside the vault is not copied — importing it would mint a
 * second copy of a file the author can already see in their own tree, which
 * is the one outcome a "browse" gesture never means.
 */
export async function pickAssetRef(ctx: BrowseContext): Promise<WikilinkCompletion | null> {
  const picked = await host.pickFiles(dialogFilters(ctx.kinds()), false, (name) => {
    const page = ctx.pagePath();
    return page ? importDirFor(name, page) : Promise.resolve('');
  });
  if (picked.length === 0) return null;

  // Read the page AFTER the dialog closes: it may have been renamed under us.
  const page = ctx.pagePath();
  const root = ctx.projectRoot();
  if (!page || !root) return null;

  let abs = picked[0];
  if (relativeInside(abs, root) === null) {
    // ONE import policy, shared with the OS-drop and insert-bar paths: images
    // honour the "Asset path" setting, everything else lands next to
    // the page. Deciding that here too is how the two copies start to drift.
    const landed = await importIntoVault([abs], page, root);
    if (!landed || landed.length === 0) return null;
    const { dir, finalName } = landed[0];
    abs = toAbsolute(childPath(dir, finalName), root);
    await ctx.afterImport?.();
  }
  return host.assetRefForPath?.(abs, toRelative(page, root)) ?? null;
}

export interface BrowseButtonOptions {
  ctx: BrowseContext;
  /**
   * Runs the moment the button is pressed, before the dialog opens.
   *
   * The mount's chance to stand its search down: an armed debounce would
   * otherwise fire a full uncached project-tree walk WHILE the dialog is up,
   * and land its result over the file the author just picked. A mount with a
   * request counter should retire in-flight requests here for the same reason.
   */
  onOpen?(): void;
  /** The picked item, in the shape the search returns. Not called on cancel. */
  onPicked(item: WikilinkCompletion): void;
  /**
   * Runs once the dialog is gone, on EVERY path — picked, cancelled, refused,
   * failed. A mount that stood something down in `onOpen` restores it here;
   * `onPicked` alone would leave a cancel holding the change forever.
   */
  onSettled?(): void;
  /** `'inset'` for the chip bar's input edge, `'strip'` for the CM6 header. */
  variant: 'inset' | 'strip';
}

/** Whether this host can turn a picked file into a reference; without that there is no Browse button. */
export function canBrowse(): boolean {
  return host.assetRefForPath !== undefined;
}

/**
 * Build the button.
 *
 * Activation is **mousedown + `preventDefault` AND click**, not one or the
 * other. `preventDefault` on mousedown keeps focus where it is — a chip
 * popover that loses its input's focus closes itself — and does not cancel the
 * subsequent `click`; keyboard Enter/Space dispatch `click` alone, so a
 * mousedown-only handler would be inert to the keyboard. `pressed` collapses
 * the pair into one pick.
 */
export function browseButton(opts: BrowseButtonOptions): HTMLButtonElement {
  const btn = document.createElement('button');
  btn.type = 'button';
  // Literal class strings, not a template: `css-coverage.test.ts` reads the
  // selectors this file names, and an assembled one is invisible to it.
  btn.className =
    opts.variant === 'inset' ? 'asset-browse asset-browse--inset'
    : 'asset-browse asset-browse--strip';
  const label = opts.variant === 'inset' ? t('asset.browse') : t('asset.browse_files');
  btn.innerHTML = `${SVG_FOLDER}<span class="asset-browse-label"></span>`;
  btn.querySelector<HTMLElement>('.asset-browse-label')!.textContent = label;
  // The word is on screen, so the accessible name is already right; the
  // attribute exists because the glyph is `aria-hidden` and screen readers
  // announcing "Browse" alone would not say what is browsed.
  btn.setAttribute('aria-label', t('asset.browse_files'));

  let pressed = false;
  const fire = (): void => {
    if (pressed) return;
    pressed = true;
    opts.onOpen?.();
    void pickAssetRef(opts.ctx)
      .then((item) => { if (item) opts.onPicked(item); })
      .catch((e: unknown) => console.error('[asset-picker] pick failed', e))
      .finally(() => {
        pressed = false;
        opts.onSettled?.();
      });
  };
  // `stopPropagation` matters here specifically for the 'strip' variant: this
  // button lives INSIDE `.cm-tooltip-autocomplete` (completion-strip.ts
  // inserts it as the tooltip's first child), which carries its OWN ancestor
  // `mousedown` listener (@codemirror/autocomplete's `CompletionTooltip`,
  // unguarded against its completion state being closed mid-event). Without
  // this, `fire()`'s `onOpen` callback closes the completion synchronously,
  // detaching that ancestor's DOM — but the event's propagation path was
  // already computed before dispatch, so it keeps bubbling there anyway and
  // the library destructures a now-null state. Same pattern as
  // `overlay.ts:75` and `ctx-menu.ts:209`.
  btn.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation(); fire(); });
  btn.addEventListener('click', (e) => { e.preventDefault(); fire(); });
  return btn;
}
