import type { PigmentSample, WatercolorSim } from '../engine/solver.js';
import { contextFor } from '../internal/context.js';
import { clamp01 } from '../engine/math.js';
import { withSphereState } from './state.js';
import { VERTEX, FRAGMENT } from './shaders.js';

export interface SphereOptions {
  /** Draw every nth cell; 1 uses the complete simulation grid. */
  stride?: number;
  /** Point diameter in canvas pixels; by default follows the grid's display scale. */
  pointSize?: number;
}
export interface SpherePose { k: number; lift: number; grain: number; t: number; }
export interface PigmentSphere {
  /** Draw after playSides, using samples from that sim. Returns false for missing
   * samples, a disposed renderer or lost context. grain=0 leaves the canvas untouched.
   */
  draw(a: PigmentSample | null, b: PigmentSample | null, pose: SpherePose): boolean;
  dispose(): void;
}

function programFor(gl: WebGL2RenderingContext): WebGLProgram {
  const program = gl.createProgram();
  if (!program) throw new Error('Sphere program allocation failed');
  const shaders: WebGLShader[] = [];
  try {
    for (const [type, source] of [[gl.VERTEX_SHADER, VERTEX], [gl.FRAGMENT_SHADER, FRAGMENT]] as const) {
      const shader = gl.createShader(type);
      if (!shader) throw new Error('Sphere shader allocation failed');
      shaders.push(shader); gl.shaderSource(shader, source); gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) || 'Sphere shader compile failed');
      gl.attachShader(program, shader);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) || 'Sphere program link failed');
    return program;
  } catch (error) { gl.deleteProgram(program); throw error; }
  finally { shaders.forEach(shader => gl.deleteShader(shader)); }
}

/** Optional draw-only renderer. No textures, vertex buffers, simulation or clock. */
export function createSphere(sim: WatercolorSim, options: SphereOptions = {}): PigmentSphere {
  const { gl, width, height, page, ground } = contextFor(sim);
  const { stride = 1, pointSize } = options;
  if (!Number.isSafeInteger(stride) || stride < 1) throw new RangeError('Sphere stride must be a positive integer');
  if (pointSize !== undefined && (!Number.isFinite(pointSize) || pointSize <= 0)) throw new RangeError('Sphere pointSize must be positive and finite');
  const program = programFor(gl), vao = gl.createVertexArray();
  if (!vao) { gl.deleteProgram(program); throw new Error('Sphere vertex array allocation failed'); }
  const names = ['uS0', 'uD0', 'uS1', 'uD1', 'uS2', 'uD2', 'uS3', 'uD3',
    'uGrid', 'uViewport', 'uFx', 'uFy', 'uK', 'uLift', 'uTime', 'uStride', 'uPointSize',
    'uPass', 'uTint', 'uDark', 'uGrain', 'uPaper'];
  const u = Object.fromEntries(names.map(name => [name, gl.getUniformLocation(program, name)]));
  let disposed = false;
  return {
    draw(a, b, pose) {
      if (disposed || gl.isContextLost()) return false;
      contextFor(sim);
      if (![pose.k, pose.lift, pose.grain, pose.t].every(Number.isFinite)) throw new RangeError('Sphere pose must be finite');
      const grain = clamp01(pose.grain), k = b ? clamp01(pose.k) : 0;
      if (!grain) return true;
      if (!a && k < 1 || !b && k > 0) return false;
      const x = a ?? b, y = b ?? a;
      if (!x || !y) return false;
      withSphereState(gl, () => {
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
        gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
        gl.bindVertexArray(vao); gl.useProgram(program);
        for (const cap of [gl.DEPTH_TEST, gl.STENCIL_TEST, gl.SCISSOR_TEST, gl.CULL_FACE,
          gl.RASTERIZER_DISCARD, gl.SAMPLE_ALPHA_TO_COVERAGE, gl.SAMPLE_COVERAGE]) gl.disable(cap);
        gl.colorMask(true, true, true, true); gl.enable(gl.BLEND); gl.blendEquation(gl.FUNC_ADD);
        gl.uniform3fv(u.uTint, page.tint); gl.uniform1f(u.uDark, page.dark ? 1 : 0);
        gl.uniform1f(u.uPaper, ground === 'paper' ? 1 : 0);
        const textures = [x.k0.s, x.k0.d, x.k1.s, x.k1.d, y.k0.s, y.k0.d, y.k1.s, y.k1.d];
        textures.forEach((texture, unit) => {
          gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, texture);
          gl.bindSampler(unit, null); gl.uniform1i(u[names[unit]], unit);
        });
        // Fade the existing wash toward clear paper/transparency without a copy texture.
        gl.uniform1i(u.uPass, 0); gl.blendColor(0, 0, 0, grain);
        gl.blendFunc(gl.CONSTANT_ALPHA, gl.ONE_MINUS_CONSTANT_ALPHA);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
        gl.uniform2f(u.uGrid, width, height);
        gl.uniform2f(u.uViewport, gl.drawingBufferWidth, gl.drawingBufferHeight);
        gl.uniform1f(u.uFx, x.f); gl.uniform1f(u.uFy, y.f); gl.uniform1f(u.uK, k);
        gl.uniform1f(u.uLift, clamp01(pose.lift)); gl.uniform1f(u.uGrain, grain);
        gl.uniform1f(u.uTime, pose.t); gl.uniform1f(u.uStride, stride);
        gl.uniform1f(u.uPointSize, pointSize ?? Math.max(1, gl.drawingBufferWidth / width * .85));
        gl.uniform1i(u.uPass, 1); gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
        gl.drawArrays(gl.POINTS, 0, Math.ceil(width * height / stride));
      });
      return true;
    },
    dispose() {
      if (disposed) return;
      gl.deleteProgram(program); gl.deleteVertexArray(vao); disposed = true;
    },
  };
}
