// packages/moss-watercolor/lab/paper-worker.ts
import { generatePaper, type PaperRecipe, type PaperSheet } from '../src/paper/index.js';

declare const self: DedicatedWorkerGlobalScope;

// Every Float32Array of the sheet, so the reply moves their buffers instead of copying them.
const buffers = (s: PaperSheet) => {
  const out: ArrayBuffer[] = [];
  const walk = (v: unknown) => {
    if (v instanceof Float32Array) out.push(v.buffer as ArrayBuffer);
    else if (v && typeof v === 'object') Object.values(v).forEach(walk);
  };
  walk(s);
  return out;
};

// generatePaper throws a RangeError for a cell finer than the lattice or a
// coarse cell with no fitted closure; send the message so the page can show it.
self.onmessage = (e: MessageEvent<{ recipe: PaperRecipe; preset: string; grid: { width: number; height: number; cell: number }; seed: number }>) => {
  const t0 = performance.now();
  try {
    const sheet = generatePaper(e.data.recipe, e.data.grid, e.data.seed);
    self.postMessage({ sheet, preset: e.data.preset, ms: performance.now() - t0 }, buffers(sheet));
  } catch (err) {
    self.postMessage({ error: err instanceof Error ? err.message : String(err) });
  }
};
