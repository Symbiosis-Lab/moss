/**
 * The focus-point instrument: with the caret on a picture the page crops (a
 * hero's, or any image embed line), the whole picture is drawn under that line
 * with a dot on the part that must stay in view and an outline of what the
 * frame keeps at the preview's current width. A picture the page shows whole
 * gets no widget.
 * Dragging the dot redraws the preview live; letting go writes the position
 * into the picture's pipe options as one undoable edit, and the preview keeps
 * showing the written position until the rebuilt page does.
 *
 * The widget exists only while the caret is on the line it edits, and writes
 * only through the editor. The shell is asked for two things: the picture's frame
 * shape, and to crop the preview's picture while the dot moves.
 */

import type { Bus } from "../../host";
import { isolateHistory } from '@codemirror/commands';
import { StateEffect, StateField, Prec, type Extension } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, WidgetType, keymap, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { t } from "../../i18n/index";
import { assetBustQuery } from '../asset-bust';
import { sourceAssetUrl } from '../source-asset-url';
import {
  HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS, isLine,
  type HeroFrame, type HeroFrameRequest, type HeroImageFocus,
} from '../session/instrument-events';
import { pictureAtCaretField, type HeroPicture } from './picture-context';
import { coverRect, pointOfPosition, positionWord, type Point } from './image-position';

/** The slice of a resolved asset the instrument reads. */
export interface InstrumentAsset { request_url: string; mime_type: string; width: number | null; height: number | null }

export interface FocusPointOptions {
  /** The editor file's project-relative path. */
  path: () => string;
  resolveAsset: (target: string) => Promise<InstrumentAsset | null | undefined>;
  host: Bus;
}

interface Asset { target: string; url: string; aspect: number }
interface Frame { line: number; aspect: number }
interface FocusState {
  /** The picture the instrument is open on: under the caret, and not closed with Escape. */
  shown: { frameLine: number; hero: boolean; picture: HeroPicture } | null;
  /** The line Escape closed; the caret has to leave it to bring the instrument back. */
  dismissed: number | null;
  asset: Asset | null;
  frame: Frame | null;
  decos: DecorationSet;
}

const assetEffect = StateEffect.define<Asset>();
const frameEffect = StateEffect.define<Frame>();
const dismissEffect = StateEffect.define<number>();

/** A picture the page shows whole has nothing to crop, so no widget. Until the page has answered, only a hero (always a crop) shows one. */
function cropped(hero: boolean, imageAspect: number, frame: Frame | null): boolean {
  if (!frame) return hero;
  return !imageAspect || Math.abs(frame.aspect / imageAspect - 1) > CROP_TOLERANCE;
}

const CROP_TOLERANCE = 0.01;
const CENTER: Point = { x: 50, y: 50 };

interface Host {
  move(x: number, y: number): void;
  drop(x: number, y: number): void;
}
const hosts = new WeakMap<EditorView, Host>();

class FocusPointWidget extends WidgetType {
  constructor(
    private readonly url: string,
    private readonly imageAspect: number,
    private readonly frameAspect: number | null,
    private readonly at: Point,
  ) { super(); }

  eq(o: FocusPointWidget): boolean {
    return o.url === this.url && o.imageAspect === this.imageAspect && o.frameAspect === this.frameAspect
      && o.at.x === this.at.x && o.at.y === this.at.y;
  }

  ignoreEvent(): boolean { return true; }

