// Jellyscope frontend glue: loads the WASM viewer, fetches raw f32 flux planes
// and the manifest, and wires the DOM. Live stretch/composite/colormap in WASM,
// multi-clump selection + rect/lasso + rail sections in JS. No framework.
import init, { Viewer } from "./pkg/viewer.js";

const $ = (id) => document.getElementById(id);
// Resolve dist/ relative to this module's URL. Works under any base path:
// `just serve` (repo root, glue.js at /web/glue.js → /web/dist/) and
// GitHub Pages sub-paths (https://user.github.io/repo/glue.js → /repo/dist/).
const DIST = new URL("dist/", import.meta.url).href.replace(/\/$/, "");

// RGB defaults + wavelength-snapping constants, mirroring the Python app.
const DEFAULT_RGB_FILTERS = { r: "F200W", g: "F115W", b: "F090W" };
const RGB_DELTA_RG_FALLBACK = 0.836; // F200W − F115W
const RGB_DELTA_GB_FALLBACK = 0.253; // F115W − F090W

async function bytes(path) {
  const r = await fetch(`${DIST}/${path}`);
  if (!r.ok) throw new Error(`fetch ${path}: ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

const state = {
  manifest: null,
  cube: null,
  filterNames: [],
  wavelengths: {},
  pixmap: null,
  viewer: null,
  selected: new Set(),     // Set<bigint> of clump ids
  mode: "single",           // "single" | "rgb"
  dragMode: "pan",          // "pan" | "rect" | "lasso"
  rgbR: 0, rgbG: 0, rgbB: 0,
  rgbDeltaRG: null, rgbDeltaGB: null,
  rgbQ: 8,
};

async function main() {
  await init();
  state.viewer = new Viewer("view");
  requestAnimationFrame(resizeCanvas);
  window.addEventListener("resize", resizeCanvas);

  state.manifest = await (await fetch(`${DIST}/manifest.json`)).json();
  fillSelect($("dataset"), state.manifest.datasets.map((d) => d.name));

  $("dataset").addEventListener("change", onDatasetChange);
  $("cube").addEventListener("change", onCubeChange);
  $("filter").addEventListener("input", () => { updateFilterLabel(); renderImage(); });
  $("stretch").addEventListener("change", renderImage);
  $("colorscale").addEventListener("change", () => {
    updateColorbar();
    renderImage();
  });
  $("rgb-method").addEventListener("change", () => { updateRgbMethodUI(); renderImage(); });
  for (const anchor of ["R", "G", "B"]) {
    $(`rgb-${anchor.toLowerCase()}`).addEventListener("change", (e) => {
      state["rgb" + anchor] = parseInt(e.target.value, 10);
      snapRgbFromAnchor(anchor);
      syncRgbSelects();
      renderImage();
    });
  }
  $("rgb-q").addEventListener("input", (e) => {
    state.rgbQ = parseFloat(e.target.value);
    $("rgb-q-label").textContent = state.rgbQ.toFixed(1);
    renderImage();
  });
  $("boundaries").addEventListener("change", applyBoundaries);
  $("boundary-color").addEventListener("input", (e) => {
    const [r, g, b] = hexToRgb(e.target.value);
    state.viewer.setBoundaryColor(r, g, b);
  });
  $("centroids").addEventListener("change", (e) => {
    state.viewer.setShowCentroids(e.target.checked);
  });
  $("clear-selection").addEventListener("click", () => setSelection(new Set()));

  // Segmented buttons: view mode + drag tool.
  for (const btn of document.querySelectorAll('[data-mode]')) {
    btn.addEventListener("click", () => setMode(btn.dataset.mode));
  }
  for (const btn of document.querySelectorAll('[data-drag]')) {
    btn.addEventListener("click", () => setDragMode(btn.dataset.drag));
  }
  $("rail-toggle").addEventListener("click", toggleRail);

  bindPointer();
  bindDragOverlay();
  bindKeyboard();
  bindSplitters();
  onDatasetChange();
}

function hexToRgb(hex) {
  const n = parseInt(hex.replace(/^#/, ""), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

// ── Splitters ────────────────────────────────────────────────────────────────

function bindSplitters() {
  bindSplitter($("split-x"), "x", "--rail-w", {
    min: 240,
    max: () => window.innerWidth - 320,
    invert: true, // dragging left widens the rail
  });
}

function bindSplitter(el, axis, cssVar, { min, max, invert = false }) {
  const root = document.documentElement;
  let startPos = 0, startVal = 0;
  const onMove = (e) => {
    const p = axis === "x" ? e.clientX : e.clientY;
    const delta = invert ? startPos - p : p - startPos;
    const next = Math.max(min, Math.min(max(), startVal + delta));
    root.style.setProperty(cssVar, next + "px");
    resizeCanvas();
  };
  const onUp = () => {
    el.classList.remove("dragging");
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  };
  el.addEventListener("mousedown", (e) => {
    e.preventDefault();
    el.classList.add("dragging");
    startPos = axis === "x" ? e.clientX : e.clientY;
    startVal = parseFloat(getComputedStyle(root).getPropertyValue(cssVar));
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  });
}

// ── UI helpers ───────────────────────────────────────────────────────────────

function fillSelect(sel, names, selected) {
  sel.innerHTML = "";
  for (const n of names) {
    const o = document.createElement("option");
    o.value = o.textContent = n;
    if (n === selected) o.selected = true;
    sel.append(o);
  }
}

function fillRgbSelect(sel, selectedIdx) {
  sel.innerHTML = "";
  state.filterNames.forEach((name, i) => {
    const o = document.createElement("option");
    o.value = String(i);
    const wl = state.wavelengths[name];
    o.textContent = wl != null ? `${name} (${wl.toFixed(3)}µm)` : name;
    if (i === selectedIdx) o.selected = true;
    sel.append(o);
  });
}

function currentDataset() {
  return state.manifest.datasets.find((d) => d.name === $("dataset").value);
}

function setMode(mode) {
  state.mode = mode;
  for (const btn of document.querySelectorAll('[data-mode]')) {
    btn.setAttribute("aria-pressed", btn.dataset.mode === mode);
  }
  $("single-controls").hidden = mode !== "single";
  $("rgb-controls").hidden = mode !== "rgb";
  if (mode === "rgb") updateRgbMethodUI();
  updateColorbar();
  renderImage();
}

// Colormap ID → CSS linear-gradient string (bottom → top = 0 → 1).
const COLORBAR_GRADIENTS = {
  "1": "linear-gradient(to top, #440154, #414487, #2a788e, #22a884, #7ad151, #fde725)",
  "2": "linear-gradient(to top, #000004, #3b0f70, #8c2981, #de4968, #fe9f6d, #fcfdbf)",
  "3": "linear-gradient(to top, #0d0887, #6a00a8, #b12a90, #e16462, #fca636, #f0f921)",
  "4": "linear-gradient(to top, #00224e, #123570, #3b496c, #575d6d, #88836a, #d5c063, #fee838)",
  "5": "linear-gradient(to top, #000000, #7f0000, #ff0000, #ff7f00, #ffff00, #ffffff)",
  "6": "linear-gradient(to top, #ffffff, #000000)",
};

function updateColorbar() {
  const cb = $("colorbar");
  if (!cb) return;
  if (state.mode === "rgb") {
    cb.hidden = true;
    return;
  }
  cb.hidden = false;
  const cs = $("colorscale").value;
  const grad = COLORBAR_GRADIENTS[cs] || COLORBAR_GRADIENTS["1"];
  document.documentElement.style.setProperty("--colorbar-gradient", grad);
}

function setDragMode(mode) {
  state.dragMode = mode;
  for (const btn of document.querySelectorAll('[data-drag]')) {
    btn.setAttribute("aria-pressed", btn.dataset.drag === mode);
  }
  $("drag-overlay").classList.toggle("active", mode !== "pan");
}

function updateRgbMethodUI() {
  $("rgb-q-wrap").hidden = $("rgb-method").value !== "lupton";
}

function toggleRail() {
  const collapsed = $("grid").classList.toggle("rail-collapsed");
  $("rail-toggle").textContent = collapsed ? "‹" : "›";
  requestAnimationFrame(resizeCanvas);
}

// ── Dataset / cube loading ───────────────────────────────────────────────────

function onDatasetChange() {
  const ds = currentDataset();
  fillSelect($("cube"), ds.cubes.map((c) => c.name));
  onCubeChange();
}

async function onCubeChange() {
  const ds = currentDataset();
  const c = ds.cubes.find((cc) => cc.name === $("cube").value);
  state.cube = c;
  state.filterNames = c.filters.map((f) => f.name);
  state.wavelengths = {};
  for (const f of c.filters) if (f.wavelength_um != null) state.wavelengths[f.name] = f.wavelength_um;

  state.viewer.beginCube(c.nx, c.ny, c.filters.length);
  await Promise.all(
    c.filters.map(async (f, i) => {
      state.viewer.loadPlane(i, c.nx, c.ny, await bytes(f.texture_flux));
    }),
  );

  const fSlider = $("filter");
  fSlider.max = String(Math.max(0, state.filterNames.length - 1));
  const gIdx = state.filterNames.indexOf(c.rgb_default.g);
  fSlider.value = String(gIdx >= 0 ? gIdx : 0);
  updateFilterLabel();

  const d = resolveRgbDefaults(state.filterNames);
  state.rgbR = d.r; state.rgbG = d.g; state.rgbB = d.b;
  captureRgbDeltas();
  snapRgbFromAnchor("R");
  fillRgbSelect($("rgb-r"), state.rgbR);
  fillRgbSelect($("rgb-g"), state.rgbG);
  fillRgbSelect($("rgb-b"), state.rgbB);
  syncRgbSelects();
  updateRgbMethodUI();

  state.pixmap = new Int32Array((await bytes(c.pixel_clump)).buffer);
  setSelection(new Set(), { skipRerender: true });
  renderClumpList();
  pushCentroids();
  updateColorbar();
  await renderImage();
  applyBoundaries();
}

function pushCentroids() {
  const clumps = state.cube.clumps;
  const xs = new Float32Array(2 * clumps.length);
  clumps.forEach((cl, i) => {
    xs[2 * i]     = cl.x0;
    xs[2 * i + 1] = cl.y0;
  });
  state.viewer.setCentroids(xs);
  state.viewer.setShowCentroids($("centroids").checked);
}

function renderImage() {
  const v = state.viewer;
  if (state.mode === "rgb") {
    v.renderRgb(state.rgbR, state.rgbG, state.rgbB, $("rgb-method").value, state.rgbQ);
  } else {
    const idx = parseInt($("filter").value, 10);
    v.renderSingle(idx, $("stretch").value, parseInt($("colorscale").value, 10));
  }
}

function updateFilterLabel() {
  const idx = parseInt($("filter").value, 10);
  const name = state.filterNames[idx];
  const wl = state.wavelengths[name];
  $("filter-label").textContent = wl != null ? `${name} · ${wl.toFixed(3)}µm` : name;
}

// ── RGB wavelength snapping (ported 1:1 from Python app.js) ──────────────────

function knownWavelengths() {
  return state.filterNames
    .map((name, i) => ({ i, wl: state.wavelengths[name] }))
    .filter((x) => x.wl != null)
    .sort((a, b) => a.wl - b.wl);
}

function nearestKnownToWl(known, targetWl) {
  let best = known[0].i;
  let bestDist = Infinity;
  for (const { i, wl } of known) {
    const d = Math.abs(wl - targetWl);
    if (d < bestDist) { best = i; bestDist = d; }
  }
  return best;
}

function resolveRgbDefaults(filterList) {
  const named = (key) => {
    const i = filterList.indexOf(DEFAULT_RGB_FILTERS[key]);
    return i >= 0 && state.wavelengths[filterList[i]] != null ? i : -1;
  };
  let r = named("r"), g = named("g"), b = named("b");
  if (r < 0 || g < 0 || b < 0) {
    const known = knownWavelengths();
    if (known.length >= 3) {
      const lo = known[0], hi = known[known.length - 1];
      if (r < 0) r = hi.i;
      if (b < 0) b = lo.i;
      if (g < 0) g = nearestKnownToWl(known, (lo.wl + hi.wl) / 2);
    } else {
      const n = filterList.length;
      if (r < 0) r = n - 1;
      if (g < 0) g = Math.floor(n / 2);
      if (b < 0) b = 0;
    }
  }
  return { r, g, b };
}

function captureRgbDeltas() {
  const wlR = state.wavelengths[state.filterNames[state.rgbR]];
  const wlG = state.wavelengths[state.filterNames[state.rgbG]];
  const wlB = state.wavelengths[state.filterNames[state.rgbB]];
  if (wlR != null && wlG != null && wlB != null) {
    state.rgbDeltaRG = wlR - wlG;
    state.rgbDeltaGB = wlG - wlB;
  } else {
    state.rgbDeltaRG = RGB_DELTA_RG_FALLBACK;
    state.rgbDeltaGB = RGB_DELTA_GB_FALLBACK;
  }
}

function nearestFilterIndex(targetWl, fallbackIndex) {
  let best = -1;
  let bestDist = Infinity;
  state.filterNames.forEach((name, i) => {
    const wl = state.wavelengths[name];
    if (wl == null) return;
    const d = Math.abs(wl - targetWl);
    if (d < bestDist) { best = i; bestDist = d; }
  });
  return best >= 0 ? best : fallbackIndex;
}

function snapRgbFromAnchor(anchor) {
  const anchorIdx = state["rgb" + anchor];
  const wlAnchor = state.wavelengths[state.filterNames[anchorIdx]];
  if (wlAnchor == null) return;
  const dRG = state.rgbDeltaRG, dGB = state.rgbDeltaGB;
  if (anchor === "R") {
    state.rgbG = nearestFilterIndex(wlAnchor - dRG, state.rgbG);
    state.rgbB = nearestFilterIndex(wlAnchor - dRG - dGB, state.rgbB);
  } else if (anchor === "G") {
    state.rgbR = nearestFilterIndex(wlAnchor + dRG, state.rgbR);
    state.rgbB = nearestFilterIndex(wlAnchor - dGB, state.rgbB);
  } else {
    state.rgbG = nearestFilterIndex(wlAnchor + dGB, state.rgbG);
    state.rgbR = nearestFilterIndex(wlAnchor + dGB + dRG, state.rgbR);
  }
}

function syncRgbSelects() {
  $("rgb-r").value = String(state.rgbR);
  $("rgb-g").value = String(state.rgbG);
  $("rgb-b").value = String(state.rgbB);
}

// ── Clump overlays + selection ───────────────────────────────────────────────

function applyBoundaries() {
  if (!$("boundaries").checked) {
    state.viewer.set_boundaries(new Float32Array(0), new Int32Array(0), new BigInt64Array(0));
    return;
  }
  const verts = [];
  const counts = [];
  const ids = [];
  for (const cl of state.cube.clumps) {
    for (const [x, y] of cl.boundary) verts.push(x, y);
    counts.push(cl.boundary.length);
    ids.push(BigInt(cl.id));
  }
  state.viewer.set_boundaries(new Float32Array(verts), new Int32Array(counts), new BigInt64Array(ids));
  pushSelectionToViewer();
}

function pushSelectionToViewer() {
  state.viewer.setSelected(new BigInt64Array([...state.selected]));
}

// Central mutator: every code path calls this so the WASM viewer, rail, and
// clump-list buttons stay in sync.
function setSelection(nextSet, opts = {}) {
  state.selected = nextSet;
  pushSelectionToViewer();
  renderRail();
  updateClumpListPressed();
  if (!opts.skipRerender) { /* selection colour drawn by pushSelectionToViewer */ }
}

function toggleClump(id, mode) {
  const bid = BigInt(id);
  const next = new Set(state.selected);
  if (mode === "add") next.add(bid);
  else if (mode === "sub") next.delete(bid);
  else if (mode === "toggle") { next.has(bid) ? next.delete(bid) : next.add(bid); }
  else { next.clear(); next.add(bid); }   // "replace"
  setSelection(next);
}

function selectionMode(e) {
  if (e.shiftKey) return "add";
  if (e.altKey) return "sub";
  if (e.metaKey || e.ctrlKey) return "toggle";
  return "replace";
}

// ── Pointer (canvas: pan/zoom + click) ───────────────────────────────────────

function bindPointer() {
  const canvas = $("view");
  let dragging = false, moved = false, last = [0, 0];

  canvas.addEventListener("mousedown", (e) => {
    if (state.dragMode !== "pan") return;
    dragging = true; moved = false; last = [e.offsetX, e.offsetY];
  });
  canvas.addEventListener("mouseup", (e) => {
    if (state.dragMode !== "pan") return;
    if (dragging && !moved) onCanvasClick(e);
    dragging = false;
  });
  window.addEventListener("mouseup", () => { dragging = false; });
  canvas.addEventListener("mousemove", (e) => {
    const dpr = window.devicePixelRatio || 1;
    if (dragging) {
      moved = true;
      state.viewer.pan((e.offsetX - last[0]) * dpr, (e.offsetY - last[1]) * dpr);
      last = [e.offsetX, e.offsetY];
    } else if (state.dragMode === "pan") {
      canvas.style.cursor = clumpAt(e.offsetX, e.offsetY) >= 0 ? "pointer" : "grab";
    }
    showReadout(e);
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const dpr = window.devicePixelRatio || 1;
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    state.viewer.zoom(factor, e.offsetX * dpr, e.offsetY * dpr);
  }, { passive: false });
}

function clumpAt(offsetX, offsetY) {
  const dpr = window.devicePixelRatio || 1;
  const [ix, iy] = state.viewer.canvasToImage(offsetX * dpr, offsetY * dpr);
  const [x, y] = [Math.floor(ix), Math.floor(iy)];
  const { nx, ny } = state.cube;
  if (x < 0 || y < 0 || x >= nx || y >= ny) return -1;
  return state.pixmap[y * nx + x];
}

function onCanvasClick(e) {
  const id = clumpAt(e.offsetX, e.offsetY);
  if (id < 0) {
    if (selectionMode(e) === "replace") setSelection(new Set());
    return;
  }
  toggleClump(id, selectionMode(e));
}

// ── Drag overlay: rect + lasso ───────────────────────────────────────────────

function bindDragOverlay() {
  const overlay = $("drag-overlay");
  const svg = $("drag-lasso");
  let rectEl = null;
  let startPx = null;
  let lassoPts = null;
  let modeAtStart = null;

  overlay.addEventListener("mousedown", (e) => {
    if (state.dragMode === "pan") return;
    modeAtStart = selectionMode(e);
    startPx = [e.offsetX, e.offsetY];
    if (state.dragMode === "rect") {
      rectEl = document.createElement("div");
      rectEl.className = "drag-rect";
      Object.assign(rectEl.style, { left: e.offsetX + "px", top: e.offsetY + "px", width: "0", height: "0" });
      overlay.append(rectEl);
    } else {
      lassoPts = [[e.offsetX, e.offsetY]];
      svg.innerHTML = `<path d="M ${e.offsetX} ${e.offsetY}"/>`;
    }
  });

  overlay.addEventListener("mousemove", (e) => {
    if (!startPx) return;
    if (state.dragMode === "rect" && rectEl) {
      const [sx, sy] = startPx;
      const x = Math.min(sx, e.offsetX), y = Math.min(sy, e.offsetY);
      const w = Math.abs(e.offsetX - sx), h = Math.abs(e.offsetY - sy);
      Object.assign(rectEl.style, { left: x + "px", top: y + "px", width: w + "px", height: h + "px" });
    } else if (state.dragMode === "lasso" && lassoPts) {
      lassoPts.push([e.offsetX, e.offsetY]);
      svg.querySelector("path").setAttribute(
        "d",
        "M " + lassoPts.map(([x, y]) => `${x} ${y}`).join(" L ") + " Z",
      );
    }
  });

  const finish = (e) => {
    if (!startPx) return;
    if (state.dragMode === "rect") {
      const [sx, sy] = startPx;
      const ids = clumpsInRect(sx, sy, e.offsetX, e.offsetY);
      applyDragSelection(ids, modeAtStart);
      if (rectEl) { rectEl.remove(); rectEl = null; }
    } else if (state.dragMode === "lasso") {
      if (lassoPts && lassoPts.length >= 3) {
        const ids = clumpsInPolygon(lassoPts);
        applyDragSelection(ids, modeAtStart);
      }
      svg.innerHTML = "";
      lassoPts = null;
    }
    startPx = null;
    modeAtStart = null;
  };
  overlay.addEventListener("mouseup", finish);
  overlay.addEventListener("mouseleave", finish);
}

function applyDragSelection(idsFound, mode) {
  const next = new Set(state.selected);
  if (mode === "replace") next.clear();
  for (const id of idsFound) {
    const bid = BigInt(id);
    if (mode === "sub") next.delete(bid);
    else next.add(bid);
  }
  setSelection(next);
}

// Convert canvas offset coords → image coords via the viewer camera.
function canvasToImage(offsetX, offsetY) {
  const dpr = window.devicePixelRatio || 1;
  return state.viewer.canvasToImage(offsetX * dpr, offsetY * dpr);
}

function clumpsInRect(x0, y0, x1, y1) {
  const [ix0, iy0] = canvasToImage(Math.min(x0, x1), Math.min(y0, y1));
  const [ix1, iy1] = canvasToImage(Math.max(x0, x1), Math.max(y0, y1));
  const xa = Math.min(ix0, ix1), xb = Math.max(ix0, ix1);
  const ya = Math.min(iy0, iy1), yb = Math.max(iy0, iy1);
  const out = [];
  for (const cl of state.cube.clumps) {
    if (cl.x0 >= xa && cl.x0 <= xb && cl.y0 >= ya && cl.y0 <= yb) out.push(cl.id);
  }
  return out;
}

function clumpsInPolygon(canvasPoly) {
  const imagePoly = canvasPoly.map(([x, y]) => canvasToImage(x, y));
  const out = [];
  for (const cl of state.cube.clumps) {
    if (pointInPolygon(cl.x0, cl.y0, imagePoly)) out.push(cl.id);
  }
  return out;
}

// Ray-cast point-in-polygon.
function pointInPolygon(x, y, poly) {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i], [xj, yj] = poly[j];
    const crosses = (yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi + 1e-12) + xi;
    if (crosses) inside = !inside;
  }
  return inside;
}

// ── Rail rendering ───────────────────────────────────────────────────────────

function renderRail() {
  renderProperties();
  renderSeparations();
  updateClumpListPressed();
}

function renderProperties() {
  const empty = $("panel-empty");
  const dl = $("panel-props");
  const sel = [...state.selected].map((bid) => state.cube.clumps.find((c) => BigInt(c.id) === bid)).filter(Boolean);

  if (sel.length === 0) {
    empty.hidden = false;
    dl.hidden = true;
    dl.innerHTML = "";
    return;
  }
  empty.hidden = true;
  dl.hidden = false;

  const fmt = (v) => (v === 0 || v == null ? "—" : v.toFixed(3));
  let rows;
  if (sel.length === 1) {
    const cl = sel[0];
    rows = {
      ID: cl.id,
      "Area (pix)": cl.area_pix,
      "Area (kpc²)": cl.area_kpc2.toFixed(3),
      "r_eff (kpc)": fmt(cl.r_eff_kpc),
      RA: `${cl.ra_deg.toFixed(6)}°`,
      Dec: `${cl.dec_deg.toFixed(6)}°`,
    };
  } else {
    const totalArea = sel.reduce((s, c) => s + (c.area_kpc2 ?? 0), 0);
    const withReff = sel.filter((c) => c.r_eff_kpc);
    const meanReff = withReff.length ? withReff.reduce((s, c) => s + c.r_eff_kpc, 0) / withReff.length : null;
    rows = {
      Selected: sel.length,
      "Total area (kpc²)": totalArea.toFixed(3),
      "Mean r_eff (kpc)": meanReff != null ? meanReff.toFixed(3) : "—",
    };
  }
  dl.innerHTML = Object.entries(rows).map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join("");
}

function renderSeparations() {
  const table = $("seps");
  const sel = [...state.selected]
    .map((bid) => state.cube.clumps.find((c) => BigInt(c.id) === bid))
    .filter(Boolean);
  if (sel.length < 2) {
    table.innerHTML = `<tbody><tr><td class="empty">Select two or more clumps.</td></tr></tbody>`;
    return;
  }
  sel.sort((a, b) => a.id - b.id);
  const header = `<thead><tr><th></th>${sel.map((c) => `<th>#${c.id}</th>`).join("")}</tr></thead>`;
  const rows = sel.map((row) => {
    const cells = sel.map((col) => {
      if (col.id === row.id) return `<td>—</td>`;
      if (col.id < row.id) {
        const sep = separationArcsec(row.ra_deg, row.dec_deg, col.ra_deg, col.dec_deg);
        return `<td>${sep.toFixed(2)}″</td>`;
      }
      return `<td></td>`;
    }).join("");
    return `<tr><th>#${row.id}</th>${cells}</tr>`;
  }).join("");
  table.innerHTML = header + `<tbody>${rows}</tbody>`;
}

