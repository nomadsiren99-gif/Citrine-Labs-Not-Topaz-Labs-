// GPU view for the per-pixel stages (colour, tone, curve). The CPU renders the spatial stages into a
// base image; this shader applies the per-pixel stages to it on every frame, so slider and pan/zoom
// changes need no CPU render. The arithmetic follows quartz's color_adjust, tone and curve stages.
import type { LsParams } from "./quartz";

const VERT = `#version 300 es
void main() {
  ivec2 p = ivec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(vec2(p) * 2.0 - 1.0, 0.0, 1.0);
}`;

const FRAG = `#version 300 es
precision highp float;
precision highp sampler2D;
uniform sampler2D base;
uniform sampler2D lut;
uniform vec2 uvA;
uniform vec2 uvB;
uniform float colorOn;
uniform float hueShift;
uniform float adaptSat;
uniform float satBoost;
uniform float globalSat;
uniform float toneOn;
uniform float mono;
uniform vec3 monoMix;
uniform vec3 shadowTint;
uniform vec3 highlightTint;
uniform float sStr;
uniform float hStr;
uniform float pivot;
uniform float curveOn;
uniform vec4 rect;      // base rectangle in image pixels: x, y, width, height
uniform vec2 fullDims;  // full image size in pixels
uniform float basicOn;
uniform float expOn;
uniform float expGain;
uniform float bContrast;
uniform float bHigh;
uniform float bShadow;
uniform float bWhite;
uniform float bBlack;
uniform float gradeOn;
uniform float gMid;
uniform float gWidth;
uniform vec3 gShOff;
uniform float gShLum;
uniform vec3 gMidOff;
uniform float gMidLum;
uniform vec3 gHiOff;
uniform float gHiLum;
uniform float warmOn;
uniform float warmW;
uniform float vigOn;
uniform float vigAmount;
uniform float vigSize;
uniform float vigTrans;
uniform float vigRound;
uniform vec2 vigC;      // half extents of the image (cx, cy)
uniform float vigMaxR;
uniform float grainOn;
uniform float grainAmt;
uniform float grainCell;
uniform float borderOn;
uniform float borderPx;
uniform vec3 borderColor;
out vec4 outColor;

// Rust's % on floats: the result takes the sign of x.
float fmodT(float x, float y) { return x - y * trunc(x / y); }

vec3 rgb2hsv(vec3 c) {
  float mx = max(c.r, max(c.g, c.b));
  float mn = min(c.r, min(c.g, c.b));
  float d = mx - mn;
  float s = mx > 1e-6 ? d / mx : 0.0;
  float h;
  if (d < 1e-6) h = 0.0;
  else if (abs(mx - c.r) < 1e-6) h = fmodT((c.g - c.b) / d, 6.0);
  else if (abs(mx - c.g) < 1e-6) h = ((c.b - c.r) / d) + 2.0;
  else h = ((c.r - c.g) / d) + 4.0;
  h /= 6.0;
  if (h < 0.0) h += 1.0;
  return vec3(h, s, mx);
}

vec3 hsv2rgb(float h, float s, float v) {
  if (s <= 1e-6) return vec3(v);
  float h6 = fmodT(h, 1.0) * 6.0;
  float i = floor(h6);
  float f = h6 - i;
  float p = v * (1.0 - s);
  float q = v * (1.0 - s * f);
  float t = v * (1.0 - s * (1.0 - f));
  int k = int(i);
  if (k == 0) return vec3(v, t, p);
  if (k == 1) return vec3(q, v, p);
  if (k == 2) return vec3(p, v, t);
  if (k == 3) return vec3(p, q, v);
  if (k == 4) return vec3(t, p, v);
  return vec3(v, p, q);
}

vec3 colorStep(vec3 c) {
  vec3 hsv = rgb2hsv(c);
  float h = hsv.x;
  float s = hsv.y;
  float v = hsv.z;
  if (hueShift != 0.0) {
    h = fmodT(h + hueShift, 1.0);
    if (h < 0.0) h += 1.0;
  }
  if (adaptSat > 0.0) s += adaptSat * satBoost * 2.0 * s * (1.0 - s);
  else if (adaptSat < 0.0) s += s * adaptSat * 0.5;
  s *= 1.0 + globalSat;
  s = clamp(s, 0.0, 1.0);
  return clamp(hsv2rgb(h, s, v), 0.0, 1.0);
}

vec3 toneStep(vec3 c) {
  if (mono > 0.0) {
    float bw = clamp(dot(c, monoMix), 0.0, 1.0);
    c += (bw - c) * mono;
  }
  float lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
  float x = clamp((lum - (pivot - 0.5)) / ((pivot + 0.5) - (pivot - 0.5)), 0.0, 1.0);
  float t = x * x * (3.0 - 2.0 * x);
  float shadowW = (1.0 - t) * sStr;
  float highW = t * hStr;
  if (shadowW > 0.0) c += (shadowTint * lum - c) * shadowW;
  if (highW > 0.0) c += (highlightTint * lum - c) * highW;
  return clamp(c, 0.0, 1.0);
}

// Film grain cell noise in [-0.5, 0.5]: quartz's grain_hash, the same integer arithmetic.
float grainHash(uint x, uint y) {
  uint h = (x * 0x8da6b343u) ^ (y * 0xd8163841u) ^ 0x5bf03635u;
  h ^= h >> 15u;
  h *= 0x2c1b3c6du;
  h ^= h >> 12u;
  return float(h & 0xffffu) / 65535.0 - 0.5;
}

// Camera Raw style basic correction, as basic_correction.rs (per channel).
float srgb2lin(float v) { return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4); }
float lin2srgb(float l) { l = clamp(l, 0.0, 1.0); return l <= 0.0031308 ? l * 12.92 : 1.055 * pow(l, 1.0 / 2.4) - 0.055; }
float ssStep(float e0, float e1, float x) { float t = clamp((x - e0) / (e1 - e0), 0.0, 1.0); return t * t * (3.0 - 2.0 * t); }
float basicMap(float v) {
  float x = v;
  if (expOn > 0.5) x = lin2srgb(srgb2lin(x) * expGain);
  if (bContrast > 0.0) x += (ssStep(0.0, 1.0, x) - x) * bContrast;
  else if (bContrast < 0.0) x += (0.5 - x) * (-bContrast) * 0.5;
  x += bHigh * 0.35 * ssStep(0.5, 1.0, x);
  x += bShadow * 0.35 * (1.0 - ssStep(0.0, 0.5, x));
  x += bWhite * 0.2 * ssStep(0.75, 1.0, x);
  x += bBlack * 0.2 * (1.0 - ssStep(0.0, 0.25, x));
  return clamp(x, 0.0, 1.0);
}

// Three-zone colour grading, as grading.rs.
vec3 gradePixel(vec3 c) {
  float lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
  float ws = 1.0 - ssStep(gMid - gWidth, gMid, lum);
  float wh = ssStep(gMid, gMid + gWidth, lum);
  float wm = clamp(1.0 - ws - wh, 0.0, 1.0);
  c += (gShOff + gShLum) * ws;
  c += (gMidOff + gMidLum) * wm;
  c += (gHiOff + gHiLum) * wh;
  return clamp(c, 0.0, 1.0);
}

float lutAt(float v, int row) {
  int idx = int(floor(clamp(v, 0.0, 1.0) * 1023.0 + 0.5));
  return texelFetch(lut, ivec2(min(idx, 1023), row), 0).r;
}

void main() {
  vec2 uv = uvA * gl_FragCoord.xy + uvB;
  // Outside the image: the viewport background (#0a0b0d), not the edge pixels.
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { outColor = vec4(0.039, 0.043, 0.051, 1.0); return; }
  vec3 c = texture(base, uv).rgb;
  if (colorOn > 0.5) c = colorStep(c);
  if (toneOn > 0.5) c = toneStep(c);
  if (curveOn > 0.5) c = vec3(lutAt(c.r, 0), lutAt(c.g, 1), lutAt(c.b, 2));
  if (basicOn > 0.5) c = vec3(basicMap(c.r), basicMap(c.g), basicMap(c.b));
  if (gradeOn > 0.5) c = gradePixel(c);

  // Finishing, as quartz's apply_finishing: warmth, vignette, grain, border.
  // Positions are whole image pixels, so the result does not depend on the view's resolution.
  vec2 ip = floor(rect.xy + uv * rect.zw);
  if (warmOn > 0.5) {
    c.r = clamp(c.r + warmW, 0.0, 1.0);
    c.g = clamp(c.g + warmW * 0.2, 0.0, 1.0);
    c.b = clamp(c.b - warmW, 0.0, 1.0);
  }
  if (vigOn > 0.5) {
    float dx = ip.x - vigC.x;
    float dy = ip.y - vigC.y;
    float dCirc = sqrt(dx * dx + dy * dy) / vigMaxR;
    float dRect = max(abs(dx) / vigC.x, abs(dy) / vigC.y);
    float dist = (1.0 - vigRound) * dRect + vigRound * dCirc;
    float t = clamp((dist - vigSize) / vigTrans, 0.0, 1.0);
    float mask = t * t * (3.0 - 2.0 * t);
    float factor = clamp(1.0 + vigAmount * mask, 0.0, 2.0);
    c = clamp(c * factor, 0.0, 1.0);
  }
  if (grainOn > 0.5) {
    uint gx = uint(floor(ip.x / grainCell));
    uint gy = uint(floor(ip.y / grainCell));
    float norm = grainHash(gx, gy);
    float lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    float midMod = max(sqrt(max(lum * (1.0 - lum), 0.0)), 0.1);
    float noise = norm * grainAmt * midMod;
    c = clamp(c + noise, 0.0, 1.0);
  }
  if (borderOn > 0.5) {
    bool topBottom = ip.y < borderPx || ip.y + borderPx >= fullDims.y;
    if (topBottom || ip.x < borderPx || ip.x + borderPx >= fullDims.x) c = borderColor;
  }
  outColor = vec4(c, 1.0);
}`;

