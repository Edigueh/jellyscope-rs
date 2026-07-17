// Jellyscope frontend glue: loads the WASM viewer, fetches baked textures and
// the manifest, and wires the DOM controls. No framework, no build step.
import init, { Viewer } from "./pkg/viewer.js";

const $ = (id) => document.getElementById(id);
// Served from the repo root; dist/ sits beside web/, so reference it absolutely.
const DIST = "/dist";

// Fetch a raw .rgba/.i32 texture as bytes.
async function bytes(path) {
  const r = await fetch(`${DIST}/${path}`);
  if (!r.ok) throw new Error(`fetch ${path}: ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

const state = {
  manifest: null,
  cube: null, // current cube object from the manifest
  pixmap: null, // Int32Array, ny*nx
  viewer: null,
  selected: -1,
  lastSeparationClump: null, // previous clump id, for separation readout
};

async function main() {
  await init();
  state.viewer = new Viewer("view");
  resizeCanvas();
  window.addEventListener("resize", resizeCanvas);

  state.manifest = await (await fetch(`${DIST}/manifest.json`)).json();
  fillSelect($("dataset"), state.manifest.datasets.map((d) => d.name));
  $("dataset").addEventListener("change", onDatasetChange);
  $("cube").addEventListener("change", onCubeChange);
  $("filter").addEventListener("change", renderImage);
  $("stretch").addEventListener("change", renderImage);
  $("rgb-method").addEventListener("change", renderImage);
  for (const id of ["rgb-r", "rgb-g", "rgb-b"]) $(id).addEventListener("change", renderImage);
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
  state.cube = ds.cubes.find((c) => c.name === $("cube").value);
  const names = state.cube.filters.map((f) => f.name);
  fillSelect($("filter"), names, state.cube.rgb_default.g);
  fillSelect($("rgb-r"), names, state.cube.rgb_default.r);
  fillSelect($("rgb-g"), names, state.cube.rgb_default.g);
  fillSelect($("rgb-b"), names, state.cube.rgb_default.b);

  state.pixmap = new Int32Array((await bytes(state.cube.pixel_clump)).buffer);
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
  renderImage();
}

// Load and show the texture for the current mode/filter/stretch/recipe.
async function renderImage() {
  const c = state.cube;
  let path;
  if (isRgb()) {
    path = $("rgb-method").value === "lupton"
      ? c.rgb_default.texture_lupton
      : c.rgb_default.texture_percentile;
  } else {
    const f = c.filters.find((x) => x.name === $("filter").value);
    path = $("stretch").value === "log" ? f.texture_log : f.texture_asinh;
  }
  state.viewer.set_texture(c.nx, c.ny, await bytes(path), isRgb());
}

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
  window.addEventListener("mouseup", (e) => {
    if (dragging && !moved) onClick(e);
    dragging = false;
  });
  canvas.addEventListener("mousemove", (e) => {
    const dpr = window.devicePixelRatio || 1;
    if (dragging) {
      moved = true;
      state.viewer.pan((e.offsetX - last[0]) * dpr, (e.offsetY - last[1]) * dpr);
      last = [e.offsetX, e.offsetY];
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
  const rows = {
    ID: cl.id,
    Component: cl.component,
    "Area (pix)": cl.area_pix,
    "Area (kpc²)": cl.area_kpc2.toFixed(3),
    "r_eff (kpc)": cl.r_eff_kpc.toFixed(3),
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
