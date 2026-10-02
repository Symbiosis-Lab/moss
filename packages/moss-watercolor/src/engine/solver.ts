// One pigment simulation on one canvas: its state is texW x texH composition
// pixels over `divisor`, and `rect()` is where the canvas lies in its parent
// (in CSS px) -- `draw()`/`play()` size and position the canvas from it every
// call, since a caller's own layout can grow the print rectangle at runtime.
//
// Internal helpers are left loosely typed: the shape of a recorded frame or a
// side is private to this file, and only the factory's own options and
// returned API are typed for a consumer to rely on.
//
// Two halves share one factory: the solver proper
// (setPrints/reset/step/probe/draw/dispose -- the shallow-water film and its
// display) and recording-and-playback (pair/record/recorded/show/free/play
// -- each print's dispersion recorded once, then played forward/backward and
// crossfaded in optical thickness for a transition).
import { V, WATER, PIG, MEAN, GROW, PACK, PLAY, buildShowShader } from './shaders.js';
import { clamp01, smooth } from './math.js';
import { DEFAULT_PRESET, recordingSteps, recordingFrameCount, type WatercolorPreset } from './preset.js';
import { createPaper, type Paper } from '../paper/default.js';

export interface Rect { x: number; y: number; w: number; h: number; }
export type PrintSource = HTMLImageElement | HTMLCanvasElement | HTMLVideoElement | ImageBitmap;

export interface CreateSimOptions {
  canvas: HTMLCanvasElement;
  /** Composition pixels before `divisor`. */
  texW: number;
  texH: number;
  /** The simulation grid is texW/divisor x texH/divisor. */
  divisor: number;
  /** Where the canvas lies in its parent, in CSS px; read fresh every draw/play call. */
  rect: () => Rect;
  /** Defaults to `createPaper()`, the fixed 256x256 generated sheet; the shaders tile at that period, so any other size is refused. */
  paper?: Paper;
  /** Overrides merged onto {@link DEFAULT_PRESET}. */
  preset?: Partial<WatercolorPreset>;
  /** Splices one of `DIAG_VIEWS` into the display shader in place of the normal composite; 0 (the default) leaves the normal composite. */
  diagMode?: number;
}

export interface StepOptions {
  standing?: boolean;
  open?: number;
  rain?: number;
  rinse?: number;
  light?: number;
  dose?: number;
  lift?: number;
}

export interface PairOptions {
  dose?: number;
  stir?: number;
  rinse?: number;
  light?: number;
  lift?: number;
  flood?: boolean;
  /** Records one side alone (true: the lower print), for a boundary whose other print is not on hand yet. */
  only?: boolean | null;
}

export interface RecordingSide {
  fwd: boolean;
  frames: unknown[];
  n: number;
  dose: number;
  stir: number;
  lift: number;
  rinse: number;
  light: number;
  flood: boolean;
  skip: boolean;
}

export interface Pair { sides: [RecordingSide, RecordingSide]; }

export interface WatercolorSim {
  dispose(): void;
  setPrints(src: PrintSource, tgt: PrintSource): void;
  reset(): void;
  step(fwd: boolean, t: number, cure: number, stir?: number, tilt?: number, relift?: number, options?: StepOptions): void;
  probe(x: number, y: number): number[][];
  draw(fwd: boolean, cure: number, clearance?: number): void;
  pair(options?: PairOptions): Pair;
  record(pair: Pair, firstFwd: boolean, budget: number): void;
  recorded(pair: Pair): boolean;
  /** Records up to one frame's budget, then plays `p`; returns true while there is more to record. */
  show(pair: Pair, fromFwd: boolean, p: number): boolean;
  free(pair: Pair): void;
  /** p in [0, 1] from the outgoing scene (fromFwd: the lower of the pair) to the incoming; returns whether either side had a frame to show. */
  play(pair: Pair, fromFwd: boolean, p: number): boolean;
  /** Adds a `webglcontextlost` handler; returns an unsubscribe function. The factory listens on the canvas only once the first handler is added, and from then on cancels the event's default action (which is what lets the context be restored). A page may also attach its own listener to the canvas directly. */
  onContextLost(handler: (event: Event) => void): () => void;
}