// The same HSV-to-RGB as quartz, used for the split-tone tints.
function hsv2rgb(h: number, s: number, v: number): [number, number, number] {
  if (s <= 1e-6) return [v, v, v];
  const h6 = (((h % 1) + 1) % 1) * 6;
  const i = Math.floor(h6);
  const f = h6 - i;
  const p = v * (1 - s);
  const q = v * (1 - s * f);
  const t = v * (1 - s * (1 - f));
  switch (i) {
    case 0: return [v, t, p];
    case 1: return [q, v, p];
    case 2: return [p, v, t];
    case 3: return [p, q, v];
    case 4: return [t, p, v];
    default: return [v, p, q];
  }
}

// A saturated hue scaled so its luminance is 1 (quartz tone.rs `tint`).
function tint(hueDeg: number): [number, number, number] {
  const [r, g, b] = hsv2rgb((((hueDeg / 360) % 1) + 1) % 1, 0.65, 1);
  const lum = Math.max(0.2126 * r + 0.7152 * g + 0.0722 * b, 1e-3);
  return [r / lum, g / lum, b / lum];
}

/** Uniforms for the per-pixel stages, computed from the slider values exactly as quartz does. */
export type Pointwise = {
  colorOn: number; hueShift: number; adaptSat: number; satBoost: number; globalSat: number;
  toneOn: number; mono: number; monoMix: [number, number, number];
  shadowTint: [number, number, number]; highlightTint: [number, number, number];
  sStr: number; hStr: number; pivot: number; curveOn: number;
  basicOn: number; expOn: number; expGain: number; bContrast: number; bHigh: number; bShadow: number; bWhite: number; bBlack: number;
  gradeOn: number; gMid: number; gWidth: number;
  gShOff: [number, number, number]; gShLum: number; gMidOff: [number, number, number]; gMidLum: number;
  gHiOff: [number, number, number]; gHiLum: number;
  warmOn: number; warmW: number;
  vigOn: number; vigAmount: number; vigSize: number; vigTrans: number; vigRound: number;
  grainOn: number; grainAmt: number; grainSize: number;
  borderOn: number; borderThick: number; borderColor: [number, number, number];
};

