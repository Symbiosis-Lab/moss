// No text lost: every label pele's own parse() reports for a diagram must appear in the drawing.
//
// usage: node tests/diagrams/text-differential.mjs <pele.js> <corpus-dir> <svg-dir> [--json]
//   <svg-dir> holds <name>.mmd.svg for each <name>.mmd in <corpus-dir>, rendered by whichever
//   renderer is under test. The check reads only the SVG text, so it judges the renderer's output,
//   moss's post-processing and the sanitiser together.
//
// Verdicts: pass (every expected label found), fail (a label is missing, or the SVG is absent),
// inconclusive (the model yields no labels for this type, so nothing could be compared).
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [peleJs, corpus, svgDir] = process.argv.slice(2);
const asJson = process.argv.includes('--json');
const pele = await import(pathToFileURL(peleJs).href);

// Keys whose string values are drawn as text, per type. Anything not listed is an id, a style
// token or a source fragment. `*` applies to every type.
const TEXT_KEYS = {
  '*': ['text', 'label', 'title', 'descr', 'description', 'message', 'task', 'events', 'sections', 'categories', 'relationTitle', 'roleA'],
  sequence: ['description', 'message'],
  state: ['description', 'relationTitle'],
  er: ['label', 'name', 'roleA'],
  railroad: ['title', 'name'],
  treeView: ['name'],
  treemap: ['name'],
  cynefin: ['title', 'label'],
  eventmodeling: ['name'],
  sankey: ['id'],
  mindmap: ['descr'],
  gitGraph: ['lanes'],
  radar: ['title', 'label'],
  journey: ['title', 'sections', 'task', 'people'],
  timeline: ['title', 'events'],
  c4: ['title', 'label', 'descr'],
  requirement: ['name', 'text'],
  class: ['label', 'id', 'title'],
  pie: ['title', '#mapkey'],
  ishikawa: ['text'],
  info: [],
};

