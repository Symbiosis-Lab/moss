/**
 * preview-context-menu.ts — the shell side of the preview's right-click.
 *
 * The preview bridge suppresses the native WebKit menu inside the previewed
 * page and posts a `moss-context-menu` payload: pointer position in
 * iframe-viewport CSS px plus what was under the cursor
 * (source target, link, image, selection). This module turns that payload
 * into entries for the shared ctx-menu primitive and translates the
 * coordinates into shell-viewport space.
 *
 * URL policy: a copied
 * link is the PUBLISHED address, never the localhost dev-server URL. When the
 * site has no published URL yet, we copy the site-relative path ("/posts/foo/")
 * and say so in the label ("Copy link address") — a path is honest about being
 * a path, where a localhost URL masquerades as shareable. The one sanctioned
 * localhost use is "Open in browser", which deliberately opens the LOCAL
 * preview page — that is what the user is looking at.
 */

import { joinCtxGroups, openCtxMenu, type CtxMenuEntry } from "../shared/ui/ctx-menu";
import { t } from "../i18n/index";
import { withoutPreviewParams } from "./shell-marker";
import type { PictureAct, PictureActRequest } from "../editor/session/editor-position";
import { getInternalToggleState } from "../editor/internal-toggle";
import { isRootUrlPath } from "../utils/url-path";
import { extractDisplayPath } from "./path-utils";

/**
 * What the bridge says an element maps to in source. `path` (site-root-
 * relative, which equals project-root-relative) names a file other than the
 * one shown; `{ kind: 'page', path }` names a file with no position in it.
 */
export interface PreviewSourceTarget {
  kind: string;
  path?: string;
  line?: number;
  startLine?: number;
  endLine?: number;
  field?: string;
  text?: string;
  /** The annotated element: a tag, or `tag.moss-class`. */
  element?: string;
  /** A menu row's act on a picture reference at the target's line; absent, the row only opens there. */
  act?: PictureActRequest;
}

/** The preview bridge's `moss-context-menu` payload. */
export interface PreviewCtxPayload {
  type: 'moss-context-menu';
  x: number;
  y: number;
  page: string;
  /** Source places that decide the element under the cursor, nearest first. */
  targets: PreviewSourceTarget[];
  /** Selectors of the theme rules (style.css) that style the element, nearest first. */
  styles: string[];
  link: string | null;
  image: string | null;
  /** Which of the pictures sharing the clicked picture's source line it is; null with no picture. */
  imageIndex?: number | null;
  selection: string | null;
}

export function isPreviewCtxMessage(data: unknown): data is PreviewCtxPayload {
  return (
    typeof data === 'object' &&
    data !== null &&
    (data as { type?: unknown }).type === 'moss-context-menu' &&
    typeof (data as { x?: unknown }).x === 'number' &&
    typeof (data as { y?: unknown }).y === 'number' &&
    typeof (data as { page?: unknown }).page === 'string' &&
    Array.isArray((data as { targets?: unknown }).targets)
  );
}

/**
 * Translate a point from the iframe's viewport CSS px into shell-viewport
 * coordinates. The iframe's bounding rect already reflects the preview-zoom
 * CSS transform (scale) AND the element's offset in the shell, so the scale
 * factor is recovered as rect.width / clientWidth rather than asking the
 * zoom controller — one source of truth, correct in mobile frame mode too.
 */
export function translateIframePoint(
  iframe: HTMLIFrameElement,
  x: number,
  y: number,
): { x: number; y: number } {
  const rect = iframe.getBoundingClientRect();
  const scaleX = iframe.clientWidth > 0 ? rect.width / iframe.clientWidth : 1;
  const scaleY = iframe.clientHeight > 0 ? rect.height / iframe.clientHeight : 1;
  return { x: rect.left + x * scaleX, y: rect.top + y * scaleY };
}

/** Result of resolving a preview URL to its copyable form. */
export interface CopyableUrl {
  /** The text to place on the clipboard. */
  text: string;
  /** True when no site URL is configured and `text` is a site-relative path. */
  siteRelative: boolean;
}

/**
 * The copyable form of a URL found in the preview.
 *
 * - A URL on a different origin than the previewed page is already a real,
 *   published address (external links pass through the preview untouched) —
 *   copied verbatim.
 * - A same-origin URL is a dev-server address. Its path is joined to the
 *   published site URL when one is configured; otherwise the site-relative
 *   path itself is the copy (flagged so the caller can label it honestly).
 *   The query stays, less what the preview added: a published page keeps
 *   state there (a map's place, a form's prefill).
 */
export function publishedUrlFor(
  rawUrl: string,
  pageHref: string,
  siteUrl: string | null,
): CopyableUrl {
  let u: URL;
  let page: URL;
  try {
    u = new URL(withoutPreviewParams(rawUrl));
    page = new URL(pageHref);
  } catch {
    return { text: rawUrl, siteRelative: false };
  }
  if (u.origin !== page.origin) {
    return { text: rawUrl, siteRelative: false };
  }
  const path = u.pathname + u.search + u.hash;
  if (siteUrl) {
    return { text: siteUrl.replace(/\/+$/, '') + path, siteRelative: false };
  }
  return { text: path, siteRelative: true };
}

