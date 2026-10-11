// Control definitions for the right-hand panel. Field names match the Rust `Sliders` struct.
export type Dab = { kind: number; x: number; y: number; radius: number; hardness: number; opacity: number; strength: number };

export type Sliders = {
  quartz: boolean;
  brush: Dab[];
  ev: number; basicContrast: number; boostBlacks: number; boostWhites: number;
  adaptive: number; adaptiveContrast: number; adaptiveRegions: number; adaptiveBrightness: number;
  protectHighlights: number; protectShadows: number;
  hdrStrength: number; hdrDetail: number;
  details: number; detailBoost: number; detailThreshold: number; detailRadius: number; detailSharpen: number;
  processIndependently: boolean;
  adaptiveSaturation: number; saturation: number; saturationBoost: number; hue: number; colorRegions: number;
  denoise: number; noiseSuppression: number;
  curveShadows: number; curveMids: number; curveHighlights: number;
  warmth: number; toneColors: [number, number, number][]; toneStrength: number; diffusion: number;
  vignette: number; vignetteSize: number; vignetteTransition: number; vignetteCenterX: number; vignetteCenterY: number;
  grain: number; grainSize: number; grainSeed: number; borderSize: number; borderColor: [number, number, number];
  transparency: number;
  strokeEnabled: boolean; strokeKind: number; strokeX: number; strokeY: number; strokeRadius: number;
  strokeHardness: number; strokeOpacity: number; strokeStrength: number;
};

export const NEUTRAL: Sliders = {
  quartz: false,
  brush: [],
  ev: 0, basicContrast: 0, boostBlacks: 0, boostWhites: 0,
  adaptive: 0, adaptiveContrast: 0, adaptiveRegions: 4, adaptiveBrightness: 0, protectHighlights: 0, protectShadows: 0,
  hdrStrength: 0, hdrDetail: 1,
  details: 1, detailBoost: 1, detailThreshold: 0.5, detailRadius: 1.5, detailSharpen: 0, processIndependently: false,
  adaptiveSaturation: 0, saturation: 1, saturationBoost: 0, hue: 0, colorRegions: 1,
  denoise: 0, noiseSuppression: 1,
  curveShadows: 0, curveMids: 0.5, curveHighlights: 1,
  warmth: 0, toneColors: [[1, 1, 1], [1, 1, 1], [1, 1, 1], [1, 1, 1]], toneStrength: 0, diffusion: 0,
  vignette: 0, vignetteSize: 0.5, vignetteTransition: 0.5, vignetteCenterX: 0.5, vignetteCenterY: 0.5,
  grain: 0, grainSize: 1, grainSeed: 1, borderSize: 0, borderColor: [0, 0, 0], transparency: 0,
  strokeEnabled: false, strokeKind: 0, strokeX: 0.5, strokeY: 0.5, strokeRadius: 0.2,
  strokeHardness: 0.5, strokeOpacity: 1, strokeStrength: 1,
};

export type Slider = { key: keyof Sliders; label: string; min: number; max: number; step: number };
export type Section = { id: string; title: string; sliders: Slider[] };

