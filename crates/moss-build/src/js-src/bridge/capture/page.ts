/**
 * capture/page.ts - the page's own pixels, as a bitmap
 *
 * A browser will not hand a page its own rendering, so the page is
 * re-rendered by the same engine: its DOM is cloned, its images and
 * stylesheets inlined as data URLs, the clone drawn as an SVG foreignObject
 * image, and the images composited on a canvas at the rects they occupy live.
 * Same engine, same fonts, same layout as the live page. The result is an
 * untainted canvas (a blob: URL would taint it in Chromium; a data: URL does
 * not).
 *
 * Nothing runs until `capturePage` is called, and nothing it waits for may
 * wait forever: every fetch, the font wait and the SVG decode share one
 * budget (FETCH_MS), after which the capture goes ahead without the resource.
 *
 * There is a second copy of this pipeline in the landing page
 * (`site/index.html`: `fold`, `raster`, `rasterDoc`, `toData`, `inlineCss`),
 * which captures other documents (frames) rather than its own and has options
 * this one does not need (video frames, live canvases, drawing images under
 * the chrome). The two are deliberately not unified yet; a later change can
 * point the landing at this module. Differences from that copy, each measured
 * against a real render in Chromium and WebKit:
 *
 * - `body` is not given a height. The copy above pins it to the viewport
 *   height, which is wrong whenever the live body is not that tall: a flex-column
 *   body then shrinks a clipped (`overflow: hidden`, min-height 0) full-bleed hero
 *   to nothing, and a body sized by margins inside a flex `html` (the vertical
 *   starter's horizontally scrolling card strip) comes out taller than live, so
 *   its cards and everything below them are displaced. `html` alone gets the
 *   viewport height, which is what percentage and stretch sizing resolve against
 *   live.
 * - Every image that is composited is hidden in the base layer, so the base
 *   never shows a second, differently laid out copy of it.
 */

const FETCH_MS = 2500;

const capped = <T>(p: Promise<T>, ms: number, msg: string): Promise<T> => {
  let t: ReturnType<typeof setTimeout>;
  return Promise.race([
    p,
    new Promise<never>((_, no) => {
      t = setTimeout(() => no(new Error(msg)), ms);
    }),
  ]).finally(() => clearTimeout(t));
};

const fetchT = (url: string): Promise<Response> =>
  fetch(url, { signal: AbortSignal.timeout(FETCH_MS) });

// Successes are kept for good (the page's resources do not change under a
// capture); a failure is dropped so the next capture tries again.
const dataCache = new Map<string, Promise<string>>();
const cssCache = new Map<string, Promise<string>>();

function cached(cache: Map<string, Promise<string>>, key: string, get: () => Promise<string>, fallback: string): Promise<string> {
  const hit = cache.get(key);
  if (hit) return hit;
  const p = get().catch(() => {
    cache.delete(key);
    return fallback;
  });
  cache.set(key, p);
  return p;
}

const toData = (url: string): Promise<string> =>
  url.startsWith("data:")
    ? Promise.resolve(url)
    : cached(
        dataCache,
        url,
        () =>
          fetchT(url)
            .then((r) => r.blob())
            .then(
              (bl) =>
                new Promise<string>((ok) => {
                  const fr = new FileReader();
                  fr.onload = () => ok(String(fr.result));
                  fr.readAsDataURL(bl);
                })
            ),
        url
      );

const cssText = (href: string): Promise<string> =>
  cached(cssCache, href, () => fetchT(href).then((r) => r.text()), "");

