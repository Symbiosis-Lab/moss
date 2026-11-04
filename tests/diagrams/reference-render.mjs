// Scaffolding from before the build drew diagrams: what moss emits for one mermaid block, in
// JavaScript, so the render gate and the stylesheet could be exercised on any built page. The build
// now draws the figure itself and its output is what the gate should check; this copy goes when the
// gate builds its site through moss.
//
// usage as a module: renderBlock(pele, source, index) -> HTML string
// usage as a tool:   node tests/diagrams/reference-render.mjs <pele.js> <built-page.html> <out.html> [<diagrams.css>]
//   replaces every <pre><code class="language-mermaid"> in the page, and inlines the stylesheet.
import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

/** Render options moss fixes; a diagram can never set them. */
export const OPTIONS = { now: new Date('2026-01-01T00:00:00Z'), images: false, links: true, linkSchemes: ['http', 'https', 'mailto', 'tel'], responsive: false };
/** Width the narrow layout is drawn for, and the breakpoint it applies under (in the stylesheet). */
export const NARROW_WIDTH = 360;
/** Smallest text, in CSS px, a drawing may shrink its labels to. */
export const FLOOR_PX = 11;

const TYPE_NAMES = { flowchart: 'Flowchart', sequence: 'Sequence diagram', class: 'Class diagram', state: 'State diagram', er: 'Entity relationship diagram', gantt: 'Gantt chart', pie: 'Pie chart', gitGraph: 'Git graph', mindmap: 'Mind map', timeline: 'Timeline', journey: 'User journey', xychart: 'XY chart', quadrantChart: 'Quadrant chart', requirement: 'Requirement diagram', c4: 'C4 diagram', sankey: 'Sankey diagram', block: 'Block diagram', packet: 'Packet diagram', kanban: 'Kanban board', architecture: 'Architecture diagram', radar: 'Radar chart', treemap: 'Treemap', info: 'Diagram', agentflow: 'Agent flow', usecase: 'Use case diagram', railroad: 'Railroad diagram', treeView: 'Tree view', venn: 'Venn diagram', cynefin: 'Cynefin diagram', wardley: 'Wardley map', swimlane: 'Swimlane diagram', ishikawa: 'Ishikawa diagram', eventmodeling: 'Event model' };

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Gives the SVG its width rule and, if it has none, an accessible name. */
function finish(r) {
  const sizes = [...r.svg.matchAll(/font-size="([\d.]+)"/g)].map((m) => +m[1]);
  const smallest = Math.min(16, ...sizes);
  const scale = Math.min(1, FLOOR_PX / smallest);
  const w = r.width;
  let svg = r.svg.replace('style="', `style="width:min(${w}px,max(100%,${+(w * scale).toFixed(1)}px));max-width:none;height:auto;`);
  if (!/<svg[^>]*\saria-labelledby=/.test(svg.slice(0, svg.indexOf('>')))) {
    const inner = svg.match(/class="pele-title"[^>]*>([\s\S]*?)<\/text>/)?.[1];
    const title = inner?.split('<').map((p) => (p.includes('>') ? p.slice(p.indexOf('>') + 1) : p).trim()).filter(Boolean).join(' ') || undefined;
    svg = svg.replace('<svg ', `<svg aria-label="${title?.replace(/"/g, '&quot;') ?? esc(TYPE_NAMES[r.type] ?? 'Diagram')}" `);
  }
  return svg;
}

/** A short, plain reason from a renderer error, with the line when it has one. */
export function reason(e, source = '') {
  const body = source.replace(/^\s*---\n[\s\S]*?\n---\s*$/m, '').replace(/%%.*$/gm, '').trim();
  if (!body) return 'it is empty';
  if (e.code === 'unsupported-diagram') return `moss does not draw “${body.split(/\s/)[0].slice(0, 40)}” diagrams`;
  const lines = String(e.message).split('\n').map((l) => l.trim()).filter((l) => l && !/^[-^]+$/.test(l));
  const parse = /^Parse error on line|^Lexical error on line/.test(lines[0] ?? '');
  const what = parse ? (lines[1] ? `syntax error near “${[...lines[1]].slice(0, 60).join('')}”` : 'syntax error') : (lines.at(-1) ?? 'unknown error');
  return e.line ? `line ${e.line}: ${what}` : what;
}

export function renderBlock(pele, source, index) {
  try {
    const wide = pele.render(source, { ...OPTIONS, idPrefix: `d${index}w` });
    let narrow = null;
    if (wide.width > NARROW_WIDTH) {
      const n = pele.render(source, { ...OPTIONS, idPrefix: `d${index}n`, maxWidth: NARROW_WIDTH });
      if (n.width < wide.width) narrow = n;
    }
    return `<figure class="moss-diagram"${narrow ? ' data-narrow' : ''} tabindex="0"><div class="moss-diagram-wide">${finish(wide)}</div>${narrow ? `<div class="moss-diagram-narrow">${finish(narrow)}</div>` : ''}</figure>`;
  } catch (e) {
    return `<figure class="moss-diagram moss-diagram-error"><figcaption class="moss-diagram-message">This diagram could not be drawn: ${esc(reason(e, source))}.</figcaption><pre tabindex="0"><code class="language-mermaid">${esc(source)}</code></pre></figure>`;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [peleJs, page, out, css] = process.argv.slice(2);
  const pele = await import(pathToFileURL(peleJs).href);
  const unesc = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&#x27;/g, "'").replace(/&amp;/g, '&');
  let i = 0;
  let html = readFileSync(page, 'utf8').replace(/<pre[^>]*><code class="language-mermaid">([\s\S]*?)<\/code><\/pre>/g, (_, code) => renderBlock(pele, unesc(code).replace(/\n$/, ''), i++));
  if (css) html = html.replace('</head>', `<style>${readFileSync(css, 'utf8')}</style></head>`);
  writeFileSync(out, html);
  console.log(`${i} diagrams`);
}