export const SECTIONS: Section[] = [
  { id: "basic", title: "Basic Exposure", sliders: [
    { key: "ev", label: "Exposure (EV)", min: -2, max: 2, step: 0.05 },
    { key: "basicContrast", label: "Contrast", min: -1, max: 1, step: 0.01 },
    { key: "boostBlacks", label: "Boost Blacks", min: -1, max: 1, step: 0.01 },
    { key: "boostWhites", label: "Boost Whites", min: -1, max: 1, step: 0.01 },
  ]},
  { id: "adaptive", title: "Adaptive Exposure", sliders: [
    { key: "adaptive", label: "Adaptive Exposure", min: 0, max: 1, step: 0.01 },
    { key: "adaptiveRegions", label: "Regions", min: 1, max: 30, step: 1 },
    { key: "adaptiveContrast", label: "Contrast", min: -1, max: 1, step: 0.01 },
    { key: "adaptiveBrightness", label: "Brightness (EV)", min: -2, max: 2, step: 0.01 },
    { key: "protectHighlights", label: "Protect Highlights", min: 0, max: 1, step: 0.01 },
    { key: "protectShadows", label: "Protect Shadows", min: 0, max: 1, step: 0.01 },
  ]},
  { id: "hdr", title: "HDR Tone Mapping", sliders: [
    { key: "hdrStrength", label: "Compression", min: 0, max: 1, step: 0.01 },
    { key: "hdrDetail", label: "Detail", min: 0, max: 2, step: 0.01 },
  ]},
  { id: "details", title: "Details & Clarity", sliders: [
    { key: "details", label: "Strength", min: 0, max: 2, step: 0.01 },
    { key: "detailBoost", label: "Detail Boost", min: 0, max: 2, step: 0.01 },
    { key: "detailThreshold", label: "Threshold", min: 0, max: 1, step: 0.01 },
    { key: "detailRadius", label: "Radius (px)", min: 1, max: 16, step: 0.1 },
    { key: "detailSharpen", label: "Sharpen", min: 0, max: 2, step: 0.01 },
  ]},
  { id: "color", title: "Color & Vibrance", sliders: [
    { key: "adaptiveSaturation", label: "Adaptive Saturation", min: 0, max: 1, step: 0.01 },
    { key: "colorRegions", label: "Color Regions", min: 1, max: 10, step: 0.1 },
    { key: "saturation", label: "Saturation", min: 0, max: 2, step: 0.01 },
    { key: "saturationBoost", label: "Saturation Boost", min: -1, max: 1, step: 0.01 },
    { key: "hue", label: "Hue (degrees)", min: 0, max: 360, step: 1 },
  ]},
  { id: "noise", title: "Noise Suppression", sliders: [
    { key: "denoise", label: "Amount", min: 0, max: 1, step: 0.01 },
    { key: "noiseSuppression", label: "Suppression", min: 0, max: 2, step: 0.01 },
  ]},
  { id: "curve", title: "Tone Curve", sliders: [
    { key: "curveShadows", label: "Shadows output", min: 0, max: 1, step: 0.01 },
    { key: "curveMids", label: "Midtones output", min: 0, max: 1, step: 0.01 },
    { key: "curveHighlights", label: "Highlights output", min: 0, max: 1, step: 0.01 },
  ]},
  { id: "local", title: "Local Brush", sliders: [
    { key: "strokeKind", label: "Kind (0 dodge, 1 burn, 2 smooth, 3 brush out)", min: 0, max: 3, step: 1 },
    { key: "strokeX", label: "Centre X", min: 0, max: 1, step: 0.01 },
    { key: "strokeY", label: "Centre Y", min: 0, max: 1, step: 0.01 },
    { key: "strokeRadius", label: "Radius", min: 0.02, max: 0.6, step: 0.01 },
    { key: "strokeHardness", label: "Hardness", min: 0, max: 1, step: 0.01 },
    { key: "strokeOpacity", label: "Opacity", min: 0, max: 1, step: 0.01 },
    { key: "strokeStrength", label: "Overall Strength", min: 0, max: 1, step: 0.01 },
  ]},
  { id: "finish", title: "Finishing Touches", sliders: [
    { key: "warmth", label: "Warmth", min: -1, max: 1, step: 0.01 },
    { key: "toneStrength", label: "Tone Strength", min: 0, max: 1, step: 0.01 },
    { key: "diffusion", label: "Diffusion", min: 0, max: 1, step: 0.01 },
    { key: "vignette", label: "Vignette Strength", min: 0, max: 1, step: 0.01 },
    { key: "vignetteSize", label: "Vignette Size", min: 0, max: 1, step: 0.01 },
    { key: "vignetteTransition", label: "Vignette Transition", min: 0, max: 1, step: 0.01 },
    { key: "vignetteCenterX", label: "Vignette Centre X", min: 0, max: 1, step: 0.01 },
    { key: "vignetteCenterY", label: "Vignette Centre Y", min: 0, max: 1, step: 0.01 },
    { key: "grain", label: "Grain", min: 0, max: 1, step: 0.01 },
    { key: "grainSize", label: "Grain Size (px)", min: 1, max: 8, step: 1 },
    { key: "borderSize", label: "Border Size", min: 0, max: 0.2, step: 0.005 },
    { key: "transparency", label: "Transparency", min: 0, max: 1, step: 0.01 },
  ]},
];

// Our own starting points, not Topaz presets.
export const PRESETS: Record<string, Partial<Sliders>> = {
  Neutral: {},
  "Warm lift": { ev: 0.2, adaptive: 0.6, details: 1.3, saturation: 1.15, warmth: 0.3, vignette: 0.4, grain: 0.2 },
  "Soft detail": { adaptive: 0.4, details: 0.8, denoise: 0.3, saturation: 0.95 },
};

// sRGB hex <-> linear RGB triple, for the colour pickers (engine works in linear light).
export function hexToLinear(hex: string): [number, number, number] {
  const to = (v: number) => (v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4));
  const n = parseInt(hex.slice(1), 16);
  return [to(((n >> 16) & 255) / 255), to(((n >> 8) & 255) / 255), to((n & 255) / 255)];
}

export function linearToHex(c: [number, number, number]): string {
  const to = (v: number) => {
    const s = v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055;
    return Math.round(Math.min(1, Math.max(0, s)) * 255);
  };
  return "#" + c.map((v) => to(v).toString(16).padStart(2, "0")).join("");
}
