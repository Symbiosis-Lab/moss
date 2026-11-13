#!/usr/bin/env node
// Judge a design import: how close a folder moss imported looks to the site
// it came from, and whether every token the import wrote earns its place.
//
// It builds the folder with a moss binary, opens the reference and the build
// in Chromium at 390, 768 and 1280 px in each colour scheme the reference
// has, on the home page and on one post (the latest dated imported page that
// names its origin), and compares the computed font family, size, weight, line height and
// colour (and a link's underline colour) of the same named elements on both
// (body and its background, title heading, section heading, a paragraph of
// running text and a link in it, the nav, a date, a code sample), the fonts
// the browser actually drew the paragraph and title with, and the content
// column's width and left edge. Each value is identical or a gap. A
// Chinese, Japanese or Korean page, when the import has one, is compared too.
//
// --minimal drops each token the import wrote in turn, rebuilds, re-scores,
// and fails when a token's removal leaves the gaps unchanged (it does nothing
// a reader can see) or removes gaps (it makes the copy worse).
//
// A local proof, not a CI gate: the Ghost fixtures need Docker
// (ghost.mjs), and static-fixture.mjs serves the theme stylesheets without it.
//
//   node scripts/design-import/judge.mjs --reference <url> --folder <imported folder> [--moss <bin>] [--minimal] [--json]
//   node scripts/design-import/judge.mjs --fixture casper [--moss <bin>] [--minimal]   # serve, import, judge
//
// --moss defaults to target/release/moss-cli. Build it first; the judge never
// runs cargo.

import { execFile, execFileSync } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { chromium } from "@playwright/test";
import { resolveBaseURL } from "../landing-harness.mjs";

const WIDTHS = [390, 768, 1280];
const ELEMENTS = ["body", "title", "section", "text", "link", "nav", "date", "code"];

function args(argv) {
  const out = { moss: "target/release/moss-cli", minimal: false, json: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--minimal" || a === "--json") out[a.slice(2)] = true;
    else if (["--reference", "--folder", "--moss", "--fixture"].includes(a)) out[a.slice(2)] = argv[++i];
    else throw new Error(`unknown argument ${a}`);
  }
  if (!out.fixture && !(out.reference && out.folder)) throw new Error("give --reference and --folder, or --fixture");
  return out;
}

