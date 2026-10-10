import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { NEUTRAL, PRESETS, SECTIONS, Sliders, Dab, hexToLinear, linearToHex } from "./controls";
import { FACTORY_PRESETS } from "./presets";
import { LS_DEFAULT, LS_SECTIONS, LsParams, getPath, setPath as setLsPath } from "./limestone";
import { CurvePanel } from "./CurvePanel";
import { Histogram, Hist } from "./Histogram";
import { Pt } from "./curveMath";
import "./App.css";

type LsPreset = { name: string; category: string; description: string; params: LsParams };
type State = { sliders: Sliders; ls: LsParams };

const PROJECT_SCHEMA = 2;

function computeHist(img: HTMLImageElement): Hist {
  const c = document.createElement("canvas");
  c.width = img.width;
  c.height = img.height;
  const ctx = c.getContext("2d");
  const h: Hist = { r: new Array(256).fill(0), g: new Array(256).fill(0), b: new Array(256).fill(0), l: new Array(256).fill(0) };
  if (!ctx) return h;
  ctx.drawImage(img, 0, 0);
  const px = ctx.getImageData(0, 0, c.width, c.height).data;
  for (let i = 0; i < px.length; i += 4) {
    h.r[px[i]]++;
    h.g[px[i + 1]]++;
    h.b[px[i + 2]]++;
    const y = Math.round(0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2]);
    h.l[Math.min(255, Math.max(0, y))]++;
  }
  return h;
}

