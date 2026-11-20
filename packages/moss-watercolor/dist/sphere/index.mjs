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
precision highp float; precision highp sampler2D;
uniform sampler2D uS0, uD0, uS1, uD1, uS2, uD2, uS3, uD3;
uniform vec2 uGrid, uViewport;
uniform float uFx, uFy, uK, uLift, uTime, uStride, uPointSize;
uniform int uPass;
out vec3 vInk;
float hash(float n) { return fract(sin(n * 12.9898 + 78.233) * 43758.5453); }
vec3 pigment(sampler2D s0, sampler2D d0, sampler2D s1, sampler2D d1, float f, ivec2 cell) {
  vec3 s = mix(texelFetch(s0, cell, 0).rgb, texelFetch(s1, cell, 0).rgb, f);
  vec3 d = mix(texelFetch(d0, cell, 0).rgb, texelFetch(d1, cell, 0).rgb, f);
  float density = dot(s, vec3(1.0 / 3.0));
  return min(d + .85 * max(density + (s - density) * 2.2, 0.0), vec3(4.0));
}
void main() {
  if (uPass == 0) {
    vec2 q = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(q * 2.0 - 1.0, 0.0, 1.0); gl_PointSize = 1.0; vInk = vec3(0.0); return;
  }
  float id = float(gl_VertexID) * uStride;
  ivec2 cell = ivec2(mod(id, uGrid.x), floor(id / uGrid.x));
  vec2 uv = (vec2(cell) + .5) / uGrid;
  vec3 a = uK >= 1.0 ? vec3(0.0) : pigment(uS0, uD0, uS1, uD1, uFx, cell);
  vec3 b = uK <= 0.0 ? vec3(0.0) : pigment(uS2, uD2, uS3, uD3, uFy, cell);
  vInk = mix(a, b, uK);
  // Equal-area directions keep neighbouring cells' colours together. The radial
  // distribution puts more grains near the centre and leaves a ragged edge.
  float z = uv.y * 2.0 - 1.0, phi = uv.x * 6.2831853;
  vec3 direction = vec3(sqrt(max(0.0, 1.0 - z*z)) * vec2(cos(phi), sin(phi)), z);
  float egg = .5 * (3.0*z*z - 1.0), pear = .5 * (5.0*z*z*z - 3.0*z);
  float radius = pow(hash(id + 1.0), .65) * (.79 + .08 * sin(uTime * .48) * egg
    + .06 * sin(uTime * .48 * 1.94 + .7) * pear + .04 * hash(id + 19.0));
  vec3 cloud = direction * radius;
  float c = cos(uTime * .12), s = sin(uTime * .12);
  cloud.xz = mat2(c, -s, s, c) * cloud.xz;
  cloud.xy = mat2(.94, -.342, .342, .94) * cloud.xy;
  vec2 aspect = min(uViewport.x, uViewport.y) / uViewport;
  vec2 projected = cloud.xy * aspect / (1.0 - cloud.z * .22);
  gl_Position = vec4(mix(uv * 2.0 - 1.0, projected, uLift), 0.0, 1.0);
  gl_PointSize = uPointSize * mix(1.0, .8 + .2 * (cloud.z + 1.0), uLift);
}`;
const FRAGMENT = `#version 300 es
precision highp float; precision highp int;
uniform vec3 uTint;
uniform float uDark, uGrain, uPaper;
uniform int uPass;
in vec3 vInk;
out vec4 colour;
void main() {
  if (uPass == 0) { colour = vec4(uTint * uPaper, uPaper); return; }
  float radius = length(gl_PointCoord - .5) * 2.0;
  float coverage = (1.0 - smoothstep(.55, 1.0, radius)) * uGrain;
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