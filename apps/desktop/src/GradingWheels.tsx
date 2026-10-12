// Colour grading wheels (Lightroom / Camera Raw style). Drag inside a wheel: the angle sets the hue,
// the distance from the centre sets the saturation. Double-click a wheel to clear it.
import { useRef } from "react";
import type { ColorWheelValue, Grading } from "./quartz";

type ZoneKey = "shadows" | "midtones" | "highlights";
const ZONES: [ZoneKey, string][] = [["shadows", "Shadows"], ["midtones", "Midtones"], ["highlights", "Highlights"]];
const SIZE = 120;

export function GradingWheels({ value, onChange }: { value: Grading; onChange: (g: Grading) => void }) {
  const setZone = (zone: ZoneKey, patch: Partial<ColorWheelValue>) => onChange({ ...value, [zone]: { ...value[zone], ...patch } });
  return (
    <div className="grading">
      {ZONES.map(([key, label]) => (
        <Wheel key={key} label={label} v={value[key]} onChange={(patch) => setZone(key, patch)} />
      ))}
      <label className="slider">
        <span>Blending<em>{value.blending.toFixed(2)}</em></span>
        <input type="range" min={0} max={1} step={0.01} value={value.blending}
          onChange={(e) => onChange({ ...value, blending: Number(e.target.value) })} />
      </label>
      <label className="slider">
        <span>Balance<em>{value.balance.toFixed(2)}</em></span>
        <input type="range" min={-1} max={1} step={0.01} value={value.balance}
          onChange={(e) => onChange({ ...value, balance: Number(e.target.value) })} />
      </label>
    </div>
  );
}

function Wheel({ label, v, onChange }: { label: string; v: ColorWheelValue; onChange: (patch: Partial<ColorWheelValue>) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const pick = (e: React.PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    const dx = e.clientX - (r.left + r.width / 2);
    const dy = e.clientY - (r.top + r.height / 2);
    const hue = (Math.round((Math.atan2(dy, dx) * 180) / Math.PI) + 360) % 360;
    const sat = Math.min(1, Math.hypot(dx, dy) / (r.width / 2));
    onChange({ hue, sat });
  };
  const rad = (v.sat * SIZE) / 2;
  const dotX = SIZE / 2 + Math.cos((v.hue * Math.PI) / 180) * rad;
  const dotY = SIZE / 2 + Math.sin((v.hue * Math.PI) / 180) * rad;
  return (
    <div className="wheel-box">
      <span className="small">{label}</span>
      <div className="wheel" ref={ref}
        onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); pick(e); }}
        onPointerMove={(e) => { if (e.buttons === 1) pick(e); }}
        onDoubleClick={() => onChange({ hue: 0, sat: 0 })}>
        <div className="wheel-dot" style={{ left: dotX, top: dotY }} />
      </div>
      <span className="small">Hue {v.hue}° · Sat {Math.round(v.sat * 100)}%</span>
      <label className="slider">
        <span>Luminance<em>{v.lum.toFixed(2)}</em></span>
        <input type="range" min={-1} max={1} step={0.01} value={v.lum} onChange={(e) => onChange({ lum: Number(e.target.value) })} />
      </label>
    </div>
  );
}
