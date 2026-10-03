// Curtis, Anderson, Seims, Fleischer and Salesin, "Computer-Generated
// Watercolor", SIGGRAPH 1997: a shallow-water film (depth, velocity, fibre
// saturation), paper relief, and a rim current at the wet edge. These
// shaders are an original implementation of that model -- no source from
// any other implementation is reused.
//
// WATER/PIG/MEAN/SHOW/GROW step and display one film. PACK/PLAY are the
// recording-and-playback half: PACK keeps only what a frame needs to be
// replayed later (liquid+depth, deposit+dissolved-fraction); PLAY blends two
// recorded frames per side and crosses between a transition's outgoing and
// incoming side, in optical thickness rather than in pixels.
export const V = `#version 300 es
in vec2 q; out vec2 vUv; void main(){ vUv = q; gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); }`;

export const HEAD = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform vec2 uSize; uniform sampler2D uPaper; uniform vec3 uTint; uniform float uDark;
// The one endpoint envelope (engine/math.ts endpointPresence): 0 at either end of a leg, 1 mid-leg.
// uEnd is which print that end is: 0 the outgoing, 1 the incoming.
uniform float uPresence, uEnd;
// A transparent ground (uGround, set once) draws the wash over nothing, not over the page; uDrain
// (engine/math.ts drain) is how much of its pigment a position keeps: 1 at either end, the preset's drainFloor mid-leg.
uniform float uGround, uDrain;
// The prints at three resolutions, each a plain texture: full, the sim grid,
// and a quarter of it for the footprint. They are downscaled on the 2D canvas
// rather than by generateMipmap, so no driver's mip chain is in the picture.
uniform sampler2D uSrc, uTgt, uSrcLo, uTgtLo, uSrcFt, uTgtFt;
vec4 P(vec2 t){ return mix(texture(uPaper, t / 256.0), texture(uPaper, t / 977.0 + 0.37), 0.45); }
float wetOf(float h){ return smoothstep(0.0004, 0.004, h); }
// On a light page the pigment is shadow: ink density is what a printed pixel
// takes out of the page's light; a soft-edged or shadowed pixel is its colour
// laid over the page at its alpha. On a dark page (uDark) the same model runs
// on the complement: the page is dark paper, a print's light marks are the
// pigment, and a pixel's density is how far its complement falls below the
// paper's, so the pigment keeps its own colour and clear sheet is the paper.
// A page channel at 1 has no complement to take density from, so on a dark page that channel is clear
// whatever the print holds there (dividing by it gives 0/0, and a stand-in epsilon would read the clear
// sheet itself as dense in that channel and drown the others' pigment).
vec3 absorb(vec4 c){
  vec3 d = 1.0 - uTint;
  vec3 r = uDark > 0.5 ? mix((1.0 - c.rgb) / max(d, vec3(1e-3)), vec3(1.0), lessThan(d, vec3(1e-3))) : c.rgb / uTint;
  return -log(max(mix(vec3(1.0), clamp(r, 0.0, 1.0), c.a), vec3(0.02)));
}
// The pixel the display writes for a density A. Light page: a premultiplied
// transmittance layer, clear where nothing is inked or wet, so the host's page
// shows through. Dark page: opaque, the paper itself with the light pigment on
// it; and since a mark darker than the paper has no density to show, the
// plain print (at either end of a leg: the print itself) is blended back in by
// the envelope, so the ends are exactly the print.
vec4 composite(vec3 A, vec4 plain){
  if (uGround > 0.5) {
    // Transparent ground: the drained pigment over nothing, with the page's own paper under it only
    // while the wash has not yet begun (opaque at the ends, gone by the end of the endpoint margin).
    // The pigment is a premultiplied layer (colour c, coverage a) that, over the paper colour, gives
    // the opaque pixel above. Light: c = tint*(T-m), the shadow layer. Dark: c = (1-tint)(1-T) +
    // tint*a, the light pigment; both lie in [0, a], so plain source-over places them on any background.
    // The pigment's thickness is a pure scale (uDrain), so a thinned film stays translucent.
    vec3 T = clamp(exp(-A * uDrain), 0.0, 1.0); float m = min(T.r, min(T.g, T.b)), a = 1.0 - m;
    vec3 c = uDark > 0.5 ? (1.0 - uTint) * (1.0 - T) + uTint * a : uTint * (T - m);
    float g = 1.0 - uPresence;
    vec4 s = vec4(c + (1.0 - a) * g * uTint, a + (1.0 - a) * g);
    // a print rarely sits on exactly the paper colour, and a mark on the wrong side of it has no
    // density to show: the plain print itself, opaque, covers the ends
    return mix(vec4(mix(uTint, plain.rgb, plain.a), 1.0), s, uPresence);
  }
  vec3 T = clamp(exp(-A), 0.0, 1.0); float a = 1.0 - min(T.r, min(T.g, T.b));
  if (uDark > 0.5) return vec4(mix(mix(uTint, plain.rgb, plain.a), 1.0 - (1.0 - uTint) * T, uPresence), 1.0);
  return vec4(uTint * (T - (1.0 - a)), a);
}
// Everything the wash does to a print goes through these two, so the envelope
// lives in one place: the print seen through the water bends only as far as
// it is present, and the shown density is the plain print's at either end.
vec2 bend(vec2 uv, vec2 slope, float wet){ return uv + slope * 0.03 * wet * uPresence; }
vec3 present(vec3 shown, vec3 plain){ return mix(plain, shown, uPresence); }
// the wetted sheet: the prints' footprint blurred (coarse mip) and broken by
// the fibres, so the wet edge is ragged like a wet-in-wet wash, not a frame
// the sheet under the scene: the union of the two prints' silhouettes (not their
// soft shadows), blurred a little and broken by the fibres; and the next
// print's own silhouette, which is where the film gathers as it dries
float sil(float a, vec2 uv){ vec4 pap = P(uv * uSize); return smoothstep(0.5, 0.9, a + (pap.g - 0.5) * 0.35 + (pap.b - 0.5) * 0.15); }
float foot(vec2 uv){ return sil(max(texture(uSrcFt, uv).a, texture(uTgtFt, uv).a), uv); }
float footT(vec2 uv){ return sil(texture(uTgtFt, uv).a, uv); }`;

// Water: (depth, u, v, saturation).
export const WATER = HEAD + `
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

