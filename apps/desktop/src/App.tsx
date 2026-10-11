import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { NEUTRAL, SECTIONS, Sliders, hexToLinear, linearToHex } from "./controls";
import { LS_DEFAULT, LS_SECTIONS, LsParams, getPath, setPath as setLsPath } from "./quartz";
import { CurvePanel } from "./CurvePanel";
import { HueWheel } from "./HueWheel";
import fullLogo from "./assets/citrine-full-white.svg";
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
  const mode = "quartz" as const; // Quartz is the only engine in the UI
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
  const [zoom, setZoom] = useState<"fit" | number>("fit");
  // Real zoom: a full-resolution crop around `centre` (image pixels), panned by dragging.
  const [imageSize, setImageSize] = useState<[number, number] | null>(null);
  const [centre, setCentre] = useState<[number, number] | null>(null);
  const [cropUrl, setCropUrl] = useState<string | null>(null);
  const [cropDims, setCropDims] = useState<[number, number]>([0, 0]);
  const [cropCentre, setCropCentre] = useState<[number, number] | null>(null);
  const viewRef = useRef<HTMLDivElement>(null);
  // Split view: the image box is the largest rectangle with the photo's aspect ratio inside the view.
  const [splitEl, setSplitEl] = useState<HTMLDivElement | null>(null);
  const [splitSize, setSplitSize] = useState<[number, number]>([0, 0]);
  const [splitAspect, setSplitAspect] = useState<number | null>(null);
  useEffect(() => {
    if (!splitEl) return;
    const ro = new ResizeObserver(() => setSplitSize([splitEl.clientWidth, splitEl.clientHeight]));
    ro.observe(splitEl);
    setSplitSize([splitEl.clientWidth, splitEl.clientHeight]);
    return () => ro.disconnect();
  }, [splitEl]);
  const drag = useRef<{ x: number; y: number } | null>(null);
  const [hist, setHist] = useState<Hist | null>(null);
  const [channel, setChannel] = useState<"rgb" | "l" | "r" | "g" | "b">("rgb");
  const [past, setPast] = useState<State[]>([]);
  const [future, setFuture] = useState<State[]>([]);
  const [openSec, setOpenSec] = useState<Record<string, boolean>>({ basic_exposure: true, adaptive_exposure: true, curve: true });
  const [view, setView] = useState<"single" | "split" | "side">("single");
  const [wipe, setWipe] = useState(0.5);
  const [renderMs, setRenderMs] = useState<number | null>(null);
  const [presetQuery, setPresetQuery] = useState("");
  const lastEdit = useRef(0);

  const current: State = { sliders, ls };
  const apply = (s: State) => {
    setSliders(s.sliders);
    setLs(s.ls);
  };

  // Load Quartz's exact factory presets from the backend.
  useEffect(() => {
    invoke<LsPreset[]>("quartz_presets").then(setLsPresets).catch((e) => setStatus(String(e)));
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

  // Hand-off files sit beside the plug-in's settings file (<name>.settings.json), which holds the
  // settings Photoshop last used. Without one (first apply) the defaults are used.
  const handoffBase = (p: string) => p.replace(/\.ctrn$/i, "");
  const openPath = (p: string) => {
    setPath(p);
    setStatus(`Opened ${p}`);
    if (!p.toLowerCase().endsWith(".ctrn")) return;
    invoke<string>("read_text_file", { path: handoffBase(p) + ".settings.json" })
      .then((text) => setLs({ ...LS_DEFAULT, ...JSON.parse(text) }))
      .catch(() => setLs(LS_DEFAULT));
  };

  // A file opened from Photoshop or Finder loads here. One opened at launch is picked up from the
  // backend, because the open event can fire before this listener exists.
  useEffect(() => {
    invoke<string | null>("take_pending_open").then((p) => { if (p) openPath(p); });
  }, []);
  useEffect(() => {
    const unlisten = listen<string>("open-file", (e) => openPath(e.payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // OK for a Photoshop hand-off: the settings go back to the plug-in, which renders the layer itself.
  const sendToPhotoshop = () => {
    setStatus("Sending to Photoshop...");
    invoke("write_text_file", { path: handoffBase(path) + ".result.json", contents: JSON.stringify(ls, null, 2) })
      .then(() => setStatus("Sent to Photoshop"))
      .catch((e) => setStatus(String(e)));
  };

  // Cancel for a Photoshop hand-off: Photoshop applies nothing.
  const cancelHandoff = () => {
    invoke("write_text_file", { path: handoffBase(path) + ".cancel", contents: "" })
      .then(() => setStatus("Cancelled: Photoshop applies nothing"))
      .catch((e) => setStatus(String(e)));
  };

  // Drag and drop an image onto the window.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop" && e.payload.paths.length > 0) openPath(e.payload.paths[0]);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const shownLs = useMemo(() => (showBefore ? LS_DEFAULT : ls), [showBefore, ls]);

  // Image size when the path changes; the crop centre starts in the middle.
  useEffect(() => {
    if (!path) return;
    invoke<[number, number]>("image_size", { path })
      .then(([w, h]) => { setImageSize([w, h]); setCentre([w / 2, h / 2]); })
      .catch((e) => setStatus(String(e)));
  }, [path]);

  // Full-resolution crop for the zoom views, debounced; re-rendered when sliders or centre change.
  useEffect(() => {
    if (zoom === "fit" || !path || !imageSize || !centre) return;
    const id = window.setTimeout(() => {
      const vw = viewRef.current?.clientWidth ?? 900;
      const vh = viewRef.current?.clientHeight ?? 600;
      const w = Math.min(imageSize[0], Math.ceil(vw / zoom));
      const h = Math.min(imageSize[1], Math.ceil(vh / zoom));
      const x = Math.max(0, Math.min(imageSize[0] - w, Math.round(centre[0] - w / 2)));
      const y = Math.max(0, Math.min(imageSize[1] - h, Math.round(centre[1] - h / 2)));
      const asked: [number, number] = [centre[0], centre[1]];
      invoke<string>("render_quartz_crop", { path, params: shownLs, x, y, w, h })
        .then((b64) => { setCropUrl(`data:image/png;base64,${b64}`); setCropDims([w, h]); setCropCentre(asked); })
        .catch((e) => setStatus(String(e)));
    }, 250);
    return () => window.clearTimeout(id);
  }, [zoom, path, imageSize, centre, shownLs]);

  // Live preview. While a slider moves, a 480 px draft is shown at once; when it settles (250 ms)
  // the full 960 px preview replaces it. Only the newest request may update the image, so a late
  // reply never overwrites a newer one.
  const previewReq = useRef(0);
  useEffect(() => {
    if (!path) return;
    const show = (side: number) => {
      const id = ++previewReq.current;
      const t0 = performance.now();
      return invoke<string>("render_quartz_preview", { path, params: shownLs, side })
        .then((b64) => {
          if (id !== previewReq.current) return;
          setPreview(`data:image/png;base64,${b64}`);
          setRenderMs(Math.round(performance.now() - t0));
          setStatus("Preview ready.");
        })
        .catch((e) => setStatus(String(e)));
    };
    show(480);
    const settle = window.setTimeout(() => show(960), 250);
    return () => window.clearTimeout(settle);
  }, [path, shownLs]);

  // Unadjusted preview for the split and side-by-side views.
  useEffect(() => {
    if (!path || view === "single") return;
    const id = window.setTimeout(() => {
      const req = invoke<string>("render_quartz_preview", { path, params: LS_DEFAULT });
      req.then((b64) => setBefore(`data:image/png;base64,${b64}`)).catch(() => setBefore(null));
    }, 150);
    return () => window.clearTimeout(id);
  }, [path, view]);

  // Histogram from the preview pixels.
  useEffect(() => {
    if (!preview) return;
    const img = new Image();
    img.onload = () => setHist(computeHist(img));
    img.src = preview;
  }, [preview]);

  const chooseQuartz = (p: LsPreset) => {
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
    setLs((s) => {
      const next = structuredClone(s) as Record<string, unknown>;
      for (const k of keys) next[k] = structuredClone(LS_DEFAULT[k as keyof LsParams]);
      return next as LsParams;
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

  // Divider drag: moves the clip and the line directly in the DOM, one frame at a time, so the
  // app does not re-render on every pointer move. The final position is committed on release.
  const afterRef = useRef<HTMLImageElement>(null);
  const lineRef = useRef<HTMLDivElement>(null);
  const wipeFrac = useRef<number | null>(null);
  const wipeTo = (e: React.PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.querySelector(".compare")?.getBoundingClientRect();
    if (!box) return;
    const f = Math.max(0, Math.min(1, (e.clientX - box.left) / box.width));
    wipeFrac.current = f;
    requestAnimationFrame(() => {
      if (afterRef.current) afterRef.current.style.clipPath = `inset(0 0 0 ${f * 100}%)`;
      if (lineRef.current) lineRef.current.style.left = `${f * 100}%`;
    });
  };

  // Wheel zoom around the cursor: the image point under the pointer stays under it.
  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    if (!imageSize || !centre) return;
    // Two-finger drag (no ctrl): pan, the content follows the fingers.
    if (!e.ctrlKey) {
      if (zoom === "fit") return;
      e.preventDefault();
      const nx = Math.max(0, Math.min(imageSize[0], centre[0] + e.deltaX / zoom));
      const ny = Math.max(0, Math.min(imageSize[1], centre[1] + e.deltaY / zoom));
      setCentre([nx, ny]);
      return;
    }
    // Pinch (ctrl + wheel on macOS): zoom about the cursor; the point under it stays put.
    e.preventDefault();
    const img = e.currentTarget.querySelector("img")?.getBoundingClientRect();
    // Fit scale: the zoom at which the whole picture just fits the view. Zooming out past it returns to Fit.
    const vw = viewRef.current?.clientWidth ?? 900;
    const vh = viewRef.current?.clientHeight ?? 600;
    const fitScale = Math.min(vw / imageSize[0], vh / imageSize[1]);
    const cur = zoom === "fit" ? fitScale : zoom;
    const next = Math.min(8, Math.max(0.5, cur * Math.exp(-e.deltaY * 0.01)));
    if (next <= fitScale) {
      setZoom("fit");
      setCentre([imageSize[0] / 2, imageSize[1] / 2]);
      return;
    }
    const r = e.currentTarget.getBoundingClientRect();
    const scale = zoom === "fit" && img ? img.width / imageSize[0] : cur;
    const ox = zoom === "fit" && img ? (e.clientX - img.left) / scale : centre[0] + (e.clientX - r.left - r.width / 2) / cur;
    const oy = zoom === "fit" && img ? (e.clientY - img.top) / scale : centre[1] + (e.clientY - r.top - r.height / 2) / cur;
    const cx = ox - (e.clientX - r.left - r.width / 2) / next;
    const cy = oy - (e.clientY - r.top - r.height / 2) / next;
    setCentre([Math.max(0, Math.min(imageSize[0], cx)), Math.max(0, Math.min(imageSize[1], cy))]);
    setZoom(next);
  };

  const exportImage = () => {
    setStatus("Exporting...");
    const req = invoke("export_quartz", { path, outPath, params: ls });
    req.then(() => setStatus(`Exported to ${outPath}`)).catch((e) => setStatus(String(e)));
  };

  const savePreset = async () => {
    const picked = await save({ defaultPath: "preset.json", filters: [{ name: "Preset", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    const body = ls;
    invoke("write_text_file", { path: picked, contents: JSON.stringify(body, null, 2) })
      .then(() => setStatus(`Preset saved to ${picked}`))
      .catch((e) => setStatus(String(e)));
  };

  const saveProject = async () => {
    const picked = await save({ defaultPath: "session.photoproj.json", filters: [{ name: "Project", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    const doc = { schemaVersion: PROJECT_SCHEMA, app: "photo-editor", mode, source: path, quartz: ls };
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
      setLs({ ...LS_DEFAULT, ...doc.quartz });
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
        <div className="brand"><img src={fullLogo} alt="Citrine Labs" /></div>
        <input className="search" placeholder="Search presets" value={presetQuery} onChange={(e) => setPresetQuery(e.target.value)} />
        {groupedLs.map(([cat, list]) => (
          <div key={cat}>
            <h2>{cat}</h2>
            {list.map((p) => (
              <button key={p.name} title={p.description} className={preset === p.name ? "sel" : ""} onClick={() => chooseQuartz(p)}>{p.name}</button>
            ))}
          </div>
        ))}
        <h2>Files</h2>
        <button onClick={savePreset}>Save preset...</button>
        <button onClick={saveProject}>Save project...</button>
        <button onClick={loadProject}>Open project...</button>
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
          <span className="spacer" />
          <button className={view === "single" ? "sel" : ""} onClick={() => setView("single")}>Single</button>
          <button className={view === "split" ? "sel" : ""} onClick={() => setView("split")}>Split Wipe</button>
          <button className={view === "side" ? "sel" : ""} onClick={() => setView("side")}>Side-by-Side</button>
          <button className={zoom === "fit" ? "sel" : ""} onClick={() => setZoom("fit")}>Fit</button>
          <button className={zoom === 1 ? "sel" : ""} onClick={() => setZoom(1)}>100%</button>
          <button className={zoom === 2 ? "sel" : ""} onClick={() => setZoom(2)}>200%</button>
        </div>
        {view === "split" && preview && before ? (
          <div className="viewport split" ref={setSplitEl}
            onPointerDown={(e) => { drag.current = { x: e.clientX, y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); wipeTo(e); }}
            onPointerMove={(e) => { if (drag.current) wipeTo(e); }}
            onPointerUp={() => { drag.current = null; if (wipeFrac.current !== null) setWipe(wipeFrac.current); }}>
            <div className="compare" style={(() => {
              const aspect = splitAspect ?? 1;
              let bw = splitSize[0], bh = bw / aspect;
              if (bh > splitSize[1]) { bh = splitSize[1]; bw = bh * aspect; }
              return { width: `${bw}px`, height: `${bh}px` };
            })()}>
              <img src={before} alt="before" onLoad={(e) => setSplitAspect(e.currentTarget.naturalWidth / e.currentTarget.naturalHeight)} />
              <img ref={afterRef} src={preview} alt="after" style={{ clipPath: `inset(0 0 0 ${wipe * 100}%)` }} />
              <div ref={lineRef} className="wipe-line" style={{ left: `${wipe * 100}%` }}><span>‹ ›</span></div>
              <span className="tag l">Before</span>
              <span className="tag r">After</span>
            </div>
          </div>
        ) : view === "side" && preview && before ? (
          <div className="viewport side">
            <img src={before} alt="before" />
            <img src={preview} alt="after" />
          </div>
        ) : zoom !== "fit" ? (
          <div className="viewport crop" ref={viewRef} onWheel={onWheel}
            onPointerDown={(e) => { drag.current = { x: e.clientX, y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
            onPointerMove={(e) => {
              if (!drag.current || !centre || !imageSize) return;
              const dx = (e.clientX - drag.current.x) / zoom;
              const dy = (e.clientY - drag.current.y) / zoom;
              drag.current = { x: e.clientX, y: e.clientY };
              setCentre([Math.max(0, Math.min(imageSize[0], centre[0] - dx)), Math.max(0, Math.min(imageSize[1], centre[1] - dy))]);
            }}
            onPointerUp={() => { drag.current = null; }}>
            {cropUrl ? (
              <img src={cropUrl} alt="1:1 crop" draggable={false}
                style={{
                  width: `${cropDims[0] * zoom}px`, height: `${cropDims[1] * zoom}px`,
                  // Move with the pointer right away; the sharp crop replaces this when it arrives.
                  transform: cropCentre && centre
                    ? `translate(${-(centre[0] - cropCentre[0]) * zoom}px, ${-(centre[1] - cropCentre[1]) * zoom}px)`
                    : undefined,
                }} />
            ) : <p>Rendering crop...</p>}
            {imageSize && centre ? (
              <span className="crop-info">{zoom * 100}% · centre {Math.round(centre[0])}, {Math.round(centre[1])} of {imageSize[0]}×{imageSize[1]} · drag to pan</span>
            ) : null}
          </div>
        ) : (
          <div className={`viewport zoom-${zoom}`} ref={viewRef} onWheel={onWheel}>
            {preview ? (
              <img src={preview} alt="preview"
                style={zoom === "fit" ? undefined : { width: `${zoom * 100}%`, maxWidth: "none" }} />
            ) : <p>No preview</p>}
          </div>
        )}
        <Histogram hist={hist} channel={channel} onChannel={setChannel} />
        <div className="toolbar">
          <button onClick={chooseOutput}>Choose...</button>
          <input placeholder="Output path, e.g. /Users/you/out.png" value={outPath} onChange={(e) => setOutPath(e.target.value)} />
          {path.toLowerCase().endsWith(".ctrn") ? (
            <>
              <button onClick={cancelHandoff}>Cancel</button>
              <button onClick={sendToPhotoshop}>OK: send to Photoshop</button>
            </>
          ) : (
            <button disabled={!path || !outPath} onClick={exportImage}>Export</button>
          )}
        </div>
        <p className="status">{status}{renderMs !== null ? `  |  preview render ${renderMs} ms` : ""}</p>
      </main>

      <aside className="right">
        {mode === "quartz" ? (
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
                  {isOpen && sec.id === "color" && (
                    <HueWheel value={ls.color.hue} onChange={(v) => updateLs("color", "hue", v)} />
                  )}
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
