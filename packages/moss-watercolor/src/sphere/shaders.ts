export const VERTEX = `#version 300 es
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

export const FRAGMENT = `#version 300 es
precision highp float;
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
