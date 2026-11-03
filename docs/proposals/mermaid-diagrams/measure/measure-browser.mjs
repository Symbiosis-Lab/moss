// Prototype measurement on a real moss page.
//   A  : build-time SVG, one wide render (what a naive build-time path ships)
//   A2 : build-time SVG pair (wide + maxWidth 360), switched by a CSS container query; no JS
//   A3 : build-time SVG, wide, natural size, scrolling inside a focusable figure
//   A4 : A2 plus bounded shrink: scale to fit, never below 11px text, scroll past that
//   C  : pele.global.js shipped as a site script, mount() in the browser
// usage: node measure-browser.mjs <built-site-dir> <pele-dist-dir> <out.json>
import http from 'node:http';
import zlib from 'node:zlib';
import { readFileSync, writeFileSync, mkdirSync, cpSync, rmSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';
import { chromium, webkit } from 'playwright';

const [site, peleDist, outFile] = process.argv.slice(2);
const pele = await import('file://' + join(peleDist, 'pele.js'));
const unesc = s => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&#x27;/g, "'").replace(/&amp;/g, '&');
const BLOCK = /<pre[^>]*><code class="language-mermaid">([\s\S]*?)<\/code><\/pre>/g;
const THEME = `<style>.moss-diagram{margin:1.5rem 0;container-type:inline-size}.moss-diagram .pele{--pele-font:var(--moss-font-body);--pele-bg:var(--moss-color-bg,Canvas);--pele-fg:var(--moss-color-text,CanvasText)}
.moss-diagram-narrow{display:none}figure.moss-diagram-scroll{overflow-x:auto}
@container (max-width: 639px){.moss-diagram-wide.has-narrow{display:none}.moss-diagram-narrow{display:block}}</style>`;
const NOW = new Date('2026-01-01T00:00:00Z');

const base = readFileSync(join(site, 'index.html'), 'utf8');
const root = process.env.VARIANTS_DIR || join(process.env.TMPDIR || '/tmp', 'moss-diagram-variants'); rmSync(root, { recursive: true, force: true });
for (const v of ['A', 'A2', 'A3', 'A4', 'C']) { mkdirSync(join(root, v), { recursive: true }); cpSync(site, join(root, v), { recursive: true, dereference: true }); }

let i = 0;
const timing = { A: 0, A2: 0, A3: 0, A4: 0 };
const htmlA = base.replace(BLOCK, (_, code) => {
  const t = performance.now(); const r = pele.render(unesc(code), { idPrefix: 'd' + (i++), now: NOW }); timing.A += performance.now() - t;
  return `<figure class="moss-diagram">${r.svg}</figure>`;
}).replace('</head>', THEME + '</head>');
i = 0;
const htmlA2 = base.replace(BLOCK, (_, code) => {
  const src = unesc(code); const t = performance.now();
  const w = pele.render(src, { idPrefix: 'd' + i + 'w', now: NOW });
  let n = null; if (w.width > 360) { const r = pele.render(src, { idPrefix: 'd' + i + 'n', now: NOW, maxWidth: 360 }); if (r.width < w.width) n = r; }
  i++; timing.A2 += performance.now() - t;
  return `<figure class="moss-diagram"><div class="moss-diagram-wide${n ? ' has-narrow' : ''}">${w.svg}</div>${n ? `<div class="moss-diagram-narrow">${n.svg}</div>` : ''}</figure>`;
}).replace('</head>', THEME + '</head>');
i = 0;
const htmlA3 = base.replace(BLOCK, (_, code) => {
  const t = performance.now(); const r = pele.render(unesc(code), { idPrefix: 'd' + (i++), now: NOW, responsive: false }); timing.A3 += performance.now() - t;
  return `<figure class="moss-diagram moss-diagram-scroll" tabindex="0">${r.svg}</figure>`;
}).replace('</head>', THEME + '</head>');
i = 0;
// Bounded shrink: an SVG may scale down to fit, but never below FLOOR px for its smallest text; past that it scrolls.
const FLOOR = 11;
const bounded = (r) => {
  const sizes = [...r.svg.matchAll(/font-size="([\d.]+)"/g)].map(m => +m[1]); sizes.push(16);
  const sc = Math.min(1, FLOOR / Math.min(...sizes)); const w = r.width;
  return r.svg.replace('style="', `style="width:min(${w}px,max(100%,${(w * sc).toFixed(1)}px));max-width:none;height:auto;`);
};
const htmlA4 = base.replace(BLOCK, (_, code) => {
  const src = unesc(code); const t = performance.now();
  const w = pele.render(src, { idPrefix: 'd' + i + 'w', now: NOW, responsive: false });
  let n = null; if (w.width > 360) { const r = pele.render(src, { idPrefix: 'd' + i + 'n', now: NOW, maxWidth: 360, responsive: false }); if (r.width < w.width) n = r; }
  i++; timing.A4 += performance.now() - t;
  return `<figure class="moss-diagram moss-diagram-scroll" tabindex="0"><div class="moss-diagram-wide${n ? ' has-narrow' : ''}">${bounded(w)}</div>${n ? `<div class="moss-diagram-narrow">${bounded(n)}</div>` : ''}</figure>`;
}).replace('</head>', THEME + '</head>');
writeFileSync(join(root, 'A', 'index.html'), htmlA);
writeFileSync(join(root, 'A3', 'index.html'), htmlA3);
writeFileSync(join(root, 'A4', 'index.html'), htmlA4);
writeFileSync(join(root, 'A2', 'index.html'), htmlA2);
cpSync(join(peleDist, 'pele.global.js'), join(root, 'C', 'pele.global.js'));
const init = `<script src="/pele.global.js" defer></script><script>addEventListener('DOMContentLoaded',async()=>{const t=performance.now();for(const c of document.querySelectorAll('code.language-mermaid')){const pre=c.parentElement;const f=document.createElement('figure');f.className='moss-diagram';const src=c.textContent;pre.replaceWith(f);try{await pele.mount(f,src);}catch(e){}}window.__renderMs=performance.now()-t;});</script>`;
const htmlC = base.replace('</head>', THEME + init + '</head>');
writeFileSync(join(root, 'C', 'index.html'), htmlC);

const gz = b => zlib.gzipSync(b, { level: 9 }).length, br = b => zlib.brotliCompressSync(b).length;
const peleJs = readFileSync(join(peleDist, 'pele.global.js'));
const weight = {
  base: { raw: Buffer.byteLength(base), br: br(Buffer.from(base)) },
  A: { raw: Buffer.byteLength(htmlA), br: br(Buffer.from(htmlA)) },
  A2: { raw: Buffer.byteLength(htmlA2), br: br(Buffer.from(htmlA2)) },
  A3: { raw: Buffer.byteLength(htmlA3), br: br(Buffer.from(htmlA3)) },
  A4: { raw: Buffer.byteLength(htmlA4), br: br(Buffer.from(htmlA4)) },
  C: { raw: Buffer.byteLength(htmlC) + peleJs.length, br: br(Buffer.from(htmlC)) + br(peleJs) },
};

const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
function serve(dir) {
  return new Promise(res => { const s = http.createServer((q, r) => { let p = decodeURIComponent(q.url.split('?')[0]); if (p.endsWith('/')) p += 'index.html'; const f = join(dir, p); if (!existsSync(f)) { r.writeHead(404); return r.end(); } r.writeHead(200, { 'content-type': types[extname(f)] || 'application/octet-stream' }); r.end(readFileSync(f)); }); s.listen(0, '127.0.0.1', () => res(s)); });
}

const MEASURE = () => {
  const visible = el => el.getClientRects().length > 0;
  const svgs = [...document.querySelectorAll('.moss-diagram svg.pele')].filter(visible);
  const small = []; let min = Infinity; let overflowFig = 0;
  for (const svg of svgs) {
    const scale = svg.getBoundingClientRect().width / svg.viewBox.baseVal.width;
    let m = Infinity; for (const t of svg.querySelectorAll('text')) { const fs = parseFloat(getComputedStyle(t).fontSize); if (fs && t.textContent.trim()) m = Math.min(m, fs * scale); }
    if (m < 11) small.push((svg.getAttribute('aria-roledescription') || '?') + '=' + m.toFixed(1));
    min = Math.min(min, m);
    const fig = svg.closest('.moss-diagram'); if (fig.scrollWidth > fig.clientWidth + 1) overflowFig++;
  }
  const doc = document.documentElement;
  return { diagrams: svgs.length, codeBlocks: document.querySelectorAll('code.language-mermaid').length, minPx: +min.toFixed(1), under11px: small.length, smallList: small.slice(0, 12), overflowFig, pageHScroll: doc.scrollWidth > doc.clientWidth, renderMs: window.__renderMs ?? null };
};

const results = [];
for (const [ename, engine] of [['chromium', chromium], ['webkit', webkit]]) {
  const browser = await engine.launch();
  for (const v of ['A', 'A2', 'A3', 'A4', 'C']) {
    const server = await serve(join(root, v)); const url = `http://127.0.0.1:${server.address().port}/`;
    for (const width of [375, 1280]) for (const scheme of ['light', 'dark']) for (const js of [true, false]) {
      if (!js && (width !== 375 || scheme !== 'light')) continue;
      const ctx = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: scheme, javaScriptEnabled: js });
      const page = await ctx.newPage();
      if (ename === 'chromium' && js) await page.addInitScript(() => { window.__cls = 0; new PerformanceObserver(l => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true }); });
      const t0 = Date.now(); await page.goto(url, { waitUntil: 'load' }); await page.waitForTimeout(500); const loadMs = Date.now() - t0;
      let m;
      m = await page.evaluate(MEASURE);
      const cls = (ename === 'chromium' && js) ? +(await page.evaluate(() => window.__cls)).toFixed(4) : null;
      results.push({ engine: ename, variant: v, width, scheme, js, loadMs, cls, ...m });
      await ctx.close();
    }
    server.close();
  }
  await browser.close();
}
writeFileSync(outFile, JSON.stringify({ nodeBuildMs: timing, weight, results }, null, 1));
console.log('node build-time render ms:', JSON.stringify(timing));
console.log('weight (bytes):', JSON.stringify(weight));
for (const r of results) console.log([r.engine.padEnd(8), r.variant.padEnd(2), r.width, r.scheme.padEnd(5), r.js ? 'js  ' : 'nojs', `render=${r.renderMs?.toFixed?.(0) ?? '-'}ms`, `cls=${r.cls}`, `svg=${r.diagrams}`, `code=${r.codeBlocks}`, `minPx=${r.minPx}`, `<11px=${r.under11px}`, `figScroll=${r.overflowFig}`, `hscroll=${r.pageHScroll}`].join(' '));
for (const v of ['A', 'A2', 'A3', 'A4', 'C']) console.log(v, 'small @375 chromium:', JSON.stringify(results.find(r => r.variant === v && r.width === 375 && r.engine === 'chromium' && r.js)?.smallList));