/** Runs in the page: the named elements' computed values and the content column. */
function measure() {
  const textLink = (css) => [...document.querySelectorAll(css)].find((a) => !a.closest("nav, header, footer") && !a.getAttribute("href").startsWith("#"));
  const pick = {
    body: document.body,
    title: document.querySelector("h1") ?? [...document.querySelectorAll("h2, h3")].find((h) => h.querySelector("a[href]") || h.closest("a")),
    section: document.querySelector("h2") ?? document.querySelector("h3"),
    link: textLink("p a[href], li a[href]") ?? textLink("a[href]"),
    nav: document.querySelector("nav"),
    // moss writes a date as `.date`, not `<time>`.
    date: document.querySelector("time") ?? document.querySelector(".date"),
    code: document.querySelector("code"),
  };
  pick.text = pick.link?.closest("p, li") ?? document.querySelector("main p, article p");
  const out = {};
  for (const [name, el] of Object.entries(pick)) {
    if (!el) continue;
    el.dataset.judge = name;
    const c = getComputedStyle(el);
    const size = parseFloat(c.fontSize);
    out[name] = {
      family: c.fontFamily.split(",")[0].trim().replace(/^["']|["']$/g, "").toLowerCase(),
      size,
      weight: Number(c.fontWeight),
      line: c.lineHeight === "normal" ? "normal" : Math.round((parseFloat(c.lineHeight) / size) * 100) / 100,
      colour: c.color,
      underline: c.textDecorationLine.includes("underline") ? c.textDecorationColor : "none",
    };
  }
  const paper = [document.body, document.documentElement].map((e) => getComputedStyle(e).backgroundColor).find((c) => !/rgba\(0, 0, 0, 0\)|transparent/.test(c));
  if (out.body) out.body.paper = paper ?? "rgb(255, 255, 255)";
  let wrap = pick.section ?? pick.link;
  while (wrap && wrap !== document.body && getComputedStyle(wrap).maxWidth === "none") wrap = wrap.parentElement;
  const column = wrap && wrap !== document.body ? wrap : document.querySelector("main");
  if (column) {
    const r = column.getBoundingClientRect();
    out.column = { width: Math.round(r.width), left: Math.round(r.left) };
  }
  out.scheme = { text: getComputedStyle(document.body).color, paper: getComputedStyle(document.body).backgroundColor };
  return out;
}

const rgba = (s) => (s.match(/[\d.]+/g) ?? []).map(Number).concat([1]).slice(0, 4);
function same(prop, a, b) {
  if (prop === "colour" || prop === "paper" || (prop === "underline" && a !== "none" && b !== "none")) return rgba(a).every((v, i) => Math.abs(v - rgba(b)[i]) <= (i === 3 ? 0.02 : 1));
  if (prop === "size") return Math.abs(a - b) <= 0.5;
  if (prop === "line") return a === b || (typeof a === "number" && typeof b === "number" && Math.abs(a - b) <= 0.02);
  if (prop === "width" || prop === "left") return Math.abs(a - b) <= 2;
  if (prop === "rendered") return a.join() === b.join();
  return a === b;
}

/** Every difference between two measurements, keyed so two runs compare as sets. */
function gaps(ref, got, where) {
  const out = [];
  for (const name of [...ELEMENTS, "column"]) {
    const a = ref[name];
    const b = got[name];
    if (!a && !b) continue;
    if (!a || !b) {
      out.push({ key: `${where} ${name} missing`, detail: a ? "only on the reference" : "only on the import" });
      continue;
    }
    for (const prop of Object.keys(a)) {
      if (!same(prop, a[prop], b[prop])) out.push({ key: `${where} ${name}.${prop}`, detail: `${a[prop]} vs ${b[prop]}` });
    }
  }
  return out;
}

function build(moss, folder) {
  fs.rmSync(path.join(folder, ".moss/cache"), { recursive: true, force: true });
  execFileSync(moss, ["build", folder, "--no-plugins"], { stdio: "pipe", timeout: 120_000 });
}

/** The pages compared: the home page, and the first imported page with an origin, on both sides. */
function pagesOf(folder, reference, site) {
  const out = [{ name: "home", ref: reference, got: site }];
  const notes = fs.readdirSync(folder).filter((f) => f.endsWith(".md") && !["index.md", "footer.md"].includes(f)).sort();
  const text = (note) => fs.readFileSync(path.join(folder, note), "utf8");
  const date = (note) => text(note).match(/^date: "?([^"\n]+)"?$/m)?.[1] ?? "";
  notes.sort((a, b) => date(b).localeCompare(date(a)));
  for (const note of notes) {
    const origin = text(note).match(/^origin: "?([^"\n]+)"?$/m)?.[1];
    if (!origin) continue;
    out.push({ name: "post", ref: new URL(new URL(origin).pathname, reference).href, got: new URL(`${note.slice(0, -3)}/`, site).href });
    break;
  }
  // A Chinese, Japanese or Korean page, where moss swaps in its Han font stacks.
  const han = notes.find((note) => /^lang: "?(zh|ja|ko)\b/m.test(text(note)) && /^origin:/m.test(text(note)));
  if (han) {
    const origin = text(han).match(/^origin: "?([^"\n]+)"?$/m)[1];
    out.push({ name: "han", ref: new URL(new URL(origin).pathname, reference).href, got: new URL(`${han.slice(0, -3)}/`, site).href });
  }
  return out;
}

