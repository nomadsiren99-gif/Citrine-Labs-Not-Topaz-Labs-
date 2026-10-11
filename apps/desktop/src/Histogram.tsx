export type Hist = { r: number[]; g: number[]; b: number[]; l: number[] };
type Channel = "rgb" | "r" | "g" | "b" | "l";

// Capped at the 99th percentile of non-zero bins, so a single spike does not flatten everything.
function scaleOf(bins: number[][]): number {
  const all = bins.flat().filter((v) => v > 0).sort((a, b) => a - b);
  if (all.length === 0) return 1;
  return Math.max(1, all[Math.floor(all.length * 0.99)]);
}

export function pathFor(bins: number[], scale: number): string {
  const pts = bins.map((v, i) => `${i},${60 - Math.min(1, v / scale) * 60}`);
  return `M0,60 L${pts.join(" L")} L255,60 Z`;
}

export function Histogram({ hist, channel, onChannel }: { hist: Hist | null; channel: Channel; onChannel: (c: Channel) => void }) {
  if (!hist) return <div className="histogram" />;
  const show = channel === "rgb" ? [hist.r, hist.g, hist.b] : [channel === "r" ? hist.r : channel === "g" ? hist.g : channel === "b" ? hist.b : hist.l];
  const scale = scaleOf(show);
  const colors = channel === "rgb" ? ["#e0524a", "#4fbf5f", "#4a7fe0"] : [channel === "l" ? "#c9c9c9" : channel === "r" ? "#e0524a" : channel === "g" ? "#4fbf5f" : "#4a7fe0"];
  return (
    <div>
      <div className="chan">
        {(["rgb", "l", "r", "g", "b"] as Channel[]).map((c) => (
          <button key={c} className={channel === c ? "sel" : ""} onClick={() => onChannel(c)}>{c === "rgb" ? "RGB" : c.toUpperCase()}</button>
        ))}
      </div>
      <svg className="histogram" viewBox="0 0 255 60" preserveAspectRatio="none" aria-label="histogram">
        {show.map((bins, i) => (
          <path key={i} d={pathFor(bins, scale)} fill={colors[i]} fillOpacity={channel === "rgb" ? 0.45 : 0.6} />
        ))}
      </svg>
    </div>
  );
}

