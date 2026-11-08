/**
 * The shell's half of the editing instruments: it can see the preview, so it
 * answers what the editor asks about a picture, applies the editor's live crop,
 * and draws the words box over the preview. It never writes a file; the editor
 * does every write. A crop the editor just wrote is held on the preview until
 * a rebuilt page shows it, so the picture never jumps back to the old crop.
 */

import { host } from "../host";
import {
  HERO_ALIGN, HERO_FRAME, HERO_FRAME_REQUEST, HERO_IMAGE_FOCUS, HERO_WORDS, isLine,
  type HeroAlign, type HeroAlignRequest, type HeroFrame, type HeroFrameRequest, type HeroImageFocus, type HeroWordsAt,
} from '../editor/session/instrument-events';
import { normalizePath } from '../editor/path-domain';
import { t } from "../i18n/index";
import type { NavigationManager } from './navigation-manager';
import { translateIframePoint } from './preview-context-menu';

/** The picture a frame request names: inside the hero that opens on `line`, else inside the element the build annotated with `data-source-line` (or that carries it itself). */
export const pictureSelector = (line: number): string =>
  `[data-source-range^="${line}-"] img, [data-source-line="${line}"] img, img[data-source-line="${line}"]`;

const POSITION = /^\d{1,3}% \d{1,3}%$/;
const SCROLL_IDLE_MS = 150;
/** Rebuilt pages that may land without the held crop (an earlier build finishing) before the hold is let go. */
const HELD_REFRESHES = 3;

const X_WORD: Record<string, number> = { left: 0, center: 50, right: 100 };
const Y_WORD: Record<string, number> = { top: 0, center: 50, bottom: 100 };

/** The point a CSS `object-position` value names, in percent: `top left`, `left`, `20% 62%`. Null for anything else. */
export function pointOfCssPosition(value: string): { x: number; y: number } | null {
  const pct = (w: string) => (/^\d{1,3}(\.\d+)?%$/.test(w) ? Number(w.slice(0, -1)) : null);
  const isX = (w: string) => Object.hasOwn(X_WORD, w);
  const isY = (w: string) => Object.hasOwn(Y_WORD, w);
  const words = value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 1) {
    const [w] = words;
    if (isX(w)) return { x: X_WORD[w], y: 50 };
    if (isY(w)) return { x: 50, y: Y_WORD[w] };
    const p = pct(w);
    return p === null ? null : { x: p, y: 50 };
  }
  if (words.length !== 2) return null;
  let [a, b] = words;
  if ((a !== 'center' && isY(a)) || (b !== 'center' && isX(b))) [a, b] = [b, a];
  const x = isX(a) ? X_WORD[a] : pct(a);
  const y = isY(b) ? Y_WORD[b] : pct(b);
  return x === null || y === null ? null : { x, y };
}

/** The `object-position` an inline style attribute sets, or null. */
const objectPositionOf = (style: unknown): string | null =>
  typeof style === 'string' ? /(?:^|;)\s*object-position\s*:\s*([^;]+)/i.exec(style)?.[1] ?? null : null;

interface Rect { left: number; top: number; width: number; height: number }

/** Which guide a box dropped at `box` is nearest: its left edge to the start, its middle to the centre, its right edge to the end. */
export function nearestSnap(box: { left: number; width: number }, hero: { left: number; width: number }): HeroAlign {
  const d: [HeroAlign, number][] = [
    ['start', Math.abs(box.left - hero.left)],
    ['center', Math.abs(box.left + box.width / 2 - (hero.left + hero.width / 2))],
    ['end', Math.abs(box.left + box.width - (hero.left + hero.width))],
  ];
  return d.reduce((best, cur) => (cur[1] < best[1] ? cur : best))[0];
}

export interface PreviewInstruments {
  /** The previewed page changed under what is drawn over it (a rebuild landed): draw again. */
  refresh(): void;
  destroy(): void;
}

