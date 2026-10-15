/**
 * Any content picture of a page opens in the full-screen viewer: figures,
 * lone images, grid-cell pictures and gallery items, in reading order. A
 * picture inside a link, a hero or a `.no-viewer` element is left alone.
 */

import { describe, test, expect, beforeEach, afterEach, afterAll } from "vitest";
import { loadViewer, unloadViewer } from "./fixtures/load-viewer";

const PAGE = `
  <main>
    <figure class="moss-image"><img src="/a.jpg" alt="Harbour"><figcaption>Dusk, <a href="/places/">the quay</a>. Photo: A. Example</figcaption></figure>
    <figure class="moss-image"><img src="/b.jpg" alt=""><figcaption>Caption only</figcaption></figure>
    <p><a href="/page/"><picture><img src="/linked.jpg" alt="Linked"></picture></a></p>
    <div class="moss-hero"><figure class="moss-image"><img src="/hero.jpg" alt="Hero"></figure></div>
    <div class="no-viewer"><figure class="moss-image"><img src="/quiet.jpg" alt="Quiet"></figure></div>
    <figure class="moss-image"><img src="/plain.jpg" alt=""></figure>
    <div class="moss-grid"><div class="moss-grid-card"><figure class="moss-image"><img src="/c.jpg" alt="Cell"><figcaption>In a cell</figcaption></figure></div></div>
    <p><picture><img src="/lone.jpg" alt="Alone"></picture></p>
    <p>A <img src="/inline.jpg" alt="mark"> in a sentence.</p>
    <div class="moss-gallery"><div class="moss-gallery-item"><a class="moss-gallery-link" href="/g.jpg" data-title="Gallery one"><img src="/g.jpg" alt="Gallery one"></a></div><div class="moss-gallery-item"><a class="moss-gallery-link" href="/h.jpg" data-title="Gallery two"><img src="/h.jpg" alt="Gallery two"></a><p class="moss-gallery-caption">Two, <a href="/projects/quay/">Quay Project</a>, 1931</p></div></div>
  </main>
  <div id="lightbox" role="dialog" aria-modal="true" hidden tabindex="-1">
    <button class="lightbox-close">x</button>
    <div class="lightbox-content"><img class="lightbox-image" hidden></div>
    <div class="lightbox-caption"><p class="lightbox-title"></p><a class="lightbox-article-link" href="" hidden>article</a></div>
    <button class="lightbox-nav lightbox-prev">&lsaquo;</button>
    <button class="lightbox-nav lightbox-next">&rsaquo;</button>
  </div>`;

const img = (src: string) => document.querySelector(`img[src="${src}"]`) as HTMLImageElement;
const lightbox = () => document.getElementById("lightbox") as HTMLElement;
const shown = () => (document.querySelector(".lightbox-image") as HTMLImageElement).getAttribute("src");
const title = () => document.querySelector(".lightbox-title") as HTMLElement;
const press = (key: string, init: KeyboardEventInit = {}) =>
  document.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));

const load = (html = PAGE) => loadViewer(html);

afterAll(unloadViewer);

