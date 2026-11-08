// Block embed widgets: the WidgetType classes `cm-block-embeds.ts` places — the image,
// media card and player drawn below an embed's own source line. Each takes
// everything it needs as constructor arguments and never imports `cm-block-embeds.ts`
// (which imports this file), so the pair stays acyclic: the two callback/resolver
// types the widgets share live here and are re-read by the block-embed field.

import { EditorView, WidgetType } from '@codemirror/view';
import { imageNodeAtWidget } from '@symbiosis-lab/moss-syntax/cm6';
import { createResizeController, type ResizeController } from './cm-resize-controller';
import { iconFor, type MediaEmbedKind, type FolderCardDetail } from './cm-media-embed';
import { isFollowChord } from './cm-link-nav';
import {
  imageRowHeight, imageCapWidth, editorColumnWidth, refreshEditorColumnWidth,
  mediaRowHeight, recordVideoAspect, type MediaRowShape,
} from './block-row-height';
import { t } from "../i18n/index";
import type { UiStringMap } from "../i18n/index";
import { openCtxMenu } from "../shared/ui/ctx-menu";
import { isMacPlatform } from "../utils/kbd-label";
import { count as diagCount } from "../lib/diag";

export type ResolveImageFn = (target: string) => {
  url: string;
  width?: number;
  height?: number;
} | undefined;
export type SetImageWidthFn = (from: number, to: number, width: string | null) => void;

// ── Block widgets: image, media — drawn below the source line ────────

/**
 * Block-level image widget placed in the gap AFTER the source line via
 * `Decoration.widget({ block: true, side: 1 })` anchored at the line-END
 * boundary. Ported from cm-live-preview.ts:525-699. It replaces nothing: the
 * source line stays on screen as text and this renders the picture below it,
 * so a caret walks through every character of the source.
 *
 * The widget deliberately carries NO absolute document positions: `eq()`
 * excludes positions so edits above the image reuse the DOM (no <img> flash).
 * The resize commit resolves the Image node fresh at gesture time via
 * `imageNodeAtWidget`; `intraLineOffset` (the node's offset within its source
 * line, captured at build) survives edits above the line and disambiguates
 * multiple images on one line.
 */
export interface ImageBlockWidgetOptions {
  url: string;
  alt: string;
  intrinsicWidth?: number;
  intrinsicHeight?: number;
  intraLineOffset: number;
  width?: string;
  onSetWidth?: SetImageWidthFn;
  /** Set when the embed is the entire text of a link (`[![[x.png]]](/u)`): the
   *  rendered image is ONE clickable unit (the source line keeps its `[` and
   *  `](url)` as text). */
  link?: { href: string };
  /** Cmd/Ctrl+click handler for a linked embed. */
  onNavigate?: (href: string) => void;
}

export class ImageBlockWidget extends WidgetType {
  private rc: ResizeController | null = null;

  private readonly url: string;
  private readonly alt: string;
  private readonly intrinsicWidth: number | undefined;
  private readonly intrinsicHeight: number | undefined;
  private readonly intraLineOffset: number;
  private readonly width: string | undefined;
  private readonly onSetWidth: SetImageWidthFn | undefined;
  private readonly link: { href: string } | undefined;
  private readonly onNavigate: ((href: string) => void) | undefined;

  constructor(opts: ImageBlockWidgetOptions) {
    super();
    this.url = opts.url;
    this.alt = opts.alt;
    this.intrinsicWidth = opts.intrinsicWidth;
    this.intrinsicHeight = opts.intrinsicHeight;
    this.intraLineOffset = opts.intraLineOffset;
    this.width = opts.width;
    this.onSetWidth = opts.onSetWidth;
    this.link = opts.link;
    this.onNavigate = opts.onNavigate;
  }

  // `width` MUST be part of identity: without it CM6 reuses the old widget DOM
  // when only the width token changes, so a resize would not repaint. `href`
  // likewise — editing only the URL must not reuse DOM carrying a stale
  // tooltip and a stale navigation target.
  eq(other: ImageBlockWidget): boolean {
    return other.url === this.url && other.alt === this.alt
      && other.width === this.width
      && other.link?.href === this.link?.href;
  }

  get estimatedHeight(): number {
    return imageRowHeight(this.intrinsicWidth, this.intrinsicHeight, this.width, editorColumnWidth());
  }

