// Quartz's exact parameters (params.rs): names, defaults and ranges as in the author's repo.
export type Pt = { x: number; y: number };
// Photoshop-style curves: the master curve (points) and the per-channel curves, composed on render.
export type Curve = { enabled: boolean; points: Pt[]; red?: Pt[]; green?: Pt[]; blue?: Pt[] };
export type ColorWheelValue = { hue: number; sat: number; lum: number };
export type Grading = {
  enabled: boolean; shadows: ColorWheelValue; midtones: ColorWheelValue; highlights: ColorWheelValue;
  blending: number; balance: number;
};

export type LsParams = {
  basic_correction: { enabled: boolean; exposure: number; contrast: number; highlights: number; shadows: number; whites: number; blacks: number };
  grading: Grading;
  basic_exposure: { enabled: boolean; brightness: number; contrast: number; boost_blacks: number; boost_whites: number };
  adaptive_exposure: { enabled: boolean; adaptive_exposure: number; regions: number; contrast: number; brightness: number; protect_highlights: number; protect_shadows: number };
  details: { enabled: boolean; strength: number; detail_boost: number; threshold: number; radius: number; sharpen: number; process_independently: boolean };
  color: { enabled: boolean; adaptive_saturation: number; color_regions: number; saturation: number; saturation_boost: number; hue: number };
  noise: { enabled: boolean; suppression: number; amount: number };
  curve: Curve;
  // Optional: presets may carry tone (black & white and split toning); the GPU view applies it.
  tone?: {
    enabled: boolean; monochrome: number; mono_mix: [number, number, number];
    shadow_hue: number; shadow_strength: number; highlight_hue: number; highlight_strength: number; balance: number;
  };
  finishing: {
    enabled: boolean; transparency: number; warmth: number;
    vignette: { amount: number; size: number; transition: number; roundness: number };
    grain: { amount: number; size: number };
    border: { enabled: boolean; thickness: number; color: [number, number, number] };
  };
};

// Defaults exactly as Quartz's Default impls (params.rs).
export const LS_DEFAULT: LsParams = {
  basic_correction: { enabled: true, exposure: 0, contrast: 0, highlights: 0, shadows: 0, whites: 0, blacks: 0 },
  grading: {
    enabled: true,
    shadows: { hue: 0, sat: 0, lum: 0 }, midtones: { hue: 0, sat: 0, lum: 0 }, highlights: { hue: 0, sat: 0, lum: 0 },
    blending: 0.5, balance: 0,
  },
  basic_exposure: { enabled: true, brightness: 0, contrast: 0, boost_blacks: 0, boost_whites: 0 },
  adaptive_exposure: { enabled: true, adaptive_exposure: 0, regions: 10, contrast: 0, brightness: 0, protect_highlights: 0, protect_shadows: 0 },
  details: { enabled: true, strength: 0, detail_boost: 1, threshold: 0.1, radius: 1, sharpen: 0, process_independently: true },
  color: { enabled: true, adaptive_saturation: 0, color_regions: 10, saturation: 0, saturation_boost: 1, hue: 0 },
  noise: { enabled: true, suppression: 0, amount: 0.5 },
  curve: { enabled: false, points: [{ x: 0, y: 0 }, { x: 1, y: 1 }] },
  finishing: {
    enabled: true, transparency: 0, warmth: 0,
    vignette: { amount: 0, size: 0.5, transition: 0.5, roundness: 0 },
    grain: { amount: 0, size: 1 },
    border: { enabled: false, thickness: 0, color: [1, 1, 1] },
  },
};

export type Field = { path: string; label: string; min: number; max: number; step: number; int?: boolean };
export type LsSection = { id: keyof Omit<LsParams, "curve">; title: string; enabled?: boolean; fields: Field[]; custom?: "grading" };

