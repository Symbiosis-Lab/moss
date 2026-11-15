/**
 * The focus-point instrument. Every picture the page may crop is drawn below
 * its line at all times by the block embeds, so the text never moves when the
 * caret does. With the caret on such a line, the drawn picture carries a dot
 * on the part that must stay in view and an outline of what the frame keeps at
 * the preview's current width. A picture the page shows whole carries neither.
 * Dragging the dot (or moving it with the arrow keys, after Alt-Enter puts the
 * keyboard on it) redraws the preview live and shows the position in the
 * line as it will be written; letting go writes it into the picture's pipe
 * options as one undoable edit, and the preview keeps showing the written
 * position until the rebuilt page does.
 *
 * The dot and outline are laid over the picture and take no room of their own.
 * Writes go only through the editor. The shell is asked for two things: the
 * picture's frame shape, and to crop the preview's picture while the dot moves.
 */

import type { Bus } from "../../host";
import { isolateHistory } from '@codemirror/commands';
import { StateEffect, StateField, Prec, type EditorState, type Extension } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, WidgetType, keymap, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { syntaxTree } from '@codemirror/language';
import { t } from "../../i18n/index";
import type { ResolveImageFn } from '../cm-block-embed-widgets';
import { drawnPictureBelow } from '../cm-block-embeds';
import {
  HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS, isLine,
  type HeroFrame, type HeroFrameRequest, type HeroImageFocus,
} from '../session/instrument-events';
import { pictureAtCaretField, type HeroPicture } from './picture-context';
import { coverRect, pointOfPosition, positionWord, type Point } from './image-position';

export interface FocusPointOptions {
  /** The editor file's project-relative path. */
  path: () => string;
  /** The editor's one cache of resolved pictures. */
  resolveImage?: ResolveImageFn;
  host: Bus;
}

interface Frame { line: number; aspect: number }
/** What the dot and outline are drawn from. */
interface Overlay { line: number; imageAspect: number; frameAspect: number | null; at: Point }
interface FocusState {
  /** The picture the instrument is open on: under the caret, and not closed with Escape. */
  shown: { frameLine: number; hero: boolean; picture: HeroPicture } | null;
  /** The line Escape closed; the caret has to leave it to bring the instrument back. */
  dismissed: number | null;
  /** The line "Set focus point" asked for: its dot is drawn even where this width shows the picture whole, since wider ones crop it. */
  asked: number | null;
  frame: Frame | null;
  overlay: Overlay | null;
  /** The position word of a move in progress, shown in the line and not yet written. */
  live: string | null;
}

const frameEffect = StateEffect.define<Frame>();
/** Put the keyboard on the dot of the picture on this line once it is drawn: the preview menu's "Set focus point". */
export const focusDotEffect = StateEffect.define<number>();
const dismissEffect = StateEffect.define<number>();
const liveEffect = StateEffect.define<string | null>();

class LiveText extends WidgetType {
  constructor(private readonly text: string) { super(); }
  eq(other: LiveText): boolean { return other.text === this.text; }
  toDOM(): HTMLElement {
    const el = document.createElement('span');
    el.textContent = this.text;
    return el;
  }
}

/** The line as the drop would write it: only the characters that differ are replaced, so the rest keeps its own marks. */
function liveText(state: EditorState, shown: FocusState['shown'], live: string | null): DecorationSet {
  if (!shown || live === null) return Decoration.none;
  const { from, to, insert } = shown.picture.edit(live);
  const now = state.doc.sliceString(from, to);
  let head = 0;
  while (head < now.length && head < insert.length && now[head] === insert[head]) head++;
  let tail = 0;
  while (tail < now.length - head && tail < insert.length - head && now[now.length - 1 - tail] === insert[insert.length - 1 - tail]) tail++;
  const text = insert.slice(head, insert.length - tail);
  const [a, b] = [from + head, to - tail];
  if (a === b && !text) return Decoration.none;
  const widget = new LiveText(text);
  return Decoration.set(a === b ? Decoration.widget({ widget, side: 1 }).range(a) : Decoration.replace({ widget }).range(a, b));
}

/** A picture the page shows whole has nothing to crop, so no dot. Until the page has answered, only a hero (always a crop) shows one. */
function cropped(hero: boolean, imageAspect: number, frame: Frame | null): boolean {
  if (!frame) return hero;
  return !imageAspect || Math.abs(frame.aspect / imageAspect - 1) > CROP_TOLERANCE;
}

const CROP_TOLERANCE = 0.01;
const CENTER: Point = { x: 50, y: 50 };
const KEY_STEP: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
const clamp = (v: number) => Math.min(100, Math.max(0, v));