// Flowchart shapes Mermaid itself draws without their label.
const LABEL_LESS_SHAPES = new Set(['sm-circ', 'small-circle', 'start', 'fr-circ', 'framed-circle', 'stop', 'fork', 'join', 'f-circ', 'filled-circle', 'junction', 'collate', 'hourglass', 'bolt', 'com-link', 'lightning-bolt', 'cross-circ', 'crossed-circle', 'summary']);
// A source that hides labels on purpose cannot be judged: nothing it hides is lost.
const HIDES_LABELS = [/showBranches:\s*false/, /view:\s*["']?collapsed/];
// Values that are styling, not text.
const NOT_TEXT = /^(rgba?|hsla?)\(|^#[0-9a-f]{3,8}$/i;

// Names the parser invents that are never drawn: C4's implicit outer boundary, a tree's root.
const SYNTHETIC = { c4: ['global'], treeView: ['/'] };
// Types whose model keeps the source line, keyword included: drop the keyword.
const STRIP_KEYWORD = { ishikawa: /^(title|cause|effect)\s+/i };

// What a label looks like once drawn: markup removed, entities decoded, whitespace dropped.
// Comparing without whitespace lets a label wrapped over several lines still match.
// pele keeps an entity it has not decoded yet as ﬂ°name¶ß (named) or ﬂ°°NNN¶ß (decimal).
const ENTITY = { quot: '"', amp: '&', lt: '<', gt: '>', nbsp: ' ', infin: '∞', apos: "'" };
const norm = s => s
  .replace(/ﬂ°°(\d+)¶ß/g, (_, n) => String.fromCodePoint(+n)).replace(/ﬂ°(\w+)¶ß/g, (_, n) => ENTITY[n] ?? '')
  .replace(/&nbsp;/g, ' ').replace(/\\n/g, ' ')
  .replace(/\bfab?:fa-[\w-]+/g, '') // Font Awesome tokens are drawn as icons, not text
  .replace(/~([^~\s]+)~/g, '<$1>') // class generics: List~Page~ is drawn List<Page>
  .replace(/<br\s*\/?>/gi, ' ').replace(/[*_`~]/g, '')
  .replace(/^"|"$/g, '').replace(/#quot;/g, '"').replace(/#lt;/g, '<').replace(/#gt;/g, '>')
  .replace(/\s+/g, '');
const decode = s => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&');

function expectedLabels(model) {
  const keys = new Set(TEXT_KEYS[model.type] ?? TEXT_KEYS['*']);
  const out = new Set(); const seen = new Set();
  const groups = new Set();
  for (const g of [...(model.subgraphs ?? []), ...(model.flows?.values?.() ?? [])]) if (g && typeof g.id === 'string') groups.add(g.id);
  for (const m of JSON.stringify(model, (k, v) => v instanceof Map ? Object.fromEntries(v) : v).matchAll(/"kind":"flow","id":"([^"]+)"/g)) groups.add(m[1]);
  const walk = (v, key, depth) => {
    if (v == null || depth > 10) return;
    if (typeof v === 'string') { if (keys.has(key) && v.trim()) out.add(v.trim()); return; }
    if (typeof v !== 'object' || seen.has(v)) return;
    seen.add(v);
    if (v instanceof Map) { for (const [k, x] of v) { if (keys.has('#mapkey') && typeof k === 'string') out.add(k); walk(x, key, depth + 1); } return; }
    if (Array.isArray(v)) { for (const x of v) walk(x, key, depth + 1); return; }
    if (LABEL_LESS_SHAPES.has(v.type) || LABEL_LESS_SHAPES.has(v.shape)) return;
    // A block-diagram spacer or an unlabelled composite draws nothing.
    if ((v.type === 'space' || v.type === 'composite') && typeof v.label === 'string' && v.label.startsWith(String(v.id).replace(/-\d+$/, ''))) return;
    // A node that only stands for a group (an edge to the group's id) is drawn as the group.
    if (typeof v.id === 'string' && v.text === v.id && groups.has(v.id)) return;
    // Class notes and style classes carry an id, not text; their text, if any, is under `text`.
    if (model.type === 'class' && v.id !== undefined && v.memberType === undefined && v.label === undefined) { if (typeof v.text === 'string') out.add(v.text.trim()); return; }
    // An entity with an alias is drawn by its alias.
    if (model.type === 'er' && typeof v.alias === 'string' && v.alias.trim() && typeof v.label === 'string') { out.add(v.alias.trim()); return; }
    // An id is display text only when its object has no other label: `writer["Drafting Agent"]`
    // draws "Drafting Agent", never "writer".
    const own = Object.entries(v).filter(([k, x]) => keys.has(k) && typeof x === 'string' && x.trim()).map(([, x]) => x.trim());
    const ids = new Set(['id', 'alias', 'nodeId'].map(k => v[k]).filter(x => typeof x === 'string'));
    const hidden = new Set(own.length > 1 ? own.filter(x => ids.has(x)) : []);
    for (const [k, x] of Object.entries(v)) if (k !== 'source' && k !== 'options' && !(typeof x === 'string' && hidden.has(x.trim()))) walk(x, k, depth + 1);
  };
  walk(model, '', 0);
  const strip = STRIP_KEYWORD[model.type];
  const skip = new Set(SYNTHETIC[model.type] ?? []);
  return [...out].map(l => strip ? l.replace(strip, '') : l).filter(l => !skip.has(l) && !NOT_TEXT.test(l))
    // A dotted namespace is drawn as nested namespaces, one name each.
    .flatMap(l => model.type === 'class' && /^[\w]+(\.[\w]+)+$/.test(l) ? l.split('.') : [l]);
}

function drawnText(svg) {
  const parts = [];
  for (const m of svg.matchAll(/<(text|tspan|title|desc)\b[^>]*>([^<]*)/g)) parts.push(decode(m[2]));
  return norm(parts.join(''));
}

const rows = [];
for (const f of readdirSync(corpus).filter(f => f.endsWith('.mmd')).sort()) {
  const src = readFileSync(join(corpus, f), 'utf8');
  let model;
  try { model = pele.parse(src); } catch (e) { rows.push({ file: f, verdict: 'inconclusive', reason: `parse failed: ${e.message.split('\n')[0]}` }); continue; }
  if (HIDES_LABELS.some(re => re.test(src))) { rows.push({ file: f, verdict: 'inconclusive', reason: 'the source hides labels on purpose' }); continue; }
  const svgPath = join(svgDir, f + '.svg');
  if (!existsSync(svgPath)) { rows.push({ file: f, verdict: 'fail', reason: 'no SVG was produced' }); continue; }
  const labels = expectedLabels(model).map(l => [l, norm(l)]).filter(([, n]) => n);
  if (!labels.length) { rows.push({ file: f, verdict: 'inconclusive', reason: `the ${model.type} model names no labels to compare` }); continue; }
  const text = drawnText(readFileSync(svgPath, 'utf8'));
  const missing = labels.filter(([, n]) => !text.includes(n)).map(([l]) => l);
  rows.push(missing.length
    ? { file: f, verdict: 'fail', reason: `${missing.length} of ${labels.length} labels missing: ${JSON.stringify(missing.slice(0, 5))}` }
    : { file: f, verdict: 'pass', reason: `${labels.length} labels found` });
}

const count = v => rows.filter(r => r.verdict === v).length;
const summary = { pass: count('pass'), fail: count('fail'), inconclusive: count('inconclusive') };
if (asJson) console.log(JSON.stringify({ check: 'text-differential', summary, rows }, null, 1));
else {
  for (const r of rows) console.log(`${r.verdict.padEnd(12)} ${r.file.padEnd(28)} ${r.reason}`);
  console.log(`text-differential: ${summary.pass} pass, ${summary.fail} fail, ${summary.inconclusive} inconclusive`);
}
process.exitCode = summary.fail ? 1 : 0;