/** Each page's measurements at every width and scheme, and the font files it failed to load. */
async function survey(browser, pages, side, schemes) {
  const page = await browser.newPage();
  const failedFonts = new Set();
  page.on("response", (r) => r.request().resourceType() === "font" && r.status() >= 400 && failedFonts.add(r.url()));
  page.on("requestfailed", (r) => r.resourceType() === "font" && failedFonts.add(r.url()));
  // A face with `font-display: optional` is dropped when it arrives a few ms
  // late, so which font a page draws would be a coin toss. Both sides are
  // measured as if every face were `block`: the fonts a page draws once
  // loaded, not who won the race.
  await page.route("**/*", async (route) => {
    const res = await route.fetch();
    const type = res.headers()["content-type"] ?? "";
    if (!/text\/(html|css)/.test(type)) return route.fulfill({ response: res });
    const body = (await res.text()).replace(/font-display\s*:\s*optional/g, "font-display: block");
    await route.fulfill({ response: res, body });
  });
  const out = {};
  for (const target of pages) {
    // A face with `font-display: optional` is drawn only once it is cached, so
    // each page is loaded once before it is measured, or the result is a coin toss.
    await page.goto(target[side], { waitUntil: "networkidle" });
    await page.evaluate(() => document.fonts.ready);
    for (const scheme of schemes) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(target[side], { waitUntil: "networkidle" });
        await page.evaluate(() => document.fonts.ready);
        const measured = await page.evaluate(measure);
        for (const name of ["text", "title"]) {
          if (measured[name]) measured[name].rendered = await renderedFonts(page, name);
        }
        out[`${target.name} ${width} ${scheme}`] = measured;
      }
    }
  }
  await page.close();
  return { measured: out, failedFonts: [...failedFonts] };
}

/** The fonts the browser actually drew an element's own text with, which a
 * declared stack cannot show: a face that failed to load, or the fallback a
 * Chinese character took. */
async function renderedFonts(page, name) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument");
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: `[data-judge="${name}"]` });
  const { fonts } = nodeId ? await cdp.send("CSS.getPlatformFontsForNode", { nodeId }) : { fonts: [] };
  await cdp.detach();
  return [...new Set(fonts.map((f) => f.familyName))].sort();
}

/** The reference's schemes: dark only when its page changes under it. */
async function schemesOf(browser, url) {
  const page = await browser.newPage();
  const look = async (colorScheme) => {
    await page.emulateMedia({ colorScheme });
    await page.goto(url, { waitUntil: "networkidle" });
    return JSON.stringify((await page.evaluate(measure)).scheme);
  };
  const differs = (await look("light")) !== (await look("dark"));
  await page.close();
  return differs ? ["light", "dark"] : ["light"];
}

function score(ref, got) {
  const all = Object.keys(ref.measured).flatMap((where) => gaps(ref.measured[where], got.measured[where], where));
  for (const font of got.failedFonts) all.push({ key: `font ${font} failed`, detail: "the import names a font that does not load" });
  return all;
}

/** The theme's token declarations, one entry per token and block
 * (`--moss-font-body` on `:root` apart from the same token for Han pages). */
function tokensOf(css) {
  const out = new Map();
  let block = "";
  for (const line of css.split("\n")) {
    if (line.trim().endsWith("{")) block = line.trim().slice(0, -1).trim();
    const m = line.match(/^\s*(--moss-[\w-]+)\s*:/);
    if (!m) continue;
    const key = block === ":root" ? m[1] : `${m[1]} in ${block}`;
    out.set(key, [...(out.get(key) ?? []), line]);
  }
  return out;
}

function dirBytes(dir) {
  if (!fs.existsSync(dir)) return 0;
  return fs.readdirSync(dir).reduce((n, f) => n + fs.statSync(path.join(dir, f)).size, 0);
}

