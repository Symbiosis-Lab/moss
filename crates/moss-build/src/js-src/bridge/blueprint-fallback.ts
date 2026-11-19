/**
 * blueprint-fallback.ts — the live blueprint grid over every picture that is
 * waiting. PREVIEW/EDITOR WEBVIEW ONLY.
 *
 * WHAT IT DRAWS ON
 * ----------------
 * `asset-placeholder.ts` catches any <img> or <video> that fails to load, gives
 * it an empty stand-in and tags it `.moss-img-fallback`. This module lays the
 * live grid over it. The grid is the only grid: nothing else paints one.
 *
 * Both are preview-only. The preview server injects the placeholder into <head>
 * and the bridge before </body>; neither reaches a published page, because moss
 * refuses to publish a site with a broken file reference
 * (`refuse_publish`) rather than dressing the hole up for a
 * stranger. So nothing here can change emitted output.
 *
 * Sizing preservation
 * -------------------
 * We do NOT restructure the DOM around the element. Its context sizing selectors
 * are direct-child (`.moss-card-cover > img`, `.moss-collection-cover img`,
 * `.moss-hero img`, `.site-logo`, …), so wrapping/replacing the element would
 * drop the box + aspect-ratio. Instead the <img> stays exactly where it is at
 * `opacity: 0` (still sized by its selectors, still the heal-anchor for
 * `moss-asset-ready`), and we overlay an absolutely-positioned <canvas> host
 * that tracks the <img>'s offset box. Same box, same aspect-ratio, no emitted
 * markup changes. The element stays visible: its stand-in is transparent, and
 * the injected style draws the waiting edge on it.
 *
 * Multi-instance
 * --------------
 * A folder index can carry many missing covers. Each overlay is gated by an
 * IntersectionObserver: the animated rAF (`startBlueprintGrid`, which owns its
 * own particles/rAF/ResizeObserver/pointer listeners per call) runs ONLY while the
 * cover is on-screen and is fully torn down when it scrolls off — so N covers
 * never mean N live rAF loops, only the visible few. Reusing `startBlueprintGrid`
 * keeps ONE source of truth for the animation.
 *
 * Pending derived text
 * ---------------------
 * A `![[file#text]]` embed whose text is still being read renders a
 * `.moss-derived--pending` block. It is a <div>, so it needs no overlay
 * tracking: the canvas host fills the block itself. It shares the visibility
 * gate and teardown with the image placeholders; a block that stops being
 * pending (the text arrived in a morph) drops its grid.
 *
 * prefers-reduced-motion
 * ----------------------
 * The grid still mounts. `startBlueprintGrid` reads the same preference and
 * paints one still frame per size or theme, with no animation loop, so a
 * waiting picture looks the same, at rest.
 */
import { startBlueprintGrid } from "@symbiosis-lab/moss-blueprint-grid";

/** Elements the placeholder has tagged as waiting. */
const FALLBACK_SELECTOR = "img.moss-img-fallback, video.moss-img-fallback";

type Waiting = HTMLImageElement | HTMLVideoElement;

/** A derived-text block still being read, or a link card's picture slot
 *  whose picture is not in hand; the grid fills it. */
const PENDING_SELECTOR = '.moss-derived--pending, .moss-card-cover[data-cover="missing"]';

/** Visibility-gated grid on a canvas host: animates only while on-screen. */
interface Gated {
  container: HTMLDivElement;
  io: IntersectionObserver | null;
  /** Non-null while the grid rAF is running; null while paused/off-screen. */
  stopGrid: (() => void) | null;
}

interface PendingInstance extends Gated {
  block: HTMLElement;
  prevPosition: string;
}

interface Instance extends Gated {
  container: HTMLDivElement;
  ro: ResizeObserver | null;
  onResize: (() => void) | null;
}

function createHost(doc: Document): HTMLDivElement {
  const container = doc.createElement("div");
  container.className = "moss-blueprint-fallback";
  container.setAttribute("aria-hidden", "true");
  container.style.position = "absolute";
  container.style.overflow = "hidden";
  const canvas = doc.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.display = "block";
  canvas.style.width = "100%";
  canvas.style.height = "100%";
  // pointer-events: none on the canvas so the container still receives the
  // pointer and touch movement startBlueprintGrid tracks, and clicks bubble to
  // the card link.
  canvas.style.pointerEvents = "none";
  container.appendChild(canvas);
  return container;
}

/** Run the grid only while `g.container` is on-screen, so N hosts never mean N live rAF loops. */
function gate(g: Gated, isLive: () => boolean, beforeStart?: () => void): void {
  const start = (): void => {
    if (g.stopGrid || !isLive()) return;
    beforeStart?.();
    g.stopGrid = startBlueprintGrid(g.container, { canvas: g.container.querySelector("canvas")! });
  };
  const stop = (): void => {
    g.stopGrid?.();
    g.stopGrid = null;
  };
  if (typeof IntersectionObserver !== "undefined") {
    g.io = new IntersectionObserver((entries) => {
      const last = entries[entries.length - 1];
      if (last && last.isIntersecting) start();
      else stop();
    });
    g.io.observe(g.container);
  } else {
    // No IntersectionObserver (old environment) → run unconditionally.
    start();
  }
}

/**
 * Lay the live grid over every `.moss-img-fallback` in `doc`, and over each
 * pending derived-text block. Returns a teardown that stops every running grid
 * and disconnects all observers.
 */
