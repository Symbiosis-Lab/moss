/** Preserve the caller's complete state footprint, including separate read/draw targets.
 * Uniforms belong only to the sphere program; no caller-owned object is modified.
 */
export function withSphereState(gl: WebGL2RenderingContext, draw: () => void): void {
  const get = (p: number) => gl.getParameter(p);
  const program = get(gl.CURRENT_PROGRAM), vao = get(gl.VERTEX_ARRAY_BINDING);
  const framebuffer = get(gl.DRAW_FRAMEBUFFER_BINDING), viewport = get(gl.VIEWPORT);
  const active = get(gl.ACTIVE_TEXTURE), mask = get(gl.COLOR_WRITEMASK), colour = get(gl.BLEND_COLOR);
  const blend = [gl.BLEND_SRC_RGB, gl.BLEND_DST_RGB, gl.BLEND_SRC_ALPHA, gl.BLEND_DST_ALPHA].map(get);
  const equation = [get(gl.BLEND_EQUATION_RGB), get(gl.BLEND_EQUATION_ALPHA)];
  const caps = [gl.BLEND, gl.DEPTH_TEST, gl.STENCIL_TEST, gl.SCISSOR_TEST, gl.CULL_FACE,
    gl.RASTERIZER_DISCARD, gl.SAMPLE_ALPHA_TO_COVERAGE, gl.SAMPLE_COVERAGE];
  const enabled = caps.map(cap => gl.isEnabled(cap));
  const textures: (WebGLTexture | null)[] = [], samplers: (WebGLSampler | null)[] = [];
  for (let unit = 0; unit < 8; unit++) {
    gl.activeTexture(gl.TEXTURE0 + unit);
    textures.push(get(gl.TEXTURE_BINDING_2D)); samplers.push(get(gl.SAMPLER_BINDING));
  }
  try { draw(); }
  finally {
    for (let unit = 0; unit < 8; unit++) {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(gl.TEXTURE_2D, textures[unit]); gl.bindSampler(unit, samplers[unit]);
    }
    gl.activeTexture(active);
    gl.useProgram(program); gl.bindVertexArray(vao);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, framebuffer);
    gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
    gl.colorMask(mask[0], mask[1], mask[2], mask[3]);
    gl.blendColor(colour[0], colour[1], colour[2], colour[3]);
    gl.blendFuncSeparate(blend[0], blend[1], blend[2], blend[3]);
    gl.blendEquationSeparate(equation[0], equation[1]);
    caps.forEach((cap, i) => enabled[i] ? gl.enable(cap) : gl.disable(cap));
  }
}