describe("any picture of a page opens in the viewer", () => {
  beforeEach(() => load());
  afterEach(() => {
    if (!lightbox().hidden) press("Escape");
  });

  test("a figure opens with its caption, link and credit intact", () => {
    img("/a.jpg").click();
    expect(lightbox().hidden).toBe(false);
    expect(shown()).toContain("/a.jpg");
    expect(title().textContent).toBe("Dusk, the quay. Photo: A. Example");
    expect(title().querySelector("a")?.getAttribute("href")).toBe("/places/");
  });

  test("arrow keys step through the page's pictures in reading order and wrap", () => {
    img("/a.jpg").click();
    const seen: (string | null)[] = [];
    for (let i = 0; i < 6; i++) {
      press("ArrowRight");
      seen.push(shown());
    }
    expect(seen.map((s) => s!.split("/").pop())).toEqual(["b.jpg", "c.jpg", "lone.jpg", "g.jpg", "h.jpg", "a.jpg"]);
    press("ArrowLeft");
    expect(shown()).toContain("/h.jpg");
  });

  test("the on-screen controls step too", () => {
    img("/b.jpg").click();
    (document.querySelector(".lightbox-next") as HTMLElement).click();
    expect(shown()).toContain("/c.jpg");
    (document.querySelector(".lightbox-prev") as HTMLElement).click();
    expect(shown()).toContain("/b.jpg");
  });

  test("a gallery item joins the same sequence", () => {
    (document.querySelector(".moss-gallery-link") as HTMLElement).click();
    expect(shown()).toContain("/g.jpg");
    press("ArrowRight");
    press("ArrowRight");
    expect(shown()).toContain("/a.jpg");
    expect(title().textContent).toContain("Dusk");
  });

  test("a gallery item's caption, link included, is the viewer's caption; without one the title shows", () => {
    (document.querySelectorAll(".moss-gallery-link")[1] as HTMLElement).click();
    expect(title().textContent).toBe("Two, Quay Project, 1931");
    expect(title().querySelector("a")?.getAttribute("href")).toBe("/projects/quay/");
    press("ArrowLeft");
    expect(title().textContent).toBe("Gallery one");
    expect(title().querySelector("a")).toBeNull();
  });

  test("a picture is a keyboard-operable button named by its alt text or its caption", () => {
    const withAlt = img("/a.jpg");
    expect(withAlt.getAttribute("role")).toBe("button");
    expect(withAlt.tabIndex).toBe(0);
    expect(withAlt.hasAttribute("aria-label")).toBe(false);
    const captionOnly = img("/b.jpg");
    expect(captionOnly.getAttribute("aria-label")).toBe("Caption only");
    captionOnly.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(lightbox().hidden).toBe(false);
    expect(shown()).toContain("/b.jpg");
  });

  test("Escape closes and focus goes back to the picture", () => {
    const opener = img("/c.jpg");
    opener.focus();
    opener.click();
    press("Escape");
    expect(lightbox().hidden).toBe(true);
    expect(document.activeElement).toBe(opener);
  });

  test("a picture announces that it opens a dialog", () => {
    expect(img("/a.jpg").getAttribute("aria-haspopup")).toBe("dialog");
  });

  test("a lone picture with alt text only shows no caption in the viewer", () => {
    img("/lone.jpg").click();
    expect(shown()).toContain("/lone.jpg");
    expect(title().hidden).toBe(true);
    expect(title().textContent).toBe("");
  });

  test("the close button closes", () => {
    img("/a.jpg").click();
    (document.querySelector(".lightbox-close") as HTMLElement).click();
    expect(lightbox().hidden).toBe(true);
  });

  test("Tab stays inside the open viewer", () => {
    img("/a.jpg").click();
    const close = document.querySelector(".lightbox-close") as HTMLElement;
    const next = document.querySelector(".lightbox-next") as HTMLElement;
    next.focus();
    press("Tab");
    expect(document.activeElement).toBe(close);
    press("Tab", { shiftKey: true });
    expect(document.activeElement).toBe(next);
  });

  test("linked, hero, opted-out, unnamed and inline pictures stay plain images", () => {
    const stayOnPage = (e: Event) => e.preventDefault();
    document.addEventListener("click", stayOnPage);
    for (const src of ["/linked.jpg", "/hero.jpg", "/quiet.jpg", "/plain.jpg", "/inline.jpg"]) {
      expect(img(src).hasAttribute("role"), src).toBe(false);
      img(src).click();
      expect(lightbox().hidden, src).toBe(true);
    }
    document.removeEventListener("click", stayOnPage);
  });
});

describe("a page with one picture", () => {
  afterEach(() => {
    if (!lightbox().hidden) press("Escape");
  });

  test("shows no previous or next control", async () => {
    const lone = '<main><figure class="moss-image"><img src="/a.jpg" alt="Harbour"></figure></main>';
    await load(PAGE.replace(/<main>[\s\S]*<\/main>/, lone));
    img("/a.jpg").click();
    expect((document.querySelector(".lightbox-prev") as HTMLElement).hidden).toBe(true);
    expect((document.querySelector(".lightbox-next") as HTMLElement).hidden).toBe(true);
  });
});

describe("a caption copied into the viewer", () => {
  afterEach(() => {
    if (!lightbox().hidden) press("Escape");
  });

  test("keeps no id, so the page and the viewer never share one", async () => {
    await load(
      PAGE.replace("Photo: A. Example", 'Photo: <span id="credit">A. Example</span>')
    );
    img("/a.jpg").click();
    expect(title().querySelector("#credit")).toBeNull();
    expect(title().textContent).toBe("Dusk, the quay. Photo: A. Example");
    expect(document.querySelectorAll("#credit")).toHaveLength(1);
  });
});

describe("a page that changes after the script loaded", () => {
  afterEach(() => {
    if (!lightbox().hidden) press("Escape");
  });

  const figure = (src: string) =>
    `<figure class="moss-image"><img src="${src}" alt="Added"><figcaption>Added later</figcaption></figure>`;

  test("a picture added to the page opens and joins the sequence", async () => {
    await load();
    document.querySelector("main")!.insertAdjacentHTML("beforeend", figure("/late.jpg"));
    await Promise.resolve();
    expect(img("/late.jpg").getAttribute("role")).toBe("button");
    img("/late.jpg").click();
    expect(lightbox().hidden).toBe(false);
    expect(shown()).toContain("/late.jpg");
    expect(title().textContent).toBe("Added later");
    press("ArrowRight");
    expect(shown()).toContain("/a.jpg");
  });

  test("a page that arrives without the viewer's markup gets one on first open", async () => {
    await load('<main><p>Words only.</p></main>');
    expect(document.getElementById("lightbox")).toBeNull();
    document.querySelector("main")!.insertAdjacentHTML("beforeend", figure("/late.jpg"));
    img("/late.jpg").click();
    expect(lightbox().hidden).toBe(false);
    expect(lightbox().getAttribute("role")).toBe("dialog");
    expect(shown()).toContain("/late.jpg");
    (document.querySelector(".lightbox-close") as HTMLElement).click();
    expect(lightbox().hidden).toBe(true);
  });
});
