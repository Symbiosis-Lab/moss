/**
 * blueprint-fallback.ts — upgrade static missing-image placeholders to the
 * live blueprint grid. PREVIEW/EDITOR WEBVIEW ONLY.
 *
 * WHAT IT UPGRADES
 * ----------------
 * `asset-placeholder.ts` catches any <img> that fails to load and paints a
 * STATIC blueprint-grid SVG data-URI over it, tagged `.moss-img-fallback`. This
 * module turns that still grid into an animated one.
 *
 * Both are preview-only. The preview server injects the placeholder into <head>
 * and the bridge before </body>; neither reaches a published page, because moss
 * refuses to publish a site with a broken file reference
 * (`refuse_publish`) rather than dressing the hole up for a
 * stranger. So nothing here can change emitted output.
 *
 * Sizing preservation
 * -------------------
 * We do NOT restructure the DOM around the <img>. Its context sizing selectors
 * are direct-child (`.moss-card-cover > img`, `.moss-collection-cover img`,
 * `.moss-hero img`, `.site-logo`, …), so wrapping/replacing the element would
 * drop the box + aspect-ratio. Instead the <img> stays exactly where it is at
 * `opacity: 0` (still sized by its selectors, still the heal-anchor for
 * `moss-asset-ready`), and we overlay an absolutely-positioned <canvas> host
 * that tracks the <img>'s offset box. Same box, same aspect-ratio, no emitted
 * markup changes.
 *
 * Multi-instance
 * --------------
 * A folder index can carry many missing covers. Each overlay is gated by an
 * IntersectionObserver: the animated rAF (`startBlueprintGrid`, which owns its
 * own particles/rAF/ResizeObserver/mousemove per call) runs ONLY while the
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
 * We skip the upgrade entirely; the static SVG blueprint placeholder (itself a
 * still grid) remains — no canvas, no rAF, no motion.
 */
// SHARED MODULE, duplicated on purpose: `./blueprint-grid.ts` in this
// directory is a byte-for-byte copy of the desktop app's own
// `blueprint-grid.ts` component, which the desktop app's own UI
// also imports directly (preview screen, launcher, editor pane
// state). It has no I/O and no desktop-only dependency, so duplicating it
// here is the whole fix for now; the desktop copy should be retired in favor
// of importing this one from the published moss-build crate/package once
// that path exists. Keep the two copies identical by hand until then.
import { startBlueprintGrid } from "./blueprint-grid";

/** Only <img> the shell already tagged as a missing-image fallback. */
const FALLBACK_SELECTOR = "img.moss-img-fallback";

/** A derived-text block still being read; the grid fills it. */
const PENDING_SELECTOR = ".moss-derived--pending";

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
  img: HTMLImageElement;
  container: HTMLDivElement;
  prevOpacity: string;
  ro: ResizeObserver | null;
  onResize: (() => void) | null;
  onLoad: (() => void) | null;
}

function createHost(doc: Document): HTMLDivElement {
  const container = doc.createElement("div");
  container.className = "moss-blueprint-fallback";
  container.setAttribute("aria-hidden", "true");
  container.style.position = "absolute";
  container.style.overflow = "hidden";
  const canvas = doc.createElement("canvas");
  canvas.id = "blueprint-canvas";
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.display = "block";
  canvas.style.width = "100%";
  canvas.style.height = "100%";
  // pointer-events: none on the canvas so the container still receives the
  // mousemove startBlueprintGrid tracks, and clicks bubble to the card link.
  canvas.style.pointerEvents = "none";
  container.appendChild(canvas);
  return container;
}

