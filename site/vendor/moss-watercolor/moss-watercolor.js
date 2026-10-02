// Built from packages/moss-watercolor by scripts/build-site-watercolor.mjs — do not hand-edit.
"use strict";
var MossWatercolor = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // <stdin>
  var stdin_exports = {};
  __export(stdin_exports, {
    DEFAULT_PHASE_BOUNDS: () => DEFAULT_PHASE_BOUNDS,
    DEFAULT_PRESET: () => DEFAULT_PRESET,
    DIAG_VIEWS: () => DIAG_VIEWS,
    advance: () => advance,
    createPaper: () => createPaper,
    createSim: () => createSim,
    phaseOf: () => phaseOf,
    retarget: () => retarget
  });

  // packages/moss-watercolor/src/engine/shaders.ts
  var V = `#version 300 es
in vec2 q; out vec2 vUv; void main(){ vUv = q; gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }`;
  var HEAD = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform vec2 uSize; uniform sampler2D uPaper; uniform vec3 uTint;
// The prints at three resolutions, each a plain texture: full, the sim grid,
// and a quarter of it for the footprint. They are downscaled on the 2D canvas
// rather than by generateMipmap, so no driver's mip chain is in the picture.
uniform sampler2D uSrc, uTgt, uSrcLo, uTgtLo, uSrcFt, uTgtFt;
vec4 P(vec2 t){ return mix(texture(uPaper, t / 256.0), texture(uPaper, t / 977.0 + 0.37), 0.45); }
float wetOf(float h){ return smoothstep(0.0004, 0.004, h); }
// ink density of a printed pixel: what it takes out of the paper's light;
// a soft-edged or shadowed pixel is its colour laid over the page at its alpha
vec3 absorb(vec4 c){ vec3 r = mix(vec3(1.0), clamp(c.rgb / uTint, 0.0, 1.0), c.a); return -log(max(r, vec3(0.02))); }
// the wetted sheet: the prints' footprint blurred (coarse mip) and broken by
// the fibres, so the wet edge is ragged like a wet-in-wet wash, not a frame
// the sheet under the scene: the union of the two prints' silhouettes (not their
// soft shadows), blurred a little and broken by the fibres; and the next
// print's own silhouette, which is where the film gathers as it dries
float sil(float a, vec2 uv){ vec4 pap = P(uv * uSize); return smoothstep(0.5, 0.9, a + (pap.g - 0.5) * 0.35 + (pap.b - 0.5) * 0.15); }
float foot(vec2 uv){ return sil(max(texture(uSrcFt, uv).a, texture(uTgtFt, uv).a), uv); }
float footT(vec2 uv){ return sil(texture(uTgtFt, uv).a, uv); }`;
  var WATER = HEAD + `
