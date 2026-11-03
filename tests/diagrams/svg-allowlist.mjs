// Every diagram SVG stays inside a closed vocabulary: known elements, known attributes, no script,
// no event handlers, no reference outside the document except an allowed link.
//
// usage: node tests/diagrams/svg-allowlist.mjs <svg-dir> [--prefix-from-name] [--json]
//        node tests/diagrams/svg-allowlist.mjs --self-test
//   Checks every *.svg in <svg-dir>. A file starting "ERR" is a recorded render failure and is
//   skipped as inconclusive: there is no drawing to judge.
//
// This is a second, independent implementation of the rule the build's sanitiser enforces, so a
// regression in either shows up as a disagreement. It fails closed: markup it cannot tokenise
// strictly is a failure, never a skip.
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

export const ELEMENTS = new Set(['svg', 'g', 'path', 'rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon', 'text', 'tspan', 'title', 'desc', 'defs', 'linearGradient', 'radialGradient', 'stop', 'marker', 'clipPath', 'a']);
const PRESENTATION = new Set(['fill', 'fill-opacity', 'fill-rule', 'stroke', 'stroke-width', 'stroke-dasharray', 'stroke-dashoffset', 'stroke-linecap', 'stroke-linejoin', 'stroke-opacity', 'opacity', 'transform', 'font-size', 'font-family', 'font-weight', 'font-style', 'text-anchor', 'dominant-baseline', 'letter-spacing', 'clip-path', 'marker-end', 'marker-start']);
const GEOMETRY = new Set(['x', 'y', 'x1', 'x2', 'y1', 'y2', 'cx', 'cy', 'r', 'rx', 'ry', 'width', 'height', 'd', 'points', 'viewBox', 'offset', 'stop-color', 'stop-opacity', 'gradientUnits', 'gradientTransform', 'markerWidth', 'markerHeight', 'refX', 'refY', 'orient', 'markerUnits', 'preserveAspectRatio', 'dx', 'dy', 'textLength', 'lengthAdjust']);
const STRUCTURE = new Set(['target', 'class', 'id', 'role', 'xmlns', 'xml:space', 'style', 'href', 'rel', 'tabindex', 'focusable']);
const STYLE_PROPS = new Set(['stroke-dasharray', 'stroke-dashoffset', 'stroke-opacity', 'fill-opacity', 'stroke-linecap', 'stroke-linejoin', 'max-width', 'width', 'height', 'color', 'fill', 'stroke', 'stroke-width', 'font', 'font-family', 'font-size', 'font-weight', 'font-style', 'letter-spacing', 'word-spacing', 'text-transform', 'direction', 'rx', 'ry', 'opacity']);
const LINK_SCHEMES = /^(https?:|mailto:|tel:)/i;

