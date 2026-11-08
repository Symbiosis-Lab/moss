// Block embeds: the picture, player or card drawn BELOW an embed's own source line.
//
// An embed alone on its line keeps its source line as visible text, and
// `buildBlockEmbedDecorations` places the rendered block after it (`blockEmbedsIn`
// is the one enumerator both the decoration builder and the click/hover paths
// walk). Nothing in the markdown editor is replaced by a widget: a picture in a
// line of prose, a link, a task and a comment are all just their styled
// characters. The widget
// classes the decorations place live in `cm-block-embed-widgets.ts`.

import { EditorState, StateField, type Extension } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import {
  embedParts, isEmbedNode, folderParamsFromEmbed, linkUnitOfEmbed,
  shortcodeBodyRanges, inShortcodeBody,
} from '@symbiosis-lab/moss-syntax/cm6';
import { folderCardDetail, type MediaEmbedKind, type FolderListingFacts } from './cm-media-embed';
import { refsResolvedEffect } from './cm-reference-resolver';
import { refreshEditorColumnWidth, sampleBlockChromeHeights } from './block-row-height';
import {
  ImageBlockWidget, MediaEmbedBlockWidget,
  type ResolveImageFn, type SetImageWidthFn,
} from './cm-block-embed-widgets';

/** An embed node's placement: its parts, the link it is the sole content of
 *  (`[![[x]]](url)` reads as one clickable unit), and whether nothing but
 *  whitespace shares its line outside that unit. */
interface EmbedSite {
  parts: NonNullable<ReturnType<typeof embedParts>>;
  unit: ReturnType<typeof linkUnitOfEmbed>;
  line: { from: number; to: number };
  alone: boolean;
}

function embedSite(state: EditorState, node: Parameters<typeof embedParts>[0] & { node: unknown }): EmbedSite | null {
  const parts = embedParts(node, state.doc);
  if (!parts) return null;
  const line = state.doc.lineAt(node.from);
  const unit = linkUnitOfEmbed(node.node as EmbedTreeNode, state.doc);
  const unitFrom = unit ? unit.link.from : node.from;
  const unitTo = unit ? unit.link.to : node.to;
  const rest = state.doc.sliceString(line.from, unitFrom) + state.doc.sliceString(unitTo, line.to);
  return { parts, unit, line: { from: line.from, to: line.to }, alone: rest.trim() === '' };
}

/** An embed alone on its line: its source stays as text and `buildBlockEmbedDecorations`
 *  draws the picture/player as a block widget below the line. */
export interface BlockEmbed extends EmbedSite {
  /** The embed node's own span (the `![[…]]` / `![…](…)`, not the link around it). */
  from: number;
  to: number;
  treeNode: EmbedTreeNode;
}

export function blockEmbedsIn(state: EditorState, from: number, to: number): BlockEmbed[] {
  const out: BlockEmbed[] = [];
  syntaxTree(state).iterate({
    from, to,
    enter(node) {
      if (!isEmbedNode(node.name)) return;
      const site = embedSite(state, node);
      if (site?.alone) out.push({ ...site, from: node.from, to: node.to, treeNode: node.node as unknown as EmbedTreeNode });
      return false;
    },
  });
  return out;
}

/** The block embed whose line holds `pos` (the rendered block's own position is its line end). */
export function blockEmbedOnLine(state: EditorState, pos: number): BlockEmbed | null {
  const line = state.doc.lineAt(pos);
  return blockEmbedsIn(state, line.from, line.to)[0] ?? null;
}

/** The embed a caret or hover at `pos` is on, with the position inside it that the card providers key off (`probe`, its target). Any position of a block embed's line counts; an inline embed counts over any of its own characters, either edge included. An inline picture is only its source text: nothing is drawn for it, so this is how its card is found. */
export function embedAt(state: EditorState, pos: number): { from: number; to: number; probe: number; inline: boolean; parts: EmbedSite['parts'] } | null {
  const block = blockEmbedOnLine(state, pos);
  if (block) return { from: block.from, to: block.to, probe: block.parts.targetFrom, inline: false, parts: block.parts };
  for (const side of [1, -1] as const) {
    for (let node: EmbedTreeNode | null = syntaxTree(state).resolveInner(pos, side) as unknown as EmbedTreeNode | null; node; node = node.parent) {
      if (!isEmbedNode(node.name)) continue;
      const parts = embedParts(node as unknown as Parameters<typeof embedParts>[0], state.doc);
      if (parts) return { from: node.from, to: node.to, probe: parts.targetFrom, inline: true, parts };
    }
  }
  return null;
}

// `ResolveImageFn` and `SetImageWidthFn` live with the widgets that take them
// (cm-block-embed-widgets.ts).
/** The discriminant tag of `ReferenceKind` (e.g. `"image"`, `"folder-index-iframe"`),
 *  spelled out as a string union. */