export function pointwiseFrom(ls: LsParams): Pointwise {
  const c = ls.color;
  const t = ls.tone;
  const curveOn = ls.curve.enabled && ls.curve.points.length >= 2;
  const toneOn = (t?.enabled ?? false) && ((t?.monochrome ?? 0) !== 0 || (t?.shadow_strength ?? 0) !== 0 || (t?.highlight_strength ?? 0) !== 0);
  const mm = t?.mono_mix ?? [0, 0, 0];
  const mixSum = Math.max(Math.abs(mm[0] + mm[1] + mm[2]), 1e-3);
  return {
    colorOn: c.enabled ? 1 : 0,
    hueShift: c.hue / 360,
    adaptSat: c.adaptive_saturation * 0.7,
    satBoost: c.saturation_boost,
    globalSat: c.saturation,
    toneOn: toneOn ? 1 : 0,
    mono: t?.monochrome ?? 0,
    monoMix: [mm[0] / mixSum, mm[1] / mixSum, mm[2] / mixSum],
    shadowTint: tint(t?.shadow_hue ?? 0),
    highlightTint: tint(t?.highlight_hue ?? 0),
    sStr: t?.shadow_strength ?? 0,
    hStr: t?.highlight_strength ?? 0,
    pivot: Math.min(Math.max(0.5 + (t?.balance ?? 0) * 0.35, 0.1), 0.9),
    curveOn: curveOn ? 1 : 0,
    ...finishingFrom(ls),
    ...basicFrom(ls),
    ...gradingFrom(ls),
  };
}