function attrOk(el, name, value, prefix, why) {
  if (name.startsWith('data-') || name.startsWith('aria-')) return true;
  if (!PRESENTATION.has(name) && !GEOMETRY.has(name) && !STRUCTURE.has(name)) return why(`attribute ${el}@${name}`);
  // Only href, url() and style can reach outside the drawing, so only they are scanned for
  // schemes and escapes: a label such as "Data: sales" is plain text.
  if (name === 'style' && /expression\s*\(|@import|\\/i.test(value)) return why(`unsafe style ${value.slice(0, 40)}`);
  if (name === 'xmlns' && value !== 'http://www.w3.org/2000/svg') return why(`xmlns ${value}`);
  if (name === 'href') {
    if (el !== 'a') return why(`href on <${el}>`);
    if (!(LINK_SCHEMES.test(value) || value.startsWith('/') && !value.startsWith('//') || value.startsWith('#') || /^[^:]*$/.test(value) && !value.startsWith('//'))) return why(`link ${value.slice(0, 40)}`);
  }
  if (name === 'target' && !/^_(blank|self|parent|top)$/.test(value)) return why(`target ${value}`);
  if (name === 'id' && prefix && !value.startsWith(prefix)) return why(`id ${value} lacks the diagram prefix ${prefix}`);
  for (const m of value.matchAll(/url\(\s*([^)]*)\)/gi)) {
    const ref = m[1].replace(/['"]/g, '');
    if (!ref.startsWith('#')) return why(`url(${ref.slice(0, 40)}) leaves the document`);
    if (prefix && !ref.startsWith('#' + prefix)) return why(`url(${ref}) points outside this diagram`);
  }
  if (name === 'style') for (const decl of value.split(';')) {
    const prop = decl.split(':')[0].trim();
    if (prop && !prop.startsWith('--') && !STYLE_PROPS.has(prop)) return why(`style property ${prop}`);
  }
  return true;
}

export function checkSvg(svg, prefix) {
  const problems = [];
  const why = m => { problems.push(m); return false; };
  const TOKEN = /<(\/?)([a-zA-Z][\w:-]*)((?:\s+[\w:-]+="[^"<]*")*)\s*(\/?)>|([^<]+)|(<)/gy;
  const stack = []; let rootSeen = false; let m;
  TOKEN.lastIndex = 0;
  while (TOKEN.lastIndex < svg.length && (m = TOKEN.exec(svg))) {
    const [, close, name, attrs, selfClose, text, stray] = m;
    if (stray) { why(`markup that does not tokenise at ${m.index}: ${svg.slice(m.index, m.index + 30)}`); break; }
    if (text !== undefined) { if (!stack.length && text.trim()) why('text outside the root'); continue; }
    if (!ELEMENTS.has(name)) why(`element <${name}>`);
    if (close) { if (stack.pop() !== name) why(`unbalanced </${name}>`); continue; }
    if (!stack.length) { if (rootSeen) why('a second root element'); if (name !== 'svg') why('root is not <svg>'); if (!/\sxmlns="http:\/\/www\.w3\.org\/2000\/svg"/.test(attrs)) why('root outside the SVG namespace'); rootSeen = true; }
    for (const a of attrs.matchAll(/([\w:-]+)="([^"]*)"/g)) attrOk(name, a[1], a[2], prefix, why);
    if (!selfClose) stack.push(name);
  }
  if (stack.length) why(`unclosed <${stack.at(-1)}>`);
  if (!rootSeen) why('no <svg> root');
  return problems;
}

/** The planted and accepted samples the build's own sanitiser is tested against: the cases this copy gets wrong. */
export function selfTestFailures() {
  const samples = JSON.parse(readFileSync(new URL('../../crates/moss-build/tests/fixtures/mermaid-corpus/allowlist-samples.json', import.meta.url), 'utf8'));
  const wrap = (f) => samples.wrap.replace('{}', f);
  const wrong = [
    ...samples.refused.filter((f) => !checkSvg(wrap(f), samples.prefix).length).map((f) => `accepted ${f}`),
    ...samples.refused_documents.filter((d) => !checkSvg(d, '').length).map((d) => `accepted ${d}`),
    ...samples.accepted.filter((f) => checkSvg(wrap(f), samples.prefix).length).map((f) => `refused ${f}: ${checkSvg(wrap(f), samples.prefix)}`),
  ];
  return wrong;
}

function selfTest() {
  const wrong = selfTestFailures();
  for (const w of wrong) console.log(w);
  console.log(`svg-allowlist self-test: ${wrong.length ? 'fail' : 'pass'}`);
  process.exitCode = wrong.length ? 1 : 0;
}

if (process.argv[1] && import.meta.url === `file://${process.argv[1]}` && process.argv[2] === '--self-test') selfTest();
else if (process.argv[1] && import.meta.url === `file://${process.argv[1]}`) {
  const dir = process.argv[2]; const asJson = process.argv.includes('--json');
  const prefix = process.env.DIAGRAM_ID_PREFIX || '';
  const rows = [];
  for (const f of readdirSync(dir).filter(f => f.endsWith('.svg')).sort()) {
    const svg = readFileSync(join(dir, f), 'utf8');
    if (svg.startsWith('ERR')) { rows.push({ file: f, verdict: 'inconclusive', reason: 'render failed, nothing to check' }); continue; }
    const p = checkSvg(svg, prefix);
    rows.push(p.length ? { file: f, verdict: 'fail', reason: p.slice(0, 4).join('; ') } : { file: f, verdict: 'pass', reason: 'inside the allowlist' });
  }
  const count = v => rows.filter(r => r.verdict === v).length;
  const summary = { pass: count('pass'), fail: count('fail'), inconclusive: count('inconclusive') };
  if (asJson) console.log(JSON.stringify({ check: 'svg-allowlist', summary, rows }, null, 1));
  else { for (const r of rows) if (r.verdict !== 'pass') console.log(`${r.verdict.padEnd(12)} ${r.file.padEnd(32)} ${r.reason}`); console.log(`svg-allowlist: ${summary.pass} pass, ${summary.fail} fail, ${summary.inconclusive} inconclusive`); }
  process.exitCode = summary.fail ? 1 : 0;
}
