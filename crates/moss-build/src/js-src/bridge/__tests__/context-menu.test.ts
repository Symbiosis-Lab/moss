/**
 * Tests for the preview's right-click context reporter (bridge side).
 *
 * The bridge suppresses the native WebKit menu and posts a
 * `moss-context-menu` payload to the shell: pointer position in
 * iframe-viewport px plus what was under the cursor — resolved source
 * target, link href, image src, selection text. The shell renders the menu;
 * nothing here draws anything.
 */
import { describe, it, expect, vi } from "vitest";
import {
  buildContextPayload,
  isEditableTarget,
  installContextMenu,
  type PreviewContextPayload,
} from "../context-menu";

const PAGE = "http://localhost:4832/posts/hello/?__moss_shell=1";

function payloadFor(
  el: Element | null,
  overrides: Partial<Parameters<typeof buildContextPayload>[1]> = {},
): PreviewContextPayload {
  return buildContextPayload(el, {
    x: 10,
    y: 20,
    pageHref: PAGE,
    selection: null,
    ...overrides,
  });
}

describe("buildContextPayload", () => {
  it("carries the message type, coordinates and page URL", () => {
    const p = payloadFor(null);
    expect(p.type).toBe("moss-context-menu");
    expect(p.x).toBe(10);
    expect(p.y).toBe(20);
    expect(p.page).toBe(PAGE);
  });

  it("resolves a relative link href against the document URL", () => {
    document.body.innerHTML = '<a href="../other/"><span id="t">x</span></a>';
    const p = payloadFor(document.getElementById("t"));
    expect(p.link).toBe("http://localhost:4832/posts/other/");
  });

  it("passes an absolute external link through verbatim", () => {
    document.body.innerHTML = '<a id="t" href="https://example.com/a">x</a>';
    const p = payloadFor(document.getElementById("t"));
    expect(p.link).toBe("https://example.com/a");
  });

  it.each(["#frag", "javascript:void(0)", "data:text/plain,x", "vbscript:x"])(
    "reports no link for scheme/fragment href %s",
    (href) => {
      document.body.innerHTML = `<a id="t" href="${href}">x</a>`;
      const p = payloadFor(document.getElementById("t"));
      expect(p.link).toBeNull();
    },
  );

  it("reports the image src resolved against the document URL", () => {
    document.body.innerHTML = '<img id="t" src="cover.jpg">';
    const p = payloadFor(document.getElementById("t"));
    expect(p.image).toBe("http://localhost:4832/posts/hello/cover.jpg");
  });

  it("reports both link and image for an image wrapped in a link", () => {
    document.body.innerHTML = '<a href="/gallery/"><img id="t" src="/img/a.png"></a>';
    const p = payloadFor(document.getElementById("t"));
    expect(p.link).toBe("http://localhost:4832/gallery/");
    expect(p.image).toBe("http://localhost:4832/img/a.png");
  });

  it("a gallery picture's link is the viewer, not a link to copy", () => {
    document.body.innerHTML =
      '<div class="moss-gallery-item" data-source-line="4"><a class="moss-gallery-link" href="/a.png"><img id="t" src="/a.png"></a></div>';
    expect(payloadFor(document.getElementById("t")).link).toBeNull();
  });

  it("counts the picture among those sharing its line, in page order, file by file", () => {
    document.body.innerHTML = `
      <p data-source-line="3"><img id="p0" src="/a.png"> <img id="p1" src="/a.png"></p>
      <div class="moss-gallery" data-layout="rows">
        <div class="moss-gallery-item" data-source-line="9"><a class="moss-gallery-link" href="/a.png"><img id="g0" src="/a.png"></a></div>
        <div class="moss-gallery-item" data-source-line="9"><a class="moss-gallery-link" href="/b.png"><img id="g1" src="/b.png"></a></div>
        <div class="moss-gallery-item" data-source-line="10"><a class="moss-gallery-link" href="/c.png"><img id="g2" src="/c.png"></a></div>
        <div class="moss-gallery-item" data-source-line="9" aria-hidden="true"><img id="copy" src="/b.png"></div>
      </div>
      <section class="moss-hero" data-source-range="12-14"><img id="hero" src="/w.png"><h1 data-source-line="13">Hi</h1></section>
      <a class="moss-card" data-source-file="notes/first.md"><p data-source-line="3"><img id="other" src="/c.png"></p></a>
      <img id="theme" src="/logo.png">`;
    const index = (id: string) => payloadFor(document.getElementById(id)).imageIndex;
    expect(["p0", "p1", "g0", "g1", "g2", "copy", "hero", "other", "theme"].map(index)).toEqual([0, 1, 0, 1, 0, 0, 0, 0, 0]);
    document.body.innerHTML = "<p id='t'>words</p>";
    expect(index("t")).toBeNull();
  });

  it("carries every source target under the cursor, nearest first", () => {
    document.body.innerHTML =
      '<article data-source-file="posts/a.md"><p id="t" data-source-line="7">x</p></article>';
    expect(payloadFor(document.getElementById("t")).targets).toEqual([
      { kind: "body-line", line: 7, path: "posts/a.md", element: "p" },
      { kind: "page", path: "posts/a.md", element: "article" },
    ]);
  });

  it("carries the theme selectors for the element as an array", () => {
    document.body.innerHTML = '<p id="t">x</p>';
    expect(Array.isArray(payloadFor(document.getElementById("t")).styles)).toBe(true);
  });

  it("trims the selection and drops whitespace-only selections", () => {
    expect(payloadFor(null, { selection: "  hello  " }).selection).toBe("hello");
    expect(payloadFor(null, { selection: "   " }).selection).toBeNull();
    expect(payloadFor(null, { selection: undefined }).selection).toBeNull();
  });
});

describe("isEditableTarget", () => {
  it("is true for inputs and textareas", () => {
    expect(isEditableTarget(document.createElement("input"))).toBe(true);
    expect(isEditableTarget(document.createElement("textarea"))).toBe(true);
  });

  it("is false for ordinary content and null", () => {
    expect(isEditableTarget(document.createElement("p"))).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});

describe("installContextMenu", () => {
  function rightClick(el: Element): MouseEvent {
    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 33,
      clientY: 44,
    });
    el.dispatchEvent(ev);
    return ev;
  }

  function install(): ReturnType<typeof vi.fn> {
    const post = vi.fn();
    const win = new Proxy(window, {
      get(target, prop) {
        if (prop === "parent") return { postMessage: post };
        const v = Reflect.get(target, prop);
        return typeof v === "function" ? v.bind(target) : v;
      },
    }) as unknown as Window;
    installContextMenu(win);
    return post;
  }

  it("suppresses the native menu and posts the payload", () => {
    document.body.innerHTML = "<p id='t'>x</p>";
    const post = install();
    const ev = rightClick(document.getElementById("t")!);
    expect(ev.defaultPrevented).toBe(true);
    expect(post).toHaveBeenCalledTimes(1);
    const [payload, origin] = post.mock.calls[0];
    expect(payload.type).toBe("moss-context-menu");
    expect(payload.x).toBe(33);
    expect(payload.y).toBe(44);
    expect(origin).toBe("*");
  });

  it("leaves the native menu on editable targets (Paste stays available)", () => {
    document.body.innerHTML = "<input id='t'>";
    const post = install();
    const ev = rightClick(document.getElementById("t")!);
    expect(ev.defaultPrevented).toBe(false);
    expect(post).not.toHaveBeenCalled();
  });
});