// Basic correction values, as the shader's basicMap reads them. Off when all sliders are zero.
function basicFrom(ls: LsParams) {
  const b = ls.basic_correction;
  const on = b.enabled && (b.exposure !== 0 || b.contrast !== 0 || b.highlights !== 0 || b.shadows !== 0 || b.whites !== 0 || b.blacks !== 0);
  return {
    basicOn: on ? 1 : 0,
    expOn: b.exposure !== 0 ? 1 : 0,
    expGain: Math.pow(2, b.exposure),
    bContrast: b.contrast, bHigh: b.highlights, bShadow: b.shadows, bWhite: b.whites, bBlack: b.blacks,
  };
}

// Colour grading zones, as grading.rs: each wheel gives a colour offset with no luminance change, plus
// a luminance offset. The midtone centre and transition width come from balance and blending.
function gradingFrom(ls: LsParams) {
  const g = ls.grading;
  const neutral = [g.shadows, g.midtones, g.highlights].every((w) => w.sat === 0 && w.lum === 0);
  const zone = (w: { hue: number; sat: number; lum: number }): { off: [number, number, number]; lum: number } => {
    const [tr, tg, tb] = hsv2rgb((((w.hue / 360) % 1) + 1) % 1, 1, 1);
    const lum = 0.2126 * tr + 0.7152 * tg + 0.0722 * tb;
    const chroma = Math.min(Math.max(w.sat, 0), 1) * 0.25;
    return { off: [(tr - lum) * chroma, (tg - lum) * chroma, (tb - lum) * chroma], lum: Math.min(Math.max(w.lum, -1), 1) * 0.25 };
  };
  const sh = zone(g.shadows), md = zone(g.midtones), hi = zone(g.highlights);
  return {
    gradeOn: g.enabled && !neutral ? 1 : 0,
    gMid: Math.min(Math.max(0.5 + g.balance * 0.3, 0.1), 0.9),
    gWidth: 0.1 + Math.min(Math.max(g.blending, 0), 1) * 0.5,
    gShOff: sh.off, gShLum: sh.lum, gMidOff: md.off, gMidLum: md.lum, gHiOff: hi.off, gHiLum: hi.lum,
  };
}

// Finishing as quartz applies it (warmth, vignette, grain, border). Transparency blends with the
// original image, which the GPU does not have, so the CPU view handles it.
function finishingFrom(ls: LsParams) {
  const f = ls.finishing;
  const on = f.enabled;
  const v = f.vignette;
  return {
    warmOn: on && f.warmth !== 0 ? 1 : 0,
    warmW: f.warmth * 0.15,
    vigOn: on && v.amount !== 0 ? 1 : 0,
    vigAmount: v.amount,
    vigSize: v.size,
    vigTrans: Math.max(v.transition, 0.01),
    vigRound: v.roundness,
    grainOn: on && f.grain.amount > 0 ? 1 : 0,
    grainAmt: f.grain.amount * 0.15,
    grainSize: f.grain.size,
    borderOn: on && f.border.enabled && f.border.thickness > 0 ? 1 : 0,
    borderThick: f.border.thickness,
    borderColor: f.border.color as [number, number, number],
  };
}

