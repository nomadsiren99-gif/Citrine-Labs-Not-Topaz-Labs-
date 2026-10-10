import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { NEUTRAL, PRESETS, SECTIONS, Sliders, Dab, hexToLinear, linearToHex } from "./controls";
import { FACTORY_PRESETS } from "./presets";
import "./App.css";

// Curve editor: three draggable points at input 0, 0.5 and 1. Y is the output level.
function CurveEditor({ values, onChange }: { values: [number, number, number]; onChange: (v: [number, number, number]) => void }) {
  const size = 220;
  const pad = 12;
  const toX = (x: number) => pad + x * (size - 2 * pad);
  const toY = (y: number) => size - pad - y * (size - 2 * pad);
  const dragging = useRef<number | null>(null);
  const xs = [0, 0.5, 1];
  const move = (e: React.PointerEvent<SVGSVGElement>) => {
    if (dragging.current === null) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const y = Math.min(1, Math.max(0, (size - pad - (e.clientY - rect.top)) / (size - 2 * pad)));
    const next = [...values] as [number, number, number];
    next[dragging.current] = y;
    onChange(next);
  };
  return (
    <svg width={size} height={size} className="curve" onPointerMove={move}
      onPointerUp={() => (dragging.current = null)} onPointerLeave={() => (dragging.current = null)}>
      <rect x={0} y={0} width={size} height={size} fill="#0a0b0d" />
      <line x1={toX(0)} y1={toY(0)} x2={toX(1)} y2={toY(1)} stroke="#333" strokeDasharray="4 4" />
      <polyline fill="none" stroke="#7fd6ef" strokeWidth={2}
        points={xs.map((x, i) => `${toX(x)},${toY(values[i])}`).join(" ")} />
      {xs.map((x, i) => (
        <circle key={i} cx={toX(x)} cy={toY(values[i])} r={6} fill="#00a6d6" style={{ cursor: "ns-resize" }}
          onPointerDown={(e) => { dragging.current = i; (e.target as Element).setPointerCapture(e.pointerId); }} />
      ))}
    </svg>
  );
}

function Histogram({ bins }: { bins: number[] }) {
  if (bins.length === 0) return <div className="histogram" />;
  const max = Math.max(1, ...bins);
  return (
    <svg className="histogram" viewBox="0 0 256 60" preserveAspectRatio="none" aria-label="luminance histogram">
      {bins.map((v, i) => (
        <rect key={i} x={i} y={60 - (v / max) * 60} width={1} height={(v / max) * 60} fill="#7fd6ef" />
      ))}
    </svg>
  );
}