// Ranges from the comments in params.rs.
export const LS_SECTIONS: LsSection[] = [
  { id: "basic_correction", title: "Basic Correction", enabled: true, fields: [
    { path: "exposure", label: "Exposure", min: -5, max: 5, step: 0.01 },
    { path: "contrast", label: "Contrast", min: -1, max: 1, step: 0.01 },
    { path: "highlights", label: "Highlights", min: -1, max: 1, step: 0.01 },
    { path: "shadows", label: "Shadows", min: -1, max: 1, step: 0.01 },
    { path: "whites", label: "Whites", min: -1, max: 1, step: 0.01 },
    { path: "blacks", label: "Blacks", min: -1, max: 1, step: 0.01 },
  ]},
  { id: "grading", title: "Color Grading", enabled: true, fields: [], custom: "grading" },
  { id: "adaptive_exposure", title: "Adaptive Exposure", enabled: true, fields: [
    { path: "adaptive_exposure", label: "Adaptive Exposure", min: 0, max: 3, step: 0.01 },
    { path: "regions", label: "Regions", min: 1, max: 30, step: 1, int: true },
    { path: "contrast", label: "Contrast", min: -1, max: 1, step: 0.01 },
    { path: "brightness", label: "Brightness", min: -1, max: 1, step: 0.01 },
    { path: "protect_highlights", label: "Protect Highlights", min: 0, max: 1, step: 0.01 },
    { path: "protect_shadows", label: "Protect Shadows", min: 0, max: 1, step: 0.01 },
  ]},
  { id: "details", title: "Details & Clarity", enabled: true, fields: [
    { path: "strength", label: "Strength", min: -1, max: 3, step: 0.01 },
    { path: "detail_boost", label: "Detail Boost", min: 0, max: 3, step: 0.01 },
    { path: "threshold", label: "Threshold", min: 0, max: 1, step: 0.01 },
    { path: "radius", label: "Radius", min: 0.5, max: 30, step: 0.1 },
    { path: "sharpen", label: "Sharpen", min: 0, max: 2, step: 0.01 },
  ]},
  { id: "color", title: "Color & Vibrance", enabled: true, fields: [
    { path: "adaptive_saturation", label: "Adaptive Saturation", min: -1, max: 2, step: 0.01 },
    { path: "color_regions", label: "Color Regions", min: 1, max: 30, step: 1, int: true },
    { path: "saturation", label: "Saturation", min: -1, max: 1, step: 0.01 },
    { path: "saturation_boost", label: "Saturation Boost", min: 0, max: 2, step: 0.01 },
    { path: "hue", label: "Hue", min: -180, max: 180, step: 1 },
  ]},
  { id: "noise", title: "Noise Suppression", enabled: true, fields: [
    { path: "suppression", label: "Suppression", min: 0, max: 1, step: 0.01 },
    { path: "amount", label: "Amount", min: 0, max: 1, step: 0.01 },
  ]},
  { id: "finishing", title: "Finishing Touches", enabled: true, fields: [
    { path: "warmth", label: "Warmth", min: -1, max: 1, step: 0.01 },
    { path: "transparency", label: "Transparency", min: 0, max: 1, step: 0.01 },
    { path: "vignette.amount", label: "Vignette Amount", min: -1, max: 1, step: 0.01 },
    { path: "vignette.size", label: "Vignette Size", min: 0.1, max: 1, step: 0.01 },
    { path: "vignette.transition", label: "Vignette Feather", min: 0, max: 1, step: 0.01 },
    { path: "vignette.roundness", label: "Vignette Roundness", min: 0, max: 1, step: 0.01 },
    { path: "grain.amount", label: "Grain Amount", min: 0, max: 1, step: 0.01 },
    { path: "grain.size", label: "Grain Size", min: 0.5, max: 3, step: 0.01 },
    { path: "border.thickness", label: "Border Thickness", min: 0, max: 0.2, step: 0.001 },
  ]},
];

export function getPath(obj: unknown, path: string): number {
  return path.split(".").reduce((o: any, k) => o[k], obj) as number;
}

export function setPath<T>(obj: T, sectionId: string, path: string, value: number): T {
  const copy: any = structuredClone(obj);
  const parts = [sectionId, ...path.split(".")];
  let cur = copy;
  for (let i = 0; i < parts.length - 1; i++) cur = cur[parts[i]];
  cur[parts[parts.length - 1]] = value;
  return copy;
}