const UNIFORMS = [
  "base", "lut", "uvA", "uvB", "colorOn", "hueShift", "adaptSat", "satBoost", "globalSat",
  "toneOn", "mono", "monoMix", "shadowTint", "highlightTint", "sStr", "hStr", "pivot", "curveOn",
  "basicOn", "expOn", "expGain", "bContrast", "bHigh", "bShadow", "bWhite", "bBlack",
  "gradeOn", "gMid", "gWidth", "gShOff", "gShLum", "gMidOff", "gMidLum", "gHiOff", "gHiLum",
  "rect", "fullDims", "warmOn", "warmW", "vigOn", "vigAmount", "vigSize", "vigTrans", "vigRound",
  "vigC", "vigMaxR", "grainOn", "grainAmt", "grainCell", "borderOn", "borderPx", "borderColor",
] as const;

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type);
  if (!sh) throw new Error("cannot create shader");
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh) ?? "shader compile failed");
  return sh;
}

export class GpuView {
  private gl: WebGL2RenderingContext;
  private loc: Record<string, WebGLUniformLocation | null> = {};
  private baseTex: WebGLTexture;
  private lutTex: WebGLTexture;

  constructor(private canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false, premultipliedAlpha: false });
    if (!gl) throw new Error("WebGL 2 is not available");
    this.gl = gl;
    const prog = gl.createProgram();
    if (!prog) throw new Error("cannot create program");
    gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) ?? "link failed");
    gl.useProgram(prog);
    for (const name of UNIFORMS) this.loc[name] = gl.getUniformLocation(prog, name);
    gl.uniform1i(this.loc.base, 0);
    gl.uniform1i(this.loc.lut, 1);

    this.baseTex = this.texture(gl.TEXTURE0, gl.LINEAR);
    this.lutTex = this.texture(gl.TEXTURE1, gl.NEAREST);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  private texture(unit: number, filter: number): WebGLTexture {
    const gl = this.gl;
    gl.activeTexture(unit);
    const tex = gl.createTexture();
    if (!tex) throw new Error("cannot create texture");
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    return tex;
  }

  /** Uploads the spatial base (RGBA8, row 0 = top). */
  setBase(w: number, h: number, rgba: Uint8Array) {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.baseTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
  }

  /** Uploads the three curve tables: 1024 entries each, red then green then blue. */
  setLut(lut: Float32Array) {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.lutTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R32F, 1024, 3, 0, gl.RED, gl.FLOAT, lut);
  }

  /**
   * Draws the base for a view centred on `centre` (image pixels) at `zoom` (screen pixels per image
   * pixel). `base` is the image rectangle the uploaded base covers: x, y, width, height.
   */
  draw(centre: [number, number], zoom: number, base: [number, number, number, number], pw: Pointwise, full: [number, number]) {
    const gl = this.gl;
    const dpr = window.devicePixelRatio || 1;
    const cssW = this.canvas.clientWidth;
    const cssH = this.canvas.clientHeight;
    const W = Math.max(1, Math.round(cssW * dpr));
    const H = Math.max(1, Math.round(cssH * dpr));
    if (this.canvas.width !== W || this.canvas.height !== H) {
      this.canvas.width = W;
      this.canvas.height = H;
    }
    gl.viewport(0, 0, W, H);
    const [x0, y0, bw, bh] = base;
    // uv = uvA * fragCoord + uvB, with fragCoord in device pixels, origin bottom-left.
    const ax = 1 / (dpr * zoom * bw);
    const ay = -1 / (dpr * zoom * bh);
    const bx = (centre[0] - cssW / (2 * zoom) - x0) / bw;
    const by = (centre[1] + cssH / (2 * zoom) - y0) / bh;
    gl.uniform2f(this.loc.uvA, ax, ay);
    gl.uniform2f(this.loc.uvB, bx, by);
    gl.uniform1f(this.loc.colorOn, pw.colorOn);
    gl.uniform1f(this.loc.hueShift, pw.hueShift);
    gl.uniform1f(this.loc.adaptSat, pw.adaptSat);
    gl.uniform1f(this.loc.satBoost, pw.satBoost);
    gl.uniform1f(this.loc.globalSat, pw.globalSat);
    gl.uniform1f(this.loc.toneOn, pw.toneOn);
    gl.uniform1f(this.loc.mono, pw.mono);
    gl.uniform3f(this.loc.monoMix, ...pw.monoMix);
    gl.uniform3f(this.loc.shadowTint, ...pw.shadowTint);
    gl.uniform3f(this.loc.highlightTint, ...pw.highlightTint);
    gl.uniform1f(this.loc.sStr, pw.sStr);
    gl.uniform1f(this.loc.hStr, pw.hStr);
    gl.uniform1f(this.loc.pivot, pw.pivot);
    gl.uniform1f(this.loc.curveOn, pw.curveOn);
    gl.uniform1f(this.loc.basicOn, pw.basicOn);
    gl.uniform1f(this.loc.expOn, pw.expOn);
    gl.uniform1f(this.loc.expGain, pw.expGain);
    gl.uniform1f(this.loc.bContrast, pw.bContrast);
    gl.uniform1f(this.loc.bHigh, pw.bHigh);
    gl.uniform1f(this.loc.bShadow, pw.bShadow);
    gl.uniform1f(this.loc.bWhite, pw.bWhite);
    gl.uniform1f(this.loc.bBlack, pw.bBlack);
    gl.uniform1f(this.loc.gradeOn, pw.gradeOn);
    gl.uniform1f(this.loc.gMid, pw.gMid);
    gl.uniform1f(this.loc.gWidth, pw.gWidth);
    gl.uniform3f(this.loc.gShOff, ...pw.gShOff);
    gl.uniform1f(this.loc.gShLum, pw.gShLum);
    gl.uniform3f(this.loc.gMidOff, ...pw.gMidOff);
    gl.uniform1f(this.loc.gMidLum, pw.gMidLum);
    gl.uniform3f(this.loc.gHiOff, ...pw.gHiOff);
    gl.uniform1f(this.loc.gHiLum, pw.gHiLum);

    // Finishing. Vignette centre and grain cell follow quartz: the grain cell scales with the full image size.
    const [fw, fh] = full;
    const scale = Math.max(fw, fh, 1) / 1000;
    gl.uniform4f(this.loc.rect, x0, y0, bw, bh);
    gl.uniform2f(this.loc.fullDims, fw, fh);
    gl.uniform1f(this.loc.warmOn, pw.warmOn);
    gl.uniform1f(this.loc.warmW, pw.warmW);
    gl.uniform1f(this.loc.vigOn, pw.vigOn);
    gl.uniform1f(this.loc.vigAmount, pw.vigAmount);
    gl.uniform1f(this.loc.vigSize, pw.vigSize);
    gl.uniform1f(this.loc.vigTrans, pw.vigTrans);
    gl.uniform1f(this.loc.vigRound, pw.vigRound);
    gl.uniform2f(this.loc.vigC, Math.max((fw - 1) / 2, 0.5), Math.max((fh - 1) / 2, 0.5));
    gl.uniform1f(this.loc.vigMaxR, Math.sqrt(Math.max((fw - 1) / 2, 0.5) ** 2 + Math.max((fh - 1) / 2, 0.5) ** 2));
    gl.uniform1f(this.loc.grainOn, pw.grainOn);
    gl.uniform1f(this.loc.grainAmt, pw.grainAmt);
    gl.uniform1f(this.loc.grainCell, Math.max(pw.grainSize * scale, 1));
    // Border thickness in pixels, as quartz: relative to the shorter side, at least 1, at most half of it.
    const shorter = Math.min(fw, fh);
    const borderPx = Math.min(Math.max(Math.round(shorter * pw.borderThick), 1), Math.ceil(shorter / 2));
    gl.uniform1f(this.loc.borderOn, pw.borderOn);
    gl.uniform1f(this.loc.borderPx, borderPx);
    gl.uniform3f(this.loc.borderColor, ...pw.borderColor);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }
}