async function judge({ reference, folder, moss, minimal }) {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "design-judge-"));
  fs.cpSync(folder, work, { recursive: true, filter: (src) => !/\.moss\/(build\.nosync|cache)(\/|$)/.test(src) });
  build(moss, work);
  // The symlink, not its target: each rebuild re-points it at a new generation.
  const { baseURL, close } = await resolveBaseURL(undefined, { root: path.join(work, ".moss/build.nosync/current") });
  const site = { url: baseURL, close };
  const browser = await chromium.launch();
  try {
    const schemes = await schemesOf(browser, reference);
    const pages = pagesOf(work, reference, site.url);
    const ref = await survey(browser, pages, "ref", schemes);
    const found = score(ref, await survey(browser, pages, "got", schemes));
    const stylePath = path.join(work, ".moss/theme/style.css");
    const css = fs.existsSync(stylePath) ? fs.readFileSync(stylePath, "utf8") : "";
    const tokens = tokensOf(css);
    const report = {
      reference,
      pages: pages.map((p) => p.ref),
      schemes,
      widths: WIDTHS,
      gaps: found,
      referenceFontFailures: ref.failedFonts,
      tokens: tokens.size,
      declarations: [...tokens.values()].reduce((n, l) => n + l.length, 0),
      bytes: { stylesheet: Buffer.byteLength(css), fonts: dirBytes(path.join(work, ".moss/theme/fonts")) },
      minimal: null,
    };
    if (minimal) {
      const baseline = new Set(found.map((g) => g.key));
      report.minimal = [];
      for (const [token, lines] of tokens) {
        // A rule for pages in some languages can be judged only on such a page.
        if (token.includes("[lang|=") && !pages.some((p) => p.name === "han")) {
          report.minimal.push({ token, verdict: "not measured", gapsWithout: null, added: 0, removed: 0 });
          continue;
        }
        fs.writeFileSync(stylePath, css.split("\n").filter((l) => !lines.includes(l)).join("\n"));
        build(moss, work);
        const without = new Set(score(ref, await survey(browser, pages, "got", schemes)).map((g) => g.key));
        const added = [...without].filter((k) => !baseline.has(k));
        const removed = [...baseline].filter((k) => !without.has(k));
        const verdict = added.length === 0 && removed.length === 0 ? "redundant" : added.length > 0 && removed.length === 0 ? "earns its place" : removed.length > 0 && added.length === 0 ? "makes it worse" : "trades gaps";
        report.minimal.push({ token, verdict, gapsWithout: without.size, added: added.length, removed: removed.length });
      }
      fs.writeFileSync(stylePath, css);
    }
    return report;
  } finally {
    await browser.close();
    site.close();
    fs.rmSync(work, { recursive: true, force: true });
  }
}

function print(report) {
  console.log(`reference ${report.pages.join(" ")} · schemes ${report.schemes.join(", ")} · widths ${report.widths.join(", ")}`);
  console.log(`gaps ${report.gaps.length} · tokens ${report.tokens} · declarations ${report.declarations} · bytes ${report.bytes.stylesheet} stylesheet + ${report.bytes.fonts} fonts`);
  for (const g of report.gaps) console.log(`  gap  ${g.key}: ${g.detail}`);
  for (const f of report.referenceFontFailures) console.log(`  note the reference itself fails to load ${f}`);
  for (const m of report.minimal ?? []) {
    const why = m.gapsWithout === null ? "no Chinese, Japanese or Korean page to measure it on" : `gaps without it: ${m.gapsWithout}, +${m.added} −${m.removed}`;
    console.log(`  ${m.verdict.padEnd(15)} ${m.token} (${why})`);
  }
}

const failing = (report) => (report.minimal ?? []).filter((m) => m.verdict === "redundant" || m.verdict === "makes it worse");

const opts = args(process.argv.slice(2));
let fixture = null;
if (opts.fixture) {
  const { startFixture } = await import("./static-fixture.mjs");
  fixture = await startFixture(opts.fixture);
  opts.reference = fixture.url;
  opts.folder = fs.mkdtempSync(path.join(os.tmpdir(), `design-import-${opts.fixture}-`));
  // Not execFileSync: the fixture is served from this process, which must keep answering.
  const { stderr } = await promisify(execFile)(opts.moss, ["import", fixture.url, opts.folder, "-r"], { timeout: 300_000 });
  process.stderr.write(stderr.split("\n").filter((l) => l.includes("↳")).join("\n") + "\n");
}
try {
  const report = await judge(opts);
  if (opts.json) console.log(JSON.stringify(report, null, 2));
  else print(report);
  if (failing(report).length) {
    console.error(`minimality failed: ${failing(report).map((m) => `${m.token} ${m.verdict}`).join(", ")}`);
    process.exitCode = 1;
  }
} finally {
  await fixture?.close();
  if (fixture) fs.rmSync(opts.folder, { recursive: true, force: true });
}
