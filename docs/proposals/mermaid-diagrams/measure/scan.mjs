import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
const pele = await import(process.argv[2]);
for (const dir of process.argv.slice(3)) for (const f of readdirSync(dir).filter(f => f.endsWith('.mmd')).sort()) {
  const src = readFileSync(join(dir, f), 'utf8'); const t = performance.now();
  try {
    const r = pele.render(src, { idPrefix: 'd0', now: new Date('2026-01-01T00:00:00Z') });
    const s = r.svg; const bad = [];
    if (/<script/i.test(s)) bad.push('script');
    if (/<foreignObject/i.test(s)) bad.push('foreignObject');
    if (/\son[a-z]+\s*=/i.test(s)) bad.push('on*attr');
    if (/javascript:|vbscript:|data:text/i.test(s)) bad.push('bad-scheme');
    if (/url\(\s*['"]?(https?:|\/\/|javascript)/i.test(s)) bad.push('external-url()');
    for (const m of s.matchAll(/(?:href|xlink:href)="([^"]*)"/g)) if (!/^(https?:|mailto:|tel:|#|\/[^/]|[^:]*$)/i.test(m[1])) bad.push('href:' + m[1].slice(0, 30));
    if (/<style/i.test(s)) bad.push('style-el');
    if (/<\/svg>[\s\S]+$/.test(s)) bad.push('content-after-svg');
    const links = r.links.map(l => l.href + (l.internal ? '(int)' : ''));
    console.log(`${dir.split('/').pop()}/${f}\tOK\t${(performance.now() - t).toFixed(1)}ms\t${s.length}B\t${bad.length ? 'UNSAFE ' + bad.join(',') : 'clean'}\tlinks=${JSON.stringify(links)}`);
  } catch (e) { console.log(`${dir.split('/').pop()}/${f}\tERR\t${e.code}\tline=${e.line}\t${String(e.message).replace(/\n/g, ' ').slice(0, 110)}`); }
}
