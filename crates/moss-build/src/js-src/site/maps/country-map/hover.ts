/** Hover previews and the legend lift. Neither moves the map or opens the panel. */
import { media } from "../media";

export interface Point { x: number; y: number }
export interface Size { w: number; h: number }
export interface Rect { x0: number; y0: number; x1: number; y1: number }

const GAP_X = 14;
const GAP_Y = 18;
const EDGE = 8;

/**
 * Where the label goes, in map coordinates: below and right of the pointer,
 * flipped left or up at the map's edges, always inside it, and never over the
 * panel. null when neither side clears the panel.
 */
export function placeLabel(pointer: Point, label: Size, stage: Size, avoid: Rect | null): Point | null {
  const clamp = (v: number, size: number, room: number): number => Math.max(EDGE, Math.min(room - size - EDGE, v));
  const y = clamp(pointer.y + GAP_Y + label.h > stage.h - EDGE ? pointer.y - GAP_X - label.h : pointer.y + GAP_Y, label.h, stage.h);
  const right = { x: clamp(pointer.x + GAP_X, label.w, stage.w), y };
  const left = { x: clamp(pointer.x - GAP_X - label.w, label.w, stage.w), y };
  const covers = (p: Point): boolean => !!avoid && p.x < avoid.x1 && p.x + label.w > avoid.x0 && p.y < avoid.y1 && p.y + label.h > avoid.y0;
  const first = pointer.x + GAP_X + label.w > stage.w - EDGE ? [left, right] : [right, left];
  return first.find((p) => !covers(p)) ?? null;
}

const canHover = (): boolean => media("(hover: hover)");

interface HoverParts {
  stage: HTMLElement;
  svg: SVGSVGElement;
  panel: HTMLElement;
  /** The words after the name: the level, or "Not rated". */
  levelOf(path: SVGPathElement): string;
}

/** A label that follows the pointer over a country; only where the device hovers. */
export function mountHover({ stage, svg, panel, levelOf }: HoverParts): void {
  const tip = document.createElement("div");
  tip.className = "moss-country-tip";
  tip.hidden = true;
  tip.setAttribute("aria-hidden", "true");
  stage.appendChild(tip);
  svg.addEventListener("pointermove", (event) => {
    const e = event as PointerEvent;
    const path = (e.target as Element).closest<SVGPathElement>(".moss-country");
    if (!canHover() || e.pointerType === "touch" || !path) {
      tip.hidden = true;
      return;
    }
    tip.textContent = `${path.dataset.name} · ${levelOf(path)}`;
    tip.hidden = false;
    const s = stage.getBoundingClientRect();
    const p = panel.hidden ? null : panel.getBoundingClientRect();
    const at = placeLabel({ x: e.clientX - s.left, y: e.clientY - s.top }, { w: tip.offsetWidth, h: tip.offsetHeight }, { w: s.width, h: s.height }, p && { x0: p.left - s.left, y0: p.top - s.top, x1: p.right - s.left, y1: p.bottom - s.top });
    if (at) tip.style.transform = `translate(${at.x}px, ${at.y}px)`;
    else tip.hidden = true;
  });
  svg.addEventListener("pointerleave", () => (tip.hidden = true));
}

/** Hovering a legend level lifts every country at that level and dims the rest; the list below the map answers the same for the keyboard. */
export function mountLegend(figure: HTMLElement): void {
  const lift = (level: number): void => {
    figure.classList.toggle("is-lifting", level > 0);
    figure.querySelectorAll<SVGPathElement>(".moss-country").forEach((p) => p.classList.toggle("is-lifted", level > 0 && p.dataset.level === String(level)));
  };
  figure.querySelectorAll<HTMLElement>(".moss-country-legend li").forEach((item, i) => {
    item.addEventListener("mouseenter", () => canHover() && lift(i + 1));
    item.addEventListener("mouseleave", () => lift(0));
  });
}