export default function App() {
  const [mode, setMode] = useState<"ours" | "limestone">("limestone");
  const [path, setPath] = useState("");
  const [outPath, setOutPath] = useState("");
  const [sliders, setSliders] = useState<Sliders>(NEUTRAL);
  const [ls, setLs] = useState<LsParams>(LS_DEFAULT);
  const [preset, setPreset] = useState("Neutral");
  const [lsPresets, setLsPresets] = useState<LsPreset[]>([]);
  const [showBefore, setShowBefore] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const [before, setBefore] = useState<string | null>(null);
  const [status, setStatus] = useState("Open an image to start.");
  const [zoom, setZoom] = useState<"fit" | 1 | 2>("fit");
  const [hist, setHist] = useState<Hist | null>(null);
  const [channel, setChannel] = useState<"rgb" | "l" | "r" | "g" | "b">("rgb");
  const [past, setPast] = useState<State[]>([]);
  const [future, setFuture] = useState<State[]>([]);
  const [openSec, setOpenSec] = useState<Record<string, boolean>>({ basic_exposure: true, adaptive_exposure: true, curve: true });
  const [view, setView] = useState<"single" | "split" | "side">("single");
  const [wipe, setWipe] = useState(0.5);
  const [paint, setPaint] = useState(false);
  const [brushKind, setBrushKind] = useState(0);
  const [brushRadius, setBrushRadius] = useState(0.05);
  const [brushHardness, setBrushHardness] = useState(0.5);
  const [brushOpacity, setBrushOpacity] = useState(0.5);
  const [renderMs, setRenderMs] = useState<number | null>(null);
  const [presetQuery, setPresetQuery] = useState("");
  const lastEdit = useRef(0);

  const current: State = { sliders, ls };
  const apply = (s: State) => {
    setSliders(s.sliders);
    setLs(s.ls);
  };

  // Load Limestone's exact factory presets from the backend.
  useEffect(() => {
    invoke<LsPreset[]>("limestone_presets").then(setLsPresets).catch((e) => setStatus(String(e)));
  }, []);

  // Undo/redo over both models. Edits within 400 ms collapse into one step.
  const commit = () => {
    const now = Date.now();
    if (now - lastEdit.current > 400) {
      setPast((p) => [...p, current]);
      setFuture([]);
    }
    lastEdit.current = now;
  };
  const undo = () => {
    if (past.length === 0) return;
    setFuture((f) => [current, ...f]);
    apply(past[past.length - 1]);
    setPast((p) => p.slice(0, -1));
    lastEdit.current = 0;
  };
  const redo = () => {
    if (future.length === 0) return;
    setPast((p) => [...p, current]);
    apply(future[0]);
    setFuture((f) => f.slice(1));
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

  const shownSliders = useMemo(() => (showBefore ? NEUTRAL : sliders), [showBefore, sliders]);
  const shownLs = useMemo(() => (showBefore ? LS_DEFAULT : ls), [showBefore, ls]);

  // Live preview, debounced. Renders the active model.
  useEffect(() => {
    if (!path) return;
    const id = window.setTimeout(() => {
      const t0 = performance.now();
      const req = mode === "ours"
        ? invoke<string>("render_preview", { path, sliders: shownSliders })
        : invoke<string>("render_limestone_preview", { path, params: shownLs });
      req
        .then((b64) => {
          setPreview(`data:image/png;base64,${b64}`);
          setRenderMs(Math.round(performance.now() - t0));
          setStatus("Preview ready.");
        })
        .catch((e) => setStatus(String(e)));
    }, 150);
    return () => window.clearTimeout(id);
  }, [path, mode, shownSliders, shownLs]);

  // Unadjusted preview for the split and side-by-side views.
  useEffect(() => {
    if (!path || view === "single") return;
    const id = window.setTimeout(() => {
      const req = mode === "ours"
        ? invoke<string>("render_preview", { path, sliders: NEUTRAL })
        : invoke<string>("render_limestone_preview", { path, params: LS_DEFAULT });
      req.then((b64) => setBefore(`data:image/png;base64,${b64}`)).catch(() => setBefore(null));
    }, 150);
    return () => window.clearTimeout(id);
  }, [path, view, mode]);

  // Histogram from the preview pixels.
  useEffect(() => {
    if (!preview) return;
    const img = new Image();
    img.onload = () => setHist(computeHist(img));
    img.src = preview;
  }, [preview]);

  const choosePreset = (name: string) => {
    commit();
    setPreset(name);
    setSliders({ ...NEUTRAL, ...PRESETS[name] });
  };

  const chooseFactory = (idx: number) => {
    const p = FACTORY_PRESETS[idx];
    commit();
    setPreset(p.name);
    setSliders({ ...NEUTRAL, ...p.values, limestone: false, brush: [] });
  };

  const chooseLimestone = (p: LsPreset) => {
    commit();
    setPreset(p.name);
    setLs(p.params);
  };

  const updateOurs = <K extends keyof Sliders>(key: K, value: Sliders[K]) => {
    commit();
    setPreset("Custom");
    setSliders((s) => ({ ...s, [key]: value }));
  };

  const updateLs = (sectionId: string, path: string, value: number | boolean) => {
    commit();
    setPreset("Custom");
    setLs((s) => setLsPath(s, sectionId, path, value as number));
  };

  const setLsCurve = (points: Pt[]) => {
    commit();
    setPreset("Custom");
    setLs((s) => ({ ...s, curve: { enabled: points.length !== 2 || points[0].y !== 0 || points[1].y !== 1, points } }));
  };

  const resetSection = (keys: string[]) => {
    commit();
    setPreset("Custom");
    if (mode === "ours") {
      setSliders((s) => {
        const next = { ...s } as Record<string, unknown>;
        for (const k of keys) next[k] = NEUTRAL[k as keyof Sliders];
        return next as Sliders;
      });
    } else {
      setLs((s) => {
        const next = structuredClone(s) as Record<string, unknown>;
        for (const k of keys) next[k] = structuredClone(LS_DEFAULT[k as keyof LsParams]);
        return next as LsParams;
      });
    }
  };

  const paintAt = (e: React.PointerEvent<HTMLImageElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const dab: Dab = {
      kind: brushKind, x: (e.clientX - r.left) / r.width, y: (e.clientY - r.top) / r.height,
      radius: brushRadius, hardness: brushHardness, opacity: brushOpacity, strength: 1,
    };
    commit();
    setPreset("Custom");
    setSliders((s) => ({ ...s, brush: [...s.brush, dab] }));
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
    const req = mode === "ours"
      ? invoke("export_image", { path, outPath, sliders })
      : invoke("export_limestone", { path, outPath, params: ls });
    req.then(() => setStatus(`Exported to ${outPath}`)).catch((e) => setStatus(String(e)));
  };

  const savePreset = async () => {
    const picked = await save({ defaultPath: "preset.json", filters: [{ name: "Preset", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    const body = mode === "ours" ? sliders : ls;
    invoke("write_text_file", { path: picked, contents: JSON.stringify(body, null, 2) })
      .then(() => setStatus(`Preset saved to ${picked}`))
      .catch((e) => setStatus(String(e)));
  };

  const saveProject = async () => {
    const picked = await save({ defaultPath: "session.photoproj.json", filters: [{ name: "Project", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    const doc = { schemaVersion: PROJECT_SCHEMA, app: "photo-editor", mode, source: path, sliders, limestone: ls };
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
      commit();
      setSliders({ ...NEUTRAL, ...doc.sliders });
      setLs({ ...LS_DEFAULT, ...doc.limestone });
      setMode(doc.mode === "ours" ? "ours" : "limestone");
      if (typeof doc.source === "string" && doc.source) setPath(doc.source);
      setStatus(`Project loaded from ${picked}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  const groupedLs = useMemo(() => {
    const q = presetQuery.toLowerCase();
    const map = new Map<string, LsPreset[]>();
    for (const p of lsPresets) {
      if (!p.name.toLowerCase().includes(q)) continue;
      map.set(p.category, [...(map.get(p.category) ?? []), p]);
    }
    return [...map.entries()];
  }, [lsPresets, presetQuery]);

  const toggleSec = (id: string) => setOpenSec((o) => ({ ...o, [id]: !o[id] }));

  return (
    <div className="app">
      <aside className="left">
        <div className="modes">
          <button className={mode === "limestone" ? "sel" : ""} onClick={() => setMode("limestone")}>Limestone</button>
          <button className={mode === "ours" ? "sel" : ""} onClick={() => setMode("ours")}>Ours</button>
        </div>
        <input className="search" placeholder="Search presets" value={presetQuery} onChange={(e) => setPresetQuery(e.target.value)} />
        {mode === "limestone" ? (
          groupedLs.map(([cat, list]) => (
            <div key={cat}>
              <h2>{cat}</h2>
              {list.map((p) => (
                <button key={p.name} title={p.description} className={preset === p.name ? "sel" : ""} onClick={() => chooseLimestone(p)}>{p.name}</button>
              ))}
            </div>
          ))
        ) : (
          <>
            <h2>Presets</h2>
            {Object.keys(PRESETS).filter((n) => n.toLowerCase().includes(presetQuery.toLowerCase())).map((name) => (
              <button key={name} className={preset === name ? "sel" : ""} onClick={() => choosePreset(name)}>{name}</button>
            ))}
            <h2>Factory</h2>
            {FACTORY_PRESETS.map((p, i) => p.name.toLowerCase().includes(presetQuery.toLowerCase()) && (
              <button key={p.name} title={p.description} className={preset === p.name ? "sel" : ""} onClick={() => chooseFactory(i)}>{p.name}</button>
            ))}
          </>
        )}
        <h2>Files</h2>
        <button onClick={savePreset}>Save preset...</button>
        <button onClick={saveProject}>Save project...</button>
        <button onClick={loadProject}>Open project...</button>
        {mode === "ours" && (
          <>
            <h2>Snapshots</h2>
            <button onClick={() => setPast((p) => p)} disabled>Coming with history</button>
          </>
        )}
      </aside>

      <main className="center">
        <div className="toolbar">
          <button onClick={openImage}>Open image...</button>
          <input placeholder="Image path, or drop an image here" value={path} onChange={(e) => setPath(e.target.value)} />
          <button onMouseDown={() => setShowBefore(true)} onMouseUp={() => setShowBefore(false)} onMouseLeave={() => setShowBefore(false)}>Hold: before</button>
        </div>
        <div className="toolbar">
          <button onClick={undo} disabled={past.length === 0}>Undo</button>
          <button onClick={redo} disabled={future.length === 0}>Redo</button>
          {mode === "ours" && (
            <>
              <button className={paint ? "sel" : ""} onClick={() => setPaint((p) => !p)}>Paint</button>
              <select value={brushKind} onChange={(e) => setBrushKind(Number(e.target.value))}>
                <option value={0}>Dodge</option><option value={1}>Burn</option><option value={2}>Smooth</option><option value={3}>Brush Out</option>
              </select>
              <label className="small">Size <input type="range" min={0.01} max={0.3} step={0.005} value={brushRadius} onChange={(e) => setBrushRadius(Number(e.target.value))} /></label>
              <label className="small">Hard <input type="range" min={0} max={1} step={0.01} value={brushHardness} onChange={(e) => setBrushHardness(Number(e.target.value))} /></label>
              <label className="small">Opacity <input type="range" min={0} max={1} step={0.01} value={brushOpacity} onChange={(e) => setBrushOpacity(Number(e.target.value))} /></label>
              <button onClick={() => { commit(); setSliders((s) => ({ ...s, brush: [] })); }} disabled={sliders.brush.length === 0}>Clear paint ({sliders.brush.length})</button>
            </>
          )}
          <span className="spacer" />
          <button className={view === "single" ? "sel" : ""} onClick={() => setView("single")}>Single</button>
          <button className={view === "split" ? "sel" : ""} onClick={() => setView("split")}>Split Wipe</button>
          <button className={view === "side" ? "sel" : ""} onClick={() => setView("side")}>Side-by-Side</button>
          <button className={zoom === "fit" ? "sel" : ""} onClick={() => setZoom("fit")}>Fit</button>
          <button className={zoom === 1 ? "sel" : ""} onClick={() => setZoom(1)}>100%</button>
          <button className={zoom === 2 ? "sel" : ""} onClick={() => setZoom(2)}>200%</button>
        </div>
        {view === "split" && preview && before ? (
          <div className="viewport split">
            <img src={before} alt="before" className="layer" />
            <img src={preview} alt="after" className="layer" style={{ clipPath: `inset(0 ${(1 - wipe) * 100}% 0 0)` }} />
            <input className="wipe" type="range" min={0} max={1} step={0.01} value={wipe} onChange={(e) => setWipe(Number(e.target.value))} aria-label="split position" />
          </div>
        ) : view === "side" && preview && before ? (
          <div className="viewport side">
            <img src={before} alt="before" />
            <img src={preview} alt="after" />
          </div>
        ) : (
          <div className={`viewport zoom-${zoom}`}>
            {preview ? (
              <img src={preview} alt="preview" className={paint && mode === "ours" ? "painting" : ""}
                style={zoom === "fit" ? undefined : { width: `${zoom * 100}%`, maxWidth: "none" }}
                onPointerDown={(e) => { if (paint && mode === "ours") paintAt(e); }}
                onPointerMove={(e) => { if (paint && mode === "ours" && e.buttons === 1) paintAt(e); }} />
            ) : <p>No preview</p>}
          </div>
        )}
        <Histogram hist={hist} channel={channel} onChannel={setChannel} />
        <div className="toolbar">
          <button onClick={chooseOutput}>Choose...</button>
          <input placeholder="Output path, e.g. /Users/you/out.png" value={outPath} onChange={(e) => setOutPath(e.target.value)} />
          <button disabled={!path || !outPath} onClick={exportImage}>Export</button>
        </div>
        <p className="status">{status}{renderMs !== null ? `  |  preview render ${renderMs} ms` : ""}</p>
      </main>

      <aside className="right">
        {mode === "limestone" ? (
          <>
            {LS_SECTIONS.map((sec) => {
              const isOpen = !!openSec[sec.id];
              const enabled = (ls[sec.id] as { enabled?: boolean }).enabled;
              return (
                <section key={sec.id} className="section">
                  <header onClick={() => toggleSec(sec.id)}>
                    <span>{isOpen ? "▾" : "▸"} {sec.title}</span>
                    <span>
                      <label className="small" onClick={(e) => e.stopPropagation()}>
                        <input type="checkbox" checked={!!enabled} onChange={(e) => { commit(); setLs((s) => ({ ...s, [sec.id]: { ...(s[sec.id] as object), enabled: e.target.checked } })); }} /> on
                      </label>
                      <button onClick={(e) => { e.stopPropagation(); resetSection([sec.id]); }}>Reset</button>
                    </span>
                  </header>
                  {isOpen && sec.fields.map((f) => (
                    <label key={f.path} className="slider">
                      <span>{f.label}<em>{Number(getPath(ls[sec.id], f.path)).toFixed(f.int ? 0 : 2)}</em></span>
                      <input type="range" min={f.min} max={f.max} step={f.step} value={Number(getPath(ls[sec.id], f.path))}
                        onChange={(e) => updateLs(sec.id, f.path, Number(e.target.value))} />
                    </label>
                  ))}
                  {isOpen && sec.id === "details" && (
                    <label className="check"><input type="checkbox" checked={ls.details.process_independently}
                      onChange={(e) => updateLs("details", "process_independently", e.target.checked)} /> Process Details Independently</label>
                  )}
                  {isOpen && sec.id === "finishing" && (
                    <div className="colors">
                      <label className="check"><input type="checkbox" checked={ls.finishing.border.enabled}
                        onChange={(e) => updateLs("finishing", "border.enabled", e.target.checked)} /> Border</label>
                      <label>Border colour <input type="color" value={linearToHex(ls.finishing.border.color)}
                        onChange={(e) => { commit(); setLs((s) => ({ ...s, finishing: { ...s.finishing, border: { ...s.finishing.border, color: hexToLinear(e.target.value) } } })); }} /></label>
                    </div>
                  )}
                </section>
              );
            })}
            <section className="section">
              <header onClick={() => toggleSec("curve")}>
                <span>{openSec.curve ? "▾" : "▸"} Curve</span>
                <button onClick={(e) => { e.stopPropagation(); setLsCurve([{ x: 0, y: 0 }, { x: 1, y: 1 }]); }}>Reset</button>
              </header>
              {openSec.curve && <CurvePanel points={ls.curve.points} histogram={hist ? hist.l : null} onChange={setLsCurve} />}
            </section>
          </>
        ) : (
          <>
            {SECTIONS.map((sec) => {
              const keys = sec.sliders.map((s) => s.key);
              const isOpen = !!openSec[sec.id];
              return (
                <section key={sec.id} className="section">
                  <header onClick={() => toggleSec(sec.id)}>
                    <span>{isOpen ? "▾" : "▸"} {sec.title}</span>
                    <button onClick={(e) => { e.stopPropagation(); resetSection(keys as string[]); }}>Reset</button>
                  </header>
                  {isOpen && sec.sliders.map((s) => (
                    <label key={s.key} className="slider">
                      <span>{s.label}<em>{Number(sliders[s.key]).toFixed(2)}</em></span>
                      <input type="range" min={s.min} max={s.max} step={s.step} value={Number(sliders[s.key])}
                        onChange={(e) => updateOurs(s.key, Number(e.target.value) as never)} />
                    </label>
                  ))}
                  {isOpen && sec.id === "local" && (
                    <label className="check"><input type="checkbox" checked={sliders.strokeEnabled} onChange={(e) => updateOurs("strokeEnabled", e.target.checked)} /> Apply stroke</label>
                  )}
                </section>
              );
            })}
          </>
        )}
      </aside>
    </div>
  );
}
