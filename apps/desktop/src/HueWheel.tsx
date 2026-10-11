// Hue wheel: drag around the ring to set the hue angle (-180..180 degrees). Double-click resets to 0.
// The ring colours are a guide; the number shown is the value the engine uses.
import { useRef } from "react";

type Props = { value: number; onChange: (deg: number) => void };

export function HueWheel({ value, onChange }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const angleAt = (e: React.PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.round((Math.atan2(e.clientY - (r.top + r.height / 2), e.clientX - (r.left + r.width / 2)) * 180) / Math.PI);
  };
  return (
    <div className="hue-box">
      <div className="hue-wheel" ref={ref}
        onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); onChange(angleAt(e)); }}
        onPointerMove={(e) => { if (e.buttons === 1) onChange(angleAt(e)); }}
        onDoubleClick={() => onChange(0)}>
        <div className="hue-dot" style={{ transform: `rotate(${value}deg) translateX(58px)` }} />
      </div>
      <span className="small">Hue {value}°</span>
    </div>
  );
}