  toDOM(view?: EditorView): HTMLElement {
    refreshEditorColumnWidth(view?.contentDOM);

    const outer = document.createElement('div');
    outer.className = 'cm-lp-image-row';

    const wrapper = document.createElement('div');
    wrapper.className = 'cm-lp-image-block';
    if (this.width) {
      wrapper.style.width = this.width;
    }
    if (this.intrinsicWidth && this.intrinsicHeight) {
      const cap = imageCapWidth(this.intrinsicWidth, this.intrinsicHeight);
      wrapper.style.maxWidth = `${Math.round(
        this.width ? cap : Math.min(this.intrinsicWidth, cap)
      )}px`;
      wrapper.style.aspectRatio = `${this.intrinsicWidth} / ${this.intrinsicHeight}`;
    }

    const img = document.createElement('img');
    img.className = 'cm-lp-image';
    img.alt = this.alt;
    img.style.cssText = 'width:100%;height:auto;display:block;';

    const onLoad = () => {
      diagCount('cm.img.load');
      const unsized = !this.width && !(this.intrinsicWidth && this.intrinsicHeight);
      if (unsized) {
        diagCount('cm.img.load.unsized');
        try { view?.requestMeasure(); } catch (_) {}
      }
    };

    img.src = this.url;
    img.onload = onLoad;
    wrapper.appendChild(img);

    if (this.onSetWidth) {
      this.rc = createResizeController({
        wrapper,
        getContentWidth: () =>
          outer.getBoundingClientRect().width ||
          Math.max((view?.contentDOM?.offsetWidth ?? 600) - 56, 120),
        onCommit: (w) => {
          if (!view) return;
          const node = imageNodeAtWidget(view, outer, this.intraLineOffset);
          if (node) this.onSetWidth!(node.from, node.to, w);
        },
      });
      wrapper.appendChild(this.rc.handle);
    }

    if (this.link) {
      const href = this.link.href;
      wrapper.classList.add('cm-lp-image-block--linked', 'cm-link-clickable');
      wrapper.setAttribute('data-tooltip', href);
      wrapper.addEventListener('click', (e: MouseEvent) => {
        if (!isFollowChord(e)) return;
        e.preventDefault();
        e.stopPropagation();
        this.onNavigate?.(href);
      });
    }

    outer.appendChild(wrapper);
    return outer;
  }

  destroy() {
    if (this.rc) { this.rc.destroy(); this.rc = null; }
  }

  ignoreEvent() { return false; }
}

/** i18n key per kind. Resolved at render time (not baked into a module const)
 *  so a locale switch is reflected on the next repaint. Moved here (with
 *  `MediaEmbedBlockWidget`) from cm-media-embed.ts — the only consumer. */
const KIND_LABEL_KEYS: Record<MediaEmbedKind, keyof UiStringMap> = {
  folder: 'editmisc.kind.folder',
  app: 'editmisc.kind.app',
  external: 'editmisc.kind.external',
  pdf: 'editmisc.kind.pdf',
  video: 'editmisc.kind.video',
  audio: 'editmisc.kind.audio',
  html: 'editmisc.kind.html',
  '3d': 'editmisc.kind.3d',
  notebook: 'editmisc.kind.notebook',
  table: 'editmisc.kind.table',
  embed: 'editmisc.kind.embed',
};

/** The follow chord as a `.ctx-kbd` chip: the platform's own modifier. */
function modifierChip(): string {
  return isMacPlatform() ? '⌘↩' : 'Ctrl+↩';
}

function labelFor(kind: string): string {
  return t(KIND_LABEL_KEYS[kind as MediaEmbedKind] ?? KIND_LABEL_KEYS.embed);
}

/**
 * MediaEmbedBlockWidget — a resizable, 100%-width placeholder card for
 * non-image embeds.
 */
export interface MediaEmbedBlockWidgetOptions {
  kind: string;
  name: string;
  target: string;
  intraLineOffset: number;
  onSetWidth: (from: number, to: number, w: string | null) => void;
  resizable: boolean;
  width?: string;
  src?: string;
  folder?: FolderCardDetail;
  onFollow?: () => void;
  linkHref?: string;
}

export class MediaEmbedBlockWidget extends WidgetType {
  private rc: ResizeController | null = null;

  private readonly kind: string;
  private readonly name: string;
  private readonly target: string;
  private readonly intraLineOffset: number;
  private readonly onSetWidth: (from: number, to: number, w: string | null) => void;
  private readonly resizable: boolean;
  private readonly width: string | undefined;
  private readonly src: string | undefined;
  private readonly folder: FolderCardDetail | undefined;
  private readonly onFollow: (() => void) | undefined;
  private readonly linkHref: string | undefined;
  private readonly folderKey: string;

  constructor(opts: MediaEmbedBlockWidgetOptions) {
    super();
    this.kind = opts.kind;
    this.name = opts.name;
    this.target = opts.target;
    this.intraLineOffset = opts.intraLineOffset;
    this.onSetWidth = opts.onSetWidth;
    this.resizable = opts.resizable;
    this.width = opts.width;
    this.src = opts.src;
    this.folder = opts.folder;
    this.onFollow = opts.onFollow;
    this.linkHref = opts.linkHref;
    this.folderKey = opts.folder
      ? `${opts.folder.title} ${opts.folder.childCount} ${opts.folder.chips.join(',')}`
      : '';
  }

  eq(other: MediaEmbedBlockWidget): boolean {
    return other.kind === this.kind
      && other.name === this.name
      && other.target === this.target
      && other.src === this.src
      && other.width === this.width
      && other.resizable === this.resizable
      && other.folderKey === this.folderKey
      && !!other.onFollow === !!this.onFollow
      && other.linkHref === this.linkHref;
  }

