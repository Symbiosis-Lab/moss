/**
 * contents-lens.ts — the contents ruler of a page with `contents: pictures`
 * opens like a wheel around the pointer.
 *
 * At rest the ruler is the plain column of dashes. Two layers make the
 * opening:
 *
 * - The hit layer is the ruler's own links, which never move vertically: one
 *   20px strip per section, widened sideways into a lane at rest and, while
 *   open, to the far edge of the drawn titles. The pointer's height over them
 *   gives a continuous focus `f` (row `i`'s centre is `f = i`).
 * - The drawn layer is each link's dash, picture and label, translated and
 *   scaled by transform and opacity only. A label is a surface of its own that
 *   shows the whole title (wrapped, never cut) and may reach over the page. Rows near `f` grow to the height a
 *   picture needs; the rest keep their height and are pushed aside, so the
 *   drawn rows tile without gaps and the point under the pointer stays under
 *   it.
 *
 * Because every strip is hit-tested at its rest position, aiming at a row
 * never moves it away: the row drawn under the pointer is the strip's own row.
 * The pure half (`falloff`, `focusAt`, `layoutRows`) is unit-tested; the
 * mount below only wires events to it.
 */

/** A dash's row, at rest. */
export const REST_ROW = 20;
/** Space above and below a picture in its fully open row. The picture's own
 *  size is read from the stylesheet (`--moss-contents-pic`), not repeated here. */
export const PIC_PAD = 8;
/** A row with no picture, fully open: just a little taller than its label. */
export const LABEL_ROW = 24;
/** Rows either side of the focus that are touched at all. */
export const RADIUS = 2.5;
/** The pointer must rest this long before the ruler opens. */
export const INTENT_MS = 80;
/** A label far from the focus keeps this much ink, so every title is legible. */
export const LABEL_FLOOR = 0.55;
/** Below this share of its size (about 5px) a picture reads as a speck beside
 *  its title, so it is not drawn. */
export const PIC_MIN = 0.12;
/** Time constants of the focus smoothing: slow when the pointer is near, fast
 *  when it is far, so it neither lags behind a quick move nor jitters. */
const TAU_SLOW = 90;
const TAU_FAST = 28;
/** The focus gap, in rows, from which the smoothing is fastest. */
const FAR_GAP = 3;
const OPEN_TAU = 70;
/** Within this of its goal an animated value stops: the open share (under 1%
 *  is invisible) and the focus (a small fraction of a row, under a pixel). */
const SNAP_OPEN = 0.005;
const SNAP_FOCUS = 0.002;
/** Clear space kept below the open list. */
const EDGE = 8;

/** Weight of a row `distance` rows from the focus: 1 at it, 0 at `radius`. */
export function falloff(distance: number, radius = RADIUS): number {
  const d = Math.abs(distance);
  return d >= radius ? 0 : 0.5 * (1 + Math.cos((Math.PI * d) / radius));
}

export interface Strip {
  top: number;
  height: number;
}

/**
 * The focus for a pointer height over contiguous strips: the row index plus
 * the fraction through the strip, measured from the row's centre. Above the
 * first strip it is -0.5 and below the last it is `n - 0.5`.
 */
export function focusAt(y: number, strips: Strip[]): number {
  const n = strips.length;
  if (y <= strips[0].top) return -0.5;
  const end = strips[n - 1];
  if (y >= end.top + end.height) return n - 0.5;
  const i = strips.findIndex((s) => y < s.top + s.height);
  return i + (y - strips[i].top) / strips[i].height - 0.5;
}

export interface LensInput {
  /** Focus, as `focusAt` returns it. Ignored when `all` is set. */
  f: number;
  /** 0 closed .. 1 open. */
  open: number;
  /** Open every row at once, centred on the list (reduced motion). */
  all?: boolean;
  /** Each row's height when fully open. */
  heights: number[];
  /** The least each row may be drawn, once open, wherever the focus is: the
   *  height of its title, which is shown in full on every row. Defaults to
   *  `REST_ROW`, which adds nothing. */
  floors?: number[];
  /** Top of the first strip. */
  restTop: number;
  /** The open list stays between these. */
  minY: number;
  maxY: number;
}

