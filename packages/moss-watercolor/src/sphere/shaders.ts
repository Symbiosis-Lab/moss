export const VERTEX = `#version 300 es
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
// Gradient of one warped wave of a density field. Moving grains along it gathers them
// into curved folds, the way light gathers into caustics.
vec3 fold(vec3 p, vec3 k, vec3 m, float t) {
  float warp = dot(m, p) + t;
  return cos(dot(k, p) + 1.4 * sin(warp)) * (k + 1.4 * cos(warp) * m);
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
  vInk = mix(a, b, uK);
  // Equal-area directions keep neighbouring cells' colours together; the jitter hides the
  // cell rows, which otherwise show as rings.
  vec2 jitter = vec2(hash(id, 3u), hash(id, 7u)) - .5;
  float z = clamp((uv.y + jitter.y / uGrid.y) * 2.0 - 1.0, -1.0, 1.0), phi = (uv.x + jitter.x / uGrid.x) * 6.2831853;
  // A row of the sheet would wrap the ball as a flat ring and a column as a straight meridian;
  // bending the map makes both wander. A stronger bend folds the map and piles grains into a dark line.
  z += .1 * (1.0 - z*z) * sin(2.0 * phi + 1.3 * sin(3.0 * z)); phi += 1.7 * z + .6 * sin(4.0 * z);
  vec3 direction = vec3(sqrt(max(0.0, 1.0 - z*z)) * vec2(cos(phi), sin(phi)), z);
  float egg = .5 * (3.0*z*z - 1.0), pear = .5 * (5.0*z*z*z - 3.0*z);
  // Most grains fill the ball evenly; one in five leaves it as a thinning halo.
  float h = hash(id, 1u), halo = step(.8, hash(id, 11u));
  float radius = mix(pow(.03 + .97 * h, .42) * .74, .68 + .3 * h * h, halo)
    * (1.0 + .07 * sin(uTime * .48) * egg + .05 * sin(uTime * .48 * 1.94 + .7) * pear);
  vec3 cloud = direction * radius;
  float t = uTime * .32;
  vec3 at = cloud * 3.0;
  vec3 gather = fold(at, vec3(3.1, 1.7, -2.3), vec3(-1.2, 2.1, 1.4), t)
    + fold(at, vec3(-1.9, 3.4, 2.1), vec3(2.3, .6, -1.7), -t * .8)
    + fold(at, vec3(2.2, -2.8, 3.0), vec3(1.1, 1.9, 2.2), t * 1.3);
  gather -= .8 * direction * dot(gather, direction);
  cloud += .0075 * gather * (1.0 - .6 * halo);
  float c = cos(uTime * .26), s = sin(uTime * .26);
  cloud.xz = mat2(c, -s, s, c) * cloud.xz;
  cloud.xy = mat2(.94, -.342, .342, .94) * cloud.xy;
  vec2 aspect = min(uViewport.x, uViewport.y) / uViewport;
  vec2 projected = cloud.xy * aspect / (1.0 - cloud.z * .22);
  gl_Position = vec4(mix(uv * 2.0 - 1.0, projected, uLift), 0.0, 1.0);
  float near = .5 + .5 * clamp(cloud.z, -1.0, 1.0);
  vFade = mix(1.0, (.55 + .45 * near) * (1.0 - .65 * halo * smoothstep(.7, .98, radius)), uLift);
  gl_PointSize = uPointSize * mix(1.0, (.7 + .6 * hash(id, 5u)) * (.85 + .3 * near), uLift);
}`;

export const FRAGMENT = `#version 300 es
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