export default function App() {
  const [path, setPath] = useState("");
  const [outPath, setOutPath] = useState("");
  const [sliders, setSliders] = useState<Sliders>(NEUTRAL);
  const [preset, setPreset] = useState("Neutral");
  const [showBefore, setShowBefore] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const [status, setStatus] = useState("Open an image to start.");
  const [zoom, setZoom] = useState<"fit" | 1 | 2>("fit");
  const [histogram, setHistogram] = useState<number[]>([]);
  const [past, setPast] = useState<Sliders[]>([]);
  const [future, setFuture] = useState<Sliders[]>([]);
  const [open_, setOpen] = useState<Record<string, boolean>>({ basic: true, adaptive: true });
  const lastEdit = useRef(0);
  const [paint, setPaint] = useState(false);
  const [view, setView] = useState<"single" | "split" | "side">("single");
  const [wipe, setWipe] = useState(0.5);
  const [before, setBefore] = useState<string | null>(null);
  const [presetQuery, setPresetQuery] = useState("");
  const [brushKind, setBrushKind] = useState(0);
  const [brushRadius, setBrushRadius] = useState(0.05);
  const [brushHardness, setBrushHardness] = useState(0.5);
  const [brushOpacity, setBrushOpacity] = useState(0.5);
  const [snapshots, setSnapshots] = useState<{ name: string; state: Sliders }[]>([]);
  const [renderMs, setRenderMs] = useState<number | null>(null);

  const shown = useMemo(() => (showBefore ? NEUTRAL : sliders), [showBefore, sliders]);

  // Drag and drop an image onto the window.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths.length > 0) {
        setPath(e.payload.paths[0]);
        setStatus(`Opened ${e.payload.paths[0]}`);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Re-render the preview after edits, debounced.
  useEffect(() => {
    if (!path) return;
    const id = window.setTimeout(() => {
      const t0 = performance.now();
      invoke<string>("render_preview", { path, sliders: shown })
        .then((b64) => {
          setPreview(`data:image/png;base64,${b64}`);
          setRenderMs(Math.round(performance.now() - t0));
          setStatus("Preview ready.");
        })
        .catch((e) => setStatus(String(e)));
    }, 150);
    return () => window.clearTimeout(id);
  }, [path, shown]);

  // Unadjusted preview for the split and side-by-side views.
  useEffect(() => {
    if (!path || view === "single") return;
    const id = window.setTimeout(() => {
      invoke<string>("render_preview", { path, sliders: NEUTRAL })
        .then((b64) => setBefore(`data:image/png;base64,${b64}`))
        .catch(() => setBefore(null));
    }, 150);
    return () => window.clearTimeout(id);
  }, [path, view, sliders.brush.length, shown]);

  // Histogram from the preview pixels.
  useEffect(() => {
    if (!preview) return;
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const ctx = c.getContext("2d");
      if (!ctx) return;
      ctx.drawImage(img, 0, 0);
      const px = ctx.getImageData(0, 0, c.width, c.height).data;
      const bins = new Array(256).fill(0);
      for (let i = 0; i < px.length; i += 4) {
        const y = Math.round(0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2]);
        bins[Math.min(255, Math.max(0, y))]++;
      }
      setHistogram(bins);
    };
    img.src = preview;
  }, [preview]);

  // Undo/redo. Consecutive edits within 400 ms collapse into one step.
  const commit = (before: Sliders) => {
    const now = Date.now();
    if (now - lastEdit.current > 400) {
      setPast((p) => [...p, before]);
      setFuture([]);
    }
    lastEdit.current = now;
  };
  const undo = () => {
    if (past.length === 0) return;
    setFuture((f) => [sliders, ...f]);
    setSliders(past[past.length - 1]);
    setPast((p) => p.slice(0, -1));
    setPreset("Custom");
    lastEdit.current = 0;
  };
  const redo = () => {
    if (future.length === 0) return;
    setPast((p) => [...p, sliders]);
    setSliders(future[0]);
    setFuture((f) => f.slice(1));
    setPreset("Custom");
    lastEdit.current = 0;
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== "z") return;
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const choosePreset = (name: string) => {
    commit(sliders);
    setPreset(name);
    setSliders({ ...NEUTRAL, ...PRESETS[name] });
  };

  const chooseFactory = (idx: number) => {
    const p = FACTORY_PRESETS[idx];
    commit(sliders);
    setPreset(p.name);
    // Factory values are tuned for the Limestone stage set, so switch to it.
    setSliders({ ...NEUTRAL, ...p.values, limestone: true, brush: [] });
  };

  const update = <K extends keyof Sliders>(key: K, value: Sliders[K]) => {
    commit(sliders);
    setPreset("Custom");
    setSliders((s) => ({ ...s, [key]: value }));
  };

  const paintAt = (e: React.PointerEvent<HTMLImageElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const dab: Dab = {
      kind: brushKind,
      x: (e.clientX - r.left) / r.width,
      y: (e.clientY - r.top) / r.height,
      radius: brushRadius,
      hardness: brushHardness,
      opacity: brushOpacity,
      strength: 1,
    };
    commit(sliders);
    setPreset("Custom");
    setSliders((s) => ({ ...s, brush: [...s.brush, dab] }));
  };

  const saveSnapshot = () => {
    setSnapshots((list) => [...list.slice(-8), { name: `Snapshot ${list.length + 1}`, state: sliders }]);
  };

  const recallSnapshot = (state: Sliders) => {
    commit(sliders);
    setSliders(state);
    setPreset("Custom");
  };

  const savePreset = async () => {
    const picked = await save({ defaultPath: "preset.json", filters: [{ name: "Preset", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    invoke("write_text_file", { path: picked, contents: JSON.stringify(sliders, null, 2) })
      .then(() => setStatus(`Preset saved to ${picked}`))
      .catch((e) => setStatus(String(e)));
  };

  // Project file: versioned session (source path, every control, brush strokes, model).
  const PROJECT_SCHEMA = 1;

  const saveProject = async () => {
    const picked = await save({ defaultPath: "session.photoproj.json", filters: [{ name: "Project", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    const doc = { schemaVersion: PROJECT_SCHEMA, app: "photo-editor", source: path, sliders };
    invoke("write_text_file", { path: picked, contents: JSON.stringify(doc, null, 2) })
      .then(() => setStatus(`Project saved to ${picked}`))
      .catch((e) => setStatus(String(e)));
  };

  const loadProject = async () => {
    const picked = await open({ multiple: false, filters: [{ name: "Project", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    try {
      const doc = JSON.parse(await invoke<string>("read_text_file", { path: picked }));
      if (doc.schemaVersion !== PROJECT_SCHEMA) {
        setStatus(`Unsupported project version ${doc.schemaVersion}`);
        return;
      }
      commit(sliders);
      setSliders({ ...NEUTRAL, ...doc.sliders });
      if (typeof doc.source === "string" && doc.source) setPath(doc.source);
      setPreset("Custom");
      setStatus(`Project loaded from ${picked}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  const loadPreset = async () => {
    const picked = await open({ multiple: false, filters: [{ name: "Preset", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    try {
      const text = await invoke<string>("read_text_file", { path: picked });
      const loaded = { ...NEUTRAL, ...JSON.parse(text) } as Sliders;
      commit(sliders);
      setSliders(loaded);
      setPreset("Custom");
      setStatus(`Preset loaded from ${picked}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  const resetSection = (keys: (keyof Sliders)[]) => {
    commit(sliders);
    setPreset("Custom");
    setSliders((s) => {
      const next = { ...s };
      for (const k of keys) (next as Record<string, unknown>)[k] = NEUTRAL[k];
      return next;
    });
  };

  const openImage = async () => {
    const picked = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg"] }] });
    if (typeof picked === "string") {
      setPath(picked);
      setStatus(`Opened ${picked}`);
    }
  };

  const chooseOutput = async () => {
    const picked = await save({ defaultPath: "edited.png", filters: [{ name: "PNG", extensions: ["png"] }, { name: "JPEG", extensions: ["jpg", "jpeg"] }] });
    if (typeof picked === "string") setOutPath(picked);
  };

  const exportImage = () => {
    setStatus("Exporting...");
    invoke("export_image", { path, outPath, sliders })
      .then(() => setStatus(`Exported to ${outPath}`))
      .catch((e) => setStatus(String(e)));
  };

  return (
    <div className="app">
      <aside className="left">
        <h2>Presets</h2>
        <input className="search" placeholder="Search presets" value={presetQuery} onChange={(e) => setPresetQuery(e.target.value)} />
        <button onClick={savePreset}>Save preset...</button>
        <button onClick={loadPreset}>Load preset...</button>
        <button onClick={saveProject}>Save project...</button>
        <button onClick={loadProject}>Open project...</button>
        <h2>Snapshots</h2>
        <button onClick={saveSnapshot}>Take snapshot</button>
        {snapshots.map((snap, i) => (
          <button key={i} onClick={() => recallSnapshot(snap.state)}>{snap.name}</button>
        ))}
        <h2>Presets</h2>
        {Object.keys(PRESETS).filter((n) => n.toLowerCase().includes(presetQuery.toLowerCase())).map((name) => (
          <button key={name} className={preset === name ? "sel" : ""} onClick={() => choosePreset(name)}>
            {name}
          </button>
        ))}
        <h2>Factory</h2>
        {FACTORY_PRESETS.map((p, i) => p.name.toLowerCase().includes(presetQuery.toLowerCase()) && (
          <button key={p.name} title={p.description} className={preset === p.name ? "sel" : ""} onClick={() => chooseFactory(i)}>
            {p.name}<em className="cat">{p.category}</em>
          </button>
        ))}
      </aside>

      <main className="center">
        <div className="toolbar">
          <button onClick={openImage}>Open image...</button>
          <label className="small"><input type="checkbox" checked={sliders.limestone}
            onChange={(e) => update("limestone", e.target.checked)} /> Limestone model</label>
          <input placeholder="Image path, or drop an image here" value={path} onChange={(e) => setPath(e.target.value)} />
          <button onMouseDown={() => setShowBefore(true)} onMouseUp={() => setShowBefore(false)}
            onMouseLeave={() => setShowBefore(false)}>Hold: before</button>
        </div>
        <div className="toolbar">
          <button onClick={undo} disabled={past.length === 0}>Undo</button>
          <button onClick={redo} disabled={future.length === 0}>Redo</button>
          <span className="spacer" />
          <button className={paint ? "sel" : ""} onClick={() => setPaint((p) => !p)}>Paint</button>
          <select value={brushKind} onChange={(e) => setBrushKind(Number(e.target.value))}>
            <option value={0}>Dodge</option><option value={1}>Burn</option><option value={2}>Smooth</option><option value={3}>Brush Out</option>
          </select>
          <label className="small">Size <input type="range" min={0.01} max={0.3} step={0.005} value={brushRadius} onChange={(e) => setBrushRadius(Number(e.target.value))} /></label>
          <label className="small">Hard <input type="range" min={0} max={1} step={0.01} value={brushHardness} onChange={(e) => setBrushHardness(Number(e.target.value))} /></label>
          <label className="small">Opacity <input type="range" min={0} max={1} step={0.01} value={brushOpacity} onChange={(e) => setBrushOpacity(Number(e.target.value))} /></label>
          <button onClick={() => { commit(sliders); setSliders((s) => ({ ...s, brush: [] })); }} disabled={sliders.brush.length === 0}>Clear paint ({sliders.brush.length})</button>
          <button className={zoom === "fit" ? "sel" : ""} onClick={() => setZoom("fit")}>Fit</button>
          <button className={zoom === 1 ? "sel" : ""} onClick={() => setZoom(1)}>100%</button>
          <button className={zoom === 2 ? "sel" : ""} onClick={() => setZoom(2)}>200%</button>
        </div>
        <div className="toolbar">
          <button className={view === "single" ? "sel" : ""} onClick={() => setView("single")}>Single</button>
          <button className={view === "split" ? "sel" : ""} onClick={() => setView("split")}>Split Wipe</button>
          <button className={view === "side" ? "sel" : ""} onClick={() => setView("side")}>Side-by-Side</button>
        </div>
        {view === "split" && preview && before ? (
          <div className="viewport split">
            <img src={before} alt="before" className="layer" />
            <img src={preview} alt="after" className="layer" style={{ clipPath: `inset(0 ${(1 - wipe) * 100}% 0 0)` }} />
            <input className="wipe" type="range" min={0} max={1} step={0.01} value={wipe}
              onChange={(e) => setWipe(Number(e.target.value))} aria-label="split position" />
          </div>
        ) : view === "side" && preview && before ? (
          <div className="viewport side">
            <img src={before} alt="before" />
            <img src={preview} alt="after" />
          </div>
        ) : (
        <div className={`viewport zoom-${zoom}`}>
          {preview ? (
            <img src={preview} alt="preview" className={paint ? "painting" : ""}
              style={zoom === "fit" ? undefined : { width: `${zoom * 100}%`, maxWidth: "none" }}
              onPointerDown={(e) => { if (paint) paintAt(e); }}
              onPointerMove={(e) => { if (paint && e.buttons === 1) paintAt(e); }} />
          ) : <p>No preview</p>}
        </div>
        )}
        <Histogram bins={histogram} />
        <div className="toolbar">
          <button onClick={chooseOutput}>Choose...</button>
          <input placeholder="Output path, e.g. /Users/you/out.png" value={outPath} onChange={(e) => setOutPath(e.target.value)} />
          <button disabled={!path || !outPath} onClick={exportImage}>Export</button>
        </div>
        <p className="status">{status}{renderMs !== null ? `  |  preview render ${renderMs} ms` : ""}</p>
      </main>

      <aside className="right">
        {SECTIONS.map((sec) => {
          const keys = sec.sliders.map((s) => s.key);
          const isOpen = !!open_[sec.id];
          return (
            <section key={sec.id} className="section">
              <header onClick={() => setOpen((o) => ({ ...o, [sec.id]: !isOpen }))}>
                <span>{isOpen ? "▾" : "▸"} {sec.title}</span>
                <button onClick={(e) => { e.stopPropagation(); resetSection(keys); }}>Reset</button>
              </header>
              {isOpen && sec.sliders.map((s) => (
                <label key={s.key} className="slider">
                  <span>{s.label}<em>{Number(sliders[s.key]).toFixed(2)}</em></span>
                  <input type="range" min={s.min} max={s.max} step={s.step} value={Number(sliders[s.key])}
                    onChange={(e) => update(s.key, Number(e.target.value) as never)} />
                </label>
              ))}
              {isOpen && sec.id === "finish" && (
                <div className="colors">
                  <label>Border colour <input type="color" value={linearToHex(sliders.borderColor)}
                    onChange={(e) => update("borderColor", hexToLinear(e.target.value))} /></label>
                  {sliders.toneColors.map((c, i) => (
                    <label key={i}>Tone {i + 1} <input type="color" value={linearToHex(c)}
                      onChange={(e) => {
                        const next = sliders.toneColors.map((x, j) => (j === i ? hexToLinear(e.target.value) : x)) as Sliders["toneColors"];
                        update("toneColors", next);
                      }} /></label>
                  ))}
                </div>
              )}
              {isOpen && sec.id === "curve" && (
                <CurveEditor values={[sliders.curveShadows, sliders.curveMids, sliders.curveHighlights]}
                  onChange={([a, b, c]) => { commit(sliders); setPreset("Custom"); setSliders((s) => ({ ...s, curveShadows: a, curveMids: b, curveHighlights: c })); }} />
              )}
              {isOpen && sec.id === "local" && (
                <label className="check"><input type="checkbox" checked={sliders.strokeEnabled}
                  onChange={(e) => update("strokeEnabled", e.target.checked)} /> Apply stroke</label>
              )}
              {isOpen && sec.id === "details" && (
                <label className="check"><input type="checkbox" checked={sliders.processIndependently}
                  onChange={(e) => update("processIndependently", e.target.checked)} /> Process Details Independently</label>
              )}
            </section>
          );
        })}
      </aside>
    </div>
  );
}
