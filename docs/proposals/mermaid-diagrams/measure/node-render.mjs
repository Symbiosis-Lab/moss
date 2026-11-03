import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { performance } from 'node:perf_hooks';
const peleUrl = process.argv[2]; const dir = process.argv[3];
const t0 = performance.now();
const pele = await import(peleUrl);
const tLoad = performance.now() - t0;
let ok = 0, fail = 0; const rows = [];
for (const f of readdirSync(dir).filter(f => f.endsWith('.mmd')).sort()) {
  const src = readFileSync(join(dir, f), 'utf8');
  const s = performance.now();
  try {
    const r = pele.render(src, { idPrefix: 'd0', now: new Date('2026-01-01T00:00:00Z') });
    const r2 = pele.render(src, { idPrefix: 'd0', now: new Date('2026-01-01T00:00:00Z') });
    rows.push(`${f}\tok\t${(performance.now()-s).toFixed(1)}ms\t${r.svg.length}B\t${r.type}\tdet=${r.svg===r2.svg}`); ok++;
  } catch (e) { rows.push(`${f}\tERR\t${e.code||''}\t${String(e.message).slice(0,140).replace(/\n/g,' ')}`); fail++; }
}
console.log(rows.join('\n'));
console.log(`load=${tLoad.toFixed(0)}ms ok=${ok} fail=${fail}`);
