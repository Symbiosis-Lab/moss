import { t as clamp01 } from "../math-CZ65GQ6j.mjs";
import { t as contextFor } from "../context-8HSrqj5R.mjs";

//#region src/sphere/state.ts
/** Preserve the caller's complete state footprint, including separate read/draw targets.
* Uniforms belong only to the sphere program; no caller-owned object is modified.
*/
function withSphereState(gl, draw) {
	const get = (p) => gl.getParameter(p);
	const program = get(gl.CURRENT_PROGRAM), vao = get(gl.VERTEX_ARRAY_BINDING);
	const framebuffer = get(gl.DRAW_FRAMEBUFFER_BINDING), viewport = get(gl.VIEWPORT);
	const active = get(gl.ACTIVE_TEXTURE), mask = get(gl.COLOR_WRITEMASK), colour = get(gl.BLEND_COLOR);
	const blend = [
		gl.BLEND_SRC_RGB,
		gl.BLEND_DST_RGB,
		gl.BLEND_SRC_ALPHA,
		gl.BLEND_DST_ALPHA
	].map(get);
	const equation = [get(gl.BLEND_EQUATION_RGB), get(gl.BLEND_EQUATION_ALPHA)];
	const caps = [
		gl.BLEND,
		gl.DEPTH_TEST,
		gl.STENCIL_TEST,
		gl.SCISSOR_TEST,
		gl.CULL_FACE,
		gl.RASTERIZER_DISCARD,
		gl.SAMPLE_ALPHA_TO_COVERAGE,
		gl.SAMPLE_COVERAGE
	];
	const enabled = caps.map((cap) => gl.isEnabled(cap));
	const textures = [], samplers = [];
	for (let unit = 0; unit < 8; unit++) {
		gl.activeTexture(gl.TEXTURE0 + unit);
		textures.push(get(gl.TEXTURE_BINDING_2D));
		samplers.push(get(gl.SAMPLER_BINDING));
	}
	try {
		draw();
	} finally {
		for (let unit = 0; unit < 8; unit++) {
			gl.activeTexture(gl.TEXTURE0 + unit);
			gl.bindTexture(gl.TEXTURE_2D, textures[unit]);
			gl.bindSampler(unit, samplers[unit]);
		}
		gl.activeTexture(active);
		gl.useProgram(program);
		gl.bindVertexArray(vao);
		gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, framebuffer);
		gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
		gl.colorMask(mask[0], mask[1], mask[2], mask[3]);
		gl.blendColor(colour[0], colour[1], colour[2], colour[3]);
		gl.blendFuncSeparate(blend[0], blend[1], blend[2], blend[3]);
		gl.blendEquationSeparate(equation[0], equation[1]);
		caps.forEach((cap, i) => enabled[i] ? gl.enable(cap) : gl.disable(cap));
	}
}

