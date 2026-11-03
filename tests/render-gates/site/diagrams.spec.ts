// Diagrams drawn from Mermaid text, read in a real engine: layout, legibility, contrast, keyboard
// access, accessible names and the reading with JavaScript off. The build's own tests check the
// SVG bytes; none of them can see a cascade, a font or a viewport.
//
// Each check reports pass, fail or inconclusive per diagram. A test fails if any diagram fails;
// otherwise it is skipped, never passed, if any diagram was inconclusive. So the reporter's
// "skipped" count is the inconclusive count.
//
// DIAGRAM_GATE_PAGES (comma-separated paths) points the same checks at another built site.
import { test, expect, type Page } from '@playwright/test';
import { createRequire } from 'node:module';
import type { AxeResults } from 'axe-core';
import { gatePort } from '../../../playwright/gate-ports';
import { DIAGRAM_PAGES } from '../../e2e/helpers/diagram-site';
// @ts-expect-error plain JavaScript module, shared with the command line
import { checkSvg, selfTestFailures } from '../../diagrams/svg-allowlist.mjs';

const base = process.env.DIAGRAM_GATE_URL ?? `http://localhost:${gatePort('diagrams')}`;
const PAGES = process.env.DIAGRAM_GATE_PAGES?.split(',') ?? DIAGRAM_PAGES.map((p) => `/${p}/`);
const AXE = createRequire(import.meta.url).resolve('axe-core/axe.min.js');
/** Smallest text a reader is asked to read, in CSS px. Text drawn smaller than this is never shrunk. */
const FLOOR_PX = 11;
/** Types whose labels sit outside their shape by design: under an icon, beside a point, under an actor. */
const LABELS_OUTSIDE = new Set(['architecture', 'usecase', 'wardley']);

type Verdict = { diagram: string; verdict: 'pass' | 'fail' | 'inconclusive'; detail: string };

function settle(name: string, rows: Verdict[]) {
  const fails = rows.filter((r) => r.verdict === 'fail');
  const inconclusive = rows.filter((r) => r.verdict === 'inconclusive');
  test.info().annotations.push({ type: 'counts', description: `${rows.length - fails.length - inconclusive.length} pass, ${fails.length} fail, ${inconclusive.length} inconclusive` });
  expect(rows.length, `${name}: no diagram was checked`).toBeGreaterThan(0);
  expect(fails.map((f) => `${f.diagram}: ${f.detail}`), name).toEqual([]);
  test.skip(inconclusive.length > 0, `${name}: inconclusive for ${inconclusive.map((r) => `${r.diagram} (${r.detail})`).join('; ')}`);
}