/** Run the grid only while `g.container` is on-screen, so N hosts never mean N live rAF loops. */
function gate(g: Gated, isLive: () => boolean, beforeStart?: () => void): void {
  const start = (): void => {
    if (g.stopGrid || !isLive()) return;
    beforeStart?.();
    g.stopGrid = startBlueprintGrid(g.container);
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

function prefersReducedMotion(win: Window): boolean {
  try {
    return (
      typeof win.matchMedia === "function" &&
      win.matchMedia("(prefers-reduced-motion: reduce)").matches
    );
  } catch {
    return false;
  }
}

/**
 * Start upgrading `.moss-img-fallback` placeholders in `doc` to the live
 * blueprint grid. Returns a teardown that stops every running grid, restores
 * the hidden <img>s, and disconnects all observers.
 */
export function installBlueprintFallback(
  doc: Document = document,
  win: Window = window,
): () => void {
  // Reduced motion → leave the static SVG blueprint placeholder untouched.
  if (prefersReducedMotion(win)) return () => {};

  const instances = new Map<HTMLImageElement, Instance>();
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

  function teardown(img: HTMLImageElement, restore: boolean): void {
    const inst = instances.get(img);
    if (!inst) return;
    instances.delete(img);
    inst.stopGrid?.();
    inst.io?.disconnect();
    inst.ro?.disconnect();
    if (inst.onResize) win.removeEventListener("resize", inst.onResize);
    if (inst.onLoad) img.removeEventListener("load", inst.onLoad);
    inst.container.remove();
    if (restore) img.style.opacity = inst.prevOpacity;
  }

  function upgrade(img: HTMLImageElement): void {
    if (instances.has(img) || !img.isConnected) return;
    const parent = img.parentElement;
    if (!parent) return;

    // Canvas host, overlaid on the <img>'s box (not a wrapper — see header).
    const container = createHost(doc);
    // Follow any rounding the cover applies to the image.
    try {
      const radius = win.getComputedStyle(img).borderRadius;
      if (radius && radius !== "0px") container.style.borderRadius = radius;
    } catch {
      /* getComputedStyle may be unavailable in a test stub */
    }

    // Keep the <img> as the box-definer + heal-anchor; just hide the static SVG.
    const prevOpacity = img.style.opacity;
    img.style.opacity = "0";
    parent.appendChild(container);

    const inst: Instance = {
      img,
      container,
      prevOpacity,
      io: null,
      ro: null,
      onResize: null,
      onLoad: null,
      stopGrid: null,
    };

    // container and img are siblings → they share an offsetParent, so the
    // img's offset box is directly reusable as the container's absolute box.
    const sync = (): void => {
      container.style.left = img.offsetLeft + "px";
      container.style.top = img.offsetTop + "px";
      container.style.width = img.offsetWidth + "px";
      container.style.height = img.offsetHeight + "px";
    };
    sync();

    gate(inst, () => img.isConnected, sync);

    // Track the <img>'s box as layout changes (responsive resize, reflow).
    if (typeof ResizeObserver !== "undefined") {
      inst.ro = new ResizeObserver(sync);
      inst.ro.observe(img);
      inst.ro.observe(parent);
    }
    inst.onResize = sync;
    win.addEventListener("resize", inst.onResize);

    // Heal: if a real image ever loads into this <img>, restore + drop the grid.
    inst.onLoad = () => {
      const src = img.currentSrc || img.src;
      if (img.naturalWidth > 0 && src && !src.startsWith("data:")) {
        teardown(img, true);
      }
    };
    img.addEventListener("load", inst.onLoad);

    instances.set(img, inst);
  }

  function scan(root: ParentNode): void {
    root
      .querySelectorAll<HTMLImageElement>(FALLBACK_SELECTOR)
      .forEach((img) => upgrade(img));
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
      if (
        r.type === "attributes" &&
        r.target instanceof HTMLImageElement &&
        r.target.classList.contains("moss-img-fallback")
      ) {
        upgrade(r.target);
      }
      if (r.type === "attributes" && r.target instanceof HTMLElement) {
        if (r.target.matches(PENDING_SELECTOR)) upgradePending(r.target);
        else dropPending(r.target);
      }
      for (const node of Array.from(r.addedNodes)) {
        if (node instanceof HTMLImageElement) {
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
      for (const [img, inst] of Array.from(instances)) {
        if (!inst.container.isConnected || !img.isConnected) {
          teardown(img, false);
        }
      }
    }
  });
  mo.observe(doc.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["class"],
  });

  return () => {
    mo.disconnect();
    for (const img of Array.from(instances.keys())) teardown(img, false);
    for (const block of Array.from(pending.keys())) dropPending(block);
  };
}