uniform sampler2D uW, uGrow;
uniform float uSplash, uMist, uCure, uEvap, uDrying, uOpen, uRain; uniform vec2 uTilt;
out vec4 o;
vec4 W(vec2 t){ return texture(uW, t / uSize); }
float wetAt(vec2 t){ return wetOf(W(t).r); }
// The sheet the film may cover: the prints' footprint, grown outward by uOpen
// (uGrow holds how far it must grow to reach each place), so water added over
// it (uRain) carries the pigment out and thins it rather than clearing it, and
// the wash keeps a wash's outline.
float sheet(vec2 uv){
  float f = foot(uv);
  if (uOpen <= 0.0) return f;
  float need = texture(uGrow, uv).r; vec2 e = min(uv, 1.0 - uv);
  return max(f, smoothstep(need - 0.06, need + 0.06, uOpen)) * smoothstep(0.0, 0.04, min(e.x, e.y));
}
void main(){
  vec2 t = gl_FragCoord.xy;
  vec4 w0 = W(t); float s = w0.a;
  vec2 back = t - w0.gb; vec4 wb = W(back);
  float h = wb.r; vec2 vel = wb.gb;
  float hl = (W(t+vec2(1,0)).r + W(t-vec2(1,0)).r + W(t+vec2(0,1)).r + W(t-vec2(0,1)).r) * 0.25;
  h = mix(h, hl, 0.25);
  vec4 pap = P(t);
  // the splash: a handful of big drops land on the printed area and their
  // impact pushes the water outward (the momentum is what makes the film move
  // and the dissolution fast), with a light mist so every part wets
  vec2 px = 1.0 / uSize;
  float f = max(sheet(vUv), max(max(sheet(vUv + vec2(5.0, 0.0) * px), sheet(vUv - vec2(5.0, 0.0) * px)), max(sheet(vUv + vec2(0.0, 5.0) * px), sheet(vUv - vec2(0.0, 5.0) * px))));
  const vec2 drops[8] = vec2[8](vec2(0.30, 0.62), vec2(0.62, 0.42), vec2(0.46, 0.80), vec2(0.72, 0.72), vec2(0.24, 0.28), vec2(0.55, 0.15), vec2(0.88, 0.14), vec2(0.86, 0.86));
  for (int i = 0; i < 8; i++) {
    vec2 c = drops[i] * uSize; vec2 rd = t - c; float rl = max(length(rd), 1e-3);
    float R = 110.0 * (0.8 + 0.4 * float(i % 3) * 0.5);
    float fall = smoothstep(R, R * 0.25, rl);
    // a drop lands where it falls: water past the print's edge dilutes the
    // liquid and gives the wet region a drop's outline, not the print's
    h += fall * uSplash * (0.1 + 0.9 * f);
    vel += (rd / rl) * fall * uSplash * 1.6 * (0.3 + 0.7 * f);
  }
  h += uMist * f * (0.5 + 1.0 * pap.b) * (1.2 - pap.r);
  // less water on the grown margin than on the prints, so the film thins toward its rim
  h += uRain * f * (0.25 + 0.75 * foot(vUv));
  // forces: downhill along the free surface and the paper's relief; the page lies flat
  float hasW = smoothstep(0.0002, 0.002, h);
  float gx = (W(t+vec2(1,0)).r + P(t+vec2(1,0)).r * 0.45) - (W(t-vec2(1,0)).r + P(t-vec2(1,0)).r * 0.45);
  float gy = (W(t+vec2(0,1)).r + P(t+vec2(0,1)).r * 0.45) - (W(t-vec2(0,1)).r + P(t-vec2(0,1)).r * 0.45);
  vel += (-0.45 * vec2(gx, gy) * 0.5) * hasW;
  // the scroll tilts the sheet: the film drifts the way the page is being
  // pushed, at once. A drift, not a force: the tilt sets a target speed the
  // film relaxes toward, so a long scroll cannot wind it up to the cap and
  // slide the whole film down the sheet (which is what put a lattice of
  // advection aliasing over the plates on a real GPU)
  vel += (uTilt - vel) * 0.15 * hasW;
  // evaporation at the wet-dry boundary drives the flow outward: the rim
  vec2 sob = vec2(wetAt(t+vec2(1.7,0.)) - wetAt(t-vec2(1.7,0.)), wetAt(t+vec2(0.,1.7)) - wetAt(t-vec2(0.,1.7))) * 0.5;
  float em = length(sob);
  // while the film stands its edge is pinned and the rim current runs outward
  // (Deegan); as it dries the contact line depins and recedes, and the film is
  // drawn inward toward the sheet, carrying its pigment with it
  if (em > 1e-4) vel += (sob / em) * em * mix(-0.22, 0.9, uDrying) * hasW;
  // the sheet wets, the page around it does not: as the film dries the contact
  // line recedes down the wettability gradient onto the sheet (Chaudhury &
  // Whitesides 1992), which is what contains the liquid to the scene
  // the page around the sheet is sized and does not wet, so the film is always
  // drawn back onto the sheet; as it dries it gathers onto the next print
  vec2 gf = vec2(sheet((t + vec2(4.0, 0.0)) * px) - sheet((t - vec2(4.0, 0.0)) * px), sheet((t + vec2(0.0, 4.0)) * px) - sheet((t - vec2(0.0, 4.0)) * px));
  vec2 gt = vec2(footT((t + vec2(4.0, 0.0)) * px) - footT((t - vec2(4.0, 0.0)) * px), footT((t + vec2(0.0, 4.0)) * px) - footT((t - vec2(0.0, 4.0)) * px));
  vel += (gf * 1.2 + gt * 3.0 * uDrying) * hasW;
  vel *= mix(0.82, 0.96, clamp(h * 6.0, 0.0, 1.0));
  float spd = length(vel); if (spd > 2.4) vel *= 2.4 / spd;
  // into the fibres, and along them
  float da = min(h, 0.006 * pap.g * (1.0 - s)); h -= da; s += da * 1.3;
  float sl = (W(t+vec2(1,0)).a + W(t-vec2(1,0)).a + W(t+vec2(0,1)).a + W(t-vec2(0,1)).a) * 0.25;
  s += 0.10 * (sl - s) * (0.7 + 0.6 * pap.b);
  // evaporation, fastest at the rim, and quick off the sheet
  // pinned rims evaporate fastest; a receding one dries like the rest of the film
  h -= uEvap * (1.0 + 2.5 * em * 8.0 * (1.0 - 0.8 * uDrying) + 6.0 * (1.0 - f));
  s = clamp(s * (1.0 - 0.006) * (1.0 - uCure), 0.0, 1.0);
  h = max(h, 0.0) * (1.0 - uCure);
  o = vec4(min(h, 1.6), vel, s);
}`;
  var PIG = HEAD + `