/** Everything the menu needs from the shell, injected for testability. */
export interface PreviewCtxDeps {
  /** Published site base URL (e.g. "https://her-blog.mosspub.com"), or null. */
  siteUrl: string | null;
  /** Whether the current page maps to an editable source file. */
  canEdit: boolean;
  /** Source file of the shown page, for row detail. */
  pageFile: string | null;
  /** Route to the editor at a target — the same dispatcher as double-click. */
  openTarget(target: PreviewSourceTarget | null): void;
  /** Clipboard write. */
  copy(text: string): void;
  /** Open a URL in the system browser. */
  openExternal(url: string): void;
  /** Open the theme stylesheet at the rule for `selector`. */
  openStyle(selector: string): void;
  /** "Show internal files" is on: theme rules are offered. */
  showInternal: boolean;
}

/** Last path segment, or null when there is no path. */
function fileName(p: string | null | undefined): string | null {
  return p ? p.slice(p.lastIndexOf('/') + 1) : null;
}

/** File name without its extension: `posts/a.md` → `a`. */
function pageName(p: string): string {
  return (fileName(p) ?? '').replace(/\.[^.]+$/, '');
}

function rowLabel(target: PreviewSourceTarget): string {
  const el = target.element ?? '';
  switch (target.kind) {
    case 'frontmatter':
      if (target.field === 'nav') return t('preview.ctx_nav_settings');
      if (target.field === 'title') return t('preview.ctx_edit_title');
      return t('preview.ctx_edit_field', { field: target.field ?? '' });
    case 'body-range':
      if (el === 'section.moss-hero') return t('preview.ctx_hero_settings');
      if (el === 'div.moss-grid') return t('preview.ctx_grid_settings');
      return t('preview.ctx_edit_block');
    case 'body-line':
      switch (el.split('.')[0]) {
        case 'h1': case 'h2': case 'h3': case 'h4': case 'h5': case 'h6':
          return t('preview.ctx_edit_heading');
        case 'p': return t('preview.ctx_edit_paragraph');
        case 'img': case 'figure': case 'picture': return t('preview.ctx_edit_image');
        default: return t('preview.ctx_edit_line');
      }
    case 'page':
      if (!target.path) return t('preview.ctx_edit_page');
      return t('preview.ctx_edit_named_page', { name: pageName(target.path) });
    default:
      return t('preview.ctx_edit_line');
  }
}

/**
 * The edit group: one row per source target, nearest first. After the last
 * target in a file, a page row for that file is added unless the targets
 * already name it. An unannotated element (no targets) offers the page itself;
 * chrome (a `none` target) offers nothing to edit.
 */
function editEntries(ctx: PreviewCtxPayload, deps: PreviewCtxDeps): CtxMenuEntry[] {
  if (ctx.targets.some((x) => x.kind === 'none')) return [];
  if (ctx.targets.length === 0) {
    return deps.canEdit
      ? [{ label: t('preview.ctx_edit_page'), action: () => deps.openTarget(null) }]
      : [];
  }
  const rows: CtxMenuEntry[] = [];
  ctx.targets.forEach((target, i) => {
    // The grid's row belongs to its own area: nothing nearer than its cells was hit.
    if (target.element === 'div.moss-grid' && !ctx.targets.slice(0, i).every((x) => x.element === 'div.moss-grid-card')) return;
    const file = target.path ?? deps.pageFile;
    const name = fileName(file);
    rows.push({
      label: rowLabel(target),
      ...(name ? { detail: name } : {}),
      action: () => deps.openTarget(target),
    });
    const path = target.path;
    if (!path) return;
    const isLastInFile = ctx.targets.map((x) => x.path).lastIndexOf(path) === i;
    const hasPageRow = ctx.targets.some((x) => x.kind === 'page' && x.path === path);
    if (!isLastInFile || hasPageRow) return;
    const page: PreviewSourceTarget = { kind: 'page', path };
    rows.push({
      label: t('preview.ctx_edit_named_page', { name: pageName(path) }),
      detail: fileName(path)!,
      action: () => deps.openTarget(page),
    });
  });
  return rows;
}

/** The style group: one row per theme rule that styles the element, while internal files are shown. */
function styleEntries(ctx: PreviewCtxPayload, deps: PreviewCtxDeps): CtxMenuEntry[] {
  if (!deps.showInternal) return [];
  return (ctx.styles ?? []).map((selector) => ({
    label: t('preview.ctx_style_rule', { selector }),
    detail: 'style.css',
    action: () => deps.openStyle(selector),
  }));
}

/** Moss's own structures that crop the picture in them to a frame. */
const CROPPING = new Set(['section.moss-hero', 'div.moss-grid-card']);

