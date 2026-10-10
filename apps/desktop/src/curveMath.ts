// Limestone curve maths, ported from its generate_curve_lut (stages/curve.rs): piecewise cubic
// Hermite with Catmull-Rom tangents, clamped to 0..1, sampled on a 1024-entry LUT.
export type Pt = { x: number; y: number };

export function sortPoints(points: Pt[]): Pt[] {
  return [...points].sort((a, b) => a.x - b.x);
}

export function curveLut(points: Pt[]): number[] {
  const lut = new Array<number>(1024).fill(0);
  if (points.length < 2) {
    for (let i = 0; i < 1024; i++) lut[i] = i / 1023;
    return lut;
  }
  const p = sortPoints(points);
  const n = p.length;
  for (let i = 0; i < 1024; i++) {
    const x = i / 1023;
    if (x <= p[0].x) { lut[i] = p[0].y; continue; }
    if (x >= p[n - 1].x) { lut[i] = p[n - 1].y; continue; }
    let idx = 0;
    while (idx < n - 1 && p[idx + 1].x < x) idx++;
    const p0 = idx > 0 ? p[idx - 1] : p[idx];
    const p1 = p[idx];
    const p2 = p[idx + 1];
    const p3 = idx + 2 < n ? p[idx + 2] : p2;
    const dx = Math.max(p2.x - p1.x, 1e-5);
    const t = (x - p1.x) / dx;
    const m1 = (p2.y - p0.y) / Math.max(p2.x - p0.x, 1e-5);
    const m2 = (p3.y - p1.y) / Math.max(p3.x - p1.x, 1e-5);
    const t2 = t * t, t3 = t2 * t;
    const y = (2 * t3 - 3 * t2 + 1) * p1.y + (t3 - 2 * t2 + t) * dx * m1 + (-2 * t3 + 3 * t2) * p2.y + (t3 - t2) * dx * m2;
    lut[i] = Math.min(1, Math.max(0, y));
  }
  return lut;
}

export const LINEAR_CURVE: Pt[] = [{ x: 0, y: 0 }, { x: 1, y: 1 }];