uniform sampler2D uW, uS, uD;
uniform sampler2D uNear, uWhole;
uniform float uTime, uCure, uLift, uAds, uMix, uMixG, uDrying, uStir, uRelift, uRinse, uLight, uDose;
layout(location=0) out vec4 oS; layout(location=1) out vec4 oD;
vec4 S(vec2 t){ return texture(uS, t / uSize); }
vec2 curlN(vec2 p){
  vec2 q = p / 46.0 + vec2(0.0, uTime * 0.45); float e = 1.6;
  float n0 = texture(uPaper, q / 7.0).r, nx = texture(uPaper, (q + vec2(e / 46.0, 0.0)) / 7.0).r, ny = texture(uPaper, (q + vec2(0.0, e / 46.0)) / 7.0).r;
  return vec2(ny - n0, n0 - nx) * (46.0 / e) * 0.05;
}
void main(){
  vec2 t = gl_FragCoord.xy; vec2 px = 1.0 / uSize;
  vec4 w = texture(uW, vUv); float h = w.r; vec2 vel = w.gb; float wet = wetOf(h);
  // the liquid rides the flow plus unresolved convection
  vec2 velP = vel + curlN(t) * 1.6 * smoothstep(0.025, 0.28, h);
  vec3 s = S(t - velP).rgb;
  // What the sheet has taken is not yet bound: while the film is wet and moving,
  // the settled pigment is dragged along with it (a fraction of the flow the
  // liquid follows), and it binds as the sheet cures. So a print does not appear
  // in place at full sharpness: it comes in smeared with the current and
  // gathers into its strokes as the film stops, the mirror of the dissolve.
  // The sheet's record of what has dissolved (l) belongs to the place, not the pigment.
  float mob = wet * (1.0 - uCure) * 0.6;
  vec4 dd = texture(uD, vUv); float l = dd.a;
  vec3 d = texture(uD, (t - velP * mob) / uSize).rgb;
  vec4 pap = P(t);
  // molecular diffusion while wet, and the stirring of the film at large
  // scale: the liquid relaxes toward its neighbourhood's mean and, more
  // slowly, toward the whole film's mean, so it becomes one colour
  // what mixes is concentration (pigment per volume), so a thin rim holds
  // little pigment and the veil fades to the water's edge instead of outlining it
  float h0 = 0.02; float hc = max(h, h0);
  vec4 wl = texture(uW, vUv + vec2(px.x, 0.0)), wr = texture(uW, vUv - vec2(px.x, 0.0)), wu = texture(uW, vUv + vec2(0.0, px.y)), wd = texture(uW, vUv - vec2(0.0, px.y));
  // exchange between neighbours is driven by the concentration difference and
  // limited by the thinner of the two films, so it conserves pigment and a
  // thin rim neither gains nor gives much
  // Mixing is the work of the flow, not a constant: an eddy diffusivity goes as
  // the speed of the film (Prandtl's mixing length, K = l|u|). While the film
  // stands still after the splash the pigment only creeps, and the drops'
  // blooms and the fronts between the colours stay; the drying current then
  // stirs the film (the resolved rim current, and the convection cells that
  // evaporation drives in a drying film, below the grid) and it goes to one
  // colour in a moment, just as the sheet begins to take. Long unmixed,
  // briefly mixed: what a wet-in-wet wash does.
  float agit = 0.12 + 0.88 * clamp(length(vel) * 2.0 + uStir * 0.5 + uDrying, 0.0, 1.0);
  float diffF = clamp(0.45 * (0.35 + h * 45.0), 0.0, 0.45) * wet * agit;
  vec3 c = s / hc;
  vec3 ex = vec3(0.0);
  ex += (S(t+vec2(1,0)).rgb / max(wl.r, h0) - c) * min(wl.r, hc);
  ex += (S(t-vec2(1,0)).rgb / max(wr.r, h0) - c) * min(wr.r, hc);
  ex += (S(t+vec2(0,1)).rgb / max(wu.r, h0) - c) * min(wu.r, hc);
  ex += (S(t-vec2(0,1)).rgb / max(wd.r, h0) - c) * min(wd.r, hc);
  s += diffF * 0.25 * ex;
  // the neighbourhood and whole-film means come from the explicit box
  // downsample below, not from a mip chain: the sim state is never mipmapped
  vec4 nr = texture(uNear, vUv), wh = texture(uWhole, vec2(0.5));
  float hn = nr.a, hw = wh.a;
  vec3 nearc = nr.rgb / max(hn, h0), wholec = wh.rgb / max(hw, h0);
  s += uMix * agit * wet * (nearc - c) * min(hn, hc); s += uMixG * wet * (wholec - c) * min(hw, hc);
  s = max(s, vec3(0.0));
  // clean water flushed through the film carries its share of the pigment off
  // the sheet, while the film's mean pigment per wet cell (the sheet is ~85%
  // wet once open) is darker than copy can be read over (uLight)
  s *= 1.0 - uRinse * wet * smoothstep(uLight * 0.85, uLight, dot(wh.rgb, vec3(1.0 / 3.0)) / 0.85);
  // the water re-wets the old print: its ink dissolves into the film, all of it
  vec3 Ao = absorb(texture(uSrcLo, vUv));
  // dissolution is faster where the film moves: flow thins the boundary layer
  float dl = wet * uLift * (1.0 + 2.0 * min(length(vel), 1.5) + uStir) * (1.0 - l) * (0.7 + 0.6 * pap.g);
  // uDose charges the print's colour, not its grey: a coloured mark laid on wetter gives up more
  // pigment than it shows, while black and grey give up their own, so a dark mark cannot grey the
  // wash; a near-neutral mark's faint tint is left uncharged, or a near-black would turn to colour
  float grey = min(Ao.r, min(Ao.g, Ao.b)), top = max(Ao.r, max(Ao.g, Ao.b));
  float colour = smoothstep(0.2, 0.5, (top - grey) / max(top, 1e-3));
  dl = min(dl, 1.0 - l); l += dl; s += (Ao + (Ao - grey) * (uDose - 1.0) * colour) * dl;
  // the sheet takes pigment where the next print is: uptake proportional to
  // what is in the liquid and to the capacity still free, per channel
  vec3 cap = absorb(texture(uTgtLo, vUv));
  // the image areas hold more than the print shows while wet (over-inked);
  // the cure brings the excess to the print's own density
  // a wash poured again to go back: the fresh water takes the deposit it had
  // laid down back into suspension, so it can settle where the other print is
  vec3 rl = d * wet * uRelift * (1.0 - uDrying) * (0.6 + 0.8 * pap.g);
  d -= rl; s += rl;
  vec3 ad = uAds * wet * s * max(cap * 1.6 - d, vec3(0.0));
  ad = min(ad, s);
  s -= ad; d += ad;
  // nothing leaves: where the film has gone the pigment in it settles onto the
  // sheet, all of it; the receding front has already carried most of it inward
  float settle = (0.004 + 0.25 * uDrying) * pow(1.0 - wet, 3.0) * clamp(1.0 + (0.5 - pap.r) * 2.0, 0.05, 2.5);
  vec3 st = s * settle; s -= st; d += st;
  // drying: the sheet sharpens onto the print
  l = max(l, uCure);
  oS = vec4(min(s * (1.0 - uCure), 8.0), 0.0); oD = vec4(min(d, 8.0), l);
}`;
  var MEAN = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform sampler2D uA, uB; uniform vec2 uInSize; uniform int uBlock; uniform bool uJoin;
out vec4 o;
void main(){
  ivec2 o0 = ivec2(gl_FragCoord.xy) * uBlock; vec4 acc = vec4(0.0); float n = 0.0;
  for (int y = 0; y < 64; y++) { if (y >= uBlock) break; for (int x = 0; x < 64; x++) { if (x >= uBlock) break;
    ivec2 q = o0 + ivec2(x, y); if (q.x >= int(uInSize.x) || q.y >= int(uInSize.y)) continue;
    vec4 a = texelFetch(uA, q, 0); acc += uJoin ? vec4(a.rgb, texelFetch(uB, q, 0).r) : a; n += 1.0; } }
  o = acc / max(n, 1.0);
}`;
  var DIAG_VIEWS = {
    4: "exp(-texture(uS, uv).rgb * 3.0)",
    // liquid
    5: "exp(-texture(uD, uv).rgb)",
    // deposit
    6: "vec3(texture(uW, uv).r * 3.0)",
    // water depth
    8: "vec3(texture(uD, uv).a)",
    // how much of the old print has dissolved
    9: "vec3(0.5 + texture(uW, uv).gb * 0.5, 0.5)",
    // velocity: red +x, green +y, grey still
    10: "vec3(length(gh * 0.03 * wet) * uSize.x * 0.5)"
    // refraction offset
  };
  function buildShowShader(diagMode = 0) {
    const diagView = DIAG_VIEWS[diagMode];
    return HEAD + `
uniform sampler2D uW, uS, uD; uniform float uCure, uClearance;
out vec4 o;
void main(){
  vec2 uv = vUv; vec2 px = 1.0 / uSize;
  vec4 w = texture(uW, uv); float h = w.r; float wet = wetOf(h);
  // the print seen through standing water bends with the surface
  vec2 gh = vec2(texture(uW, uv + vec2(px.x, 0.0)).r - texture(uW, uv - vec2(px.x, 0.0)).r, texture(uW, uv + vec2(0.0, px.y)).r - texture(uW, uv - vec2(0.0, px.y)).r);
  vec2 at = uv + gh * 0.03 * wet;
  ${diagView ? `o = vec4(${diagView}, 1.0); return;` : ""}
  vec4 dd = texture(uD, uv); float l = dd.a;
  vec3 s = texture(uS, uv).rgb;
  vec2 t = uv * uSize; vec4 pap = P(t);
  float gmod = 1.0 + 0.4 * ((pap.r - 0.5) * 1.4 + (pap.b - 0.5) * 0.6);
  vec3 A = absorb(texture(uSrc, at)) * (1.0 - l);
  // ink is already an optical thickness; the fixed deposit cures onto the print
  float g = max(gmod, 0.05);
  // the suspended pigment shows with its hue stretched about its density: the
  // same darkness, more colour; the deposit keeps the print's own colours
  float sl = dot(s, vec3(1.0 / 3.0)); vec3 sc = max(sl + (s - sl) * 2.2, 0.0);
  vec3 ink = min(mix(dd.rgb * g, absorb(texture(uTgt, at)), uCure) + 0.85 * sc * g, vec3(4.0));
  A += ink * (1.0 + 0.2 * wet);
  // The darkening P*T over the known page colour P, written as a premultiplied
  // pixel so the canvas is truly clear where nothing is inked or wet: alpha is
  // the deepest channel's absorption and the colour carries the rest.
  // Clear the pigment along the paper grain, without a coloured overlay.
  A *= mix(.22, 1.0, smoothstep(uClearance - .12, uClearance + .12, pap.g));
  vec3 T = clamp(exp(-A), 0.0, 1.0); float a = 1.0 - min(T.r, min(T.g, T.b));
  o = vec4(uTint * (T - (1.0 - a)), a);
}`;
  }
  var GROW = HEAD + `
out vec4 o;
void main(){
  float need = foot(vUv) > 0.5 ? 0.0 : 2.0;
  for (int k = 1; k <= 8 && need > 1.0; k++) {
    float r = float(k) / 8.0;
    for (int i = 0; i < 12; i++) {
      float a = float(i) * 0.5236; vec2 d = vec2(cos(a), sin(a));
      float reach = 0.11 * r * (0.55 + 0.9 * P(vUv * uSize * 0.5 + d * 37.0).g);
      if (foot(clamp(vUv - d * reach, 0.0, 1.0)) > 0.5) need = r;
    }
  }
  o = vec4(need, 0.0, 0.0, 1.0);
}`;
  var PACK = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform sampler2D uW, uS, uD;
