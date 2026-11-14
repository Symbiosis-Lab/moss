#!/usr/bin/env node
// A reference site for the design import judge with no Docker: Ghost's Casper
// or Source stylesheet, unmodified, served on loopback under the home page and
// post in crates/moss-build/tests/fixtures/design-import/. A local proof, not
// a CI gate.
//
//   node scripts/design-import/static-fixture.mjs casper         # prints the URL, serves until killed
//   node scripts/design-import/static-fixture.mjs casper-auto    # Casper set to follow the system's dark mode
//   node scripts/design-import/static-fixture.mjs source         # Source, sans titles
//   node scripts/design-import/static-fixture.mjs source-serif   # Source, serif titles
//
// Every font file the pages link is answered with moss's own colophon face,
// so the import has a real font to copy and both sides load the same one.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../crates/moss-build/tests/fixtures/design-import");

const VARIANTS = {
  casper: { theme: "casper", fill: { html_class: "", body_class: "" } },
  "casper-auto": { theme: "casper", fill: { html_class: ' class="auto-color"', body_class: "" } },
  source: { theme: "source", fill: { title_font: "sans", serif_fonts: "" } },
  "source-serif": { theme: "source", fill: { title_font: "serif", serif_fonts: "" } },
};

// The Traditional Chinese post is a page in that language, the one kind of page
// moss gives its Han font stacks.
const HAN_POST = ["田野筆記：山脊上的晨光", "2026-10-20", "小徑穿過樺樹林，走上山脊，晨光一下子就來了。"];

const POSTS = {
  "/first-light/": ["First light on the ridge", "2026-11-02", "The path climbs through birch and then opens onto the ridge, where the morning arrives all at once."],
  "/river-count/": ["Counting herons on the river", "2026-10-26", "Eleven herons in an hour, and one kingfisher that would not sit still."],
};

export function startFixture(variant, port = 0) {
  const spec = VARIANTS[variant];
  if (!spec) throw new Error(`unknown fixture ${variant}; one of ${Object.keys(VARIANTS).join(", ")}`);
  const dir = path.join(FIXTURES, spec.theme);
  const fill = (file, values) => Object.entries(values).reduce((html, [k, v]) => html.replaceAll(`{{${k}}}`, v), fs.readFileSync(path.join(dir, file), "utf8"));
  const home = fill("home.html", spec.fill);
  const postPage = ([title, date, text], lang = "en") => fill("post.html", { ...spec.fill, title, date, text, lang });
  const css = fs.readFileSync(path.join(dir, "screen.css"));
  const font = fs.readFileSync(path.resolve(FIXTURES, "../../../src/assets/fonts/LongCang-colophon.woff2"));
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://fixture.invalid");
    const send = (status, type, body) => res.writeHead(status, { "content-type": type }).end(body);
    if (url.pathname === "/") return send(200, "text/html; charset=utf-8", home);
    if (url.pathname === "/assets/built/screen.css") return send(200, "text/css", css);
    if (url.pathname.startsWith("/assets/fonts/") && url.pathname.endsWith(".woff2")) return send(200, "font/woff2", font);
    const post = POSTS[url.pathname] ?? (url.pathname === "/about/" ? ["About", "2026-01-05", "A weekly letter about walking and looking."] : null);
    if (post) return send(200, "text/html; charset=utf-8", postPage(post));
    if (url.pathname === "/zh-hant/") return send(200, "text/html; charset=utf-8", postPage(HAN_POST, "zh-Hant"));
    send(404, "text/plain", "not found");
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => {
    resolve({ url: `http://127.0.0.1:${server.address().port}/`, close: () => new Promise((r) => server.close(r)) });
  }));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const variant = process.argv[2] ?? "casper";
  const { url } = await startFixture(variant, Number(process.argv[3] ?? 0));
  console.log(url);
}