/** The dot and outline laid over a drawn picture. It reports where the dot goes and writes nothing itself. */
class FocusOverlay {
  readonly root = document.createElement('div');
  private readonly scrim = document.createElement('div');
  private readonly frame = document.createElement('div');
  readonly dot = document.createElement('button');
  private shape: Overlay | null = null;
  private at: Point = CENTER;
  /** A move in progress and not yet written: by the pointer, or by the arrow keys. */
  private gesture: 'drag' | 'keys' | null = null;

  constructor(view: EditorView, move: (p: Point) => void, drop: (p: Point) => void) {
    this.root.className = 'cm-focus-point';
    this.scrim.className = 'cm-focus-point-scrim';
    this.frame.className = 'cm-focus-point-frame';
    this.dot.className = 'cm-focus-point-dot';
    this.dot.type = 'button';
    this.dot.setAttribute('role', 'slider');
    this.dot.setAttribute('aria-label', t('instrument.focus_aria'));
    this.root.append(this.scrim, this.frame, this.dot);

    const pointer = (e: MouseEvent): Point => {
      const r = this.root.getBoundingClientRect();
      return { x: clamp(((e.clientX - r.left) / (r.width || 1)) * 100), y: clamp(((e.clientY - r.top) / (r.height || 1)) * 100) };
    };
    const go = (p: Point) => { this.at = p; this.paint(); move(p); };
    this.dot.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      this.gesture = 'drag';
      this.root.classList.add('is-adjusting');
      this.dot.setPointerCapture?.(e.pointerId);
    });
    // The press is the dot's: the editor must not read it as a click on the picture.
    this.dot.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation(); });
    this.dot.addEventListener('pointermove', (e) => { if (this.gesture === 'drag') go(pointer(e)); });
    const end = (e: PointerEvent) => {
      if (this.gesture !== 'drag') return;
      this.gesture = null;
      this.root.classList.remove('is-adjusting');
      drop(pointer(e));
    };
    this.dot.addEventListener('pointerup', end);
    this.dot.addEventListener('pointercancel', end);
    this.dot.addEventListener('keydown', (e) => {
      const step = KEY_STEP[e.key];
      if (step) {
        const by = e.shiftKey ? 10 : 1;
        this.gesture = 'keys';
        this.root.classList.add('is-adjusting');
        go({ x: clamp(Math.round(this.at.x) + step[0] * by), y: clamp(Math.round(this.at.y) + step[1] * by) });
      } else if (e.key === 'Enter' || e.key === 'Escape') {
        // Escape leaves the picture as it was: the blur then drops the dot where it started.
        if (e.key === 'Escape' && this.shape) { this.at = this.shape.at; this.paint(); }
        view.focus();
      } else {
        return;
      }
      e.preventDefault();
      e.stopPropagation();
    });
    this.dot.addEventListener('blur', () => {
      this.root.classList.remove('is-adjusting');
      if (this.gesture !== 'keys') return;
      this.gesture = null;
      drop(this.at);
    });
  }

  /** Lay the overlay over `block` and draw it for `shape`. A gesture in progress keeps its own dot position. */
  show(block: HTMLElement, shape: Overlay): void {
    if (this.root.parentElement !== block) block.append(this.root);
    this.shape = shape;
    if (!this.gesture) this.at = shape.at;
    this.paint();
  }

  hide(): void {
    // First: removing a focused dot blurs it, and that blur must not write.
    this.gesture = null;
    this.root.remove();
    this.root.classList.remove('is-adjusting');
  }

  private paint(): void {
    const { x, y } = this.at;
    this.dot.style.left = `${x}%`;
    this.dot.style.top = `${y}%`;
    this.dot.setAttribute('aria-valuenow', String(Math.round(x)));
    this.dot.setAttribute('aria-valuetext', `${Math.round(x)}% ${Math.round(y)}%`);
    const shape = this.shape;
    const box = this.root.getBoundingClientRect();
    const imageAspect = shape?.imageAspect || (box.height ? box.width / box.height : 0);
    const known = !!shape && shape.frameAspect !== null && imageAspect > 0;
    this.frame.hidden = !known;
    this.scrim.hidden = !known;
    if (!known) return;
    const r = coverRect(imageAspect, shape.frameAspect!, x / 100, y / 100);
    const [l, tp, rt, b] = [r.left * 100, r.top * 100, (r.left + r.width) * 100, (r.top + r.height) * 100];
    Object.assign(this.frame.style, { left: `${l}%`, top: `${tp}%`, width: `${r.width * 100}%`, height: `${r.height * 100}%` });
    this.scrim.style.clipPath = `polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, ${l}% ${tp}%, ${l}% ${b}%, ${rt}% ${b}%, ${rt}% ${tp}%, ${l}% ${tp}%)`;
  }
}