  toDOM(view: EditorView): HTMLElement {
    const host = hosts.get(view);
    const row = document.createElement('div');
    row.className = 'cm-focus-point';
    const pic = document.createElement('div');
    pic.className = 'cm-focus-point-pic';
    pic.setAttribute('role', 'img');
    pic.setAttribute('aria-label', t('instrument.focus_aria'));
    let aspect = this.imageAspect || 1.5;
    pic.style.aspectRatio = String(aspect);
    const img = document.createElement('img');
    img.src = this.url;
    img.alt = '';
    img.draggable = false;
    const scrim = document.createElement('div');
    scrim.className = 'cm-focus-point-scrim';
    const frame = document.createElement('div');
    frame.className = 'cm-focus-point-frame';
    const dot = document.createElement('div');
    dot.className = 'cm-focus-point-dot';
    pic.append(img, scrim, frame, dot);
    const hint = document.createElement('div');
    hint.className = 'cm-focus-point-hint';
    hint.textContent = t('instrument.focus_hint');
    row.append(pic, hint);

    const draw = (x: number, y: number) => {
      dot.style.left = `${x}%`;
      dot.style.top = `${y}%`;
      if (this.frameAspect === null) { frame.hidden = true; scrim.hidden = true; return; }
      const r = coverRect(aspect, this.frameAspect, x / 100, y / 100);
      const [l, tp, rt, b] = [r.left * 100, r.top * 100, (r.left + r.width) * 100, (r.top + r.height) * 100];
      Object.assign(frame.style, { left: `${l}%`, top: `${tp}%`, width: `${r.width * 100}%`, height: `${r.height * 100}%` });
      scrim.style.clipPath = `polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, ${l}% ${tp}%, ${l}% ${b}%, ${rt}% ${b}%, ${rt}% ${tp}%, ${l}% ${tp}%)`;
    };
    draw(this.at.x, this.at.y);
    if (!this.imageAspect) {
      img.addEventListener('load', () => {
        if (!img.naturalWidth || !img.naturalHeight) return;
        aspect = img.naturalWidth / img.naturalHeight;
        pic.style.aspectRatio = String(aspect);
        draw(parseFloat(dot.style.left), parseFloat(dot.style.top));
      });
    }

    const pointer = (e: MouseEvent): Point => {
      const r = pic.getBoundingClientRect();
      const clamp = (v: number) => Math.min(100, Math.max(0, v));
      return { x: clamp(((e.clientX - r.left) / (r.width || 1)) * 100), y: clamp(((e.clientY - r.top) / (r.height || 1)) * 100) };
    };
    let dragging = false;
    pic.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      dragging = true;
      pic.setPointerCapture?.(e.pointerId);
      const p = pointer(e);
      draw(p.x, p.y);
      host?.move(p.x, p.y);
    });
    pic.addEventListener('pointermove', (e) => {
      if (!dragging) return;
      const p = pointer(e);
      draw(p.x, p.y);
      host?.move(p.x, p.y);
    });
    const end = (e: PointerEvent) => {
      if (!dragging) return;
      dragging = false;
      const p = pointer(e);
      host?.drop(p.x, p.y);
    };
    pic.addEventListener('pointerup', end);
    pic.addEventListener('pointercancel', end);
    return row;
  }
}

