import { useRef } from "react";
import { Pt, curveLut, sortPoints, LINEAR_CURVE } from "./curveMath";

// Lightroom / Photoshop style curve: click the curve to add a point, drag a point to move it,
// double-click an interior point or drag it off the graph to remove it. The end points keep
// their x at 0 and 1. The histogram sits behind the curve on the input (x) axis.
const S = 256;

export function CurvePanel({ points, onChange, histogram }: { points: Pt[]; onChange: (p: Pt[]) => void; histogram: number[] | null }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const drag = useRef<{ index: number } | null>(null);
  const pts = sortPoints(points);

  const toXY = (e: { clientX: number; clientY: number }): Pt => {
    const r = svgRef.current!.getBoundingClientRect();
    return { x: Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), y: Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height)) };
  };
  const sx = (x: number) => x * S;
  const sy = (y: number) => (1 - y) * S;

  const curve = curveLut(pts);
  const curveD = curve.map((v, i) => `${i === 0 ? "M" : "L"}${sx(i / 1023)},${sy(v)}`).join(" ");

  const histD = (() => {
    if (!histogram) return "";
    const scale = Math.max(1, ...histogram.filter((v) => v > 0).sort((a, b) => a - b).slice(-Math.max(1, Math.floor(histogram.filter((v) => v > 0).length * 0.01))));
    return `M0,${S} ` + histogram.map((v, i) => `L${(i / 255) * S},${S - Math.min(1, v / scale) * S * 0.9}`).join(" ") + ` L${S},${S} Z`;
  })();

  const isEnd = (i: number, arr: Pt[]) => i === 0 || i === arr.length - 1;

  const onDown = (e: React.PointerEvent<SVGElement>) => {
    const p = toXY(e);
    // Find a point under the pointer (within 6 px), else add one on the curve.
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
    const p = toXY(e);
    const i = drag.current.index;
    drag.current = null;
    // Dragged far outside the graph: remove the interior point.
    if (!isEnd(i, pts) && (e.clientY < svgRef.current!.getBoundingClientRect().top - 30 || e.clientY > svgRef.current!.getBoundingClientRect().bottom + 30)) {
      onChange(pts.filter((_, j) => j !== i));
      return;
    }
    void p;
  };

  const onDoubleClick = (e: React.MouseEvent<SVGElement>) => {
    const p = toXY(e);
    pts.forEach((q, i) => {
      if (!isEnd(i, pts) && Math.hypot(sx(q.x) - sx(p.x), sy(q.y) - sy(p.y)) < 8) onChange(pts.filter((_, j) => j !== i));
    });
  };

  return (
    <div className="curvebox">
      <svg ref={svgRef} width={S} height={S} className="curve" onPointerDown={onDown} onPointerMove={onMove}
        onPointerUp={onUp} onDoubleClick={onDoubleClick} onPointerCancel={() => (drag.current = null)}>
        <rect width={S} height={S} fill="#0a0b0d" />
        {[0.25, 0.5, 0.75].map((g) => (
          <g key={g}>
            <line x1={sx(g)} y1={0} x2={sx(g)} y2={S} stroke="#1f2229" />
            <line x1={0} y1={sy(g)} x2={S} y2={sy(g)} stroke="#1f2229" />
          </g>
        ))}
        {histD && <path d={histD} fill="#5a6070" fillOpacity={0.45} />}
        <line x1={0} y1={S} x2={S} y2={0} stroke="#333" strokeDasharray="4 4" />
        <path d={curveD} fill="none" stroke="#7fd6ef" strokeWidth={2} />
        {pts.map((q, i) => (
          <circle key={i} cx={sx(q.x)} cy={sy(q.y)} r={5} fill={isEnd(i, pts) ? "#00a6d6" : "#ffffff"} stroke="#001018" />
        ))}
      </svg>
      <div className="curvebar">
        <span>{pts.length} points</span>
        <button onClick={() => onChange(LINEAR_CURVE)}>Reset curve</button>
      </div>
    </div>
  );
}
