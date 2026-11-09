/**
 * The media-embed kind table and folder-listing detail — what the editor
 * needs to know about a non-image embed's kind, icon and folder facts.
 *
 * `MediaEmbedBlockWidget` itself (the resizable placeholder card) lives in
 * `cm-block-embed-widgets.ts`, alongside `ImageBlockWidget`. This module keeps the
 * kind→icon table and the folder-listing fold.
 */

import { folderChips, type FolderEmbedParams } from '@symbiosis-lab/moss-syntax/cm6';

/** Embed kinds this card can represent. */
export type MediaEmbedKind =
  | 'folder'
  | 'app'
  | 'external'
  | 'pdf'
  | 'video'
  | 'audio'
  | 'html'
  | '3d'
  | 'notebook'
  | 'table'
  | 'embed';

const KIND_ICONS: Record<MediaEmbedKind, string> = {
  folder: '📁',
  app: '🗔',
  external: '🔗',
  pdf: '📄',
  video: '🎬',
  audio: '🎵',
  html: '</>',
  '3d': '◈',
  notebook: '📓',
  table: '▦',
  embed: '◰',
};

/** The kind's glyph. */
export function iconFor(kind: string): string {
  return KIND_ICONS[kind as MediaEmbedKind] ?? KIND_ICONS.embed;
}

/**
 * What the last build listed under a folder, as the reference envelope carries
 * it.
 *
 * The part of the generated `FolderEmbedInfo` this module renders.
 * `cm-block-embeds.ts` passes the real type in, which type-checks
 * structurally.
 */
export interface FolderListingFacts {
  title: string | null;
  direct_child_count: number;
  descendant_count: number;
}

/**
 * What a folder-listing card says, beyond icon + label + name.
 *
 * Data, never prose: every user-visible string is resolved through `t()` inside
 * `toDOM`, so a locale switch repaints correctly.
 */
export interface FolderCardDetail {
  /** The folder index page's title; null → the caller's `name` is shown. */
  title: string | null;
  /**
   * Pages the last build listed here, after THIS embed's depth and limit.
   * null = the article map does not know this folder → render no count at
   * all. "Found and zero" and "unknown" must be different states on screen.
   */
  childCount: number | null;
  /** Canonical `key:value` chips, already ordered by `folderChips()`. */
  chips: string[];
}

/**
 * Fold the envelope's folder facts and the source-derived params into what the
 * card renders. Pure, and the single place the "no card detail" decision lives.
 *
 * Returns undefined when `params` is null — i.e. for a markdown-image folder
 * embed, which the build never turns into a listing (see folderParamsFromEmbed).
 */
export function folderCardDetail(
  info: FolderListingFacts | null | undefined,
  params: FolderEmbedParams | null,
): FolderCardDetail | undefined {
  if (!params) return undefined;
  let childCount: number | null = null;
  if (info) {
    // EXACT string compare, mirroring the build's `depth.as_deref() ==
    // Some("all")`: depth:deep, depth:1 and depth:All are all *direct*.
    const total = params.depth === 'all' ? info.descendant_count : info.direct_child_count;
    // The build truncates only when `n > 0 && n < len`, so limit:0 and an
    // over-large limit both leave the total alone.
    const limit = params.limit ?? 0;
    childCount = limit > 0 ? Math.min(limit, total) : total;
  }
  return { title: info?.title ?? null, childCount, chips: folderChips(params) };
}