/** `pageFile` is the previewed page's source, project-relative; an instrument acts only for a request naming that file. */
export function bindPreviewInstruments(
  nav: Pick<NavigationManager, 'callBridge'>,
  iframe: HTMLIFrameElement | null,
  pageFile: () => string | null,
): PreviewInstruments {
  const previewed = (path: unknown): boolean => {
    const page = pageFile();
    return typeof path === 'string' && page !== null && normalizePath(path) === normalizePath(page);
  };
  let wordsAt: { path: string; line: number } | null = null;
  let held: { path: string; line: number; position: string; refreshes: number } | null = null;
  /** The inline crop each picture had before the editor restyled it, so "put it back" restores the page's own. */
  const own = new Map<number, string>();
  /** Focus messages apply in order: the first of a gesture reads the page before restyling it. */
  let focusQueue: Promise<void> = Promise.resolve();
  let layer: HTMLElement | null = null;
  let generation = 0;
  let idle: ReturnType<typeof setTimeout> | null = null;

  const rectOf = async (selector: string): Promise<Rect | null> => {
    try { return ((await nav.callBridge('document.querySelector', selector)) as { boundingRect?: Rect } | null)?.boundingRect ?? null; } catch { return null; }
  };
  const clear = () => { layer?.remove(); layer = null; };
  const setFocus = async (line: number, position: string): Promise<void> => {
    try { await nav.callBridge('element.setStyle', pictureSelector(line), 'object-position', position); } catch { /* no bridge, or the picture is not on this page */ }
  };
  const shownStyle = async (line: number): Promise<unknown> => {
    try { return ((await nav.callBridge('document.querySelector', pictureSelector(line))) as { attributes?: Record<string, string> } | null)?.attributes?.style; } catch { return null; }
  };
  async function applyFocus({ path, line, position, held: hold }: HeroImageFocus): Promise<void> {
    if (position === null) {
      if (held?.line === line) held = null;
      await setFocus(line, own.get(line) ?? '');
      own.delete(line);
      return;
    }
    if (!own.has(line)) own.set(line, objectPositionOf(await shownStyle(line)) ?? '');
    held = hold === true ? { path, line, position, refreshes: 0 } : null;
    await setFocus(line, position);
  }

  /** A rebuilt page landed: let the held crop go once the page shows it, and put it back over an older page. */
  async function confirmHeld(): Promise<void> {
    const h = held;
    if (!h) return;
    if (!previewed(h.path)) { held = null; return; }
    const style = await shownStyle(h.line);
    if (held !== h) return;
    const shown = pointOfCssPosition(objectPositionOf(style) ?? '');
    const want = pointOfCssPosition(h.position);
    // Shown, or never going to be: the rebuilt page's crop is now the page's own.
    if ((shown && want && shown.x === want.x && shown.y === want.y) || ++h.refreshes >= HELD_REFRESHES) {
      held = null;
      own.delete(h.line);
      return;
    }
    await setFocus(h.line, h.position);
  }

  async function refresh(): Promise<void> {
    const mine = ++generation;
    const at = wordsAt && previewed(wordsAt.path) ? wordsAt.line : null;
    if (at === null || !iframe) { clear(); return; }
    const hero = `[data-source-range^="${at}-"]`;
    const [heroRect, wordsRect] = await Promise.all([rectOf(hero), rectOf(`${hero} .moss-hero-content`)]);
    if (mine !== generation) return;
    if (heroRect && wordsRect) draw(iframe, at, heroRect, wordsRect); else clear();
  }

  function draw(frameEl: HTMLIFrameElement, at: number, heroRect: Rect, wordsRect: Rect): void {
    clear();
    const frame = frameEl.getBoundingClientRect();
    const place = (r: Rect): Rect => {
      const a = translateIframePoint(frameEl, r.left, r.top);
      const b = translateIframePoint(frameEl, r.left + r.width, r.top + r.height);
      return { left: a.x - frame.left, top: a.y - frame.top, width: b.x - a.x, height: b.y - a.y };
    };
    const hero = place(heroRect);
    const words = place(wordsRect);

    const root = document.createElement('div');
    root.className = 'moss-instrument-layer';
    Object.assign(root.style, { left: `${frame.left}px`, top: `${frame.top}px`, width: `${frame.width}px`, height: `${frame.height}px` });
    const guides = new Map<HeroAlign, HTMLElement>();
    for (const [snap, x] of [['start', hero.left], ['center', hero.left + hero.width / 2], ['end', hero.left + hero.width]] as const) {
      const g = document.createElement('div');
      g.className = 'moss-instrument-guide';
      Object.assign(g.style, { left: `${x}px`, top: `${hero.top}px`, height: `${hero.height}px` });
      guides.set(snap, g);
      root.append(g);
    }
    const box = document.createElement('div');
    box.className = 'moss-instrument-words';
    box.dataset.tooltip = t('instrument.words_hint');
    Object.assign(box.style, { left: `${words.left}px`, top: `${words.top}px`, width: `${words.width}px`, height: `${words.height}px` });
    root.append(box);

    let startX: number | null = null;
    const move = (e: MouseEvent): HeroAlign => {
      const dx = e.clientX - startX!;
      box.style.transform = `translateX(${dx}px)`;
      const snap = nearestSnap({ left: words.left + dx, width: words.width }, hero);
      guides.forEach((g, k) => g.classList.toggle('is-target', k === snap));
      return snap;
    };
    box.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      startX = e.clientX;
      box.setPointerCapture?.(e.pointerId);
    });
    box.addEventListener('pointermove', (e) => { if (startX !== null) move(e); });
    box.addEventListener('pointerup', (e) => {
      if (startX === null) return;
      const align = move(e);
      startX = null;
      guides.forEach((g) => g.classList.remove('is-target'));
      box.style.transform = '';
      host.bus.emit(HERO_ALIGN, { line: at, align } satisfies HeroAlignRequest);
    });
    box.addEventListener('pointercancel', () => { startX = null; box.style.transform = ''; });

    document.body.append(root);
    layer = root;
  }

  // The page moved under the box: hide it and draw it again once the page is still.
  const quiet = () => {
    clear();
    if (idle) clearTimeout(idle);
    idle = setTimeout(() => { idle = null; void refresh(); }, SCROLL_IDLE_MS);
  };
  const onMessage = (e: MessageEvent) => {
    const type = (e.data as { type?: unknown } | null)?.type;
    if (wordsAt && (type === 'moss-scroll-position' || type === 'moss-navigation')) quiet();
  };
  window.addEventListener('message', onMessage);
  window.addEventListener('resize', quiet);

  const offs = [
    host.bus.on<HeroFrameRequest>(HERO_FRAME_REQUEST, async ({ path, line }) => {
      if (!isLine(line) || !previewed(path)) return;
      const rect = await rectOf(pictureSelector(line));
      if (rect && rect.width > 0 && rect.height > 0) host.bus.emit(HERO_FRAME, { line, width: rect.width, height: rect.height } satisfies HeroFrame);
    }),
    host.bus.on<HeroImageFocus>(HERO_IMAGE_FOCUS, (focus) => {
      const { path, line, position } = focus;
      if (!isLine(line) || !previewed(path) || (position !== null && !POSITION.test(position))) return;
      focusQueue = focusQueue.then(() => applyFocus(focus));
    }),
    host.bus.on<HeroWordsAt>(HERO_WORDS, (at) => {
      wordsAt = at && isLine(at.line) && typeof at.path === 'string' ? { path: at.path, line: at.line } : null;
      void refresh();
    }),
  ];

  return {
    refresh: () => { void refresh(); void confirmHeld(); },
    destroy: () => {
      offs.forEach((off) => off());
      window.removeEventListener('message', onMessage);
      window.removeEventListener('resize', quiet);
      if (idle) clearTimeout(idle);
      wordsAt = null;
      held = null;
      own.clear();
      generation++;
      clear();
    },
  };
}