type ReferenceKindTag =
  | 'link' | 'image' | 'iframe' | 'pdf' | 'video' | 'audio' | 'model'
  | 'folder-listing' | 'folder-index-iframe' | 'transclusion' | 'notebook' | 'table'
  | 'external' | 'anchor' | 'ambiguous' | 'not-found' | 'moved';
export type ResolveEmbedFn = (target: string) => {
  kind: ReferenceKindTag;
  src?: string;
  folder?: FolderListingFacts | null;
  follow?: () => void;
} | undefined;
export type NavigateLinkFn = (href: string) => boolean;
export type CanFollowLinkFn = (href: string) => boolean;

/**
 * Map a `ReferenceKindTag` to the `MediaEmbedBlockWidget` kind icon/label set
 * and whether the card is resizable. Ported verbatim from
 * cm-live-preview.ts:139-162 (`mediaEmbedFromKind`).
 */
function mediaEmbedFromKind(
  tag: ReferenceKindTag,
): { kind: MediaEmbedKind; resizable: boolean } | null {
  switch (tag) {
    case 'folder-index-iframe': return { kind: 'app', resizable: true };
    case 'iframe':              return { kind: 'html', resizable: true };
    case 'pdf':                 return { kind: 'pdf', resizable: true };
    case 'video':               return { kind: 'video', resizable: true };
    case 'audio':               return { kind: 'audio', resizable: true };
    case 'model':                return { kind: '3d', resizable: true };
    case 'external':            return { kind: 'external', resizable: true };
    case 'folder-listing':      return { kind: 'folder', resizable: false };
    case 'transclusion':        return { kind: 'embed', resizable: false };
    case 'notebook':            return { kind: 'notebook', resizable: false };
    case 'table':               return { kind: 'table', resizable: false };
    case 'image':
    case 'link':
    case 'anchor':
    case 'not-found': case 'ambiguous': case 'moved':
      return null;
  }
}

/**
 * `buildBlockEmbedDecorations`' block-embed classify step, pulled out to ONE
 * statement. Ported verbatim from cm-live-preview.ts:172-190
 * (`EmbedTargetClassification`/`classifyEmbedTarget`).
 */
type EmbedTargetClassification =
  | { kind: 'media'; media: { kind: MediaEmbedKind; resizable: boolean }; classified: NonNullable<ReturnType<ResolveEmbedFn>> }
  | { kind: 'image'; img: NonNullable<ReturnType<ResolveImageFn>> }
  | { kind: 'unrenderable' }
  | { kind: 'pending' };

function classifyEmbedTarget(
  target: string,
  resolveImage: ResolveImageFn | undefined,
  resolveEmbed: ResolveEmbedFn | undefined,
): EmbedTargetClassification {
  const classified = resolveEmbed?.(target);
  if (classified && classified.kind !== 'image') {
    const media = mediaEmbedFromKind(classified.kind);
    return media ? { kind: 'media', media, classified } : { kind: 'unrenderable' };
  }
  const img = resolveImage?.(target);
  return img ? { kind: 'image', img } : { kind: 'pending' };
}

/** Display name for an embed card: the basename of the target text. Ported
 *  verbatim from cm-live-preview.ts:205-210. */
function embedDisplayName(target: string): string {
  const trimmed = target.replace(/\/+$/, '');
  const slash = trimmed.lastIndexOf('/');
  const base = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
  return base || target;
}

export interface BlockEmbedsOptions {
  resolveImage?: ResolveImageFn;
  onSetWidth?: SetImageWidthFn;
  resolveEmbed?: ResolveEmbedFn;
  navigateLink?: NavigateLinkFn;
  canFollowLink?: CanFollowLinkFn;
}

/**
 * Turn every block embed in `ranges` into its decoration: the widget is placed
 * after the line's end, `block: true, side: 1`, and the source line stays as text.
 */
export function buildBlockEmbedDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[],
  opts: BlockEmbedsOptions,
): DecorationSet {
  const decos: { from: number; to: number; deco: Decoration }[] = [];
  // Computed once per build, not per embed — `shortcodeBodyRanges` walks
  // the whole block tree (cm-live-preview.ts's own comment on this call).
  const fenceBodies = shortcodeBodyRanges(state);
  for (const { from, to } of ranges) {
    for (const b of blockEmbedsIn(state, from, to)) {
      const d = blockEmbedDecoration(state, b, opts, fenceBodies);
      if (d) decos.push(d);
    }
  }
  return Decoration.set(decos.map((d) => d.deco.range(d.from, d.to)), true);
}

