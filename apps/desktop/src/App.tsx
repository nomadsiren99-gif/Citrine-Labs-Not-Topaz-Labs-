import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { NEUTRAL, SECTIONS, Sliders, hexToLinear, linearToHex } from "./controls";
import { LS_DEFAULT, LS_SECTIONS, LsParams, getPath, setPath as setLsPath } from "./quartz";
import { GpuView, pointwiseFrom } from "./gpuView";
import { GradingWheels } from "./GradingWheels";
import { CurvePanel, CurveChannel } from "./CurvePanel";
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

// The GPU view covers colour, tone, curve and finishing (warmth, vignette, grain, border). Transparency
// blends with the original image, which the GPU does not have, so the CPU view handles it.
function gpuFinishingOk(ls: LsParams): boolean {
  return ls.finishing.transparency === 0;
}

// Raw RGBA from the backend: width and height (u32, little-endian), then the pixels.
function readRaw(buf: ArrayBuffer): { w: number; h: number; px: Uint8Array } {
  const head = new DataView(buf);
  return { w: head.getUint32(0, true), h: head.getUint32(4, true), px: new Uint8Array(buf, 8) };
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
  const [cropRev, setCropRev] = useState(0); // bumps when a new crop or base arrives
  const [gpuBroken, setGpuBroken] = useState(false); // WebGL 2 unavailable: fall back to the CPU view
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
  // While panning, the crop moves by a direct DOM transform and the centre stays in a ref, so a drag
  // does not re-render the whole app on every pointer move. The centre is committed on release.
  const viewCanvas = useRef<HTMLCanvasElement | null>(null);
  const cropCanvas = useRef<HTMLCanvasElement | null>(null); // offscreen copy of the rendered crop
  const cropOrigin = useRef<[number, number, number, number] | null>(null); // x, y, w, h in image pixels
  const gpuCanvas = useRef<HTMLCanvasElement | null>(null);
  const gpuViewRef = useRef<{ view: GpuView; canvas: HTMLCanvasElement } | null>(null);
  const baseKind = useRef<"fit" | "crop" | null>(null); // what the GPU base holds
  const baseSpatial = useRef<string | null>(null); // spatial settings the GPU base was rendered with
  const baseReq = useRef(0);
  const lutRef = useRef<Float32Array | null>(null); // curve tables, kept for a new GPU view
  const panCentre = useRef<[number, number] | null>(null);
  const panFrame = useRef(0);
  const [hist, setHist] = useState<Hist | null>(null);
  const [channel, setChannel] = useState<"rgb" | "l" | "r" | "g" | "b">("rgb");
  const [past, setPast] = useState<State[]>([]);
  const [future, setFuture] = useState<State[]>([]);
  const [curveChannel, setCurveChannel] = useState<CurveChannel>("rgb");
  const [openSec, setOpenSec] = useState<Record<string, boolean>>({ basic_exposure: true, adaptive_exposure: true, curve: true });
  const [view, setView] = useState<"single" | "split" | "side">("single");
  const [wipe, setWipe] = useState(0.5);
  const [renderMs, setRenderMs] = useState<number | null>(null);
  const [presetQuery, setPresetQuery] = useState("");
  const [collection, setCollection] = useState<string>("");
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
  const gpuOk = !gpuBroken && !showBefore && view === "single" && gpuFinishingOk(ls);
  const pw = useMemo(() => pointwiseFrom(ls), [ls]);
  const spatialKey = useMemo(() => JSON.stringify([ls.basic_exposure, ls.adaptive_exposure, ls.details, ls.noise]), [ls]);

  // Image size when the path changes; the crop centre starts in the middle.
  useEffect(() => {
    if (!path) return;
    invoke<[number, number]>("image_size", { path })
      .then(([w, h]) => { setImageSize([w, h]); setCentre([w / 2, h / 2]); })
      .catch((e) => setStatus(String(e)));
  }, [path]);

  // Whether the rendered crop still covers the viewport on one axis (in image pixels). An edge of the
  // crop that is also an edge of the image needs no cover.
  const covers = (c: number, cropC: number, cropSize: number, view: number, imageLen: number) => {
    const lo = cropC - cropSize / 2;
    const hi = cropC + cropSize / 2;
    return (lo <= 0 || c - view / 2 >= lo) && (hi >= imageLen || c + view / 2 <= hi);
  };

  // Paints the rendered crop into the viewport for centre `c` at zoom `z`. This is a canvas draw from
  // the cached crop, so panning does not decode an image or re-render anything.
  const drawCrop = (c: [number, number], z: number) => {
    const cv = viewCanvas.current;
    const src = cropCanvas.current;
    const o = cropOrigin.current;
    if (!cv || !src || !o) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const vw = cv.clientWidth;
    const vh = cv.clientHeight;
    if (cv.width !== Math.round(vw * dpr) || cv.height !== Math.round(vh * dpr)) {
      cv.width = Math.round(vw * dpr);
      cv.height = Math.round(vh * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, vw, vh);
    const sw = vw / z;
    const sh = vh / z;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(src, c[0] - sw / 2 - o[0], c[1] - sh / 2 - o[1], sw, sh, 0, 0, vw, vh);
  };

  // The GPU view on the current canvas. A new view has no base yet, so the base is fetched again.
  const getGpu = (): GpuView | null => {
    const canvas = gpuCanvas.current;
    if (!canvas) return null;
    if (gpuViewRef.current?.canvas === canvas) return gpuViewRef.current.view;
    try {
      const view = new GpuView(canvas);
      gpuViewRef.current = { view, canvas };
      baseKind.current = null;
      cropOrigin.current = null;
      if (lutRef.current) view.setLut(lutRef.current);
      return view;
    } catch (e) {
      setGpuBroken(true);
      setStatus(`GPU view unavailable: ${String(e)}`);
      return null;
    }
  };

  // Paints the in-progress pan centre; called once per animation frame.
  const placeCrop = () => {
    panFrame.current = 0;
    const c = panCentre.current;
    if (!c || zoom === "fit") return;
    if (gpuOk) {
      const g = getGpu();
      const rect = cropOrigin.current;
      if (g && rect && imageSize) g.draw(c, zoom, rect, pw, imageSize);
    } else {
      drawCrop(c, zoom);
    }
  };

  // Repaints when the crop, the centre or the zoom changes.
  useEffect(() => {
    if (!gpuOk && zoom !== "fit" && centre) drawCrop(centre, zoom);
  }, [cropRev, centre, zoom, gpuOk]);

  // Full-resolution crop for the zoom views, debounced; re-rendered when sliders or centre change.
  useEffect(() => {
    if (gpuOk || zoom === "fit" || !path || !imageSize || !centre) return;
    const id = window.setTimeout(() => {
      const vw = viewRef.current?.clientWidth ?? 900;
      const vh = viewRef.current?.clientHeight ?? 600;
      // Twice the viewport, so small pans stay on the rendered crop.
      const w = Math.min(imageSize[0], Math.ceil((2 * vw) / zoom));
      const h = Math.min(imageSize[1], Math.ceil((2 * vh) / zoom));
      const x = Math.max(0, Math.min(imageSize[0] - w, Math.round(centre[0] - w / 2)));
      const y = Math.max(0, Math.min(imageSize[1] - h, Math.round(centre[1] - h / 2)));
      invoke<ArrayBuffer>("render_quartz_crop", { path, params: shownLs, x, y, w, h })
        .then((buf) => {
          // Header: width and height (u32, little-endian), then RGBA rows.
          const head = new DataView(buf);
          const cw = head.getUint32(0, true);
          const ch = head.getUint32(4, true);
          const off = document.createElement("canvas");
          off.width = cw;
          off.height = ch;
          off.getContext("2d")?.putImageData(new ImageData(new Uint8ClampedArray(buf, 8), cw, ch), 0, 0);
          cropCanvas.current = off;
          cropOrigin.current = [x, y, cw, ch];
          setCropRev((r) => r + 1);
        })
        .catch((e) => setStatus(String(e)));
    }, 250);
    return () => window.clearTimeout(id);
  }, [zoom, path, imageSize, centre, shownLs, gpuOk]);

  // GPU base at preview size for the fit view. Re-rendered when a spatial setting changes.
  useEffect(() => {
    if (!gpuOk || zoom !== "fit" || !path || !imageSize) return;
    if (!getGpu()) return;
    if (baseKind.current === "fit" && baseSpatial.current === spatialKey) return;
    const req = ++baseReq.current;
    invoke<ArrayBuffer>("render_base_preview", { path, params: ls, side: 960 })
      .then((buf) => {
        if (req !== baseReq.current) return;
        const { w, h, px } = readRaw(buf);
        getGpu()?.setBase(w, h, px);
        cropOrigin.current = [0, 0, imageSize[0], imageSize[1]];
        baseKind.current = "fit";
        baseSpatial.current = spatialKey;
        setCropRev((r) => r + 1);
      })
      .catch((e) => setStatus(String(e)));
  }, [gpuOk, zoom, path, imageSize, spatialKey]);

  // GPU base at full resolution for the zoom views: a crop around the centre, fetched when the
  // viewport leaves it. Pans inside it need no fetch.
  useEffect(() => {
    if (!gpuOk || zoom === "fit" || !path || !imageSize || !centre) return;
    const id = window.setTimeout(() => {
      if (!getGpu()) return;
      const vw = viewRef.current?.clientWidth ?? 900;
      const vh = viewRef.current?.clientHeight ?? 600;
      const r = cropOrigin.current;
      const covered = baseKind.current === "crop" && baseSpatial.current === spatialKey && !!r
        && covers(centre[0], r![0] + r![2] / 2, r![2], vw / zoom, imageSize[0])
        && covers(centre[1], r![1] + r![3] / 2, r![3], vh / zoom, imageSize[1]);
      if (covered) return;
      // Twice the viewport, so small pans stay on the base.
      const w = Math.min(imageSize[0], Math.ceil((2 * vw) / zoom));
      const h = Math.min(imageSize[1], Math.ceil((2 * vh) / zoom));
      const x = Math.max(0, Math.min(imageSize[0] - w, Math.round(centre[0] - w / 2)));
      const y = Math.max(0, Math.min(imageSize[1] - h, Math.round(centre[1] - h / 2)));
      const req = ++baseReq.current;
      invoke<ArrayBuffer>("render_base_crop", { path, params: ls, x, y, w, h })
        .then((buf) => {
          if (req !== baseReq.current) return;
          const { w: cw, h: ch, px } = readRaw(buf);
          getGpu()?.setBase(cw, ch, px);
          cropOrigin.current = [x, y, cw, ch];
          baseKind.current = "crop";
          baseSpatial.current = spatialKey;
          setCropRev((rv) => rv + 1);
        })
        .catch((e) => setStatus(String(e)));
    }, 60);
    return () => window.clearTimeout(id);
  }, [gpuOk, zoom, path, imageSize, centre, spatialKey]);

  // Curve tables for the GPU view, fetched when the curve changes.
  const curveKey = JSON.stringify(ls.curve);
  useEffect(() => {
    if (!gpuOk) return;
    invoke<ArrayBuffer>("curve_luts", { params: ls })
      .then((buf) => {
        const lut = new Float32Array(buf);
        lutRef.current = lut;
        getGpu()?.setLut(lut);
        setCropRev((rv) => rv + 1);
      })
      .catch((e) => setStatus(String(e)));
  }, [gpuOk, curveKey]);

  // Draws the GPU view when the base, the centre, the zoom or a per-pixel setting changes.
  useEffect(() => {
    if (!gpuOk || !path || !imageSize) return;
    const g = getGpu();
    const rect = cropOrigin.current;
    if (!g || !rect || baseKind.current !== (zoom === "fit" ? "fit" : "crop")) return;
    if (zoom === "fit") {
      const cv = gpuCanvas.current;
      if (!cv) return;
      const s = Math.min(cv.clientWidth / imageSize[0], cv.clientHeight / imageSize[1]);
      g.draw([imageSize[0] / 2, imageSize[1] / 2], s, rect, pw, imageSize);
    } else if (centre) {
      g.draw(centre, zoom, rect, pw, imageSize);
    }
  }, [cropRev, centre, zoom, pw, gpuOk, imageSize]);

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
    if (!gpuOk) show(480);
    const settle = window.setTimeout(() => show(960), gpuOk ? 400 : 250);
    return () => window.clearTimeout(settle);
  }, [path, shownLs, gpuOk]);

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
    // Writes the channel being edited. The curve is on when any channel is off the identity line.
    const key = curveChannel === "rgb" ? "points" : curveChannel;
    setLs((s) => {
      const curve = { ...s.curve, [key]: points };
      const isIdentity = (p?: Pt[]) => !p || (p.length === 2 && p[0].y === 0 && p[1].y === 1);
      const anyChange = [curve.points, curve.red, curve.green, curve.blue].some((p) => !isIdentity(p));
      return { ...s, curve: { ...curve, enabled: anyChange } };
    });
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
  // Zooms by `factor` about a point on screen: the image point under it stays put. Zooming out past the
  // fit scale returns to Fit.
  // The view as of the last event. Wheel bursts arrive faster than React re-renders, so each event
  // builds on this, not on the state the last render saw.
  const live = useRef<{ zoom: "fit" | number; centre: [number, number] | null }>({ zoom, centre });
  live.current = { zoom, centre };
  const zoomAt = (clientX: number, clientY: number, factor: number) => {
    const r = viewRef.current?.getBoundingClientRect();
    const v = live.current;
    if (!r || !imageSize || !v.centre) return;
    const fitScale = Math.min(r.width / imageSize[0], r.height / imageSize[1]);
    const cur = v.zoom === "fit" ? fitScale : v.zoom;
    const next = Math.min(16, Math.max(0.01, cur * factor));
    if (next <= fitScale) {
      const fitCentre: [number, number] = [imageSize[0] / 2, imageSize[1] / 2];
      live.current = { zoom: "fit", centre: fitCentre };
      setZoom("fit");
      setCentre(fitCentre);
      return;
    }
    const dx = clientX - r.left - r.width / 2;
    const dy = clientY - r.top - r.height / 2;
    const ox = v.centre[0] + dx / cur;
    const oy = v.centre[1] + dy / cur;
    const nc: [number, number] = [Math.max(0, Math.min(imageSize[0], ox - dx / next)), Math.max(0, Math.min(imageSize[1], oy - dy / next))];
    live.current = { zoom: next, centre: nc };
    setCentre(nc);
    setZoom(next);
  };
  const zoomAtRef = useRef(zoomAt);
  zoomAtRef.current = zoomAt;


  const onWheel = (e: WheelEvent) => {
    if (!imageSize || !centre) return;
    // Two-finger drag (no ctrl): pan, the content follows the fingers.
    if (!e.ctrlKey && !e.metaKey) {
      const v = live.current;
      if (v.zoom === "fit" || !v.centre) return;
      e.preventDefault();
      const nx = Math.max(0, Math.min(imageSize[0], v.centre[0] + e.deltaX / v.zoom));
      const ny = Math.max(0, Math.min(imageSize[1], v.centre[1] + e.deltaY / v.zoom));
      live.current = { zoom: v.zoom, centre: [nx, ny] };
      setCentre([nx, ny]);
      return;
    }
    // Ctrl or Cmd + wheel (trackpad pinch sends the same): smooth zoom about the pointer.
    e.preventDefault();
    const dy = Math.max(-50, Math.min(50, e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY));
    zoomAt(e.clientX, e.clientY, Math.exp(-dy * 0.01));
  };

  // Wheel handling is attached natively and not passive, so preventDefault stops the webview from
  // zooming the whole page. React's wheel listeners are passive and cannot do that.
  const wheelRef = useRef(onWheel);
  wheelRef.current = onWheel;
  useEffect(() => {
    const el = viewRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => wheelRef.current(e);
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  });

  // Trackpad pinch on macOS WebKit arrives as gesture events with a running scale. The page itself
  // must not zoom, so the events are cancelled inside the image view.
  useEffect(() => {
    const pointer = { x: 0, y: 0 };
    let last = 1;
    const move = (e: PointerEvent) => { pointer.x = e.clientX; pointer.y = e.clientY; };
    const inside = () => {
      const r = viewRef.current?.getBoundingClientRect();
      return !!r && pointer.x >= r.left && pointer.x <= r.right && pointer.y >= r.top && pointer.y <= r.bottom;
    };
    const start = (e: Event) => { if (inside()) { e.preventDefault(); last = 1; } };
    const change = (e: Event) => {
      if (!inside()) return;
      e.preventDefault();
      const scale = (e as Event & { scale: number }).scale;
      if (scale > 0) {
        const factor = scale / last;
        last = scale;
        zoomAtRef.current(pointer.x, pointer.y, factor);
      }
    };
    window.addEventListener("pointermove", move);
    document.addEventListener("gesturestart", start, { passive: false });
    document.addEventListener("gesturechange", change, { passive: false });
    return () => {
      window.removeEventListener("pointermove", move);
      document.removeEventListener("gesturestart", start);
      document.removeEventListener("gesturechange", change);
    };
  }, []);

  // + and - zoom about the centre of the view, 0 returns to Fit.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const r = viewRef.current?.getBoundingClientRect();
      if (!r) return;
      const cx = r.left + r.width / 2;
      const cy = r.top + r.height / 2;
      if (e.key === "+" || e.key === "=") zoomAtRef.current(cx, cy, 1.25);
      else if (e.key === "-") zoomAtRef.current(cx, cy, 0.8);
      else if (e.key === "0" && imageSize) { setZoom("fit"); setCentre([imageSize[0] / 2, imageSize[1] / 2]); }
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [imageSize]);

  // Space held is the hand tool (Photoshop): the page does not scroll and dragging pans the zoomed view.
  const [spaceHeld, setSpaceHeld] = useState(false);
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.code !== "Space" || e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      e.preventDefault();
      setSpaceHeld(true);
    };
    const up = (e: KeyboardEvent) => { if (e.code === "Space") setSpaceHeld(false); };
    const blur = () => setSpaceHeld(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, []);

  const welcome = (
    <div className="welcome">
      <p>Open an image to start, or drop one on the window.</p>
      <button onClick={openImage}>Open image...</button>
    </div>
  );

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

  const shownPresets = useMemo(() => {
    const q = presetQuery.toLowerCase();
    return lsPresets.filter((p) => p.category === collection && p.name.toLowerCase().includes(q));
  }, [lsPresets, collection, presetQuery]);
  // Preset thumbnails, one request at a time: the collection on screen first, then the rest in
  // library order (category by category). Jobs for an image that is no longer open are dropped.
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const thumbQueue = useRef<{ key: string; path: string; name: string; params: LsParams }[]>([]);
  const thumbAsked = useRef<Set<string>>(new Set());
  const thumbBusy = useRef(false);
  const pathRef = useRef(path);
  pathRef.current = path;
  const pumpThumbs = async () => {
    if (thumbBusy.current) return;
    thumbBusy.current = true;
    while (thumbQueue.current.length > 0) {
      const job = thumbQueue.current.shift()!;
      if (job.path !== pathRef.current) continue;
      try {
        const b64 = await invoke<string>("preset_thumbnail", { path: job.path, params: job.params });
        setThumbs((t) => ({ ...t, [job.key]: `data:image/png;base64,${b64}` }));
      } catch {
        // A preset that fails to render keeps its placeholder.
      }
    }
    thumbBusy.current = false;
  };
  useEffect(() => {
    if (!path) return;
    const fresh = shownPresets
      .map((p) => ({ key: `${path}|${p.name}`, path, name: p.name, params: p.params as LsParams }))
      .filter((j) => !thumbAsked.current.has(j.key));
    if (fresh.length === 0) return;
    fresh.forEach((j) => thumbAsked.current.add(j.key));
    const freshKeys = new Set(fresh.map((j) => j.key));
    thumbQueue.current = [...fresh, ...thumbQueue.current.filter((j) => !freshKeys.has(j.key))];
    void pumpThumbs();
  }, [path, shownPresets]);
  useEffect(() => {
    if (collections.length > 0 && !collections.some(([cat]) => cat === collection)) setCollection(collections[0][0]);
  });
  const collections = useMemo(() => {
    const m = new Map<string, number>();
    lsPresets.forEach((p) => m.set(p.category, (m.get(p.category) ?? 0) + 1));
    return [...m.entries()];
  }, [lsPresets]);

  const toggleSec = (id: string) => setOpenSec((o) => ({ ...o, [id]: !o[id] }));

  return (
    <div className="app">
      <header className="topbar">
        <img className="topbar-logo" src={fullLogo} alt="Citrine Labs" />
        <button onClick={openImage}>Open…</button>
        <div className="topbar-title">
          <strong>{path ? path.split("/").pop() : "No image open"}</strong>
          <span>{imageSize ? `${imageSize[0]} × ${imageSize[1]}` : ""}{preset ? ` · ${preset}` : ""}</span>
        </div>
        <div className="seg">
          <button className={view === "single" ? "sel" : ""} onClick={() => setView("single")}>Single</button>
          <button className={view === "split" ? "sel" : ""} onClick={() => setView("split")}>Split</button>
          <button className={view === "side" ? "sel" : ""} onClick={() => setView("side")}>Side by Side</button>
        </div>
        <span className="spacer" />
        <button onClick={undo} disabled={past.length === 0} title="Undo">↶</button>
        <button onClick={redo} disabled={future.length === 0} title="Redo">↷</button>
        <button onMouseDown={() => setShowBefore(true)} onMouseUp={() => setShowBefore(false)} onMouseLeave={() => setShowBefore(false)}>Hold: before</button>
        <div className="seg">
          <button className={zoom === "fit" ? "sel" : ""} onClick={() => setZoom("fit")}>Fit</button>
          <button className={zoom === 1 ? "sel" : ""} onClick={() => setZoom(1)}>100%</button>
          <button className={zoom === 2 ? "sel" : ""} onClick={() => setZoom(2)}>200%</button>
        </div>
        {path.toLowerCase().endsWith(".ctrn") ? (
          <>
            <button onClick={cancelHandoff}>Cancel</button>
            <button className="primary" onClick={sendToPhotoshop}>OK: send to Photoshop</button>
          </>
        ) : (
          <>
            <button onClick={chooseOutput} title={outPath || "Choose where to save"}>Output…</button>
            <button className="primary" disabled={!path || !outPath} onClick={exportImage}>Export</button>
          </>
        )}
      </header>

      <div className="body">
      <aside className="left">
        <input className="search" placeholder="Search presets" value={presetQuery} onChange={(e) => setPresetQuery(e.target.value)} />
        <h2>Collections</h2>
        {collections.map(([cat, count]) => (
          <button key={cat} className={collection === cat ? "sel" : ""} onClick={() => setCollection(cat)}>{cat} <span>{count}</span></button>
        ))}
        <h2>{collection}</h2>
        <div className="cards">
          {shownPresets.map((p) => (
            <button key={p.name} title={p.description} className={`card${preset === p.name ? " sel" : ""}`} onClick={() => chooseQuartz(p)}>
              {thumbs[`${path}|${p.name}`] ? <img className="thumb" src={thumbs[`${path}|${p.name}`]} alt="" /> : <span className="thumb empty" />}
              <span className="card-name">{p.name}</span>
            </button>
          ))}
        </div>
        <div className="library-files">
          <button onClick={savePreset}>Save preset…</button>
          <button onClick={saveProject}>Save project…</button>
          <button onClick={loadProject}>Open project…</button>
        </div>
      </aside>

      <main className="center">
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
          <div className={`viewport crop${spaceHeld ? " hand" : ""}`} ref={viewRef} onDoubleClick={(e) => zoomAt(e.clientX, e.clientY, e.altKey ? 0.5 : 2)}
            onPointerDown={(e) => { drag.current = { x: e.clientX, y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
            onPointerMove={(e) => {
              if (!drag.current || !centre || !imageSize) return;
              const dx = (e.clientX - drag.current.x) / zoom;
              const dy = (e.clientY - drag.current.y) / zoom;
              drag.current = { x: e.clientX, y: e.clientY };
              const base = panCentre.current ?? centre;
              const next: [number, number] = [Math.max(0, Math.min(imageSize[0], base[0] - dx)), Math.max(0, Math.min(imageSize[1], base[1] - dy))];
              const view = viewRef.current;
              const o = cropOrigin.current;
              const inside = !!o && !!view
                && covers(next[0], o[0] + o[2] / 2, o[2], view.clientWidth / zoom, imageSize[0])
                && covers(next[1], o[1] + o[3] / 2, o[3], view.clientHeight / zoom, imageSize[1]);
              if (inside) {
                panCentre.current = next;
                if (!panFrame.current) panFrame.current = requestAnimationFrame(placeCrop);
              } else {
                // Past the rendered crop: commit the centre so a crop around it renders.
                panCentre.current = null;
                setCentre(next);
              }
            }}
            onPointerUp={() => {
              drag.current = null;
              if (panCentre.current) { setCentre(panCentre.current); panCentre.current = null; }
            }}>
            {gpuOk ? <canvas ref={gpuCanvas} className="crop-canvas" /> : <canvas ref={viewCanvas} className="crop-canvas" />}
            {imageSize && centre ? (
              <span className="crop-info">{zoom * 100}% · centre {Math.round(centre[0])}, {Math.round(centre[1])} of {imageSize[0]}×{imageSize[1]} · drag to pan</span>
            ) : null}
          </div>
        ) : (
          <div className={`viewport zoom-${zoom}`} ref={viewRef} onDoubleClick={(e) => zoomAt(e.clientX, e.clientY, e.altKey ? 0.5 : 2)}>
            {!path ? welcome : gpuOk && zoom === "fit" ? <canvas ref={gpuCanvas} className="gpu-fit" /> : preview ? (
              <img src={preview} alt="preview"
                style={zoom === "fit" ? undefined : { width: `${zoom * 100}%`, maxWidth: "none" }} />
            ) : welcome}
          </div>
        )}
        <p className="status">{status}{renderMs !== null ? `  |  preview render ${renderMs} ms` : ""}</p>
      </main>

      <aside className="right">
        <Histogram hist={hist} channel={channel} onChannel={setChannel} />
        <div className="adjust-head"><strong>Adjust</strong><span>{preset}</span></div>
        <label className="slider effect">
          <span>Effect Amount<em>{Math.round((1 - ls.finishing.transparency) * 100)}%</em></span>
          <input type="range" min={0} max={1} step={0.01} value={1 - ls.finishing.transparency}
            onChange={(e) => { commit(); setLs((s) => setLsPath(s, "finishing", "transparency", 1 - Number(e.target.value))); }} />
        </label>
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
                  {isOpen && sec.custom === "grading" && (
                    <GradingWheels value={ls.grading} onChange={(g) => { commit(); setLs((s) => ({ ...s, grading: g })); }} />
                  )}
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
              {openSec.curve && <CurvePanel points={(curveChannel === "rgb" ? ls.curve.points : ls.curve[curveChannel]) ?? [{ x: 0, y: 0 }, { x: 1, y: 1 }]} histogram={hist ? hist.l : null} onChange={setLsCurve} channel={curveChannel} onChannel={setCurveChannel} />}
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
        <div className="adjust-foot">
          <button onClick={() => resetSection(Object.keys(LS_DEFAULT))}>Reset All</button>
        </div>
      </aside>
      </div>
    </div>
  );
}