export function focusPointExtension(opts: FocusPointOptions): Extension {
  const field = StateField.define<FocusState>({
    create: () => ({ shown: null, dismissed: null, asked: null, frame: null, overlay: null, live: null }),
    update(prev, tr) {
      const parsed = syntaxTree(tr.state) !== syntaxTree(tr.startState);
      if (!tr.docChanged && !tr.selection && !tr.effects.length && !parsed) return prev;
      const at = tr.state.field(pictureAtCaretField);
      const picture = at?.picture ?? null;
      let { dismissed, asked, frame } = prev;
      let live = tr.docChanged ? null : prev.live;
      if (picture?.line !== dismissed) dismissed = null;
      if (picture?.line !== asked) asked = null;
      if (at?.frameLine !== frame?.line) frame = null;
      for (const e of tr.effects) {
        if (e.is(frameEffect)) frame = e.value;
        else if (e.is(dismissEffect)) dismissed = e.value;
        else if (e.is(focusDotEffect) && e.value === picture?.line) asked = e.value;
        else if (e.is(liveEffect)) live = e.value;
      }
      const shown = at && picture && picture.line !== dismissed ? { frameLine: at.frameLine, hero: at.heroLine !== null, picture } : null;
      const img = shown && opts.resolveImage?.(shown.picture.target);
      const imageAspect = img && img.width && img.height ? img.width / img.height : 0;
      const overlay = shown && img && (cropped(shown.hero, imageAspect, frame) || asked === shown.picture.line)
        ? { line: shown.picture.line, imageAspect, frameAspect: frame?.aspect ?? null, at: shown.picture.position ?? CENTER }
        : null;
      if (!overlay || shown?.picture.line !== prev.shown?.picture.line) live = null;
      return { shown, dismissed, asked, frame, overlay, live };
    },
    provide: (f) => EditorView.decorations.compute([f], (state) => liveText(state, state.field(f).shown, state.field(f).live)),
  });

  const controller = ViewPlugin.fromClass(class {
    private ctx: string | null = null;
    readonly overlay: FocusOverlay;
    private frameLine = 0;
    private live: string | null = null;
    private dirty = false;
    private gone = false;
    /** The line whose dot takes the keyboard when it is drawn; dropped once the caret leaves that picture. */
    private wantDot: number | null = null;
    /** The written crop the shell is holding: its line key, and the end of the picture's line, mapped through later edits. */
    private held: { line: number; to: number } | null = null;
    private readonly off: () => void;

    constructor(private readonly view: EditorView) {
      this.overlay = new FocusOverlay(view, (p) => this.move(p.x, p.y), (p) => this.drop(p.x, p.y));
      this.off = opts.host.on<HeroFrame>(HERO_FRAME, (f) => this.onFrame(f));
      this.sync();
    }

    update(u: ViewUpdate): void {
      if (this.held && u.docChanged) this.letGoIfMoved(u);
      for (const tr of u.transactions) for (const e of tr.effects) if (e.is(focusDotEffect)) this.wantDot = e.value;
      if (u.state.field(field).shown?.picture.line !== this.wantDot) this.wantDot = null;
      this.sync();
    }

    /** Lay the dot over the drawn picture once the editor's DOM is in place. */
    private place(): void {
      this.view.requestMeasure({
        key: this,
        read: (view) => {
          const shape = view.state.field(field).overlay;
          return shape ? { shape, block: drawnPictureBelow(view, view.state.doc.line(shape.line).from) } : null;
        },
        write: (found) => {
          if (this.gone || !found?.block) this.overlay.hide();
          else this.overlay.show(found.block, found.shape);
          if (found?.block && this.wantDot === found.shape.line) {
            this.wantDot = null;
            this.overlay.dot.focus();
          }
        },
      });
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
      this.overlay.hide();
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
      this.place();
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
      this.showInLine(positionWord(x, y));
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
        this.showInLine(null);
      }
    }

    private showInLine(word: string | null): void {
      if (!this.gone && this.view.state.field(field).live !== word) this.view.dispatch({ effects: liveEffect.of(word) });
    }
  });

  const keys = Prec.high(keymap.of([{
    key: 'Escape',
    run: (view) => {
      const shown = view.state.field(field).shown;
      if (!shown) return false;
      view.dispatch({ effects: dismissEffect.of(shown.picture.line) });
      return true;
    },
  }, {
    // Not Mod-Enter, which opens what the caret is on.
    key: 'Alt-Enter',
    run: (view) => {
      const dot = view.plugin(controller)?.overlay.dot;
      if (!dot?.isConnected) return false;
      dot.focus();
      return true;
    },
  }]));

  return [field, controller, keys];
}
