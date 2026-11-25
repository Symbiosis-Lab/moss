// A `:::` block's Markdown through moss's page render, as markup and a stylesheet a shadow root can hold.
import type { DataAdapter } from "obsidian";
import { stylesheetOf, type MossSite, type PagePreview } from "../core";
import { vaultBaseHref, type SiteNote } from "../site-context";

/** `html` is the article inside two wrappers standing in for the page's `<html>` and `<body>`. */
export interface BlockRender { sheet: CSSStyleSheet; html: string }

const ROOT = "data-moss-root";
const BODY = "data-moss-body";

/** Each selector keeps the specificity of the one it replaces. */
const SCOPED: Record<string, string> = { html: `div:where([${ROOT}])`, ":root": `[${ROOT}]`, body: `div:where([${BODY}])` };

/**
 * The site's selectors for `<html>`, `:root` and `<body>`, pointed at the wrappers. The wrappers
 * carry the page's own attributes, so `body.x`, `html[data-theme]` and `html body` still match.
 */
export function scopeCss(css: string): string {
  // Only selector lists: the text before each `{`.
  return css.replace(/(^|[{}])([^{}]*)(?=\{)/g, (_, edge: string, selectors: string) => edge + selectors.replace(
    /(^|[\s,>+~(])(html|:root|body)(?![\w-])/g,
    (_m, before: string, name: string) => before + SCOPED[name],
  ));
}

/** One sheet per window, shared by every block of the same site. */
const sheets = new WeakMap<Document, { site: MossSite; sheet: CSSStyleSheet }>();

function sheetFor(doc: Document, site: MossSite): CSSStyleSheet {
  const cached = sheets.get(doc);
  if (cached && cached.site === site) return cached.sheet;
  // A constructed sheet can only be adopted in the document whose window made it.
  const sheet = new doc.defaultView!.CSSStyleSheet();
  sheet.replaceSync(scopeCss(stylesheetOf(site)));
  sheets.set(doc, { site, sheet });
  return sheet;
}

const URL_ATTRS = new Set(["href", "src", "action", "formaction"]);

// Browsers drop tab, CR and LF inside a URL, so `java\nscript:` is `javascript:`. An image cannot run its data: URL.
const runsScript = ({ name, value, ownerElement }: Attr) => {
  const url = value.replace(/[\t\n\r]/g, "").trim().toLowerCase();
  if (url.startsWith("data:image/") && name === "src" && ownerElement?.localName === "img") return false;
  return url.startsWith("javascript:") || url.startsWith("vbscript:") || url.startsWith("data:");
};

const executes = (attr: Attr) => attr.name.startsWith("on") || attr.name === "srcdoc" || (URL_ATTRS.has(attr.name) && runsScript(attr));

/** The page's markup without what must not run or repeat inside a note. */
export function blockMarkup(pageHtml: string, base: string): string {
  const page = new DOMParser().parseFromString(pageHtml, "text/html");
  page.querySelectorAll("script, source, .moss-article-title").forEach((el) => el.remove());
  // moss passes a page's raw HTML through, and a shadow root does not stop its handlers.
  for (const el of Array.from(page.querySelectorAll("*"))) {
    for (const attr of Array.from(el.attributes)) if (executes(attr)) el.removeAttribute(attr.name);
  }
  page.querySelectorAll("img").forEach((img) => img.setAttribute("src", new URL(img.getAttribute("src") ?? "", base).href));
  page.querySelectorAll("style").forEach((style) => { style.textContent = scopeCss(style.textContent ?? ""); });
  const wrap = (from: Element, marker: string, inner: string) => {
    const el = page.createElement("div");
    for (const attr of Array.from(from.attributes)) el.setAttribute(attr.name, attr.value);
    el.setAttribute(marker, "");
    el.innerHTML = inner;
    return el.outerHTML;
  };
  const article = page.querySelector("article")?.innerHTML ?? "";
  return wrap(page.documentElement, ROOT, wrap(page.body, BODY, article));
}

/** `note` is the block's note as its site sees it; `doc` is the document the block shows in. */
export function renderBlock(note: SiteNote, adapter: Pick<DataAdapter, "getResourcePath">, markdown: string, doc: Document): BlockRender | null {
  try {
    const base = vaultBaseHref(adapter, note.root === "/" ? note.path : `${note.root}/${note.path}`, note.path);
    const page: PagePreview = JSON.parse(note.site.preview(note.path, markdown, base));
    return { sheet: sheetFor(doc, note.site), html: blockMarkup(page.html, base) };
  } catch (error) {
    console.warn("[moss] Cannot render block", error);
    return null;
  }
}