// Pigment: one liquid (suspended ink density, rgb), the fixed deposit (rgb),
// and the sheet's record of how much of the old print has dissolved (l).
export const PIG = HEAD + `
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

// Means: each texel the box average of a uBlock-square of (liquid rgb, depth)
// read from uS and uW; run again over its own output for the whole-film mean.
export const MEAN = `#version 300 es
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

// Each DIAG_VIEWS[n] is spliced into SHOW in place of its normal display math
// (`buildShowShader`, below), for a caller that wants one state layer shown
// raw and opaque instead of the composited wash -- useful on a GPU whose
// driver is suspected of corrupting one particular texture. The choice is an
// explicit option (`diagMode` on `createSim`; 0, the default, leaves the
// normal composite), so the engine makes no assumption about where a page's
// own debug UI keeps that number, such as a `?diag=N` URL parameter.
export const DIAG_VIEWS: Record<number, string> = {
  4: 'exp(-texture(uS, uv).rgb * 3.0)',                     // liquid
  5: 'exp(-texture(uD, uv).rgb)',                           // deposit
  6: 'vec3(texture(uW, uv).r * 3.0)',                       // water depth
  8: 'vec3(texture(uD, uv).a)',                             // how much of the old print has dissolved
  9: 'vec3(0.5 + texture(uW, uv).gb * 0.5, 0.5)',           // velocity: red +x, green +y, grey still
  10: 'vec3(length(bend(vec2(0.0), gh, wet)) * uSize.x * 0.5)', // refraction offset, as the display bends it
};

// Display: the light the sheet takes out of the page, as a multiply layer.
export function buildShowShader(diagMode = 0): string {
  const diagView = DIAG_VIEWS[diagMode];
  return HEAD + `
uniform sampler2D uW, uS, uD; uniform float uCure, uClearance;
out vec4 o;
void main(){
  vec2 uv = vUv; vec2 px = 1.0 / uSize;
  vec4 w = texture(uW, uv); float h = w.r; float wet = wetOf(h);
  // the print seen through standing water bends with the surface
  vec2 gh = vec2(texture(uW, uv + vec2(px.x, 0.0)).r - texture(uW, uv - vec2(px.x, 0.0)).r, texture(uW, uv + vec2(0.0, px.y)).r - texture(uW, uv - vec2(0.0, px.y)).r);
  vec2 at = bend(uv, gh, wet);
  ${diagView ? `o = vec4(${diagView}, 1.0); return;` : ''}
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
  vec4 plain = uEnd > .5 ? texture(uTgt, uv) : texture(uSrc, uv);
  o = composite(present(A, absorb(plain)), plain);
}`;
}

// How far the two prints' footprint must grow to reach each place, as a
// fraction of the widest reach, ragged along the fibres: 0 on the footprint,
// 2 out of reach. Drawn once per boundary for the water's sheet().
export const GROW = HEAD + `
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

// A recorded dispersion keeps, per frame, only what the display reads:
// (liquid rgb, depth) and (deposit rgb, how much of the print has dissolved).
export const PACK = `#version 300 es
precision highp float; precision highp sampler2D;
in vec2 vUv; uniform sampler2D uW, uS, uD;
layout(location=0) out vec4 oA; layout(location=1) out vec4 oB;
void main(){ oA = vec4(texture(uS, vUv).rgb, texture(uW, vUv).r); oB = texture(uD, vUv); }`;

// Display of a mobile transition, played from two recorded dispersions: side X
// between two of its frames, side Y likewise, and uK between the two sides.
// The sides mix as optical thickness, pigment into pigment, never as pixels.
export const PLAY = HEAD + `
uniform sampler2D uXa0, uXb0, uXa1, uXb1, uYa0, uYb0, uYa1, uYb1, uXP, uYP;
uniform float uFx, uFy, uK;
out vec4 o;
vec3 side(sampler2D a0, sampler2D b0, sampler2D a1, sampler2D b1, float f, sampler2D pr, vec2 uv, float g){
  vec2 px = 1.0 / uSize;
  vec4 sh = mix(texture(a0, uv), texture(a1, uv), f), dl = mix(texture(b0, uv), texture(b1, uv), f);
  float wet = wetOf(sh.a);
  float hx = mix(texture(a0, uv + vec2(px.x, 0.0)).a, texture(a1, uv + vec2(px.x, 0.0)).a, f) - mix(texture(a0, uv - vec2(px.x, 0.0)).a, texture(a1, uv - vec2(px.x, 0.0)).a, f);
  float hy = mix(texture(a0, uv + vec2(0.0, px.y)).a, texture(a1, uv + vec2(0.0, px.y)).a, f) - mix(texture(a0, uv - vec2(0.0, px.y)).a, texture(a1, uv - vec2(0.0, px.y)).a, f);
  vec2 at = bend(uv, vec2(hx, hy), wet);
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
  vec4 plain = uEnd > .5 ? texture(uYP, uv) : texture(uXP, uv);
  o = composite(present(A, absorb(plain)), plain);
}`;