/** Runs in the page: one verdict per check per diagram. */
async function inspect(page: Page) {
  return page.evaluate(({ FLOOR_PX, outsideList }) => {
    const LABELS_OUTSIDE = new Set(outsideList);
    const out: Record<string, Verdict[]> = { drawing: [], legible: [], fit: [], contrast: [], keyboard: [], name: [], error: [] };
    const visible = (el: Element) => el.getClientRects().length > 0;
    const rgb = (css: string) => { const m = css.match(/rgba?\(([^)]+)\)/); if (!m) return null; const [r, g, b, a = 1] = m[1].split(/[ ,/]+/).filter(Boolean).map(Number); return { r, g, b, a }; };
    const lum = (c: { r: number; g: number; b: number }) => { const f = (v: number) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b); };
    const ratio = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    const pageBg = (el: Element) => { for (let e: Element | null = el; e; e = e.parentElement) { const c = rgb(getComputedStyle(e).backgroundColor); if (c && c.a > 0.99) return c; } return rgb(getComputedStyle(document.body).backgroundColor) ?? { r: 255, g: 255, b: 255, a: 1 }; };
    const heading = (fig: Element) => { let e = fig.previousElementSibling; while (e && !/^H[1-6]$/.test(e.tagName)) e = e.previousElementSibling; return e?.textContent?.trim() || `diagram ${[...document.querySelectorAll('figure.moss-diagram')].indexOf(fig)}`; };

    for (const fig of document.querySelectorAll('figure.moss-diagram')) {
      const id = heading(fig);
      if (fig.classList.contains('moss-diagram-error')) {
        const msg = fig.querySelector('.moss-diagram-message');
        const code = fig.querySelector('code.language-mermaid');
        out.error.push(msg && visible(msg) && msg.textContent!.trim() && code
          ? { diagram: id, verdict: 'pass', detail: 'source and message shown' }
          : { diagram: id, verdict: 'fail', detail: `message ${msg ? 'empty or hidden' : 'missing'}, source ${code ? 'present' : 'missing'}` });
        continue;
      }
      const drawings = [...fig.querySelectorAll(':scope svg.pele')].filter((s) => !s.parentElement!.closest('svg') && visible(s));
      if (drawings.length !== 1) { out.drawing.push({ diagram: id, verdict: 'fail', detail: `${drawings.length} visible drawings` }); continue; }
      out.drawing.push({ diagram: id, verdict: 'pass', detail: 'one visible drawing' });
      const svg = drawings[0] as SVGSVGElement;
      const type = svg.getAttribute('aria-roledescription') ?? '?';

      // Legible: effective size = computed size × the drawing's scale on screen.
      const scale = svg.getBoundingClientRect().width / svg.viewBox.baseVal.width;
      const texts = [...svg.querySelectorAll('text')].filter((t) => t.textContent!.trim() && visible(t));
      if (!texts.length || !Number.isFinite(scale)) out.legible.push({ diagram: id, verdict: 'inconclusive', detail: 'no visible text or no viewBox' });
      else {
        const small = texts.filter((t) => { const fs = parseFloat(getComputedStyle(t).fontSize); return fs * scale < Math.min(FLOOR_PX, fs) - 0.25; });
        out.legible.push(small.length ? { diagram: id, verdict: 'fail', detail: `${small.length} labels under ${FLOOR_PX}px, e.g. "${small[0].textContent!.slice(0, 30)}" at ${(parseFloat(getComputedStyle(small[0]).fontSize) * scale).toFixed(1)}px` } : { diagram: id, verdict: 'pass', detail: `scale ${scale.toFixed(2)}` });
      }

      // Fit: a node's label stays inside the node's shape, in the site's real font.
      if (LABELS_OUTSIDE.has(type)) out.fit.push({ diagram: id, verdict: 'inconclusive', detail: `${type} labels sit outside their shape by design` });
      else {
        const spill: string[] = []; let checked = 0;
        for (const node of svg.querySelectorAll('.pele-node')) {
          // A node can hold several boxes (a timeline period and its events): a label fits if
          // some box of its own node holds it.
          const boxes = [...node.querySelectorAll(':scope > rect, :scope > path, :scope > circle, :scope > ellipse, :scope > polygon')].map((sh) => (sh as SVGGraphicsElement).getBBox());
          if (!boxes.length) continue;
          for (const t of node.querySelectorAll('text')) {
            if (t.closest('.pele-node') !== node) continue; // a nested node's labels are judged against its own boxes
            checked++;
            const tb = (t as SVGGraphicsElement).getBBox();
            if (!boxes.some((b) => tb.x >= b.x - 1 && tb.x + tb.width <= b.x + b.width + 1)) spill.push(t.textContent!.slice(0, 30));
          }
        }
        out.fit.push(!checked ? { diagram: id, verdict: 'inconclusive', detail: 'no node with a shape and a label' } : spill.length ? { diagram: id, verdict: 'fail', detail: `${spill.length}/${checked} labels spill: ${JSON.stringify(spill.slice(0, 3))}` } : { diagram: id, verdict: 'pass', detail: `${checked} labels fit` });
      }

      // Contrast: each label against what is actually painted under its centre: the first opaque
      // shape the engine hit-tests below it, else the page.
      const low: string[] = []; let measured = 0, unknown = 0;
      for (const t of texts) {
        // A colour the author chose with `style` or `classDef` is theirs to judge, not moss's.
        if (t.closest('[style*="fill:"]:not(svg)') || t.closest('.pele-node')?.querySelector(':scope > [style*="fill:"]')) { unknown++; continue; }
        const fg = rgb(getComputedStyle(t).fill);
        // Hit-testing only sees the viewport, so bring the label into it first.
        t.scrollIntoView({ block: 'center', inline: 'center' });
        const box = t.getBoundingClientRect();
        let bg: ReturnType<typeof rgb> = null; let translucent = false;
        for (const el of document.elementsFromPoint(box.x + box.width / 2, box.y + box.height / 2)) {
          if (el === t || t.contains(el) || el.tagName === 'tspan' || el.tagName === 'text' || el.tagName === 'g') continue;
          if (el.tagName === 'svg') continue;
          if (!(el instanceof SVGGeometryElement)) { bg = pageBg(el); break; }
          const cs = getComputedStyle(el); const f = rgb(cs.fill);
          if (!f || cs.fill === 'none' || f.a === 0) continue;
          if (f.a * parseFloat(cs.fillOpacity || '1') < 0.99) { translucent = true; break; }
          bg = f; break;
        }
        if (!bg && !translucent) bg = pageBg(svg);
        if (!fg || !bg || fg.a < 0.99 || translucent) { unknown++; continue; }
        measured++;
        const need = parseFloat(getComputedStyle(t).fontSize) * scale >= 18.66 ? 3 : 4.5;
        const r = ratio(lum(fg), lum(bg));
        if (r < need) low.push(`"${t.textContent!.slice(0, 20)}" ${r.toFixed(2)}:1`);
      }
      out.contrast.push(low.length ? { diagram: id, verdict: 'fail', detail: low.slice(0, 3).join(', ') } : !measured ? { diagram: id, verdict: 'inconclusive', detail: `${unknown} labels over a translucent or unresolved fill` } : unknown ? { diagram: id, verdict: 'inconclusive', detail: `${measured} labels pass, ${unknown} over a translucent fill` } : { diagram: id, verdict: 'pass', detail: `${measured} labels` });

      // Keyboard: a frame that scrolls can be scrolled without a mouse.
      const scrolls = fig.scrollWidth > fig.clientWidth + 1;
      out.keyboard.push(!scrolls ? { diagram: id, verdict: 'pass', detail: 'fits, no scrolling' } : (fig as HTMLElement).tabIndex >= 0 ? { diagram: id, verdict: 'pass', detail: 'scrolls and is focusable' } : { diagram: id, verdict: 'fail', detail: 'scrolls but cannot take focus' });

      // Name: every drawing has an accessible name.
      const ref = svg.getAttribute('aria-labelledby');
      const name = (ref && document.getElementById(ref)?.textContent?.trim()) || svg.getAttribute('aria-label')?.trim();
      out.name.push(name ? { diagram: id, verdict: 'pass', detail: name.slice(0, 40) } : { diagram: id, verdict: 'fail', detail: 'no accessible name' });
    }
    const doc = document.documentElement;
    return { rows: out, pageScrolls: doc.scrollWidth > doc.clientWidth + 1, figures: document.querySelectorAll('figure.moss-diagram').length, loose: document.querySelectorAll('code.language-mermaid:not(.moss-diagram-error code)').length };
  }, { FLOOR_PX, outsideList: [...LABELS_OUTSIDE] });
}