  private get rowShape(): MediaRowShape {
    if (!this.src) return 'card';
    if (this.kind === 'video') return 'video';
    if (this.kind === 'audio') return 'audio';
    return 'card';
  }

  get estimatedHeight(): number {
    return mediaRowHeight(this.rowShape, {
      src: this.src,
      widthToken: this.width,
      columnWidth: editorColumnWidth(),
    });
  }

  toDOM(view?: EditorView): HTMLElement {
    const outer = document.createElement('div');
    outer.className = 'cm-lp-image-row';

    if (this.src && (this.kind === 'video' || this.kind === 'audio')) {
      const wrapper = document.createElement('div');
      wrapper.className = 'cm-media-embed-player';
      if (this.width) wrapper.style.width = this.width;

      if (this.kind === 'video') {
        const video = document.createElement('video');
        video.className = 'cm-media-embed-video';
        video.controls = true;
        video.preload = 'metadata';
        video.src = this.src;
        const src = this.src;
        video.addEventListener(
          'loadedmetadata',
          () => recordVideoAspect(src, video.videoWidth, video.videoHeight),
          { once: true },
        );
        wrapper.appendChild(video);
      } else {
        const audio = document.createElement('audio');
        audio.className = 'cm-media-embed-audio';
        audio.controls = true;
        audio.preload = 'metadata';
        audio.src = this.src;
        wrapper.appendChild(audio);
      }

      if (this.resizable) {
        this.rc = createResizeController({
          wrapper,
          getContentWidth: () =>
            outer.getBoundingClientRect().width ||
            Math.max((view?.contentDOM?.offsetWidth ?? 600) - 56, 120),
          onCommit: (w) => {
            if (!view) return;
            const node = imageNodeAtWidget(view, outer, this.intraLineOffset);
            if (node) this.onSetWidth(node.from, node.to, w);
          },
          ariaLabel: t('editmisc.resize_embed'),
          classPrefix: 'cm-lp-image',
        });
        wrapper.appendChild(this.rc.handle);
      }

      outer.appendChild(wrapper);
      return outer;
    }

    const card = document.createElement('div');
    card.className = 'cm-media-embed-card';
    if (this.width) card.style.width = this.width;

    const icon = document.createElement('span');
    icon.className = 'cm-media-embed-icon';
    icon.textContent = iconFor(this.kind);

    const label = document.createElement('span');
    label.className = 'cm-media-embed-label';
    label.textContent = labelFor(this.kind);

    const name = document.createElement('span');
    name.className = 'cm-media-embed-name';
    name.textContent = this.folder?.title ?? this.name;

    card.appendChild(icon);
    card.appendChild(label);
    card.appendChild(name);

    if (this.folder) {
      card.classList.add('cm-media-embed-card--folder');
      const meta = document.createElement('div');
      meta.className = 'cm-media-embed-meta';
      if (this.folder.childCount !== null) {
        const count = document.createElement('span');
        count.className = 'cm-media-embed-count';
        if (this.folder.childCount === 0) count.classList.add('cm-media-embed-count--empty');
        count.textContent = this.folder.childCount === 1
          ? t('editmisc.folder.pages_one')
          : t('editmisc.folder.pages_other', { count: String(this.folder.childCount) });
        meta.appendChild(count);
      }
      for (const chip of this.folder.chips) {
        const el = document.createElement('span');
        el.className = 'cm-media-embed-chip';
        el.textContent = chip;
        meta.appendChild(el);
      }
      if (meta.childElementCount) card.appendChild(meta);
    }

    const follow = this.onFollow;
    if (follow) {
      card.classList.add('cm-link-clickable');
      card.addEventListener('click', (e: MouseEvent) => {
        if (!isFollowChord(e)) return;
        if ((e.target as Element | null)?.closest('.cm-lp-image-handle')) {
          e.stopPropagation();
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        follow();
      });
    }

    card.addEventListener('contextmenu', (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (!follow) return;
      openCtxMenu({
        entries: [{
          label: t('editmisc.open_reference'),
          kbd: modifierChip(),
          action: () => follow(),
        }],
        position: { kind: 'point', x: e.clientX, y: e.clientY },
        ariaLabel: t('editmisc.embed_card_menu'),
        logSurface: 'media-embed-card',
      });
    });

    if (this.resizable) {
      this.rc = createResizeController({
        wrapper: card,
        getContentWidth: () =>
          outer.getBoundingClientRect().width ||
          Math.max((view?.contentDOM?.offsetWidth ?? 600) - 56, 120),
        onCommit: (w) => {
          if (!view) return;
          const node = imageNodeAtWidget(view, outer, this.intraLineOffset);
          if (node) this.onSetWidth(node.from, node.to, w);
        },
        ariaLabel: t('editmisc.resize_embed'),
        classPrefix: 'cm-lp-image',
      });
      card.appendChild(this.rc.handle);
    }

    outer.appendChild(card);
    return outer;
  }

  destroy() {
    if (this.rc) { this.rc.destroy(); this.rc = null; }
  }

  ignoreEvent() { return false; }
}