function renderClumpList() {
  const ul = $("clump-list");
  ul.innerHTML = "";
  const list = state.cube.clumps.slice().sort((a, b) => a.id - b.id);
  for (const cl of list) {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = `#${cl.id}`;
    btn.dataset.id = String(cl.id);
    btn.setAttribute("aria-pressed", state.selected.has(BigInt(cl.id)));
    btn.title = `Clump ${cl.id} · ${cl.area_kpc2.toFixed(2)} kpc²`;
    btn.addEventListener("click", (e) => toggleClump(cl.id, selectionMode(e)));
    li.append(btn);
    ul.append(li);
  }
}

function updateClumpListPressed() {
  for (const btn of $("clump-list").querySelectorAll("button[data-id]")) {
    btn.setAttribute("aria-pressed", state.selected.has(BigInt(btn.dataset.id)));
  }
}

// ── Keyboard shortcuts ───────────────────────────────────────────────────────

function bindKeyboard() {
  window.addEventListener("keydown", (e) => {
    if (e.target.matches("input, select, textarea")) return;
    const k = e.key.toLowerCase();
    if (k === "s") setMode("single");
    else if (k === "r") setMode("rgb");
    else if (k === "p") setDragMode("pan");
    else if (k === "t") setDragMode("rect");
    else if (k === "l") setDragMode("lasso");
    else if (k === "b") { $("boundaries").checked = !$("boundaries").checked; applyBoundaries(); }
    else if (k === "[" || k === "]") toggleRail();
    else if (k === "escape") setSelection(new Set());
    else if (k === "arrowleft" || k === "arrowright") {
      if (state.mode !== "single") return;
      const slider = $("filter");
      const step = k === "arrowright" ? 1 : -1;
      const next = Math.max(0, Math.min(parseInt(slider.max, 10), parseInt(slider.value, 10) + step));
      slider.value = String(next);
      updateFilterLabel();
      renderImage();
    } else return;
    e.preventDefault();
  });
}