export function createSim(opts: CreateSimOptions): WatercolorSim | null {
  const { canvas, texW, texH, divisor, rect, diagMode = 0 } = opts;
  const preset: WatercolorPreset = { ...DEFAULT_PRESET, ...opts.preset };
  const paperSource = opts.paper ?? createPaper();
  // The shaders sample the paper with a hard-coded period (`t / 256.0`), so a
  // sheet of any other size would tile at the wrong scale without any error.
  if (paperSource.width !== 256 || paperSource.height !== 256) {
    throw new Error(`createSim: paper must be 256x256 (got ${paperSource.width}x${paperSource.height}); the paper period is still a shader constant, not a uniform`);
  }
  const REC_STEPS = recordingSteps(preset), REC_FRAMES = recordingFrameCount(preset);
  const gl = canvas.getContext('webgl2', { alpha: true, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: true });
  if (!gl || !gl.getExtension('EXT_color_buffer_float')) return null;
  const contextLostHandlers = new Set<(event: Event) => void>();
  let listening = false;
  function onContextLost(handler: (event: Event) => void) {
    if (!listening) {
      listening = true;
      canvas.addEventListener('webglcontextlost', (event) => {
        event.preventDefault();
        for (const h of contextLostHandlers) h(event);
      });
    }
    contextLostHandlers.add(handler);
    return () => contextLostHandlers.delete(handler);
  }
  const compile = (type: number, src: string) => { const s = gl.createShader(type)!; gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) || 'shader compile failed'); return s; };
  const W = Math.round(texW / divisor), H = Math.round(texH / divisor);
  const tint = (() => { const c = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim(); const n = parseInt(c.slice(1), 16); return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]; })();
  // the texture units and constants every pass shares are set once, at link;
  // per step only the framebuffers, the state textures and the prints change
  const UNITS: Record<string, number> = { uW: 0, uS: 1, uD: 2, uNear: 3, uWhole: 4, uPaper: 8, uSrc: 9, uTgt: 10, uSrcLo: 11, uTgtLo: 12, uSrcFt: 13, uTgtFt: 14,
    uXa0: 0, uXb0: 1, uXa1: 2, uXb1: 3, uYa0: 4, uYb0: 5, uYa1: 6, uYb1: 7, uXP: 9, uYP: 10, uGrow: 15 };
  const prog = (fs: string) => { const p = gl.createProgram()!; gl.attachShader(p, compile(gl.VERTEX_SHADER, V)); gl.attachShader(p, compile(gl.FRAGMENT_SHADER, fs)); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) || 'program link failed');
    const u: Record<string, WebGLUniformLocation | null> = {}; const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS); for (let i = 0; i < n; i++) { const nm = gl.getActiveUniform(p, i)!.name; u[nm] = gl.getUniformLocation(p, nm); }
    gl.useProgram(p); for (const k in UNITS) if (u[k]) gl.uniform1i(u[k], UNITS[k]);
    if (u.uSize) gl.uniform2f(u.uSize, W, H); if (u.uTint) gl.uniform3f(u.uTint, tint[0], tint[1], tint[2]);
    return { p, u }; };
  const buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.disable(gl.BLEND);
  // a shader one GPU's compiler rejects must not take the page down with it:
  // the wash falls back to the cut, and the log says which program and why
  let water, pig, show, mean, pack, play, grow;
  try { water = prog(WATER); pig = prog(PIG); show = prog(buildShowShader(diagMode)); mean = prog(MEAN); pack = prog(PACK); play = prog(PLAY); grow = prog(GROW); }
  catch (e) { console.error('wash sim unavailable, cutting instead:', (e as Error).message); return null; }
  let nStep = 0;
  const tex = (w: number, h: number, wrap: number = gl.CLAMP_TO_EDGE, float = true) => { const t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
    if (float) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null); return t!; };
  const fbo = (texes: WebGLTexture[]) => { const f = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, f);
    texes.forEach((t, i) => gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, t, 0));
    gl.drawBuffers(texes.map((_, i) => gl.COLOR_ATTACHMENT0 + i)); return f!; };
  const wT = [tex(W, H), tex(W, H)], wF = wT.map((t) => fbo([t]));
  const pT = [0, 1].map(() => [tex(W, H), tex(W, H)]), pF = pT.map((ts) => fbo(ts));
  let wi = 0, pi = 0;
  const BLOCK = 16, NW = Math.ceil(W / BLOCK), NH = Math.ceil(H / BLOCK);
  const nearT = tex(NW, NH), nearF = fbo([nearT]), wholeT = tex(1, 1), wholeF = fbo([wholeT]);
  const growT = tex(W, H), growF = fbo([growT]);
  const means = (S: WebGLTexture, Wt: WebGLTexture) => {
    gl.useProgram(mean.p);
    gl.viewport(0, 0, NW, NH); gl.bindFramebuffer(gl.FRAMEBUFFER, nearF);
    bind(0, S); bind(1, Wt); gl.uniform1i(mean.u.uA, 0); gl.uniform1i(mean.u.uB, 1); gl.uniform2f(mean.u.uInSize, W, H); gl.uniform1i(mean.u.uBlock, BLOCK); gl.uniform1i(mean.u.uJoin, 1);
    draw();
    gl.viewport(0, 0, 1, 1); gl.bindFramebuffer(gl.FRAMEBUFFER, wholeF);
    bind(0, nearT); gl.uniform1i(mean.u.uA, 0); gl.uniform2f(mean.u.uInSize, NW, NH); gl.uniform1i(mean.u.uBlock, 64); gl.uniform1i(mean.u.uJoin, 0);
    draw();
  };

  // Paper: r relief, g absorbency, b fibre, a pore -- tileable value noise,
  // seeded (../paper/default.ts, so a caller can regenerate or replace it).
  const paper = tex(paperSource.width, paperSource.height, gl.REPEAT, false);
  gl.bindTexture(gl.TEXTURE_2D, paper);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, paperSource.width, paperSource.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, paperSource.data as Uint8Array);

  // The two prints, straight alpha, flipped so uv (0,0) is the bottom-left as in the sim.
  const prints: Record<string, { full: WebGLTexture; lo: WebGLTexture; ft: WebGLTexture } | null> = { src: null, tgt: null };
  const scaled = (im: PrintSource, w: number, h: number) => { const c = document.createElement('canvas'); c.width = w; c.height = h; const g = c.getContext('2d')!; g.imageSmoothingQuality = 'high';
    // two halvings at a time keep the box filter honest on every browser
    let cur: CanvasImageSource = im; while ((cur as HTMLCanvasElement).width > w * 2) { const m = document.createElement('canvas'); m.width = Math.ceil((cur as HTMLCanvasElement).width / 2); m.height = Math.ceil((cur as HTMLCanvasElement).height / 2); const mg = m.getContext('2d')!; mg.imageSmoothingQuality = 'high'; mg.drawImage(cur, 0, 0, m.width, m.height); cur = m; }
    g.drawImage(cur, 0, 0, w, h); return c; };
  const upload = (t: WebGLTexture, im: TexImageSource) => { gl.bindTexture(gl.TEXTURE_2D, t); gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true); gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, im); gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false); };
  const setPrint = (key: string, im: PrintSource) => { const p = prints[key] || { full: tex(0, 0, gl.CLAMP_TO_EDGE, false), lo: tex(0, 0, gl.CLAMP_TO_EDGE, false), ft: tex(0, 0, gl.CLAMP_TO_EDGE, false) };
    upload(p.full, im); upload(p.lo, scaled(im, W, H)); upload(p.ft, scaled(im, Math.round(W / 4), Math.round(H / 4))); prints[key] = p; };

  const bind = (unit: number, t: WebGLTexture) => { gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, t); };
  const draw = () => gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  // a pass with the prints bound in the wash's direction
  const common = (pr: { p: WebGLProgram }, fwd: boolean) => {
    const [S, T] = fwd ? [prints.src!, prints.tgt!] : [prints.tgt!, prints.src!];
    gl.useProgram(pr.p);
    bind(8, paper); bind(9, S.full); bind(10, T.full); bind(11, S.lo); bind(12, T.lo); bind(13, S.ft); bind(14, T.ft);
  };
  // the canvas over the print, at the device's pixels, as the display target
  const place = () => {
    const dpr = Math.min(2, devicePixelRatio || 1), r = rect();
    const cw = Math.round(r.w * dpr), chh = Math.round(r.h * dpr);
    if (canvas.width !== cw || canvas.height !== chh) { canvas.width = cw; canvas.height = chh; }
    canvas.style.left = r.x + 'px'; canvas.style.top = r.y + 'px'; canvas.style.right = 'auto'; canvas.style.bottom = 'auto';
    canvas.style.width = r.w + 'px'; canvas.style.height = r.h + 'px';
    gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, cw, chh);
  };
  // one recorded frame: the current state, packed to what the display reads
  const keep = () => {
    const s = tex(W, H), d = tex(W, H), fb = fbo([s, d]);
    gl.viewport(0, 0, W, H); gl.useProgram(pack.p);
    bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]); draw();
    return { s, d, fb };
  };
  // a freed side holds nothing and so has recorded nothing
  const freeFrames = (side: any) => { for (const k of side.frames) { gl.deleteFramebuffer(k.fb); gl.deleteTexture(k.s); gl.deleteTexture(k.d); } side.frames = []; side.n = 0; };
  // the recording side whose half-made dispersion the sheet's state still holds
  let owner: any = null, recording = false, grownFor: any = null;

  const api: WatercolorSim = {
    // lets the GPU have the context back: a discarded simulation must not count against the page's live ones
    dispose() { gl.getExtension('WEBGL_lose_context')?.loseContext(); },
    setPrints(src, tgt) { setPrint('src', src); setPrint('tgt', tgt); },
    reset() {
      owner = null;
      gl.viewport(0, 0, W, H); gl.clearColor(0, 0, 0, 0);
      for (const f of [...wF, ...pF, nearF, wholeF]) { gl.bindFramebuffer(gl.FRAMEBUFFER, f); gl.clear(gl.COLOR_BUFFER_BIT); }
    },
    // One fixed step at time t (seconds). The schedule: flood, dissolve and
    // stir, take up, dry and cure.
    // A `standing` film never dries or takes up, however long it runs; its
    // sheet opens by `open` toward the whole paper while `rain` water is added
    // over it; `rinse` flushes clean water through it and `dose` charges the print's ink.
    step(fwd, t, cure, stir = 0, tilt = 0, relift = 0, { standing = false, open = 0, rain = 0, rinse = 0, light = preset.recLight, dose = 1, lift = 1 }: StepOptions = {}) {
      if (!recording) owner = null;
      const splash = t < preset.tSplash ? 0.05 : 0, mist = t < preset.tSplash + 0.15 ? 0.03 : 0;
      // a hot-air blast: evaporation many times the rate of standing air, which
      // thins the film, drives the rim current and settles the pigment quickly
      const drying = standing ? 0 : smooth(preset.tDry, preset.tDry + 0.4, t);
      // the sheet takes pigment once the film is stirred to one colour: uptake comes in with the drying current
      const take = standing ? 0 : smooth(preset.tTake, preset.tTake + 0.45, t);
      gl.viewport(0, 0, W, H);
      common(water, fwd); bind(15, growT); gl.bindFramebuffer(gl.FRAMEBUFFER, wF[1 - wi]);
      bind(0, wT[wi]);
      gl.uniform1f(water.u.uSplash, splash); gl.uniform1f(water.u.uMist, mist); gl.uniform1f(water.u.uCure, cure);
      gl.uniform1f(water.u.uEvap, 0.0008 + 0.016 * drying); gl.uniform1f(water.u.uDrying, drying); gl.uniform2f(water.u.uTilt, 0, tilt);
      gl.uniform1f(water.u.uOpen, open); gl.uniform1f(water.u.uRain, rain);
      draw(); wi = 1 - wi;
      // the coarse levels of liquid and pigment give the neighbourhood and
      // whole-film concentrations; the stirring is slow, so every third step is enough
      if (nStep++ % 3 === 0) means(pT[pi][0], wT[wi]);
      gl.viewport(0, 0, W, H);
      common(pig, fwd); gl.bindFramebuffer(gl.FRAMEBUFFER, pF[1 - pi]);
      bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]); bind(3, nearT); bind(4, wholeT);
      gl.uniform1f(pig.u.uTime, t); gl.uniform1f(pig.u.uCure, cure);
      gl.uniform1f(pig.u.uLift, 0.075 * lift); gl.uniform1f(pig.u.uAds, 0.18 * take);
      // the whole film is stirred by the drying current (and by the hand that scrolls), not before
      gl.uniform1f(pig.u.uMix, 0.06); gl.uniform1f(pig.u.uMixG, 0.005 + 0.025 * Math.max(drying, Math.min(1, stir)));
      gl.uniform1f(pig.u.uDrying, drying); gl.uniform1f(pig.u.uStir, stir); gl.uniform1f(pig.u.uRelift, relift); gl.uniform1f(pig.u.uRinse, rinse); gl.uniform1f(pig.u.uLight, light); gl.uniform1f(pig.u.uDose, dose);
      draw(); pi = 1 - pi;
    },
    // read one texel of the state, for the harness: [h,u,v,sat], s.rgb, [d.rgb,l]
    probe(x, y) {
      const out: number[][] = [];
      for (const [f, n] of [[wF[wi], 1], [pF[pi], 2]] as const) { gl.bindFramebuffer(gl.FRAMEBUFFER, f);
        for (let i = 0; i < n; i++) { gl.readBuffer(gl.COLOR_ATTACHMENT0 + i); const px = new Float32Array(4); gl.readPixels(x, y, 1, 1, gl.RGBA, gl.FLOAT, px); out.push([...px].map((v) => +v.toFixed(4))); } }
      return out;
    },
    // the display pass onto the canvas
    draw(fwd, cure, clearance = -.2) {
      place();
      common(show, fwd);
      bind(0, wT[wi]); bind(1, pT[pi][0]); bind(2, pT[pi][1]);
      gl.uniform1f(show.u.uCure, cure);
      gl.uniform1f(show.u.uClearance, clearance);
      draw();
    },
    // Mobile transitions. Each print of a boundary is washed out by this same
    // simulation, over the wet sheet the two prints share, from solid to one
    // mixed wash -- stopping before the film dries or takes anything up. The
    // transition plays the outgoing dispersion forward, mixes the two washes,
    // then plays the incoming dispersion backward, so the next scene gathers
    // out of its own wash; scrolling back plays the same frames back.
    // pair() holds a boundary's two recordings, in scene order like the prints.
    // Options shape the lower print's dispersion only: `dose` charges its
    // colour (the closing's logos give up more pigment than they show), `lift`
    // scales how fast its ink lets go, `stir` how far the film is mixed toward
    // one colour, and `rinse` is the flush per step once the wash is mixed,
    // while its mean pigment is above `light`: 0 keeps a dark wash whose colour
    // is the point, and a light of 0 flushes a wash off the sheet. `only` records
    // one side alone (true: the lower print), for a boundary whose other print
    // is not on hand yet.
    pair({ dose = 1, stir = 1, rinse = preset.recRinse, light = preset.recLight, lift = 1, flood = false, only = null }: PairOptions = {}) {
      return { sides: [true, false].map((fwd) => ({ fwd, frames: [], n: 0, dose: fwd ? dose : 1, stir: fwd ? stir : 1, lift: fwd ? lift : 1, rinse: fwd ? rinse : preset.recRinse, light: fwd ? light : preset.recLight, flood: fwd && flood, skip: only !== null && fwd !== only })) as unknown as [RecordingSide, RecordingSide] };
    },
    // Records up to `budget` steps, the side the transition needs first first:
    // the two sides share the simulation's state, so one runs at a time.
    record(pair, firstFwd, budget) {
      for (const side of firstFwd ? pair.sides : [...pair.sides].reverse()) {
        if (side.skip || side.n >= REC_STEPS) continue;
        // anything else that reset or stepped the sheet meanwhile voids a half-made recording
        if (side.frames.length && owner !== side) freeFrames(side);
        if (!side.frames.length) {
          // the reach the sheet grows by belongs to the pair's footprint, the same for both sides
          if (grownFor !== pair) { gl.viewport(0, 0, W, H); common(grow, true); gl.bindFramebuffer(gl.FRAMEBUFFER, growF); draw(); grownFor = pair; }
          api.reset(); nStep = 0; owner = side; side.frames.push(keep());
        }
        recording = true;
        while (budget-- > 0 && side.n < REC_STEPS) {
          const t = side.n * preset.dt;
          const flood = side.flood ? preset.recFlood(t) : 0;   // an open of 3 reaches past every place's need
          api.step(side.fwd, t, 0, preset.recStir(t) * side.stir, 0, flood,
            { standing: true, open: preset.recOpen(t) + 2 * flood, rain: Math.max(preset.recRain(t), .12 * flood), rinse: t >= preset.recRinseFrom ? side.rinse : 0, light: side.light, dose: side.dose, lift: side.lift });
          if (++side.n % preset.recEvery === 0 || side.n === REC_STEPS) side.frames.push(keep());
        }
        recording = false;
        return;
      }
    },
    recorded(pair) { return pair.sides.every((side) => side.skip || side.n >= REC_STEPS); },
    // a frame of a recording on screen: a frame's budget recorded ahead, then p
    // drawn; true while there is more to record, so the caller comes back
    show(pair, fromFwd, p) { api.record(pair, fromFwd, preset.stepsPerFrame); api.play(pair, fromFwd, p); return !api.recorded(pair); },
    free(pair) { for (const side of pair.sides) freeFrames(side); },
    // p in [0, 1] from the outgoing scene (fromFwd: the lower of the pair) to the incoming.
    play(pair, fromFwd, p) {
      const [out, inc] = fromFwd ? pair.sides : [pair.sides[1], pair.sides[0]];
      // u = 0 is the solid print, u = 1 the mixed wash; a side still being
      // recorded shows its latest frame
      const at = (side: any, u: number) => { const n = side.frames.length; if (!n) return null;
        const x = Math.min(clamp01(u) * (REC_FRAMES - 1), n - 1), i = Math.floor(x), j = Math.min(i + 1, n - 1);
        return { k0: side.frames[i], k1: side.frames[j], f: x - i, print: (side.fwd ? prints.src : prints.tgt)!.full }; };
      let x: any = at(out, p / preset.playOut), y: any = at(inc, (1 - p) / (1 - preset.playIn)), k = smooth(preset.playOut, preset.playIn, p);
      if (!y) { y = x; k = 0; }
      if (!x) return false;
      place();
      gl.useProgram(play.p);
      bind(0, x.k0.s); bind(1, x.k0.d); bind(2, x.k1.s); bind(3, x.k1.d);
      bind(4, y.k0.s); bind(5, y.k0.d); bind(6, y.k1.s); bind(7, y.k1.d);
      bind(8, paper); bind(9, x.print); bind(10, y.print);
      gl.uniform1f(play.u.uFx, x.f); gl.uniform1f(play.u.uFy, y.f); gl.uniform1f(play.u.uK, k);
      draw();
      return true;
    },
    onContextLost,
  };
  return api;
}