//#endregion
//#region src/sphere/shaders.ts
const VERTEX = `#version 300 es
precision highp float; precision highp int; precision highp sampler2D;
uniform sampler2D uS0, uD0, uS1, uD1, uS2, uD2, uS3, uD3;
uniform vec2 uGrid, uViewport;
uniform float uFx, uFy, uK, uLift, uTime, uStride, uPointSize;
uniform int uPass;
out vec3 vInk;
out float vFade;
// An integer hash: a sine hash loses precision at large ids on some GPUs.
float hash(float n, uint salt) {
  uint x = uint(n) * 747796405u + salt * 2891336453u;
  x ^= x >> 16; x *= 0x7feb352du; x ^= x >> 15; x *= 0x846ca68bu; x ^= x >> 16;
  return float(x >> 8) / 16777216.0;
}
vec3 pigment(sampler2D s0, sampler2D d0, sampler2D s1, sampler2D d1, float f, ivec2 cell) {
  vec3 s = mix(texelFetch(s0, cell, 0).rgb, texelFetch(s1, cell, 0).rgb, f);
  vec3 d = mix(texelFetch(d0, cell, 0).rgb, texelFetch(d1, cell, 0).rgb, f);
  float density = dot(s, vec3(1.0 / 3.0));
  return min(d + .85 * max(density + (s - density) * 2.2, 0.0), vec3(4.0));
}
float wave(vec3 p, vec3 k, vec3 m, float t) {
  return sin(dot(k, p) + 1.4 * sin(dot(m, p) + t));
}
void main() {
  if (uPass == 0) {
    vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); gl_PointSize = 1.0; vInk = vec3(0.0); vFade = 1.0; return;
  }
  float id = float(gl_VertexID) * uStride;
  ivec2 cell = ivec2(mod(id, uGrid.x), floor(id / uGrid.x));
  vec2 uv = (vec2(cell) + .5) / uGrid;
  vec3 a = uK >= 1.0 ? vec3(0.0) : pigment(uS0, uD0, uS1, uD1, uFx, cell);
  vec3 b = uK <= 0.0 ? vec3(0.0) : pigment(uS2, uD2, uS3, uD3, uFy, cell);
  // Equal-area directions keep neighbouring cells' colours together; the jitter hides the
  // cell rows, which otherwise show as rings.
  vec2 jitter = vec2(hash(id, 3u), hash(id, 7u)) - .5;
  float z = clamp((uv.y + jitter.y / uGrid.y) * 2.0 - 1.0, -1.0, 1.0), phi = (uv.x + jitter.x / uGrid.x) * 6.2831853;
  // A row of the sheet would wrap the ball as a flat ring and a column as a straight meridian;
  // bending the map makes both wander. A stronger bend folds the map and piles grains into a dark line.
  z += .1 * (1.0 - z*z) * sin(2.0 * phi + 1.3 * sin(3.0 * z)); phi += 1.7 * z + .6 * sin(4.0 * z);
  vec3 direction = vec3(sqrt(max(0.0, 1.0 - z*z)) * vec2(cos(phi), sin(phi)), z);
  // The change of scene moves through the ball in patches, so two pigments sit side by side
  // instead of mixing to grey.
  float turn = .5 + .32 * sin(dot(direction, vec3(2.3, -1.1, 1.7)) * 2.2 + 1.3 * sin(3.1 * direction.z))
    + .18 * (hash(id, 17u) - .5);
  vInk = mix(a, b, smoothstep(turn - .04, turn + .04, uK));
  float egg = .5 * (3.0*z*z - 1.0), pear = .5 * (5.0*z*z*z - 3.0*z);
  // A pale haze through the ball, most grains toward its rim, and one in eight as a thinning
  // halo. Threads are drawn where a drifting wave field crosses zero, by opacity rather than
  // by moving grains, so they stay soft and the outline holds.
  float h = hash(id, 1u), halo = step(.88, hash(id, 11u)), shell = step(.45, hash(id, 13u));
  float rim = .55 + .2 * pow(h, .6);
  float radius = mix(mix(pow(.03 + .97 * h, .34) * .7, rim, shell), .7 + .3 * h * h, halo)
    * (1.0 + .07 * sin(uTime * .48) * egg + .05 * sin(uTime * .48 * 1.94 + .7) * pear);
  vec3 cloud = direction * radius;
  float t = uTime * .32;
  vec3 af = cloud * 2.0;
  float F = wave(af, vec3(3.1, 1.7, -2.3), vec3(-1.2, 2.1, 1.4), t)
    + wave(af, vec3(-1.9, 3.4, 2.1), vec3(2.3, .6, -1.7), -t * .8)
    + .7 * wave(af, vec3(2.2, -2.8, 3.0), vec3(1.1, 1.9, 2.2), t * 1.3);
  float thread = exp(-F * F * 3.0) * (.35 + .65 * smoothstep(.45, .72, radius));
  float c = cos(uTime * .26), s = sin(uTime * .26);
  cloud.xz = mat2(c, -s, s, c) * cloud.xz;
  cloud.xy = mat2(.94, -.342, .342, .94) * cloud.xy;
  vec2 aspect = min(uViewport.x, uViewport.y) / uViewport;
  vec2 projected = cloud.xy * aspect / (1.0 - cloud.z * .22);
  gl_Position = vec4(mix(uv * 2.0 - 1.0, projected, uLift), 0.0, 1.0);
  float near = .5 + .5 * clamp(cloud.z, -1.0, 1.0);
  float body = .2 + .8 * thread;
  vFade = mix(1.0, (.6 + .4 * near) * body * (1.0 - .7 * halo * smoothstep(.72, 1.0, radius)), uLift);
  gl_PointSize = uPointSize * mix(1.0, (.7 + .6 * hash(id, 5u)) * (.85 + .3 * near) * (1.0 + .6 * thread), uLift);
}`;
const FRAGMENT = `#version 300 es
precision highp float; precision highp int;
uniform vec3 uTint;
uniform float uDark, uGrain, uPaper;
uniform int uPass;
in vec3 vInk;
in float vFade;
out vec4 colour;
void main() {
  if (uPass == 0) { colour = vec4(uTint * uPaper, uPaper); return; }
  float radius = length(gl_PointCoord - .5) * 2.0;
  float coverage = (1.0 - smoothstep(.55, 1.0, radius)) * uGrain * vFade;
  vec3 T = exp(-vInk); float m = min(T.r, min(T.g, T.b)), alpha = 1.0 - m;
  if (alpha < .001 || coverage <= 0.0) discard;
  vec3 c = uDark > .5 ? (1.0 - uTint) * (1.0 - T) + uTint * alpha : uTint * (T - m);
  colour = vec4(c, alpha) * coverage;
}`;