async function inlineCss(css: string, base: string): Promise<string> {
  const urls = [
    ...new Set(
      [...css.matchAll(/url\((['"]?)([^'")]+)\1\)/g)]
        .map((m) => m[2])
        .filter((u) => !u.startsWith("data:") && !u.startsWith("#"))
    ),
  ];
  const got = await Promise.all(
    urls.map((u) => {
      let abs: string;
      try {
        abs = new URL(u, base).href;
      } catch {
        return null;
      }
      return toData(abs).then((d) => [u, d] as const);
    })
  );
  for (const g of got) if (g) css = css.split(g[0]).join(g[1]);
  return css;
}

// Safari draws a box-shadow or backdrop-filter inside a foreignObject image as
// a hard offset block, and the live page paints them again anyway.
const FLAT =
  "*, *::before, *::after { box-shadow: none !important; text-shadow: none !important; filter: none !important; backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }";

const rectInFrame = (vw: number, vh: number, r: DOMRect): boolean =>
  r.bottom > 0 && r.top < vh && r.right > 0 && r.left < vw;

/** The document's element tree, cloned to render standalone. */
async function fold(doc: Document): Promise<HTMLElement> {
  const liveRoot = doc.documentElement;
  const root = liveRoot.cloneNode(true) as HTMLElement;
  const live = [liveRoot, ...Array.from(liveRoot.querySelectorAll("*"))];
  const copy = [root, ...Array.from(root.querySelectorAll("*"))] as HTMLElement[];
  const drop: Element[] = [];
  const work: Promise<unknown>[] = [];
  const vp = doc.scrollingElement || liveRoot;
  const vw = vp.clientWidth;
  const vh = vp.clientHeight;
  for (let i = 0; i < live.length; i++) {
    const L = live[i] as HTMLElement;
    const C = copy[i];
    if (!C) break;
    const tag = L.tagName;
    if (
      tag === "SCRIPT" || tag === "NOSCRIPT" || tag === "META" || tag === "TITLE" ||
      (tag === "LINK" && (L as HTMLLinkElement).rel !== "stylesheet")
    ) {
      drop.push(C);
      continue;
    }
    // The <img> already carries the browser's own pick via currentSrc, and a
    // <source>'s relative srcset would resolve against the wrong base once
    // serialized.
    if (tag === "SOURCE") {
      drop.push(C);
      continue;
    }
    if (tag === "IFRAME" || tag === "CANVAS") {
      const hole = doc.createElement("div");
      hole.setAttribute("style", `display:block;width:${L.clientWidth}px;height:${L.clientHeight}px;` + (L.getAttribute("style") || ""));
      if (L.id) hole.id = L.id;
      hole.className = L.className;
      C.replaceWith(hole);
      continue;
    }
    if (tag === "VIDEO") {
      C.remove();
      continue;
    }
    if (tag === "IMG") {
      const im = L as HTMLImageElement;
      C.removeAttribute("srcset");
      C.removeAttribute("loading");
      const src = im.currentSrc || im.src;
      const loaded = im.complete && im.naturalWidth > 0;
      if (loaded) C.style.opacity = "1";
      // Off-screen images are not read at all; a bare src left on the clone
      // would be fetched for real by the foreignObject.
      if (!rectInFrame(vw, vh, im.getBoundingClientRect())) {
        C.removeAttribute("src");
        continue;
      }
      // Composited from the live pixels afterwards (WebKit can omit a decoded
      // <img> inside a foreignObject), so the base layer keeps only the box.
      if (loaded) C.style.visibility = "hidden";
      if (src) work.push(toData(src).then((d) => C.setAttribute("src", d)));
      continue;
    }
    if (tag === "STYLE") {
      work.push(inlineCss(L.textContent || "", doc.baseURI).then((css) => { C.textContent = css; }));
      continue;
    }
    if (tag === "LINK") {
      const st = doc.createElement("style");
      C.replaceWith(st);
      const href = (L as HTMLLinkElement).href;
      work.push(cssText(href).then((css) => inlineCss(css, href)).then((css) => { st.textContent = css; }));
      continue;
    }
    if (L.style && L.style.backgroundImage && L.style.backgroundImage.includes("url(")) {
      work.push(inlineCss(L.style.backgroundImage, doc.baseURI).then((v) => { C.style.backgroundImage = v; }));
    }
  }
  await Promise.all(work);
  drop.forEach((n) => n.remove());
  // A `--` inside a comment breaks the XML serialization.
  const tw = doc.createTreeWalker(root, NodeFilter.SHOW_COMMENT);
  const comments: Node[] = [];
  while (tw.nextNode()) comments.push(tw.currentNode);
  comments.forEach((c) => c.parentNode?.removeChild(c));
  return root;
}

/** An element tree (plus the stylesheets it needs) at w x h, drawn into an Image. */
async function raster(node: HTMLElement, w: number, h: number): Promise<HTMLImageElement> {
  // The engine must have the faces before it lays the text out; a face that
  // never arrives gets the fallback, which is off-brand, not broken.
  await capped(document.fonts ? document.fonts.ready : Promise.resolve(), FETCH_MS, "fonts timed out").catch(() => {});
  const host = document.createElement("div");
  host.setAttribute("xmlns", "http://www.w3.org/1999/xhtml");
  host.setAttribute("style", `position:relative;width:${w}px;height:${h}px;overflow:hidden;`);
  host.appendChild(node);
  const svgDoc = new DOMParser().parseFromString('<svg xmlns="http://www.w3.org/2000/svg"/>', "image/svg+xml");
  host.querySelectorAll("style").forEach((st) => {
    const css = st.textContent || "";
    st.textContent = "";
    st.appendChild(svgDoc.createCDATASection(css.replace(/]]>/g, "]]]]><![CDATA[>")));
  });
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><foreignObject width="100%" height="100%">${new XMLSerializer().serializeToString(host)}</foreignObject></svg>`;
  const img = new Image();
  await capped(
    new Promise<void>((ok, no) => {
      img.onload = () => ok();
      img.onerror = () => no(new Error("scene raster failed"));
      img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
    }),
    FETCH_MS,
    "scene raster timed out"
  );
  return img;
}

/**
 * The document as pixels, at its own current viewport size and scroll
 * position, one canvas pixel per CSS pixel.
 */
export async function capturePage(doc: Document = document): Promise<HTMLCanvasElement> {
  const win = doc.defaultView!;
  const w = win.innerWidth;
  const h = win.innerHeight;
  const se = doc.scrollingElement || doc.documentElement;
  const html = await fold(doc);
  // Inside the SVG image `:root` is the <svg>, so the document's `:root`
  // rules would land on the wrong element.
  html.querySelectorAll("style").forEach((st) => {
    st.textContent = (st.textContent || "").replace(/:root\b/g, "html");
  });
  html.style.width = (se.clientWidth || w) + "px";
  html.style.height = h + "px";
  html.style.overflow = "visible";
  // No body height: see the module comment. Overflow is visible because the
  // body's own would otherwise make it a scroll container inside the
  // foreignObject (there is no viewport to propagate to) and a scrollbar would
  // steal width from everything it centres.
  const body = html.querySelector("body") as HTMLElement | null;
  if (body) body.style.overflow = "visible";
  const flat = doc.createElement("style");
  flat.textContent = FLAT;
  html.appendChild(flat);
  const shift = document.createElement("div");
  shift.setAttribute("style", `position:absolute;left:${-se.scrollLeft}px;top:${-se.scrollTop}px;width:${w}px;height:${h}px;`);
  shift.appendChild(html);
  const base = await raster(shift, w, h);

  const result = document.createElement("canvas");
  result.width = w;
  result.height = h;
  const g = result.getContext("2d")!;
  g.drawImage(base, 0, 0);
  // Clip to the element's own box and every ancestor that clips it.
  const clipTo = (node: Element) => {
    g.beginPath();
    g.rect(0, 0, w, h);
    g.clip();
    for (let el: Element | null = node; el && el !== doc.body; el = el.parentElement) {
      const style = win.getComputedStyle(el);
      if (el === node || /hidden|clip|auto|scroll/.test(style.overflow)) {
        const r = el.getBoundingClientRect();
        g.beginPath();
        g.roundRect(r.x, r.y, r.width, r.height, parseFloat(style.borderRadius) || 0);
        g.clip();
      }
    }
  };
  const images = Array.from(doc.images).filter(
    (im) => im.complete && im.naturalWidth && rectInFrame(w, h, im.getBoundingClientRect())
  );
  for (const im of images) {
    const r = im.getBoundingClientRect();
    const css = win.getComputedStyle(im);
    if (css.visibility === "hidden" || css.display === "none" || !r.width || !r.height) continue;
    g.save();
    clipTo(im);
    const fit = css.objectFit;
    const scale = fit === "contain"
      ? Math.min(r.width / im.naturalWidth, r.height / im.naturalHeight)
      : Math.max(r.width / im.naturalWidth, r.height / im.naturalHeight);
    const dw = fit === "fill" ? r.width : im.naturalWidth * scale;
    const dh = fit === "fill" ? r.height : im.naturalHeight * scale;
    const pos = css.objectPosition.split(/\s+/).map((v) => (v.endsWith("%") ? parseFloat(v) / 100 : 0.5));
    g.drawImage(im, r.x + (r.width - dw) * (pos[0] ?? 0.5), r.y + (r.height - dh) * (pos[1] ?? 0.5), dw, dh);
    g.restore();
  }
  return result;
}