for (const path of PAGES) {
  for (const scheme of ['light', 'dark'] as const) {
    for (const width of [375, 1280]) {
      test(`${path} ${scheme} ${width}px`, async ({ page }) => {
        await page.emulateMedia({ colorScheme: scheme });
        await page.setViewportSize({ width, height: 900 });
        await page.goto(base + path);
        await page.evaluate(() => document.fonts.ready);
        const r = await inspect(page);
        expect(r.pageScrolls, 'the page scrolls sideways').toBe(false);
        expect(r.loose, 'a mermaid code block was left undrawn outside an error figure').toBe(0);
        const all: Verdict[] = [];
        for (const [check, rows] of Object.entries(r.rows)) all.push(...rows.map((row) => ({ ...row, diagram: `${check} · ${row.diagram}` })));
        settle(`${path} ${scheme} ${width}px`, all);
      });
    }
  }

  test(`${path} reads the same with JavaScript off`, async ({ browser, page }) => {
    await page.goto(base + path);
    const on = await inspect(page);
    const ctx = await browser.newContext({ javaScriptEnabled: false });
    const off = await ctx.newPage();
    await off.goto(base + path);
    const offFigures = await off.locator('figure.moss-diagram').count();
    const offDrawings = await off.locator('figure.moss-diagram:not(.moss-diagram-error) svg.pele:visible').count();
    await ctx.close();
    expect(on.figures, 'no diagrams on the page').toBeGreaterThan(0);
    expect(offFigures, 'figures with JavaScript off').toBe(on.figures);
    expect(offDrawings, 'drawings with JavaScript off').toBe(on.rows.drawing.filter((d) => d.verdict === 'pass').length);
  });

  for (const scheme of ['light', 'dark'] as const) {
    test(`${path} axe finds no WCAG 2.1 AA violation in the diagrams, ${scheme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width: 375, height: 900 });
      await page.goto(base + path);
      await page.addScriptTag({ path: AXE });
      const violations = await page.evaluate(async () => {
        const { axe } = window as unknown as { axe: { run: (ctx: object, opts: object) => Promise<AxeResults> } };
        const result = await axe.run({ include: [['figure.moss-diagram']] }, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] } });
        return result.violations.map((v) => `${v.id} (${v.nodes.length}): ${v.nodes[0]?.target.join(' ')}`);
      });
      expect(violations).toEqual([]);
    });
  }
}

// A second reading of the build's allowlist, from outside it: every drawing on the built page,
// as the server sends it, holds to the same closed vocabulary. The checker proves itself on the
// samples the build's sanitiser is tested against before its verdict counts.
test('every drawing as served stays inside the SVG allowlist', async ({ request }) => {
  expect(selfTestFailures()).toEqual([]);
  const rows: Verdict[] = [];
  for (const path of PAGES) {
    const html = await (await request.get(`${base}${path}`)).text();
    // A layout's wrapper holds exactly one drawing, and the allowlist admits no <div> inside it.
    for (const [i, svg] of [...html.matchAll(/<div class="moss-diagram-(?:wide|narrow)"[^>]*>([\s\S]*?)<\/div>/g)].map((m) => m[1]).entries()) {
      const problems: string[] = checkSvg(svg, '');
      rows.push({ diagram: `${path} drawing ${i}`, verdict: problems.length ? 'fail' : 'pass', detail: problems.slice(0, 3).join('; ') || 'inside the allowlist' });
    }
  }
  settle('allowlist', rows);
});