layout(location=0) out vec4 oA; layout(location=1) out vec4 oB;
void main(){ oA = vec4(texture(uS, vUv).rgb, texture(uW, vUv).r); oB = texture(uD, vUv); }`;
  var PLAY = HEAD + `
uniform sampler2D uXa0, uXb0, uXa1, uXb1, uYa0, uYb0, uYa1, uYb1, uXP, uYP;
uniform float uFx, uFy, uK;
out vec4 o;
vec3 side(sampler2D a0, sampler2D b0, sampler2D a1, sampler2D b1, float f, sampler2D pr, vec2 uv, float g){
  vec2 px = 1.0 / uSize;
  vec4 sh = mix(texture(a0, uv), texture(a1, uv), f), dl = mix(texture(b0, uv), texture(b1, uv), f);
  float wet = wetOf(sh.a);
  float hx = mix(texture(a0, uv + vec2(px.x, 0.0)).a, texture(a1, uv + vec2(px.x, 0.0)).a, f) - mix(texture(a0, uv - vec2(px.x, 0.0)).a, texture(a1, uv - vec2(px.x, 0.0)).a, f);
  float hy = mix(texture(a0, uv + vec2(0.0, px.y)).a, texture(a1, uv + vec2(0.0, px.y)).a, f) - mix(texture(a0, uv - vec2(0.0, px.y)).a, texture(a1, uv - vec2(0.0, px.y)).a, f);
  vec2 at = uv + vec2(hx, hy) * 0.03 * wet;
  vec3 s = sh.rgb; float sl = dot(s, vec3(1.0 / 3.0)); vec3 sc = max(sl + (s - sl) * 2.2, 0.0);
  vec3 ink = min(dl.rgb * g + 0.85 * sc * g, vec3(4.0));
  return absorb(texture(pr, at)) * (1.0 - dl.a) + ink * (1.0 + 0.2 * wet);
}
void main(){
  vec2 uv = vUv; vec4 pap = P(uv * uSize);
  float g = max(1.0 + 0.4 * ((pap.r - 0.5) * 1.4 + (pap.b - 0.5) * 0.6), 0.05);
  vec3 A = uK <= 0.0 ? side(uXa0, uXb0, uXa1, uXb1, uFx, uXP, uv, g)
         : uK >= 1.0 ? side(uYa0, uYb0, uYa1, uYb1, uFy, uYP, uv, g)
         : mix(side(uXa0, uXb0, uXa1, uXb1, uFx, uXP, uv, g), side(uYa0, uYb0, uYa1, uYb1, uFy, uYP, uv, g), uK);
  vec3 T = clamp(exp(-A), 0.0, 1.0); float a = 1.0 - min(T.r, min(T.g, T.b));
  o = vec4(uTint * (T - (1.0 - a)), a);
}`;

  // packages/moss-watercolor/src/engine/math.ts
  var clamp01 = (v) => Math.min(1, Math.max(0, v));
  var smooth = (a, b, t) => {
    const x = clamp01((t - a) / (b - a));
    return x * x * (3 - 2 * x);
  };

  // packages/moss-watercolor/src/engine/preset.ts
  var DEFAULT_PRESET = {
    dt: 1 / 120,
    stepsPerFrame: 36,
    tSplash: 0.2,
    tDry: 1.2,
    tTake: 1.2,
    tCure: 0.9,
    tTotal: 2.1,
    recDuration: 1.7,
    recEvery: 8,
    recStir: (t) => 1.5 * smooth(0.25, 0.9, t),
    recOpen: (t) => smooth(0.45, 1.35, t),
    recRain: (t) => 0.012 * smooth(0.45, 0.7, t) * (1 - smooth(1.4, 1.7, t)),
    recFlood: (t) => smooth(1, 1.3, t),
    recRinseFrom: 0.9,
    recRinse: 0.02,
    recLight: 0.28,
    playOut: 0.42,
    playIn: 0.58
  };
  function recordingSteps(preset) {
    return Math.round(preset.recDuration / preset.dt);
  }
  function recordingFrameCount(preset) {
    return Math.ceil(recordingSteps(preset) / preset.recEvery) + 1;
  }

  // packages/moss-watercolor/src/paper/random.ts
  function seededRandom(seed) {
    let a0 = seed;
    return () => {
      a0 = a0 + 1831565813 | 0;
      let t = Math.imul(a0 ^ a0 >>> 15, 1 | a0);
      t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
      return ((t ^ t >>> 14) >>> 0) / 4294967296;
    };
  }

  // packages/moss-watercolor/src/paper/default.ts
  function createPaper({ width = 256, height = 256, seed = 90210 } = {}) {
    const rnd = seededRandom(seed);
    const lattice = (n) => {
      const g = new Float32Array(n * n);
      for (let i = 0; i < g.length; i++) g[i] = rnd();
      return g;
    };
    const sm = (t) => t * t * (3 - 2 * t);
    const value = (g, n, x, y) => {
      const gx = x * n, gy = y * n, x0 = Math.floor(gx) % n, y0 = Math.floor(gy) % n, x1 = (x0 + 1) % n, y1 = (y0 + 1) % n, fx = sm(gx - Math.floor(gx)), fy = sm(gy - Math.floor(gy));
      const a = g[y0 * n + x0], b = g[y0 * n + x1], c = g[y1 * n + x0], d = g[y1 * n + x1];
      return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy;
    };
    const octs = [4, 8, 16, 32, 64, 128];
    const fbm = (lats, x, y, o0) => {
      let s = 0, amp = 1, tot = 0;
      for (let o = o0; o < octs.length; o++) {
        s += amp * value(lats[o], octs[o], x, y);
        tot += amp;
        amp *= 0.55;
      }
      return s / tot;
    };
    const L = [0, 1, 2, 3].map(() => octs.map(lattice));
    const ch = [0, 1, 2, 3].map(() => new Float32Array(width * height));
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const i = y * width + x, u = x / width, v = y / height;
      ch[0][i] = fbm(L[0], u, v, 2) * 0.7 + 0.3 * rnd();
      ch[1][i] = fbm(L[1], u, v, 1);
      ch[2][i] = fbm(L[2], u, v, 0);
      ch[3][i] = fbm(L[3], u, v, 3);
    }
    const px = new Uint8Array(width * height * 4);
    for (let k = 0; k < 4; k++) {
      let lo = 1, hi = 0;
      for (const t of ch[k]) {
        lo = Math.min(lo, t);
        hi = Math.max(hi, t);
      }
      for (let i = 0; i < width * height; i++) px[i * 4 + k] = (ch[k][i] - lo) / (hi - lo) * 255;
    }
    return { width, height, data: px };
  }

  // packages/moss-watercolor/src/engine/solver.ts
  function createSim(opts) {
    const { canvas, texW, texH, divisor, rect, diagMode = 0 } = opts;
    const preset = { ...DEFAULT_PRESET, ...opts.preset };
    const paperSource = opts.paper ?? createPaper();
    if (paperSource.width !== 256 || paperSource.height !== 256) {
      throw new Error(`createSim: paper must be 256x256 (got ${paperSource.width}x${paperSource.height}); the paper period is still a shader constant, not a uniform`);
    }
    const REC_STEPS = recordingSteps(preset), REC_FRAMES = recordingFrameCount(preset);
    const gl = canvas.getContext("webgl2", { alpha: true, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: true });
    if (!gl || !gl.getExtension("EXT_color_buffer_float")) return null;
    const contextLostHandlers = /* @__PURE__ */ new Set();
    let listening = false;
    function onContextLost(handler) {
      if (!listening) {
        listening = true;
        canvas.addEventListener("webglcontextlost", (event) => {
          event.preventDefault();
          for (const h of contextLostHandlers) h(event);
        });
      }
      contextLostHandlers.add(handler);
      return () => contextLostHandlers.delete(handler);
    }
    const compile = (type, src) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) || "shader compile failed");
      return s;
    };
    const W = Math.round(texW / divisor), H = Math.round(texH / divisor);
    const tint = (() => {
      const c = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim();
      const n = parseInt(c.slice(1), 16);
      return [(n >> 16) / 255, (n >> 8 & 255) / 255, (n & 255) / 255];
    })();
    const UNITS = {
      uW: 0,
      uS: 1,
      uD: 2,
      uNear: 3,
      uWhole: 4,
      uPaper: 8,
      uSrc: 9,
      uTgt: 10,
      uSrcLo: 11,
      uTgtLo: 12,
      uSrcFt: 13,
      uTgtFt: 14,
      uXa0: 0,
      uXb0: 1,
      uXa1: 2,
      uXb1: 3,
      uYa0: 4,
      uYb0: 5,
      uYa1: 6,
      uYb1: 7,
      uXP: 9,
      uYP: 10,
      uGrow: 15
    };
    const prog = (fs) => {
      const p = gl.createProgram();
      gl.attachShader(p, compile(gl.VERTEX_SHADER, V));
      gl.attachShader(p, compile(gl.FRAGMENT_SHADER, fs));
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) || "program link failed");
      const u = {};
      const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
      for (let i = 0; i < n; i++) {
        const nm = gl.getActiveUniform(p, i).name;
        u[nm] = gl.getUniformLocation(p, nm);
      }
      gl.useProgram(p);
      for (const k in UNITS) if (u[k]) gl.uniform1i(u[k], UNITS[k]);
      if (u.uSize) gl.uniform2f(u.uSize, W, H);
      if (u.uTint) gl.uniform3f(u.uTint, tint[0], tint[1], tint[2]);
      return { p, u };
    };
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    gl.disable(gl.BLEND);
    let water, pig, show, mean, pack, play, grow;
    try {
      water = prog(WATER);
      pig = prog(PIG);
      show = prog(buildShowShader(diagMode));
      mean = prog(MEAN);
      pack = prog(PACK);
      play = prog(PLAY);
      grow = prog(GROW);
    } catch (e) {
      console.error("wash sim unavailable, cutting instead:", e.message);
      return null;
    }
    let nStep = 0;
    const tex = (w, h, wrap = gl.CLAMP_TO_EDGE, float = true) => {
      const t = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
      if (float) gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, w, h, 0, gl.RGBA, gl.HALF_FLOAT, null);
      return t;
    };
    const fbo = (texes) => {
      const f = gl.createFramebuffer();
      gl.bindFramebuffer(gl.FRAMEBUFFER, f);
      texes.forEach((t, i) => gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0 + i, gl.TEXTURE_2D, t, 0));
      gl.drawBuffers(texes.map((_, i) => gl.COLOR_ATTACHMENT0 + i));
      return f;
    };
    const wT = [tex(W, H), tex(W, H)], wF = wT.map((t) => fbo([t]));
    const pT = [0, 1].map(() => [tex(W, H), tex(W, H)]), pF = pT.map((ts) => fbo(ts));
    let wi = 0, pi = 0;
    const BLOCK = 16, NW = Math.ceil(W / BLOCK), NH = Math.ceil(H / BLOCK);
    const nearT = tex(NW, NH), nearF = fbo([nearT]), wholeT = tex(1, 1), wholeF = fbo([wholeT]);
    const growT = tex(W, H), growF = fbo([growT]);
    const means = (S, Wt) => {
      gl.useProgram(mean.p);
      gl.viewport(0, 0, NW, NH);
      gl.bindFramebuffer(gl.FRAMEBUFFER, nearF);
      bind(0, S);
      bind(1, Wt);
      gl.uniform1i(mean.u.uA, 0);
      gl.uniform1i(mean.u.uB, 1);
      gl.uniform2f(mean.u.uInSize, W, H);
      gl.uniform1i(mean.u.uBlock, BLOCK);
      gl.uniform1i(mean.u.uJoin, 1);
      draw();
      gl.viewport(0, 0, 1, 1);
      gl.bindFramebuffer(gl.FRAMEBUFFER, wholeF);
      bind(0, nearT);
      gl.uniform1i(mean.u.uA, 0);
      gl.uniform2f(mean.u.uInSize, NW, NH);
      gl.uniform1i(mean.u.uBlock, 64);
      gl.uniform1i(mean.u.uJoin, 0);
      draw();
    };
    const paper = tex(paperSource.width, paperSource.height, gl.REPEAT, false);
    gl.bindTexture(gl.TEXTURE_2D, paper);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, paperSource.width, paperSource.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, paperSource.data);
    const prints = { src: null, tgt: null };
    const scaled = (im, w, h) => {
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      const g = c.getContext("2d");
      g.imageSmoothingQuality = "high";
      let cur = im;
      while (cur.width > w * 2) {
        const m = document.createElement("canvas");
        m.width = Math.ceil(cur.width / 2);
        m.height = Math.ceil(cur.height / 2);
        const mg = m.getContext("2d");
        mg.imageSmoothingQuality = "high";
        mg.drawImage(cur, 0, 0, m.width, m.height);
        cur = m;
      }
      g.drawImage(cur, 0, 0, w, h);
      return c;
    };
    const upload = (t, im) => {
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, im);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    };
    const setPrint = (key, im) => {
      const p = prints[key] || { full: tex(0, 0, gl.CLAMP_TO_EDGE, false), lo: tex(0, 0, gl.CLAMP_TO_EDGE, false), ft: tex(0, 0, gl.CLAMP_TO_EDGE, false) };
      upload(p.full, im);
      upload(p.lo, scaled(im, W, H));
      upload(p.ft, scaled(im, Math.round(W / 4), Math.round(H / 4)));
      prints[key] = p;
    };
    const bind = (unit, t) => {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(gl.TEXTURE_2D, t);
    };
    const draw = () => gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    const common = (pr, fwd) => {
      const [S, T] = fwd ? [prints.src, prints.tgt] : [prints.tgt, prints.src];
      gl.useProgram(pr.p);
      bind(8, paper);
      bind(9, S.full);
      bind(10, T.full);
      bind(11, S.lo);
      bind(12, T.lo);
      bind(13, S.ft);
      bind(14, T.ft);
    };
    const place = () => {
      const dpr = Math.min(2, devicePixelRatio || 1), r = rect();
      const cw = Math.round(r.w * dpr), chh = Math.round(r.h * dpr);
      if (canvas.width !== cw || canvas.height !== chh) {
        canvas.width = cw;
        canvas.height = chh;
      }
      canvas.style.left = r.x + "px";
      canvas.style.top = r.y + "px";
      canvas.style.right = "auto";
      canvas.style.bottom = "auto";
      canvas.style.width = r.w + "px";
      canvas.style.height = r.h + "px";
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, cw, chh);
    };
    const keep = () => {
      const s = tex(W, H), d = tex(W, H), fb = fbo([s, d]);
      gl.viewport(0, 0, W, H);
      gl.useProgram(pack.p);
      bind(0, wT[wi]);
      bind(1, pT[pi][0]);
      bind(2, pT[pi][1]);
      draw();
      return { s, d, fb };
    };
    const freeFrames = (side) => {
      for (const k of side.frames) {
        gl.deleteFramebuffer(k.fb);
        gl.deleteTexture(k.s);
        gl.deleteTexture(k.d);
      }
      side.frames = [];
      side.n = 0;
    };
    let owner = null, recording = false, grownFor = null;
    const api = {
      // lets the GPU have the context back: a discarded simulation must not count against the page's live ones
      dispose() {
        gl.getExtension("WEBGL_lose_context")?.loseContext();
      },
      setPrints(src, tgt) {
        setPrint("src", src);
        setPrint("tgt", tgt);
      },
      reset() {
        owner = null;
        gl.viewport(0, 0, W, H);
        gl.clearColor(0, 0, 0, 0);
        for (const f of [...wF, ...pF, nearF, wholeF]) {
          gl.bindFramebuffer(gl.FRAMEBUFFER, f);
          gl.clear(gl.COLOR_BUFFER_BIT);
        }
      },
      // One fixed step at time t (seconds). The schedule: flood, dissolve and
      // stir, take up, dry and cure.
      // A `standing` film never dries or takes up, however long it runs; its
      // sheet opens by `open` toward the whole paper while `rain` water is added
      // over it; `rinse` flushes clean water through it and `dose` charges the print's ink.
      step(fwd, t, cure, stir = 0, tilt = 0, relift = 0, { standing = false, open = 0, rain = 0, rinse = 0, light = preset.recLight, dose = 1, lift = 1 } = {}) {
        if (!recording) owner = null;
        const splash = t < preset.tSplash ? 0.05 : 0, mist = t < preset.tSplash + 0.15 ? 0.03 : 0;
        const drying = standing ? 0 : smooth(preset.tDry, preset.tDry + 0.4, t);
        const take = standing ? 0 : smooth(preset.tTake, preset.tTake + 0.45, t);
        gl.viewport(0, 0, W, H);
        common(water, fwd);
        bind(15, growT);
        gl.bindFramebuffer(gl.FRAMEBUFFER, wF[1 - wi]);
        bind(0, wT[wi]);
        gl.uniform1f(water.u.uSplash, splash);
        gl.uniform1f(water.u.uMist, mist);
        gl.uniform1f(water.u.uCure, cure);
        gl.uniform1f(water.u.uEvap, 8e-4 + 0.016 * drying);
        gl.uniform1f(water.u.uDrying, drying);
        gl.uniform2f(water.u.uTilt, 0, tilt);
        gl.uniform1f(water.u.uOpen, open);
        gl.uniform1f(water.u.uRain, rain);
        draw();
        wi = 1 - wi;
        if (nStep++ % 3 === 0) means(pT[pi][0], wT[wi]);
        gl.viewport(0, 0, W, H);
        common(pig, fwd);
        gl.bindFramebuffer(gl.FRAMEBUFFER, pF[1 - pi]);
        bind(0, wT[wi]);
        bind(1, pT[pi][0]);
        bind(2, pT[pi][1]);
        bind(3, nearT);
        bind(4, wholeT);
        gl.uniform1f(pig.u.uTime, t);
        gl.uniform1f(pig.u.uCure, cure);
        gl.uniform1f(pig.u.uLift, 0.075 * lift);
        gl.uniform1f(pig.u.uAds, 0.18 * take);
        gl.uniform1f(pig.u.uMix, 0.06);
        gl.uniform1f(pig.u.uMixG, 5e-3 + 0.025 * Math.max(drying, Math.min(1, stir)));
        gl.uniform1f(pig.u.uDrying, drying);
        gl.uniform1f(pig.u.uStir, stir);
        gl.uniform1f(pig.u.uRelift, relift);
        gl.uniform1f(pig.u.uRinse, rinse);
        gl.uniform1f(pig.u.uLight, light);
        gl.uniform1f(pig.u.uDose, dose);
        draw();
        pi = 1 - pi;
      },
      // read one texel of the state, for the harness: [h,u,v,sat], s.rgb, [d.rgb,l]
      probe(x, y) {
        const out = [];
        for (const [f, n] of [[wF[wi], 1], [pF[pi], 2]]) {
          gl.bindFramebuffer(gl.FRAMEBUFFER, f);
          for (let i = 0; i < n; i++) {
            gl.readBuffer(gl.COLOR_ATTACHMENT0 + i);
            const px = new Float32Array(4);
            gl.readPixels(x, y, 1, 1, gl.RGBA, gl.FLOAT, px);
            out.push([...px].map((v) => +v.toFixed(4)));
          }
        }
        return out;
      },
      // the display pass onto the canvas
      draw(fwd, cure, clearance = -0.2) {
        place();
        common(show, fwd);
        bind(0, wT[wi]);
        bind(1, pT[pi][0]);
        bind(2, pT[pi][1]);
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
      pair({ dose = 1, stir = 1, rinse = preset.recRinse, light = preset.recLight, lift = 1, flood = false, only = null } = {}) {
        return { sides: [true, false].map((fwd) => ({ fwd, frames: [], n: 0, dose: fwd ? dose : 1, stir: fwd ? stir : 1, lift: fwd ? lift : 1, rinse: fwd ? rinse : preset.recRinse, light: fwd ? light : preset.recLight, flood: fwd && flood, skip: only !== null && fwd !== only })) };
      },
      // Records up to `budget` steps, the side the transition needs first first:
      // the two sides share the simulation's state, so one runs at a time.
      record(pair, firstFwd, budget) {
        for (const side of firstFwd ? pair.sides : [...pair.sides].reverse()) {
          if (side.skip || side.n >= REC_STEPS) continue;
          if (side.frames.length && owner !== side) freeFrames(side);
          if (!side.frames.length) {
            if (grownFor !== pair) {
              gl.viewport(0, 0, W, H);
              common(grow, true);
              gl.bindFramebuffer(gl.FRAMEBUFFER, growF);
              draw();
              grownFor = pair;
            }
            api.reset();
            nStep = 0;
            owner = side;
            side.frames.push(keep());
          }
          recording = true;
          while (budget-- > 0 && side.n < REC_STEPS) {
            const t = side.n * preset.dt;
            const flood = side.flood ? preset.recFlood(t) : 0;
            api.step(
              side.fwd,
              t,
              0,
              preset.recStir(t) * side.stir,
              0,
              flood,
              { standing: true, open: preset.recOpen(t) + 2 * flood, rain: Math.max(preset.recRain(t), 0.12 * flood), rinse: t >= preset.recRinseFrom ? side.rinse : 0, light: side.light, dose: side.dose, lift: side.lift }
            );
            if (++side.n % preset.recEvery === 0 || side.n === REC_STEPS) side.frames.push(keep());
          }
          recording = false;
          return;
        }
      },
      recorded(pair) {
        return pair.sides.every((side) => side.skip || side.n >= REC_STEPS);
      },
      // a frame of a recording on screen: a frame's budget recorded ahead, then p
      // drawn; true while there is more to record, so the caller comes back
      show(pair, fromFwd, p) {
        api.record(pair, fromFwd, preset.stepsPerFrame);
        api.play(pair, fromFwd, p);
        return !api.recorded(pair);
      },
      free(pair) {
        for (const side of pair.sides) freeFrames(side);
      },
      // p in [0, 1] from the outgoing scene (fromFwd: the lower of the pair) to the incoming.
      play(pair, fromFwd, p) {
        const [out, inc] = fromFwd ? pair.sides : [pair.sides[1], pair.sides[0]];
        const at = (side, u) => {
          const n = side.frames.length;
          if (!n) return null;
          const x2 = Math.min(clamp01(u) * (REC_FRAMES - 1), n - 1), i = Math.floor(x2), j = Math.min(i + 1, n - 1);
          return { k0: side.frames[i], k1: side.frames[j], f: x2 - i, print: (side.fwd ? prints.src : prints.tgt).full };
        };
        let x = at(out, p / preset.playOut), y = at(inc, (1 - p) / (1 - preset.playIn)), k = smooth(preset.playOut, preset.playIn, p);
        if (!y) {
          y = x;
          k = 0;
        }
        if (!x) return false;
        place();
        gl.useProgram(play.p);
        bind(0, x.k0.s);
        bind(1, x.k0.d);
        bind(2, x.k1.s);
        bind(3, x.k1.d);
        bind(4, y.k0.s);
        bind(5, y.k0.d);
        bind(6, y.k1.s);
        bind(7, y.k1.d);
        bind(8, paper);
        bind(9, x.print);
        bind(10, y.print);
        gl.uniform1f(play.u.uFx, x.f);
        gl.uniform1f(play.u.uFy, y.f);
        gl.uniform1f(play.u.uK, k);
        draw();
        return true;
      },
      onContextLost
    };
    return api;
  }

  // packages/moss-watercolor/src/engine/host.ts
  function advance(sim, state, goal, fwd, stir, tilt, relift, paint, preset = DEFAULT_PRESET) {
    if (state.t > goal + preset.dt) {
      sim.reset();
      state.t = 0;
      state.drawn = -1;
    }
    let budget = preset.stepsPerFrame, count = 0;
    while (state.t < goal && state.t < preset.tTotal && budget-- > 0) {
      sim.step(fwd, state.t, smooth(preset.tCure, preset.tTotal, state.t), stir, tilt, relift);
      state.t += preset.dt;
      count++;
    }
    if (state.t >= goal - preset.dt && state.t !== state.drawn) {
      paint(state.t);
      state.drawn = state.t;
    }
    return { caughtUp: state.t >= goal - preset.dt, count };
  }

  // packages/moss-watercolor/src/model/index.ts
  var DEFAULT_PHASE_BOUNDS = { outEnd: DEFAULT_PRESET.playOut, inStart: DEFAULT_PRESET.playIn };
  function phaseOf(p, bounds = DEFAULT_PHASE_BOUNDS) {
    if (p <= bounds.outEnd) return "out";
    if (p < bounds.inStart) return "mix";
    return "in";
  }
  function retarget(from, to, p, next, bounds = DEFAULT_PHASE_BOUNDS) {
    const { outEnd, inStart } = bounds;
    const phase = phaseOf(p, bounds);
    const incoming = Math.min(1, (1 - p) / (1 - inStart));
    if (next === from) {
      const reversed = phase === "out" ? 1 - (1 - inStart) * p / outEnd : phase === "mix" ? outEnd + inStart - p : outEnd * incoming;
      return { from: to, to: from, p: reversed };
    }
    if (phase === "out" || phase === "mix" && smooth(outEnd, inStart, p) < 0.5) return { from, to: next, p };
    return { from: to, to: next, p: outEnd * incoming };
  }
  return __toCommonJS(stdin_exports);
})();