// ── Readout + geometry ───────────────────────────────────────────────────────

function showReadout(e) {
  const dpr = window.devicePixelRatio || 1;
  const [ix, iy] = state.viewer.canvasToImage(e.offsetX * dpr, e.offsetY * dpr);
  const w = state.cube.wcs;
  const ra = w.crval[0] + (w.scale[0] * (ix - w.crpix[0])) / w.cos_dec;
  const dec = w.crval[1] + w.scale[1] * (iy - w.crpix[1]);
  $("coord-readout").textContent =
    `x ${ix.toFixed(1)}, y ${iy.toFixed(1)} · RA ${ra.toFixed(5)}° Dec ${dec.toFixed(5)}°`;
}

function separationArcsec(ra1, dec1, ra2, dec2) {
  const rad = Math.PI / 180;
  const [r1, d1, r2, d2] = [ra1 * rad, dec1 * rad, ra2 * rad, dec2 * rad];
  const a = Math.sin((d2 - d1) / 2) ** 2 + Math.cos(d1) * Math.cos(d2) * Math.sin((r2 - r1) / 2) ** 2;
  return (2 * Math.asin(Math.sqrt(a))) / rad * 3600;
}

function resizeCanvas() {
  const canvas = $("view");
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();
  state.viewer?.resize(Math.round(rect.width * dpr), Math.round(rect.height * dpr));
}

main();