/**
 * The picture rows, for a picture the shown page's own source names: a body
 * picture (its line) or a hero's (its fence). Another page's picture (a
 * listing card's cover), a frontmatter one (a folder cover, the logo) and one
 * the theme draws get none; their edit rows stay. "Set focus point" is offered
 * where a moss structure crops; "Edit description" not on a hero, whose
 * picture is an attribute with no description.
 */
function pictureEntries(ctx: PreviewCtxPayload, deps: PreviewCtxDeps): { rows: CtxMenuEntry[]; at: PreviewSourceTarget } | null {
  const at = ctx.targets[0];
  if (!ctx.image || !at || at.path) return null;
  const hero = at.kind === 'body-range' && at.element === 'section.moss-hero';
  if (at.kind !== 'body-line' && !hero) return null;
  const picture = ctx.imageIndex ?? 0;
  const row = (label: Parameters<typeof t>[0], act: PictureAct): CtxMenuEntry => ({ label: t(label), action: () => deps.openTarget({ ...at, act: { act, picture } }) });
  const crops = ctx.targets.some((x) => CROPPING.has(x.element ?? ''));
  return {
    at,
    rows: [
      row('preview.ctx_replace_picture', 'replace'),
      ...(crops ? [row('preview.ctx_focus_point', 'focus')] : []),
      ...(hero ? [] : [row('preview.ctx_edit_description', 'description')]),
      row('preview.ctx_show_in_files', 'reveal'),
    ],
  };
}

/**
 * Build the menu entries for a preview right-click. Items are conditional on
 * what was under the cursor; groups joined with separators only where both
 * sides rendered (joinCtxGroups). Deliberately NO Reload/Back/Forward —
 * navigation belongs to the shell chrome.
 */
export function buildPreviewCtxEntries(
  ctx: PreviewCtxPayload,
  deps: PreviewCtxDeps,
): CtxMenuEntry[] {
  const picture = pictureEntries(ctx, deps);
  // On a picture one row edits the page, at the picture's line.
  const edit = picture
    ? [{ label: t('preview.ctx_edit_page'), action: () => deps.openTarget(picture.at) }]
    : editEntries(ctx, deps);
  const style = styleEntries(ctx, deps);

  const copies: CtxMenuEntry[] = [];
  if (ctx.selection) {
    const text = ctx.selection;
    copies.push({ label: t('preview.ctx_copy'), action: () => deps.copy(text) });
  }
  if (ctx.link) {
    const resolved = publishedUrlFor(ctx.link, ctx.page, deps.siteUrl);
    copies.push({
      label: t(resolved.siteRelative ? 'preview.ctx_copy_link_address' : 'preview.ctx_copy_link'),
      action: () => deps.copy(resolved.text),
    });
  }
  if (ctx.image) {
    const resolved = publishedUrlFor(ctx.image, ctx.page, deps.siteUrl);
    copies.push({
      label: t('preview.ctx_copy_image_address'),
      action: () => deps.copy(resolved.text),
    });
  }
  // The page's own address, always offered and always last — the entries above
  // are about whatever sits under the cursor, this one is about where you are.
  // Sharing the page you are looking at is the reason this entry exists, so it must
  // not depend on having right-clicked a link.
  {
    const resolved = publishedUrlFor(ctx.page, ctx.page, deps.siteUrl);
    copies.push({
      label: t(resolved.siteRelative ? 'preview.ctx_copy_page_address' : 'preview.ctx_copy_page'),
      action: () => deps.copy(resolved.text),
    });
  }

  const open: CtxMenuEntry[] = [
    {
      label: t('preview.ctx_open_in_browser'),
      action: () => deps.openExternal(withoutPreviewParams(ctx.page)),
    },
  ];

  return joinCtxGroups([picture?.rows ?? [], edit, style, copies, open]);
}

/**
 * Open the menu for a bridge payload at the pointer. The shell gives what only
 * it can do; the link answers for the page and opens what a row names.
 */
export function openPreviewCtxMenu(
  ctx: PreviewCtxPayload,
  link: { readonly node: { sourcePath: string | null } | null; open(target: PreviewSourceTarget | null): void },
  shell: Pick<PreviewCtxDeps, 'siteUrl' | 'copy' | 'openExternal' | 'openStyle'>,
): void {
  const iframe = document.getElementById('moss-preview-iframe') as HTMLIFrameElement | null;
  const pageFile = link.node?.sourcePath ?? null;
  const entries = buildPreviewCtxEntries(ctx, {
    ...shell,
    pageFile,
    // The root's home is found on disk even before the page has answered.
    canEdit: !!pageFile || isRootUrlPath(extractDisplayPath(ctx.page)),
    showInternal: getInternalToggleState(),
    openTarget: (target) => link.open(target),
  });
  if (!iframe || entries.length === 0) return;
  openCtxMenu({
    entries,
    position: { kind: 'point', ...translateIframePoint(iframe, ctx.x, ctx.y) },
    ariaLabel: t('preview.ctx_menu_aria'),
    // The keyboard is in the preview's own document, which the menu's keys never reach.
    focusFirst: true,
    logSurface: 'preview',
    logTarget: ctx.targets[0]?.kind,
  });
}
