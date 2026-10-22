/**
 * contents-lens.ts — the contents ruler of a page with `contents: pictures`
 * magnifies in place around the pointer.
 *
 * At rest the ruler is the plain column of dashes. Open, it is a fisheye with
 * one focus model:
 *
 * - The focus `f` (a row index; between two rows it is fractional) starts at
 *   the row under the pointer when it enters, then moves one row for every
 *   `PITCH` px the pointer travels from that point, clamped to the first and
 *   last row. What is picked never depends on what is drawn.
 * - Row heights follow a cosine around `f`, from `OPEN_ROW` to `FOCUS_ROW`, and
 *   the list is shifted so the focus sits under the pointer. Past either end
 *   `f` stops, and the list is drawn for the clamped focus, so it stays put.
 *   A list that would cross the window's edge is held inside it instead (or,
 *   taller than the window, made to fill it); picking is unaffected.
 * - The focused row, `round(f)`, shows its picture after its name, and a click
 *   anywhere on the open list goes to it.
 *
 * Every name is one line, so every row has a known height and nothing is
 * measured but the rows' rest positions. The motion is CSS transitions, which
 * reduced motion turns off. The pure half (`falloff`, `restFocus`, `travel`,
 * `layoutRows`) is unit-tested; the mount below only wires events to it.
 */

/** A dash's row, at rest. */
export const REST_ROW = 20;
/** An open row far from the focus: its name on one line. */
export const OPEN_ROW = 26;
/** The focused row: its name and its picture. */
export const FOCUS_ROW = 74;
/** Rows either side of the focus that grow at all. */
export const RADIUS = 2.2;
/** Pointer travel that moves the focus one row. */
export const PITCH = 34;
/** The pointer must rest this long before the list opens. */
export const INTENT_MS = 80;
/** A pointer that leaves and comes back within this keeps the list open. */
const LEAVE_MS = 160;
/** The fade behind the names reaches this far past the first and last row. */
const VEIL_PAD = 40;
/** Clear space the open list keeps from the window's top and bottom. */
const EDGE = 16;

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v));

/** Weight of a row `distance` rows from the focus: 1 at it, 0 at `radius`. */
export function falloff(distance: number, radius = RADIUS): number {
  const d = Math.abs(distance);
  return d >= radius ? 0 : 0.5 * (1 + Math.cos((Math.PI * d) / radius));
}

/** The focus under a pointer at `y` over `n` rest rows starting at `restTop`. */
export function restFocus(y: number, restTop: number, n: number): number {
  return clamp((y - restTop) / REST_ROW - 0.5, 0, n - 1);
}

/**
 * The focus after the pointer moved from `y0` (where the focus was `f0`) to
 * `y`, and the height the list is drawn for: the pointer's own while the focus
 * is inside the list, the clamped focus's past either end.
 */
export function travel(f0: number, y0: number, y: number, n: number): { f: number; y: number } {
  const f = clamp(f0 + (y - y0) / PITCH, 0, n - 1);
  return { f, y: y0 + (f - f0) * PITCH };
}

export interface LensRow {
  /** The drawn row's top, in the same coordinates as `y`. */
  top: number;
  size: number;
  /** 1 at the focus, 0 from `RADIUS` rows away. */
  weight: number;
}

/**
 * Where each of `n` rows is drawn for focus `f` held at height `y`. The point
 * `f` stands for, between the centres of the two rows around it, is put at
 * `y`, so the row at `round(f)` contains `y`, unless that would take the list
 * past `bounds`: a list that fits is then held inside them, and one taller
 * than they are covers them without a gap, its focused row inside them.
 */
export function layoutRows(f: number, y: number, n: number, bounds?: { top: number; bottom: number }): LensRow[] {
  const weight = Array.from({ length: n }, (_, i) => falloff(i - f));
  const size = weight.map((w) => OPEN_ROW + (FOCUS_ROW - OPEN_ROW) * w);
  const tops: number[] = [];
  let sum = 0;
  for (const s of size) {
    tops.push(sum);
    sum += s;
  }
  const i0 = Math.floor(f);
  const i1 = Math.min(i0 + 1, n - 1);
  const c0 = tops[i0] + size[i0] / 2;
  const c1 = tops[i1] + size[i1] / 2;
  let shift = y - (c0 + (f - i0) * (c1 - c0));
  if (bounds) {
    const a = bounds.top;
    const b = bounds.bottom - sum;
    shift = clamp(shift, Math.min(a, b), Math.max(a, b));
    // Taller than the window, the focused row itself stays on screen.
    const k = Math.round(f);
    shift = clamp(shift, bounds.top - tops[k], bounds.bottom - tops[k] - size[k]);
  }
  return size.map((s, i) => ({ top: shift + tops[i], size: s, weight: weight[i] }));
}

