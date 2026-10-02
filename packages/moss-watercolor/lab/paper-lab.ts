// packages/moss-watercolor/lab/paper-lab.ts
import { PRESETS, PRESET_TARGETS, bulkPorosity, latticeCell, type PaperRecipe, type PaperSheet, type PresetName } from '../src/paper/index.js';
import { radialSpectrum } from '../src/paper/spectrum.js';
import { shadePaper } from '../src/paper/shade.js';

const form = document.querySelector<HTMLFormElement>('#controls')!, status = document.querySelector('#status')!;
const view = document.querySelector<HTMLSelectElement>('#view')!, canvas = document.querySelector<HTMLCanvasElement>('#field')!;
const presetSelect = form.elements.namedItem('preset') as HTMLSelectElement;
for (const name of Object.keys(PRESETS)) presetSelect.add(new Option(name, name));
let sheet: PaperSheet | null = null, sheetPreset: PresetName = 'cotton-cold-press';
let worker: Worker | null = null;

// A new run replaces the old one: terminating the worker means a stale reply
// can never draw, and a long run is stopped by starting another.
function spawn(): Worker {
  worker?.terminate();
  const w = new Worker(new URL('./paper-worker.ts', import.meta.url), { type: 'module' });
  w.onmessage = (e) => {
    if (e.data.error) { status.textContent = `Failed: ${e.data.error}`; return; }
    sheet = e.data.sheet; sheetPreset = e.data.preset; status.textContent = `Generated in ${(e.data.ms / 1000).toFixed(1)} s`; draw();
  };
  w.onerror = (e) => { status.textContent = `Failed: ${e.message}`; };
  return (worker = w);
}

const sortedCopy = (f: Float32Array) => Float32Array.from(f).sort();
const mean = (a: Float32Array) => a.reduce((x, v) => x + v, 0) / a.length;
const meanPermeability = (s: PaperSheet) => Float32Array.from(s.permeability.xx, (x, i) => (x + s.permeability.yy[i]) / 2);
// Most common pore diameter (µm), in 1 µm bins: the figure a mercury-intrusion mode is compared with.
function modalDiameterUm(r: Float32Array): number {
  const bins = new Map<number, number>();
  for (const v of r) { const b = Math.floor(v * 2e6); bins.set(b, (bins.get(b) ?? 0) + 1); }
  let best = 0, count = -1;
  for (const [b, n] of bins) if (n > count || (n === count && b < best)) { best = b; count = n; }
  return best + 0.5;
}

const set = (name: string, v: number) => { (form.elements.namedItem(name) as HTMLInputElement).value = String(Number(v.toPrecision(4))); };
const get = (name: string) => Number((form.elements.namedItem(name) as HTMLInputElement).value);

function loadPreset(name: PresetName) {
  const r = PRESETS[name];
  set('grammage', r.grammage * 1e3); set('flexibility', r.flexibility); set('flocculation', r.flocculation); set('machineBias', r.machineBias);
  set('imprintDepth', (r.felt?.imprintDepth ?? 0) * 1e6); set('conformity', (r.felt?.conformity ?? 0) * 1e3);
  set('compaction', r.press.compaction); set('polishLength', r.press.polishLength * 1e3); set('polishStrength', r.press.polishStrength);
  set('contactAngle', (r.contactAngle * 180) / Math.PI); set('cell', latticeCell(PRESETS[name]) * 1e6);
}

function recipeFromForm(): PaperRecipe {
  const base = PRESETS[presetSelect.value as PresetName];
  return {
    ...base, grammage: get('grammage') / 1e3, flexibility: get('flexibility'), flocculation: get('flocculation'), machineBias: get('machineBias'),
    felt: base.felt ? { ...base.felt, imprintDepth: get('imprintDepth') / 1e6, conformity: get('conformity') / 1e3 } : null,
    press: { compaction: get('compaction'), polishLength: get('polishLength') / 1e3, polishStrength: get('polishStrength') },
    contactAngle: (get('contactAngle') * Math.PI) / 180,
  };
}

function scalarField(s: PaperSheet, v: string): Float32Array {
  if (v === 'permeability') return meanPermeability(s);
  return s[v as 'grammage' | 'porosity' | 'entryPressure' | 'washburn'];
}