export function installBlueprintFallback(
  doc: Document = document,
  win: Window = window,
): () => void {
  const instances = new Map<Waiting, Instance>();
  const pending = new Map<HTMLElement, PendingInstance>();

  function dropPending(block: HTMLElement): void {
    const inst = pending.get(block);
    if (!inst) return;
    pending.delete(block);
    inst.stopGrid?.();
    inst.io?.disconnect();
    inst.container.remove();
    block.style.position = inst.prevPosition;
  }

  function upgradePending(block: HTMLElement): void {
    if (pending.has(block) || !block.isConnected) return;
    const container = createHost(doc);
    container.style.inset = "0";
    const prevPosition = block.style.position;
    // The host is absolute, so the block must be its containing block.
    let position = "";
    try {
      position = win.getComputedStyle(block).position;
    } catch {
      /* getComputedStyle may be unavailable in a test stub */
    }
    if (position === "" || position === "static") block.style.position = "relative";
    block.appendChild(container);
    const inst: PendingInstance = {
      block,
      container,
      prevPosition,
      io: null,
      stopGrid: null,
    };
    gate(inst, () => block.isConnected);
    pending.set(block, inst);
  }

  function teardown(el: Waiting): void {
    const inst = instances.get(el);
    if (!inst) return;
    instances.delete(el);
    inst.stopGrid?.();
    inst.io?.disconnect();
    inst.ro?.disconnect();
    if (inst.onResize) win.removeEventListener("resize", inst.onResize);
    inst.container.remove();
  }

  // A restored <img> is still fetching when it leaves the fallback class: keep
  // the grid over it until it has painted, or the box shows empty in between.
  function settle(el: Waiting): void {
    if (!instances.has(el)) return;
    if (el instanceof HTMLImageElement && !el.complete) {
      el.addEventListener("load", () => {
        if (!el.classList.contains("moss-img-fallback")) teardown(el);
      }, { once: true });
      return;
    }
    teardown(el);
  }

  function upgrade(el: Waiting): void {
    if (instances.has(el) || !el.isConnected) return;
    const parent = el.parentElement;
    if (!parent) return;

    // Canvas host, overlaid on the element's box (not a wrapper — see header).
    const container = createHost(doc);
    // Follow any rounding the cover applies to the picture.
    try {
      const radius = win.getComputedStyle(el).borderRadius;
      if (radius && radius !== "0px") container.style.borderRadius = radius;
    } catch {
      /* getComputedStyle may be unavailable in a test stub */
    }
    parent.appendChild(container);

    const inst: Instance = { container, io: null, ro: null, onResize: null, stopGrid: null };

    // container and element are siblings → they share an offsetParent, so the
    // element's offset box is directly reusable as the container's absolute box.
    const sync = (): void => {
      container.style.left = el.offsetLeft + "px";
      container.style.top = el.offsetTop + "px";
      container.style.width = el.offsetWidth + "px";
      container.style.height = el.offsetHeight + "px";
    };
    sync();

    gate(inst, () => el.isConnected, sync);

    // Track the element's box as layout changes (responsive resize, reflow).
    if (typeof ResizeObserver !== "undefined") {
      inst.ro = new ResizeObserver(sync);
      inst.ro.observe(el);
      inst.ro.observe(parent);
    }
    inst.onResize = sync;
    win.addEventListener("resize", inst.onResize);

    instances.set(el, inst);
  }

  function scan(root: ParentNode): void {
    root.querySelectorAll<Waiting>(FALLBACK_SELECTOR).forEach((el) => upgrade(el));
    root
      .querySelectorAll<HTMLElement>(PENDING_SELECTOR)
      .forEach((block) => upgradePending(block));
  }

  // Initial pass — images may have failed before this ran.
  scan(doc);

  // Watch for (a) an <img> gaining `.moss-img-fallback` (error fires async),
  // (b) new fallback <img> inserted (morph re-parse), and (c) removals
  // (morph/navigation) so a running grid is never leaked.
  const mo = new MutationObserver((records) => {
    let removed = false;
    for (const r of records) {
      // The class comes on when a picture fails and off when it is restored.
      if (r.type === "attributes" && (r.target instanceof HTMLImageElement || r.target instanceof HTMLVideoElement)) {
        if (r.target.classList.contains("moss-img-fallback")) upgrade(r.target);
        else settle(r.target);
      }
      if (r.type === "attributes" && r.target instanceof HTMLElement) {
        if (r.target.matches(PENDING_SELECTOR)) upgradePending(r.target);
        else dropPending(r.target);
      }
      for (const node of Array.from(r.addedNodes)) {
        if (node instanceof HTMLImageElement || node instanceof HTMLVideoElement) {
          if (node.classList.contains("moss-img-fallback")) upgrade(node);
        } else if (node instanceof Element) {
          if (node.matches(PENDING_SELECTOR)) upgradePending(node as HTMLElement);
          scan(node);
        }
      }
      if (r.removedNodes.length > 0) removed = true;
    }
    if (removed) {
      for (const [block, inst] of Array.from(pending)) {
        if (inst.container.isConnected && block.isConnected) continue;
        dropPending(block);
        // A morph keeps a still-pending block and prunes the host inside it.
        if (block.isConnected && block.matches(PENDING_SELECTOR)) upgradePending(block);
      }
      for (const [el, inst] of Array.from(instances)) {
        if (!inst.container.isConnected || !el.isConnected) teardown(el);
      }
    }
  });
  mo.observe(doc.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["class", "data-cover"],
  });

  return () => {
    mo.disconnect();
    for (const el of Array.from(instances.keys())) teardown(el);
    for (const block of Array.from(pending.keys())) dropPending(block);
  };
}
