// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { blockMarkup, renderBlock, scopeCss } from "../src/syntax/block-render";
import { initializeCore, MossSite } from "../src/core";
import type { SiteNote } from "../src/site-context";

describe("scopeCss", () => {
  it("points the page's root and body selectors at the wrappers, compound and descendant forms too", () => {
    expect(scopeCss(":root{--a:1}html,body{margin:0}")).toBe("[data-moss-root]{--a:1}div:where([data-moss-root]),div:where([data-moss-body]){margin:0}");
    expect(scopeCss("body.home main{x:1}")).toBe("div:where([data-moss-body]).home main{x:1}");
    expect(scopeCss("html body > p{x:1}")).toBe("div:where([data-moss-root]) div:where([data-moss-body]) > p{x:1}");
    expect(scopeCss(':root[data-theme="dark"]{x:1}')).toBe('[data-moss-root][data-theme="dark"]{x:1}');
    expect(scopeCss("@media (min-width: 40em){ body{x:1} }")).toBe("@media (min-width: 40em){ div:where([data-moss-body]){x:1} }");
  });

  it("leaves classes, attribute values and declarations that only contain the words", () => {
    const css = ".body-text,.html{grid-area:body}[data-x=body]{x:1}";
    expect(scopeCss(css)).toBe(css);
  });
});

describe("blockMarkup", () => {
  const page = '<html data-theme="dark"><head><style>body{x:1}</style></head><body class="home" style="--moss-section-color:red">'
    + '<article><h1 class="moss-article-title">T</h1><p>Hi</p><img src="a.png"><script>bad()</script></article></body></html>';

  it("keeps the article inside wrappers carrying the page's html and body attributes", () => {
    const html = blockMarkup(page, "app://vault/posts/");
    const root = new DOMParser().parseFromString(html, "text/html").body.firstElementChild!;
    expect(root.getAttribute("data-theme")).toBe("dark");
    const body = root.querySelector("[data-moss-body]")!;
    expect(body.className).toBe("home");
    expect(body.getAttribute("style")).toBe("--moss-section-color:red");
    expect(body.innerHTML).toBe('<p>Hi</p><img src="app://vault/posts/a.png">');
  });

  it("points a page's own style rules for its root and body at the wrappers", () => {
    const html = blockMarkup('<html><body><article><style>body{--k:1}</style><p>Hi</p></article></body></html>', "app://vault/");
    expect(html).toContain("<style>div:where([data-moss-body]){--k:1}</style>");
  });

  it("drops the event handlers moss passes through from a page's raw HTML", () => {
    initializeCore();
    const site = new MossSite(JSON.stringify({ files: [], lang: "en" }));
    const markdown = ':::grid 2\n<img src="a.png" onerror="alert(1)">\n+++\n<p onclick="alert(2)">two</p>\n:::\n';
    const rendered = JSON.parse(site.preview("posts/a.md", markdown, "app://vault/posts/")).html as string;
    expect(rendered).toContain("onerror=");
    const html = blockMarkup(rendered, "app://vault/posts/");
    expect(html).toContain("two");
    expect(html).not.toMatch(/\son\w+=/);
    site.free();
  });

  it("drops srcdoc and javascript: URLs, whatever their case or padding", () => {
    const page = '<html><body><article><iframe srcdoc="<script>1</script>"></iframe><a href=" JavaScript:alert(1)">a</a>'
      + '<img src="javascript:alert(2)"><form action="javascript:alert(3)"><button formaction="\tjavascript:alert(4)">b</button></form>'
      + '<a href="java\nscript:alert(5)">c</a>'
      + '<a href="https://example.com/">ok</a></article></body></html>';
    const html = blockMarkup(page, "app://vault/posts/");
    expect(html).not.toMatch(/srcdoc/i);
    expect(html).not.toMatch(/java\s*script:/i);
    expect(html).toContain("https://example.com/");
  });

  it("drops vbscript: and data: URLs, but keeps a data: image in an img", () => {
    const page = '<html><body><article><a href=" VBScript:msgbox(1)">a</a><a href="data:text/html,<script>1</script>">b</a>'
      + '<iframe src="DATA:text/html,x"></iframe><img src="data:image/png;base64,AAAA"></article></body></html>';
    const html = blockMarkup(page, "app://vault/posts/");
    expect(html).not.toMatch(/vbscript:|data:text/i);
    expect(html).toContain('<img src="data:image/png;base64,AAAA">');
  });
});

describe("renderBlock", () => {
  it("styles every block of a site with one sheet built from the site's stylesheet", () => {
    initializeCore();
    const site = new MossSite(JSON.stringify({ files: [], lang: "en", theme_css: "body{--theme-mark:1}" }));
    const stylesheet = vi.spyOn(site, "stylesheet");
    const note = { site, root: "/", path: "posts/a.md" } as SiteNote;
    const adapter = { getResourcePath: (path: string) => `app://vault/${path}` };
    // jsdom has no constructable sheets; the stub keeps the text a real one would parse.
    class Sheet { text = ""; replaceSync(css: string) { this.text = css; } }
    const doc = { defaultView: { CSSStyleSheet: Sheet } } as unknown as Document;
    const first = renderBlock(note, adapter, ":::grid 2\none\n+++\ntwo\n:::\n", doc)!;
    const second = renderBlock(note, adapter, ":::grid 2\nthree\n+++\nfour\n:::\n", doc)!;
    expect(second.sheet).toBe(first.sheet);
    expect(stylesheet).toHaveBeenCalledTimes(1);
    expect((first.sheet as unknown as Sheet).text).toBe(scopeCss(site.stylesheet()));
    expect((first.sheet as unknown as Sheet).text).toContain("div:where([data-moss-body]){--theme-mark:1}");
    expect(first.html).not.toContain("<style");
    site.free();
  });
});