export interface LensRow {
  /** Translation of the row's centre from its rest position. */
  dy: number;
  /** Share of full size: scales the picture and drives the label. */
  m: number;
}

/**
 * Where each row is drawn. A row's growth is its open height over `REST_ROW`
 * times its weight, but never less than its floor (a title of two lines needs
 * its height even far from the focus); the rows before it add up to its
 * offset, and the offset at the focus is taken back off every row so the point
 * under the pointer stays put. The growth above the floors is capped so the
 * whole list fits between `minY` and `maxY`, and the list is nudged back
 * inside them where it still reaches past an end.
 */
export function layoutRows(input: LensInput): LensRow[] {
  const { f, open, all, heights, restTop, minY, maxY } = input;
  const n = heights.length;
  const weight = heights.map((_, j) => (all ? 1 : falloff(j - f)) * open);
  const base = heights.map((_, j) => Math.max(0, (input.floors?.[j] ?? REST_ROW) - REST_ROW) * open);
  const extra = heights.map((h, j) => Math.max(0, Math.max(0, h - REST_ROW) * weight[j] - base[j]));
  const total = extra.reduce((a, b) => a + b, 0);
  const room = Math.max(0, maxY - minY - n * REST_ROW - base.reduce((a, b) => a + b, 0));
  const k = total > room ? room / total : 1;
  const grow = extra.map((g, j) => base[j] + g * k);

  const before: number[] = [];
  let sum = 0;
  for (let j = 0; j < n; j++) {
    before.push(sum);
    sum += grow[j];
  }
  let anchor = sum / 2;
  if (!all) {
    const t = Math.min(n, Math.max(0, f + 0.5));
    const i = Math.min(n - 1, Math.floor(t));
    anchor = before[i] + (t - i) * grow[i];
  }
  let shift = before.map((b) => b - anchor);

  const top = restTop + shift[0];
  const bottom = restTop + n * REST_ROW + shift[n - 1] + grow[n - 1];
  let nudge = Math.max(0, minY - top);
  nudge -= Math.max(0, bottom + nudge - maxY);
  shift = shift.map((s) => s + nudge);

  return shift.map((s, j) => ({ dy: s + grow[j] / 2, m: weight[j] * k }));
}

export interface LensOptions {
  nav: HTMLElement;
  links: HTMLElement[];
  /** The space the island's bar takes at the top of the window. */
  clearance: () => number;
}

const px = (n: number): string => `${n.toFixed(2)}px`;