//#endregion
//#region src/sphere/index.ts
function programFor(gl) {
	const program = gl.createProgram();
	if (!program) throw new Error("Sphere program allocation failed");
	const shaders = [];
	try {
		for (const [type, source] of [[gl.VERTEX_SHADER, VERTEX], [gl.FRAGMENT_SHADER, FRAGMENT]]) {
			const shader = gl.createShader(type);
			if (!shader) throw new Error("Sphere shader allocation failed");
			shaders.push(shader);
			gl.shaderSource(shader, source);
			gl.compileShader(shader);
			if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) || "Sphere shader compile failed");
			gl.attachShader(program, shader);
		}
		gl.linkProgram(program);
		if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) || "Sphere program link failed");
		return program;
	} catch (error) {
		gl.deleteProgram(program);
		throw error;
	} finally {
		shaders.forEach((shader) => gl.deleteShader(shader));
	}
}
/** Optional draw-only renderer. No textures, vertex buffers, simulation or clock. */
function createSphere(sim, options = {}) {
	const { gl, width, height, page, ground } = contextFor(sim);
	const { stride = 1, pointSize } = options;
	if (!Number.isSafeInteger(stride) || stride < 1) throw new RangeError("Sphere stride must be a positive integer");
	if (pointSize !== void 0 && (!Number.isFinite(pointSize) || pointSize <= 0)) throw new RangeError("Sphere pointSize must be positive and finite");
	const program = programFor(gl), vao = gl.createVertexArray();
	if (!vao) {
		gl.deleteProgram(program);
		throw new Error("Sphere vertex array allocation failed");
	}
	const names = [
		"uS0",
		"uD0",
		"uS1",
		"uD1",
		"uS2",
		"uD2",
		"uS3",
		"uD3",
		"uGrid",
		"uViewport",
		"uFx",
		"uFy",
		"uK",
		"uLift",
		"uTime",
		"uStride",
		"uPointSize",
		"uPass",
		"uTint",
		"uDark",
		"uGrain",
		"uPaper"
	];
	const u = Object.fromEntries(names.map((name) => [name, gl.getUniformLocation(program, name)]));
	let disposed = false;
	return {
		draw(a, b, pose) {
			if (disposed || gl.isContextLost()) return false;
			contextFor(sim);
			if (![
				pose.k,
				pose.lift,
				pose.grain,
				pose.t
			].every(Number.isFinite)) throw new RangeError("Sphere pose must be finite");
			const grain = clamp01(pose.grain), k = b ? clamp01(pose.k) : 0;
			if (!grain) return true;
			if (!a && k < 1 || !b && k > 0) return false;
			const x = a ?? b, y = b ?? a;
			if (!x || !y) return false;
			withSphereState(gl, () => {
				gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
				gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
				gl.bindVertexArray(vao);
				gl.useProgram(program);
				for (const cap of [
					gl.DEPTH_TEST,
					gl.STENCIL_TEST,
					gl.SCISSOR_TEST,
					gl.CULL_FACE,
					gl.RASTERIZER_DISCARD,
					gl.SAMPLE_ALPHA_TO_COVERAGE,
					gl.SAMPLE_COVERAGE
				]) gl.disable(cap);
				gl.colorMask(true, true, true, true);
				gl.enable(gl.BLEND);
				gl.blendEquation(gl.FUNC_ADD);
				gl.uniform3fv(u.uTint, page.tint);
				gl.uniform1f(u.uDark, page.dark ? 1 : 0);
				gl.uniform1f(u.uPaper, ground === "paper" ? 1 : 0);
				[
					x.k0.s,
					x.k0.d,
					x.k1.s,
					x.k1.d,
					y.k0.s,
					y.k0.d,
					y.k1.s,
					y.k1.d
				].forEach((texture, unit) => {
					gl.activeTexture(gl.TEXTURE0 + unit);
					gl.bindTexture(gl.TEXTURE_2D, texture);
					gl.bindSampler(unit, null);
					gl.uniform1i(u[names[unit]], unit);
				});
				gl.uniform1i(u.uPass, 0);
				gl.blendColor(0, 0, 0, grain);
				gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
				gl.drawArrays(gl.TRIANGLES, 0, 3);
				gl.uniform2f(u.uGrid, width, height);
				gl.uniform2f(u.uViewport, gl.drawingBufferWidth, gl.drawingBufferHeight);
				gl.uniform1f(u.uFx, x.f);
				gl.uniform1f(u.uFy, y.f);
				gl.uniform1f(u.uK, k);
				gl.uniform1f(u.uLift, clamp01(pose.lift));
				gl.uniform1f(u.uGrain, grain);
				gl.uniform1f(u.uTime, pose.t);
				gl.uniform1f(u.uStride, stride);
				gl.uniform1f(u.uPointSize, pointSize ?? Math.max(1, gl.drawingBufferWidth / width * .85));
				gl.uniform1i(u.uPass, 1);
				gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
				gl.drawArrays(gl.POINTS, 0, Math.ceil(width * height / stride));
			});
			return true;
		},
		dispose() {
			if (disposed) return;
			gl.deleteProgram(program);
			gl.deleteVertexArray(vao);
			disposed = true;
		}
	};
}

//#endregion
export { createSphere };