export function focusPointExtension(opts: FocusPointOptions): Extension {
  const field = StateField.define<FocusState>({
    create: () => ({ shown: null, dismissed: null, asset: null, frame: null, decos: Decoration.none }),
    update(prev, tr) {
      if (!tr.docChanged && !tr.selection && !tr.effects.length) return prev;
      const at = tr.state.field(pictureAtCaretField);
      const picture = at?.picture ?? null;
      let { dismissed, asset, frame } = prev;
      if (picture?.line !== dismissed) dismissed = null;
      if (picture?.target !== asset?.target) asset = null;
      if (at?.frameLine !== frame?.line) frame = null;
      for (const e of tr.effects) {
        if (e.is(assetEffect)) asset = e.value;
        else if (e.is(frameEffect)) frame = e.value;
        else if (e.is(dismissEffect)) dismissed = e.value;
      }
      const shown = at && picture && picture.line !== dismissed ? { frameLine: at.frameLine, hero: at.heroLine !== null, picture } : null;
      let decos = Decoration.none;
      if (shown && asset?.target === shown.picture.target && cropped(shown.hero, asset.aspect, frame)) {
        const widget = new FocusPointWidget(asset.url, asset.aspect, frame?.aspect ?? null, shown.picture.position ?? CENTER);
        decos = Decoration.set([Decoration.widget({ widget, block: true, side: 1 }).range(tr.state.doc.line(shown.picture.line).to)]);
      }
      return { shown, dismissed, asset, frame, decos };
    },
    provide: (f) => EditorView.decorations.from(f, (s) => s.decos),
  });

  const controller = ViewPlugin.fromClass(class {
    private ctx: string | null = null;
    private target: string | null = null;
    private frameLine = 0;
    private live: string | null = null;
    private dirty = false;
    private gone = false;
    /** The written crop the shell is holding: its line key, and the end of the picture's line, mapped through later edits. */
    private held: { line: number; to: number } | null = null;
    private readonly off: () => void;

    constructor(private readonly view: EditorView) {
      hosts.set(view, { move: (x, y) => this.move(x, y), drop: (x, y) => this.drop(x, y) });
      this.off = opts.host.on<HeroFrame>(HERO_FRAME, (f) => this.onFrame(f));
      this.sync();
    }

    update(u: ViewUpdate): void {
      if (this.held && u.docChanged) this.letGoIfMoved(u);
      this.sync();
    }

    /**
     * An edit at or above the held picture (an undo, or typing that shifts its
     * line) makes the held crop wrong or its line key stale: let it go, so the
     * shell shows the page's own crop until the rebuild.
     */
    private letGoIfMoved(u: ViewUpdate): void {
      const h = this.held!;
      let hit = false;
      for (const tr of u.transactions) {
        tr.changes.iterChangedRanges((fromA) => { if (fromA <= h.to) hit = true; });
        h.to = tr.changes.mapPos(h.to);
      }
      if (!hit) return;
      this.held = null;
      opts.host.emit(HERO_IMAGE_FOCUS, { path: opts.path(), line: h.line, position: null } satisfies HeroImageFocus);
    }

    destroy(): void {
      this.gone = true;
      this.off();
      hosts.delete(this.view);
      this.release();
    }

    /** Follow the field: ask for the frame when the instrument opens, put the preview back when it closes. */
    private sync(): void {
      const { shown } = this.view.state.field(field);
      const ctx = shown ? `${shown.frameLine}:${shown.picture.line}` : null;
      if (ctx !== this.ctx) {
        this.release();
        this.ctx = ctx;
        this.frameLine = shown?.frameLine ?? 0;
        if (shown) opts.host.emit(HERO_FRAME_REQUEST, { path: opts.path(), line: shown.frameLine } satisfies HeroFrameRequest);
      }
      const target = shown?.picture.target ?? null;
      if (target !== this.target) {
        this.target = target;
        if (target) void this.resolve(target);
      }
    }

    private async resolve(target: string): Promise<void> {
      let asset: InstrumentAsset | null | undefined;
      try { asset = await opts.resolveAsset(target); } catch { return; }
      if (this.gone || this.target !== target || !asset?.mime_type.startsWith('image/')) return;
      const url = sourceAssetUrl(asset.request_url, assetBustQuery(asset.request_url));
      const aspect = asset.width && asset.height ? asset.width / asset.height : 0;
      this.view.dispatch({ effects: assetEffect.of({ target, url, aspect }) });
    }

    private onFrame(f: HeroFrame): void {
      if (this.gone || !isLine(f.line) || f.line !== this.frameLine || !(f.width > 0) || !(f.height > 0)) return;
      this.view.dispatch({ effects: frameEffect.of({ line: f.line, aspect: f.width / f.height }) });
    }

    private setLive(position: string | null, held = false): void {
      this.live = position;
      opts.host.emit(HERO_IMAGE_FOCUS, { path: opts.path(), line: this.frameLine, position, ...(held && { held }) } satisfies HeroImageFocus);
    }

    /** Put the preview's picture back. */
    private release(): void {
      if (this.dirty && this.frameLine) this.setLive(null);
      this.dirty = false;
      this.live = null;
    }

    private move(x: number, y: number): void {
      const position = `${Math.round(x)}% ${Math.round(y)}%`;
      if (position === this.live) return;
      this.dirty = true;
      this.setLive(position);
    }

    private drop(x: number, y: number): void {
      const picture = this.view.state.field(field).shown?.picture;
      const word = positionWord(x, y);
      const next = pointOfPosition(word);
      const now = picture?.position ?? CENTER;
      if (picture && next && (next.x !== now.x || next.y !== now.y)) {
        this.view.dispatch({ changes: picture.edit(word), userEvent: 'input.instrument', annotations: isolateHistory.of('full') });
        // The shell holds the written position past the rebuild; putting the old one back would show it for the whole wait.
        this.setLive(`${next.x}% ${next.y}%`, true);
        this.held = { line: this.frameLine, to: this.view.state.doc.line(picture.line).to };
        this.dirty = false;
      } else {
        this.release();
      }
    }
  });

  const escape = Prec.high(keymap.of([{
    key: 'Escape',
    run: (view) => {
      const shown = view.state.field(field).shown;
      if (!shown) return false;
      view.dispatch({ effects: dismissEffect.of(shown.picture.line) });
      return true;
    },
  }]));

  return [field, controller, escape];
}