/** Wire the lens to a mounted ruler. Returns its disposer. */
export function mountContentsLens(opts: LensOptions): () => void {
  const { nav, links } = opts;
  const rows = links.map((link) => ({
    link,
    dash: link.querySelector<HTMLElement>(".moss-contents-ruler-dash")!,
    label: link.querySelector<HTMLElement>(".moss-contents-ruler-label")!,
    pic: link.querySelector<HTMLElement>(".moss-contents-ruler-pic"),
  }));
  const reduced = (): boolean => !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

  let strips: Strip[] = [];
  let pointerY: number | null = null;
  let hovering = false;
  let focused: number | null = null;
  let intent = 0;
  let frame = 0;
  let last = 0;
  let open = 0;
  let f = 0;
  let picRow = LABEL_ROW;
  let titleHeights: number[] = [];
  let minY = 0;
  let reach = 0;

  const measure = (): void => {
    const frame = rows.find((r) => r.pic)?.pic;
    picRow = frame ? frame.offsetHeight + PIC_PAD : LABEL_ROW;
    minY = opts.clearance();
    titleHeights = rows.map(({ label }) => label.offsetHeight);
    reach = Math.max(...rows.map(({ label }) => label.getBoundingClientRect().right)) - links[0].getBoundingClientRect().left;
    strips = links.map((link) => {
      const box = link.getBoundingClientRect();
      return { top: box.top, height: box.height };
    });
  };
  const wants = (): boolean => hovering || focused !== null;
  const target = (): number => (pointerY !== null && hovering ? focusAt(pointerY, strips) : (focused ?? 0));

  function clear(): void {
    nav.removeAttribute("data-lens");
    nav.style.removeProperty("--moss-contents-reach");
    for (const { link, dash, label, pic } of rows) {
      for (const el of [dash, label, pic]) el?.removeAttribute("style");
      link.removeAttribute("data-lit");
    }
  }

  function draw(all: boolean): void {
    const heights = rows.map(({ link }) => (link.hasAttribute("data-pic") ? picRow : LABEL_ROW));
    const layout = layoutRows({
      f,
      open,
      all,
      heights,
      floors: titleHeights,
      restTop: strips[0].top,
      minY,
      maxY: window.innerHeight - EDGE,
    });
    nav.setAttribute("data-lens", "");
    nav.style.setProperty("--moss-contents-reach", px(reach));
    rows.forEach(({ link, dash, label, pic }, j) => {
      const { dy, m } = layout[j];
      const move = `translateY(${px(dy)})`;
      const lit = !!pic && link.hasAttribute("data-pic") && m > PIC_MIN;
      dash.style.transform = move;
      label.style.transform = move;
      // The surface follows the opening alone; only the ink fades with
      // distance, so the page never shows through a title's own surface.
      label.style.opacity = String(open);
      label.style.setProperty("--moss-contents-ink", String(LABEL_FLOOR + (1 - LABEL_FLOOR) * m));
      if (pic) {
        pic.style.transform = `${move} scale(${m.toFixed(3)})`;
        pic.style.opacity = String(m);
        pic.style.visibility = lit ? "visible" : "hidden";
      }
      link.toggleAttribute("data-lit", lit);
    });
  }

  function step(now: number): void {
    frame = 0;
    const dt = Math.min(50, now - (last || now));
    last = now;
    const want = wants();
    const goal = target();
    if (want && open === 0) f = goal;
    open += ((want ? 1 : 0) - open) * (1 - Math.exp(-dt / OPEN_TAU));
    if (Math.abs((want ? 1 : 0) - open) < SNAP_OPEN) open = want ? 1 : 0;
    const gap = Math.abs(goal - f);
    const tau = TAU_SLOW + (TAU_FAST - TAU_SLOW) * Math.min(1, gap / FAR_GAP);
    f = Math.abs(goal - f) < SNAP_FOCUS ? goal : f + (goal - f) * (1 - Math.exp(-dt / tau));
    if (!want && open === 0) {
      clear();
      last = 0;
      return;
    }
    draw(false);
    if (open !== (want ? 1 : 0) || f !== goal) frame = requestAnimationFrame(step);
    else last = 0;
  }

  /** Bring the drawing up to date: at once without motion, else on the next frame. */
  function update(): void {
    if (reduced()) {
      cancelAnimationFrame(frame);
      frame = 0;
      if (wants()) {
        open = 1;
        draw(true);
      } else {
        open = 0;
        clear();
      }
      return;
    }
    if (!frame) frame = requestAnimationFrame(step);
  }

  const listeners: Array<() => void> = [];
  const on = <K extends keyof HTMLElementEventMap>(type: K, fn: (e: HTMLElementEventMap[K]) => void): void => {
    nav.addEventListener(type, fn);
    listeners.push(() => nav.removeEventListener(type, fn));
  };
  const onResize = (): void => {
    if (strips.length === 0) return;
    measure();
    if (wants()) update();
  };
  window.addEventListener("resize", onResize);
  listeners.push(() => window.removeEventListener("resize", onResize));

  const mouse = (e: PointerEvent): boolean => e.pointerType !== "touch";
  on("pointerenter", (e) => {
    if (!mouse(e)) return;
    measure();
    pointerY = e.clientY;
    clearTimeout(intent);
    intent = window.setTimeout(() => {
      hovering = true;
      update();
    }, reduced() ? 0 : INTENT_MS);
  });
  on("pointermove", (e) => {
    if (!mouse(e) || strips.length === 0) return;
    pointerY = e.clientY;
    if (hovering) update();
  });
  on("pointerleave", () => {
    clearTimeout(intent);
    hovering = false;
    pointerY = null;
    update();
  });
  on("focusin", (e) => {
    const i = links.indexOf(e.target as HTMLElement);
    if (i < 0 || !(e.target as HTMLElement).matches(":focus-visible")) return;
    measure();
    focused = i;
    update();
  });
  on("focusout", () => {
    focused = null;
    update();
  });

  return () => {
    clearTimeout(intent);
    cancelAnimationFrame(frame);
    listeners.forEach((off) => off());
    clear();
  };
}