function blockEmbedDecoration(
  state: EditorState,
  b: BlockEmbed,
  opts: BlockEmbedsOptions,
  fenceBodies: ReturnType<typeof shortcodeBodyRanges>,
): { from: number; to: number; deco: Decoration } | null {
  const { target, alt, width: widthToken } = b.parts;
  const { line } = b;
  const intraLineOffset = b.from - line.from;

  // A `:::` fence body is STRUCTURE, not reading matter: a gallery or grid lists
  // one embed per cell, so a picture below every source line would turn six cells
  // into six full-column photographs (~3000px against six lines). Its source line
  // is all the editor shows; the preview pane draws the picture in its real layout.
  if (inShortcodeBody(fenceBodies, b.from)) return null;

  const classification = classifyEmbedTarget(target, opts.resolveImage, opts.resolveEmbed);
  // link/anchor/not-found/ambiguous, or not yet resolved: nothing to draw below.
  if (classification.kind === 'unrenderable' || classification.kind === 'pending') return null;
  const unit = b.unit;
  let widget: ImageBlockWidget | MediaEmbedBlockWidget;
  if (classification.kind === 'media') {
    const { media, classified } = classification;
    widget = new MediaEmbedBlockWidget({
      kind: media.kind,
      name: embedDisplayName(target),
      target,
      intraLineOffset,
      onSetWidth: opts.onSetWidth ?? (() => {}),
      resizable: media.resizable,
      width: media.kind === 'folder' ? undefined : widthToken,
      src: classified.src,
      folder: folderCardDetail(classified.folder, media.kind === 'folder' ? folderParamsFromEmbed(b.parts) : null),
      linkHref: unit?.href,
      onFollow: unit
        ? (opts.navigateLink && opts.canFollowLink?.(unit.href)
            ? () => { opts.navigateLink!(unit.href); }
            : undefined)
        : classified.follow,
    });
  } else {
    const { img } = classification;
    widget = new ImageBlockWidget({
      url: img.url,
      alt,
      intrinsicWidth: img.width,
      intrinsicHeight: img.height,
      intraLineOffset,
      width: widthToken,
      onSetWidth: opts.onSetWidth,
      link: unit && opts.canFollowLink?.(unit.href) ? { href: unit.href } : undefined,
      onNavigate: opts.navigateLink,
    });
  }
  // After the line's end, in the gap before the next line: the source line is
  // untouched, so the caret walks through every character of it.
  return { from: line.to, to: line.to, deco: Decoration.widget({ widget, block: true, side: 1 }) };
}

/** Structural slice of a Lezer SyntaxNode (matches `linkUnitOfEmbed`'s
 *  `TreeNode` parameter — `.parent`/`.getChild`, not just `.from`/`.to`). */
interface EmbedTreeNode {
  name: string;
  from: number;
  to: number;
  parent: EmbedTreeNode | null;
  getChild(name: string): EmbedTreeNode | null;
}

/**
 * The block embed widgets live in a `StateField`: CM6 forbids a block decoration
 * from a `ViewPlugin` ("Block decorations may not be specified via plugins").
 * It covers the whole document rather than the viewport, as a `StateField` never
 * sees `view.visibleRanges`.
 *
 * Composition guard: a `StateField`'s `update(value, tr)` never sees the view, so
 * it reads the transaction-level tag CM6 sets on a composition-driven edit
 * (`tr.isUserEvent('input.type.compose')`) and keeps the old set, mapped.
 */
export function blockEmbedsExtension(opts: BlockEmbedsOptions): Extension {
  const field = StateField.define<DecorationSet>({
    create(state) {
      return buildBlockEmbedDecorations(state, [{ from: 0, to: state.doc.length }], opts);
    },
    update(value, tr) {
      if (tr.isUserEvent('input.type.compose')) return value.map(tr.changes);
      const treeAdvanced = syntaxTree(tr.state) != syntaxTree(tr.startState);
      // refsResolvedEffect: the reference resolver's async batch lands with no
      // doc change of its own — without this, an embed stays classified
      // "pending" (raw) forever once resolveImage/resolveEmbed start returning
      // real data, since nothing else here would ever trigger a rebuild.
      const resolved = tr.effects.some((e) => e.is(refsResolvedEffect));
      if (!tr.docChanged && !treeAdvanced && !resolved) return value;
      return buildBlockEmbedDecorations(tr.state, [{ from: 0, to: tr.state.doc.length }], opts);
    },
    provide(f) { return EditorView.decorations.from(f); },
  });
  return [
    field,
    // [perf] On a geometry change (editor resize / divider drag — no state
    // transaction, so toDOM never re-runs), refresh the shared column-width
    // cache in the measure READ phase so off-viewport estimatedHeight tracks
    // the LIVE column: a stale cache re-anchors the scroll every frame, which
    // freezes image-heavy docs. sampleBlockChromeHeights rides
    // along under the same rule (block-row-height.ts). This is the one part
    // of the old inline ViewPlugin that stays a ViewPlugin: it dispatches no
    // decorations, only refreshes the shared cache, so the block-decoration
    // restriction above does not apply to it.
    EditorView.updateListener.of((update: ViewUpdate) => {
      if (update.geometryChanged) {
        update.view.requestMeasure({ read: (v) => {
          refreshEditorColumnWidth(v.contentDOM);
          sampleBlockChromeHeights(v.contentDOM);
        } });
      }
    }),
  ];
}
