import { vi } from 'vitest';

/** Stateful test double for the GL footprint, not a rasterizer. */
export function mockGL() {
  const params = new Map<number, unknown>(), enabled = new Set<number>();
  const textures = Array.from({ length: 16 }, (_, i) => ({ texture: i }));
  const samplers = Array.from({ length: 16 }, (_, i) => ({ sampler: i }));
  let active = 11, serial = 1000;
  const constants = ['CURRENT_PROGRAM', 'VERTEX_ARRAY_BINDING', 'DRAW_FRAMEBUFFER_BINDING',
    'READ_FRAMEBUFFER_BINDING', 'VIEWPORT', 'ACTIVE_TEXTURE', 'COLOR_WRITEMASK', 'BLEND_COLOR',
    'BLEND_SRC_RGB', 'BLEND_DST_RGB', 'BLEND_SRC_ALPHA', 'BLEND_DST_ALPHA', 'BLEND_EQUATION_RGB',
    'BLEND_EQUATION_ALPHA', 'BLEND', 'DEPTH_TEST', 'STENCIL_TEST', 'SCISSOR_TEST', 'CULL_FACE',
    'RASTERIZER_DISCARD', 'SAMPLE_ALPHA_TO_COVERAGE', 'SAMPLE_COVERAGE', 'TEXTURE_BINDING_2D',
    'SAMPLER_BINDING', 'DRAW_FRAMEBUFFER', 'FRAMEBUFFER', 'TEXTURE_2D', 'FUNC_ADD', 'CONSTANT_ALPHA',
    'ONE_MINUS_CONSTANT_ALPHA', 'ONE', 'ONE_MINUS_SRC_ALPHA', 'TRIANGLES', 'POINTS', 'VERTEX_SHADER',
    'FRAGMENT_SHADER', 'COMPILE_STATUS', 'LINK_STATUS'];
  const g: Record<string, any> = { TEXTURE0: 100, drawingBufferWidth: 390, drawingBufferHeight: 371 };
  constants.forEach((key, i) => { g[key] = i + 1; });
  for (const key of ['CURRENT_PROGRAM', 'VERTEX_ARRAY_BINDING', 'DRAW_FRAMEBUFFER_BINDING', 'READ_FRAMEBUFFER_BINDING']) params.set(g[key], { key });
  params.set(g.VIEWPORT, [3, 5, 111, 123]); params.set(g.COLOR_WRITEMASK, [true, false, true, false]);
  params.set(g.BLEND_COLOR, [.1, .2, .3, .4]);
  for (const key of ['BLEND_SRC_RGB', 'BLEND_DST_RGB', 'BLEND_SRC_ALPHA', 'BLEND_DST_ALPHA', 'BLEND_EQUATION_RGB', 'BLEND_EQUATION_ALPHA']) params.set(g[key], serial++);
  for (const key of ['DEPTH_TEST', 'SCISSOR_TEST', 'SAMPLE_COVERAGE']) enabled.add(g[key]);
  g.getParameter = (key: number) => key === g.ACTIVE_TEXTURE ? g.TEXTURE0 + active
    : key === g.TEXTURE_BINDING_2D ? textures[active] : key === g.SAMPLER_BINDING ? samplers[active] : params.get(key);
  g.activeTexture = (value: number) => { active = value - g.TEXTURE0; };
  g.bindTexture = (_: number, value: any) => { textures[active] = value; };
  g.bindSampler = (unit: number, value: any) => { samplers[unit] = value; };
  g.isEnabled = (cap: number) => enabled.has(cap);
  g.enable = (cap: number) => enabled.add(cap); g.disable = (cap: number) => enabled.delete(cap);
  g.useProgram = (value: unknown) => params.set(g.CURRENT_PROGRAM, value);
  g.bindVertexArray = (value: unknown) => params.set(g.VERTEX_ARRAY_BINDING, value);
  g.bindFramebuffer = (target: number, value: unknown) => {
    params.set(g.DRAW_FRAMEBUFFER_BINDING, value);
    if (target === g.FRAMEBUFFER) params.set(g.READ_FRAMEBUFFER_BINDING, value);
  };
  g.viewport = (...value: number[]) => params.set(g.VIEWPORT, value);
  g.colorMask = (...value: boolean[]) => params.set(g.COLOR_WRITEMASK, value);
  g.blendColor = (...value: number[]) => params.set(g.BLEND_COLOR, value);
  g.blendFuncSeparate = (sr: number, dr: number, sa: number, da: number) => {
    [g.BLEND_SRC_RGB, g.BLEND_DST_RGB, g.BLEND_SRC_ALPHA, g.BLEND_DST_ALPHA].forEach((key, i) => params.set(key, [sr, dr, sa, da][i]));
  };
  g.blendFunc = (s: number, d: number) => g.blendFuncSeparate(s, d, s, d);
  g.blendEquationSeparate = (rgb: number, alpha: number) => {
    params.set(g.BLEND_EQUATION_RGB, rgb); params.set(g.BLEND_EQUATION_ALPHA, alpha);
  };
  g.blendEquation = (value: number) => g.blendEquationSeparate(value, value);
  g.getUniformLocation = (_: unknown, name: string) => name;
  g.getShaderParameter = g.getProgramParameter = () => true;
  g.isContextLost = () => false;
  for (const key of ['createProgram', 'createShader', 'createVertexArray']) g[key] = vi.fn(() => ({ id: serial++ }));
  for (const key of ['shaderSource', 'compileShader', 'attachShader', 'linkProgram', 'deleteShader',
    'deleteProgram', 'deleteVertexArray', 'uniform1i', 'uniform1f', 'uniform2f', 'uniform3fv', 'drawArrays']) g[key] = vi.fn();
  const snapshot = () => ({ params: new Map(params), enabled: [...enabled].sort(), textures: [...textures], samplers: [...samplers], active });
  return { gl: g as WebGL2RenderingContext, snapshot };
}
