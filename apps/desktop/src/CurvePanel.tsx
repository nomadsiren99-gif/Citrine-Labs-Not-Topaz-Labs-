import { useRef, useState } from "react";
import { Pt, curveLut, sortPoints, LINEAR_CURVE } from "./curveMath";

// Photoshop-style curves. Pick a channel (RGB or one colour); click the curve to add a point, drag a
// point to move it, drag an interior point off the graph or double-click it to remove it. Click a
// point to select it: its input and output values show, the arrow keys nudge it (Shift for bigger
// steps) and Delete removes it. The end points keep their x at 0 and 1.
export type CurveChannel = "rgb" | "red" | "green" | "blue";
const CHANNELS: [CurveChannel, string, string][] = [
  ["rgb", "RGB", "#e6e6e6"], ["red", "Red", "#ff6b6b"], ["green", "Green", "#6bdc7a"], ["blue", "Blue", "#6ba7ff"],
];
const S = 256;

export function CurvePanel({ points, onChange, histogram, channel, onChannel }: {
  points: Pt[]; onChange: (p: Pt[]) => void; histogram: number[] | null;
  channel: CurveChannel; onChannel: (c: CurveChannel) => void;
}) {
  const svgRef = useRef<SVGSVGElement>(null);
  const drag = useRef<{ index: number } | null>(null);
  const [sel, setSel] = useState<number | null>(null);
  const pts = sortPoints(points);
  const colour = CHANNELS.find(([id]) => id === channel)![2];

  const toXY = (e: { clientX: number; clientY: number }): Pt => {
    const r = svgRef.current!.getBoundingClientRect();
    return { x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), y: Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height)) };
  };
  const sx = (x: number) => x * S;
  const sy = (y: number) => (1 - y) * S;
  const curve = curveLut(pts);
  const curveD = curve.map((v, i) => `${i === 0 ? "M" : "L"}${sx(i / 1023)},${sy(v)}`).join(" ");
  const isEnd = (i: number, arr: Pt[]) => i === 0 || i === arr.length - 1;

  const histD = (() => {
    if (!histogram) return "";
    const nonZero = histogram.filter((v) => v > 0).sort((a, b) => a - b);
    const scale = Math.max(1, ...nonZero.slice(-Math.max(1, Math.floor(nonZero.length * 0.01))));
    return `M0,${S} ` + histogram.map((v, i) => `L${(i / 255) * S},${S - Math.min(1, v / scale) * S * 0.9}`).join(" ") + ` L${S},${S} Z`;
  })();

  const onDown = (e: React.PointerEvent<SVGElement>) => {
    svgRef.current?.focus();
    const p = toXY(e);
    let hit = -1;
    pts.forEach((q, i) => {
      if (Math.hypot(sx(q.x) - sx(p.x), sy(q.y) - sy(p.y)) < 8) hit = i;
    });
    let next = pts;
    if (hit < 0) {
      const y = curve[Math.round(p.x * 1023)];
      next = sortPoints([...pts, { x: p.x, y }]);
      hit = next.findIndex((q) => q.x === p.x);
      onChange(next);
    }
    drag.current = { index: hit };
    setSel(hit);
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onMove = (e: React.PointerEvent<SVGElement>) => {
    if (!drag.current) return;
    const p = toXY(e);
    const i = drag.current.index;
    const next = pts.map((q) => ({ ...q }));
    if (isEnd(i, pts)) {
      next[i].y = p.y;
    } else {
      next[i] = {
        x: Math.min(Math.max(p.x, next[i - 1].x + 0.001), next[i + 1].x - 0.001),
        y: p.y,
      };
    }
    onChange(next);
  };

  const onUp = (e: React.PointerEvent<SVGElement>) => {
    if (!drag.current) return;
    const i = drag.current.index;
    drag.current = null;
    const r = svgRef.current!.getBoundingClientRect();
    // Dragged far outside the graph: remove the interior point.
    if (!isEnd(i, pts) && (e.clientY < r.top - 30 || e.clientY > r.bottom + 30)) {
      onChange(pts.filter((_, j) => j !== i));
      setSel(null);
    }
  };

  const onDoubleClick = (e: React.MouseEvent<SVGElement>) => {
    const p = toXY(e);
    pts.forEach((q, i) => {
      if (!isEnd(i, pts) && Math.hypot(sx(q.x) - sx(p.x), sy(q.y) - sy(p.y)) < 8) onChange(pts.filter((_, j) => j !== i));
    });
    setSel(null);
  };

  const onKey = (e: React.KeyboardEvent<SVGElement>) => {
    if (sel === null) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      if (!isEnd(sel, pts)) { onChange(pts.filter((_, j) => j !== sel)); setSel(null); }
      e.preventDefault();
      return;
    }
    const step = (e.shiftKey ? 5 : 1) / 255;
    const d = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] }[e.key];
    if (!d) return;
    e.preventDefault();
    const next = pts.map((q) => ({ ...q }));
    const i = sel;
    if (!isEnd(i, pts)) next[i].x = Math.min(Math.max(next[i].x + d[0], next[i - 1].x + 0.001), next[i + 1].x - 0.001);
    next[i].y = Math.min(1, Math.max(0, next[i].y + d[1]));
    onChange(next);
  };

  const selected = sel !== null && sel < pts.length ? pts[sel] : null;

  return (
    <div className="curvebox">
      <div className="curve-channels">
        {CHANNELS.map(([id, label]) => (
          <button key={id} className={channel === id ? "sel" : ""} onClick={() => { onChannel(id); setSel(null); }}>{label}</button>
        ))}
      </div>
      <svg ref={svgRef} width={S} height={S} className="curve" tabIndex={0} onKeyDown={onKey}
        onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onDoubleClick={onDoubleClick}
        onPointerCancel={() => (drag.current = null)}>
        <rect width={S} height={S} fill="#0a0b0d" />
        {[0.25, 0.5, 0.75].map((g) => (
          <g key={g}>
            <line x1={sx(g)} y1={0} x2={sx(g)} y2={S} stroke="#1f2229" />
            <line x1={0} y1={sy(g)} x2={S} y2={sy(g)} stroke="#1f2229" />
          </g>
        ))}
        {histD && <path d={histD} fill="#5a6070" fillOpacity={0.45} />}
        <line x1={0} y1={S} x2={S} y2={0} stroke="#333" strokeDasharray="4 4" />
        <path d={curveD} fill="none" stroke={colour} strokeWidth={2} />
        {pts.map((q, i) => (
          <circle key={i} cx={sx(q.x)} cy={sy(q.y)} r={i === sel ? 6 : 5}
            fill={i === sel ? "#00a6d6" : isEnd(i, pts) ? "#7a8290" : "#ffffff"} stroke="#001018" />
        ))}
      </svg>
      <div className="curvebar">
        <span>{selected ? `Input ${Math.round(selected.x * 255)} · Output ${Math.round(selected.y * 255)}` : `${pts.length} points`}</span>
        <button onClick={() => { onChange(LINEAR_CURVE); setSel(null); }}>Reset</button>
      </div>
    </div>
  );
}