export interface LensOptions {
  nav: HTMLElement;
  links: HTMLElement[];
}

const px = (n: number): string => `${n.toFixed(2)}px`;

/** Wire the lens to a mounted ruler. Returns its disposer. */
export function mountContentsLens({ nav, links }: LensOptions): () => void {
  const n = links.length;
  const veil = document.createElement("span");
  veil.className = "moss-contents-ruler-veil";
  veil.setAttribute("aria-hidden", "true");
  nav.prepend(veil);

  /** The open list, or null at rest. */
  let lens: {
    f: number;
    f0: number;
    y0: number;
    y: number;
    /** The rows' rest tops: the list items, which never move. */
    restTops: number[];
    navTop: number;
  } | null = null;
  let intent = 0;
  let leave = 0;

  function draw(): void {
    if (!lens) return;
    const rows = layoutRows(lens.f, lens.y, n, { top: EDGE, bottom: window.innerHeight - EDGE });
    const focus = Math.round(lens.f);
    const { restTops, navTop } = lens;
    links.forEach((link, i) => {
      link.style.transform = `translateY(${px(rows[i].top - restTops[i])})`;
      link.style.blockSize = px(rows[i].size);
      link.style.setProperty("--moss-contents-ink", rows[i].weight.toFixed(3));
      link.toggleAttribute("data-focus", i === focus);
    });
    const last = rows[n - 1];
    veil.style.transform = `translateY(${px(rows[0].top - VEIL_PAD - navTop)})`;
    veil.style.blockSize = px(last.top + last.size - rows[0].top + 2 * VEIL_PAD);
  }

  function openAt(pointerY: number, focus: number): void {
    lens = {
      f: focus,
      f0: focus,
      y0: pointerY,
      y: pointerY,
      restTops: links.map((link) => link.parentElement!.getBoundingClientRect().top),
      navTop: nav.getBoundingClientRect().top,
    };
    nav.setAttribute("data-lens", "");
    draw();
  }

  function close(): void {
    clearTimeout(intent);
    clearTimeout(leave);
    if (!lens) return;
    lens = null;
    nav.removeAttribute("data-lens");
    veil.removeAttribute("style");
    for (const link of links) {
      link.removeAttribute("style");
      link.removeAttribute("data-focus");
    }
  }

  const restCentre = (i: number): number =>
    links[i].parentElement!.getBoundingClientRect().top + REST_ROW / 2;
  const restTop = (): number => links[0].parentElement!.getBoundingClientRect().top;

  const undo: Array<() => void> = [];
  const on = <K extends keyof HTMLElementEventMap>(
    target: HTMLElement | Window,
    type: K,
    fn: (e: HTMLElementEventMap[K]) => void,
  ): void => {
    target.addEventListener(type, fn as EventListener);
    undo.push(() => target.removeEventListener(type, fn as EventListener));
  };

  const mouse = (e: PointerEvent): boolean => e.pointerType !== "touch";
  on(nav, "pointerenter", (e) => {
    if (!mouse(e)) return;
    clearTimeout(leave);
    if (lens) return;
    const entry = e.clientY;
    clearTimeout(intent);
    intent = window.setTimeout(() => openAt(entry, restFocus(entry, restTop(), n)), INTENT_MS);
  });
  on(nav, "pointermove", (e) => {
    if (!lens || !mouse(e)) return;
    ({ f: lens.f, y: lens.y } = travel(lens.f0, lens.y0, e.clientY, n));
    draw();
  });
  on(nav, "pointerleave", () => {
    clearTimeout(intent);
    if (lens) leave = window.setTimeout(close, LEAVE_MS);
  });
  // A click anywhere on the open list goes to the focused row: re-dispatched
  // on that row's link once the list is closed, so it is an ordinary
  // fragment jump.
  on(nav, "click", (e) => {
    if (!lens) return;
    const target = links[Math.round(lens.f)];
    const own = (e.target as Element).closest("a") === target;
    close();
    if (own) return;
    e.preventDefault();
    target.click();
  });
  on(nav, "focusin", (e) => {
    const i = links.indexOf(e.target as HTMLElement);
    if (i < 0 || !(e.target as HTMLElement).matches(":focus-visible")) return;
    openAt(restCentre(i), i);
  });
  on(nav, "focusout", (e) => {
    if (!nav.contains(e.relatedTarget as Node | null)) close();
  });
  on(nav, "keydown", (e) => {
    if (e.key === "Escape") close();
  });
  on(window, "resize", close);

  return () => {
    close();
    undo.forEach((off) => off());
    veil.remove();
  };
}