function draw() {
  if (!sheet) return;
  const { width: W, height: H } = sheet; canvas.width = W; canvas.height = H;
  const img = new ImageData(W, H), v = view.value;
  if (v === 'relief') img.data.set(shadePaper(sheet, { azimuth: Math.PI * 0.8, elevation: Math.PI / 8 }, 0.15));
  else if (v === 'orientation') {
    for (let i = 0; i < W * H; i++) {
      const xx = sheet.orientation.xx[i], xy = sheet.orientation.xy[i], yy = sheet.orientation.yy[i];
      const angle = 0.5 * Math.atan2(2 * xy, xx - yy), aniso = Math.hypot(xx - yy, 2 * xy);
      const c = hsl((angle / Math.PI + 1) % 1, 0.7, 0.2 + 0.6 * Math.min(1, aniso * 3)); img.data.set([...c, 255], i * 4);
    }
  } else {
    const f = scalarField(sheet, v), sorted = sortedCopy(f), lo = sorted[Math.floor(0.005 * sorted.length)], hi = sorted[Math.floor(0.995 * (sorted.length - 1))];
    for (let i = 0; i < W * H; i++) { const g = 255 * Math.min(1, Math.max(0, (f[i] - lo) / (hi - lo || 1))); img.data.set([g, g, g, 255], i * 4); }
    plotHistogram(f);
  }
  canvas.getContext('2d')!.putImageData(img, 0, 0);
  plotSpectrum(sheet.thickness, W, H, sheet.cell);
  renderStats(sheet);
}

function hsl(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h * 12) % 12, a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return [f(0) * 255, f(8) * 255, f(4) * 255];
}

function plotHistogram(f: Float32Array) {
  const c = document.querySelector<HTMLCanvasElement>('#histogram')!, g = c.getContext('2d')!, bins = new Array(64).fill(0);
  const sorted = sortedCopy(f), lo = sorted[0], hi = sorted[sorted.length - 1];
  for (const v of f) bins[Math.min(63, Math.floor(((v - lo) / (hi - lo || 1)) * 64))]++;
  const max = Math.max(...bins); g.clearRect(0, 0, c.width, c.height); g.fillStyle = getComputedStyle(document.body).color;
  bins.forEach((b, i) => g.fillRect((i * c.width) / 64, c.height - (b / max) * c.height, c.width / 64 - 1, (b / max) * c.height));
}

function plotSpectrum(f: Float32Array, W: number, H: number, cell: number) {
  const c = document.querySelector<HTMLCanvasElement>('#spectrum')!, g = c.getContext('2d')!;
  g.clearRect(0, 0, c.width, c.height);
  if ((W & (W - 1)) || (H & (H - 1))) return;
  const { frequency, power } = radialSpectrum(f, W, H), logP = Array.from(power, (p) => Math.log10(p || 1e-30));
  const pMin = Math.min(...logP), pMax = Math.max(...logP), fMin = Math.log10(frequency[0]), fMax = Math.log10(frequency[frequency.length - 1]);
  g.strokeStyle = getComputedStyle(document.body).color; g.beginPath();
  logP.forEach((p, i) => { const x = ((Math.log10(frequency[i]) - fMin) / (fMax - fMin)) * c.width, y = c.height - ((p - pMin) / (pMax - pMin || 1)) * c.height; i ? g.lineTo(x, y) : g.moveTo(x, y); });
  g.stroke(); g.fillText(`wavelength ${(cell / frequency[0] * 1e3).toFixed(2)}–${(cell / frequency[frequency.length - 1] * 1e3).toFixed(3)} mm`, 6, 12);
}

function renderStats(s: PaperSheet) {
  const name = sheetPreset;
  const rows: [string, string, string][] = [
    ['Bulk porosity', bulkPorosity(s).toFixed(3), String(PRESET_TARGETS[name].porosity)],
    ['Mean thickness (µm)', (mean(s.thickness) * 1e6).toFixed(1), ''],
    ['Mean permeability (m²)', mean(meanPermeability(s)).toExponential(2), '1e-13–1e-11 (est.)'],
    ['Mean Washburn (mm/√s)', (mean(s.washburn) * 1e3).toFixed(2), '1–10'],
    ['Modal pore diameter (µm)', modalDiameterUm(s.capillaryRadius).toFixed(1), name === 'xuan-unsized' ? '14.29 (mode)' : ''],
  ];
  document.querySelector('#stats')!.innerHTML = '<tr><th>Quantity</th><th>Model</th><th>Literature</th></tr>' + rows.map((r) => `<tr><td>${r[0]}</td><td>${r[1]}</td><td>${r[2]}</td></tr>`).join('');
}

presetSelect.addEventListener('change', () => loadPreset(presetSelect.value as PresetName));
view.addEventListener('change', draw);
form.addEventListener('submit', (e) => {
  e.preventDefault(); status.textContent = 'Generating…';
  const size = Number((form.elements.namedItem('size') as HTMLSelectElement).value);
  spawn().postMessage({ recipe: recipeFromForm(), preset: presetSelect.value, grid: { width: size, height: size, cell: get('cell') * 1e-6 }, seed: get('seed') });
});
presetSelect.value = 'cotton-cold-press';
loadPreset('cotton-cold-press');
form.requestSubmit();
