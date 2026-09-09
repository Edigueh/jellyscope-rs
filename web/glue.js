// Jellyscope frontend glue: loads the WASM viewer, fetches raw f32 flux planes
// and the manifest, and wires the DOM controls. The viewer stretches,
// composites and colormaps live — nothing is pre-baked. No framework, no build.
import init, { Viewer } from "./pkg/viewer.js";

const $ = (id) => document.getElementById(id);
// Served from the repo root; dist/ sits beside web/, so reference it absolutely.
const DIST = "/dist";

// RGB defaults + wavelength-snapping constants, mirroring the Python app.js.
const DEFAULT_RGB_FILTERS = { r: "F200W", g: "F115W", b: "F090W" };
const RGB_DELTA_RG_FALLBACK = 0.836; // F200W − F115W
const RGB_DELTA_GB_FALLBACK = 0.253; // F115W − F090W

// Fetch a raw .f32/.i32 asset as bytes.
async function bytes(path) {
  const r = await fetch(`${DIST}/${path}`);
  if (!r.ok) throw new Error(`fetch ${path}: ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

const state = {
  manifest: null,
  cube: null, // current cube object from the manifest
  filterNames: [], // filter names in manifest/plane order
  wavelengths: {}, // name -> µm (only filters that have one)
  pixmap: null, // Int32Array, ny*nx
  viewer: null,
  selected: -1,
  lastSeparationClump: null, // previous clump id, for separation readout
  // RGB channel indices + locked wavelength offsets (µm) for anchor snapping.
  rgbR: 0,
  rgbG: 0,
  rgbB: 0,
  rgbDeltaRG: null,
  rgbDeltaGB: null,
  rgbQ: 8,
};

async function main() {
  await init();
  state.viewer = new Viewer("view");
  requestAnimationFrame(resizeCanvas); // measure the canvas after first layout
  window.addEventListener("resize", resizeCanvas);

  state.manifest = await (await fetch(`${DIST}/manifest.json`)).json();
  fillSelect($("dataset"), state.manifest.datasets.map((d) => d.name));
  $("dataset").addEventListener("change", onDatasetChange);
  $("cube").addEventListener("change", onCubeChange);
  $("filter").addEventListener("input", () => {
    updateFilterLabel();
    renderImage();
  });
  $("stretch").addEventListener("change", renderImage);
  $("colorscale").addEventListener("change", renderImage);
  $("rgb-method").addEventListener("change", () => {
    updateRgbMethodUI();
    renderImage();
  });
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
  for (const el of document.querySelectorAll('input[name="mode"]')) el.addEventListener("change", onModeChange);
  $("boundaries").addEventListener("change", () => applyBoundaries());

  bindPointer();
  onDatasetChange();
}

function fillSelect(sel, names, selected) {
  sel.innerHTML = "";
  for (const n of names) {
    const o = document.createElement("option");
    o.value = o.textContent = n;
    if (n === selected) o.selected = true;
    sel.append(o);
  }
}

// Fill an RGB channel <select> with filter names; option value is the INDEX.
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

  // Cache every filter's raw flux plane in the viewer.
  state.viewer.beginCube(c.nx, c.ny, c.filters.length);
  await Promise.all(
    c.filters.map(async (f, i) => {
      state.viewer.loadPlane(i, c.nx, c.ny, await bytes(f.texture_flux));
    }),
  );

  // Single-filter slider: bounds from the cube, default to the green channel.
  const fSlider = $("filter");
  fSlider.max = String(Math.max(0, state.filterNames.length - 1));
  const gIdx = state.filterNames.indexOf(c.rgb_default.g);
  fSlider.value = String(gIdx >= 0 ? gIdx : 0);
  updateFilterLabel();

  // RGB defaults + wavelength-locked snapping (ported from Python app.js).
  const d = resolveRgbDefaults(state.filterNames);
  state.rgbR = d.r;
  state.rgbG = d.g;
  state.rgbB = d.b;
  captureRgbDeltas();
  snapRgbFromAnchor("R");
  fillRgbSelect($("rgb-r"), state.rgbR);
  fillRgbSelect($("rgb-g"), state.rgbG);
  fillRgbSelect($("rgb-b"), state.rgbB);
  syncRgbSelects();
  updateRgbMethodUI();

  state.pixmap = new Int32Array((await bytes(c.pixel_clump)).buffer);
  state.selected = -1;
  state.lastSeparationClump = null;
  clearPanel();
  await renderImage();
  applyBoundaries();
}

function isRgb() {
  return document.querySelector('input[name="mode"]:checked').value === "rgb";
}

function onModeChange() {
  const rgb = isRgb();
  $("single-controls").hidden = rgb;
  $("rgb-controls").hidden = !rgb;
  if (rgb) updateRgbMethodUI();
  renderImage();
}

// Show the Q slider only for the Lupton recipe.
function updateRgbMethodUI() {
  $("rgb-q-wrap").hidden = $("rgb-method").value !== "lupton";
}

// Compute and show the current view by driving the live WASM renderers.
function renderImage() {
  const v = state.viewer;
  if (isRgb()) {
    v.renderRgb(state.rgbR, state.rgbG, state.rgbB, $("rgb-method").value, state.rgbQ);
  } else {
    const idx = parseInt($("filter").value, 10);
    v.renderSingle(idx, $("stretch").value, parseInt($("colorscale").value, 10));
  }
}

// Show the current single-filter name (+ wavelength) beside the slider.
function updateFilterLabel() {
  const idx = parseInt($("filter").value, 10);
  const name = state.filterNames[idx];
  const wl = state.wavelengths[name];
  $("filter-label").textContent = wl != null ? `${name} (${wl.toFixed(3)}µm)` : name;
}

// ── RGB wavelength defaults + snapping (ported 1:1 from Python app.js) ────────

// Filters with a known wavelength, sorted ascending by λ: [{i, wl}].
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
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  }
  return best;
}

// Resolve default R/G/B indices: prefer the named F200W/F115W/F090W (only if
// they carry a wavelength); otherwise R=argmax(λ), B=argmin(λ), G=nearest to the
// midpoint; positional fallback (last, mid, first) when <3 filters have λ.
function resolveRgbDefaults(filterList) {
  const named = (key) => {
    const i = filterList.indexOf(DEFAULT_RGB_FILTERS[key]);
    return i >= 0 && state.wavelengths[filterList[i]] != null ? i : -1;
  };
  let r = named("r");
  let g = named("g");
  let b = named("b");

  if (r < 0 || g < 0 || b < 0) {
    const known = knownWavelengths();
    if (known.length >= 3) {
      const lo = known[0];
      const hi = known[known.length - 1];
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

// Capture the locked offsets ΔRG = λ_R − λ_G and ΔGB = λ_G − λ_B at cube load.
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

// argmin over filters of |λ − targetWl|; filters without λ are skipped.
function nearestFilterIndex(targetWl, fallbackIndex) {
  let best = -1;
  let bestDist = Infinity;
  state.filterNames.forEach((name, i) => {
    const wl = state.wavelengths[name];
    if (wl == null) return;
    const d = Math.abs(wl - targetWl);
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  });
  return best >= 0 ? best : fallbackIndex;
}

// Snap the two non-anchor slots to the filters nearest the locked-offset targets
// relative to the anchor's wavelength.
function snapRgbFromAnchor(anchor) {
  const anchorIdx = state["rgb" + anchor];
  const wlAnchor = state.wavelengths[state.filterNames[anchorIdx]];
  if (wlAnchor == null) return;
  const dRG = state.rgbDeltaRG;
  const dGB = state.rgbDeltaGB;
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

// ── Clump overlays, selection, readout (unchanged behaviour) ─────────────────

// Push clump boundary polylines to the viewer (or clear if hidden).
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
  state.viewer.set_selected(BigInt(state.selected));
}

function bindPointer() {
  const canvas = $("view");
  let dragging = false;
  let moved = false;
  let last = [0, 0];

  canvas.addEventListener("mousedown", (e) => {
    dragging = true;
    moved = false;
    last = [e.offsetX, e.offsetY];
  });
  // Click detection on the canvas keeps offsetX/offsetY canvas-relative; the
  // window listener only clears the drag flag when a drag ends off-canvas.
  canvas.addEventListener("mouseup", (e) => {
    if (dragging && !moved) onClick(e);
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });
  canvas.addEventListener("mousemove", (e) => {
    const dpr = window.devicePixelRatio || 1;
    if (dragging) {
      moved = true;
      state.viewer.pan((e.offsetX - last[0]) * dpr, (e.offsetY - last[1]) * dpr);
      last = [e.offsetX, e.offsetY];
    } else {
      // Pointer over a clump signals it's clickable; grab elsewhere (pan).
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

// Pixel under the cursor → clump id via the baked pixmap grid.
function clumpAt(offsetX, offsetY) {
  const dpr = window.devicePixelRatio || 1;
  const [ix, iy] = state.viewer.canvasToImage(offsetX * dpr, offsetY * dpr);
  const [x, y] = [Math.floor(ix), Math.floor(iy)];
  const { nx, ny } = state.cube;
  if (x < 0 || y < 0 || x >= nx || y >= ny) return -1;
  return state.pixmap[y * nx + x];
}

function onClick(e) {
  const id = clumpAt(e.offsetX, e.offsetY);
  state.selected = id;
  state.viewer.set_selected(BigInt(id));
  if (id < 0) return clearPanel();
  showClump(state.cube.clumps.find((c) => c.id === id));
}

function showClump(cl) {
  $("panel-empty").hidden = true;
  const dl = $("panel-props");
  dl.hidden = false;
  const fmt = (v) => (v === 0 || v == null ? "—" : v.toFixed(3));
  const rows = {
    ID: cl.id,
    Component: cl.component,
    "Area (pix)": cl.area_pix,
    "Area (kpc²)": cl.area_kpc2.toFixed(3),
    "r_eff (kpc)": fmt(cl.r_eff_kpc),
    RA: `${cl.ra_deg.toFixed(6)}°`,
    Dec: `${cl.dec_deg.toFixed(6)}°`,
    Inside: cl.inside ? "yes" : "no",
  };
  dl.innerHTML = Object.entries(rows)
    .map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`)
    .join("");

  // Separation from the previously selected clump.
  if (state.lastSeparationClump != null && state.lastSeparationClump.id !== cl.id) {
    const p = state.lastSeparationClump;
    const sep = separationArcsec(p.ra_deg, p.dec_deg, cl.ra_deg, cl.dec_deg);
    $("readout").textContent = `separation from clump ${p.id}: ${sep.toFixed(3)}″`;
  }
  state.lastSeparationClump = cl;
}

function clearPanel() {
  $("panel-empty").hidden = false;
  $("panel-props").hidden = true;
  $("readout").textContent = "";
}

function showReadout(e) {
  const dpr = window.devicePixelRatio || 1;
  const [ix, iy] = state.viewer.canvasToImage(e.offsetX * dpr, e.offsetY * dpr);
  const w = state.cube.wcs;
  // crpix in the manifest is already 0-based (bake shifts it).
  const ra = w.crval[0] + (w.scale[0] * (ix - w.crpix[0])) / w.cos_dec;
  const dec = w.crval[1] + w.scale[1] * (iy - w.crpix[1]);
  if (state.lastSeparationClump == null) {
    $("readout").textContent = `x ${ix.toFixed(1)}, y ${iy.toFixed(1)} — RA ${ra.toFixed(5)}° Dec ${dec.toFixed(5)}°`;
  }
}

// Great-circle separation in arcsec (haversine), mirroring the Rust bake side.
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
