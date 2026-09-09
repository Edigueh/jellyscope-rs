# jellyscope-rs — handoff map

State as of 2026-08-11. Seed doc for a fresh conversation in this repo.

## What it is

Ground-up rewrite of the Python/FastAPI [jellyscope]. Architecture is
**fundamentally different — no runtime server.** A native Rust CLI bakes FITS →
static textures + `manifest.json`; a Rust→WASM WebGL2 viewer renders
client-side. Baked output is a static site. Astronomy math ported **1:1**,
pinned by golden tests against the Python original.

## Repo state

- `~/jellyscope-rs/`, branch `master`, tree clean, 11 commits, build green.
- Cargo workspace, edition 2024, clippy pedantic. ~2150 LOC.

## Two crates

### `crates/bake` — native CLI (1644 LOC)

Deps: `fitrs`, `serde`, `thiserror`.

| file | role |
|---|---|
| `fits.rs` | FITS cube reader (shape / filters / WCS) |
| `wcs.rs` | linear WCS affine + great-circle separation |
| `stretch.rs` | log + lupton-asinh, sigma-clipped background |
| `composite.rs` | percentile-asinh + Lupton RGB |
| `clumps.rs` | CSV load, monotone-chain hull, pixel grid |
| `manifest.rs` | manifest.json emit |
| `main.rs` | orchestration data/ → dist/ |

### `crates/viewer` — cdylib → WASM (507 LOC)

Deps: `wasm-bindgen =0.2.100` (pinned to the installed CLI — crate and CLI
versions must match exactly or bindings mismatch at runtime), `web-sys`,
`js-sys`.

- `gl.rs` — WebGL2 core: image program + clump-boundary program.
- `camera.rs` — pan/zoom (tested).
- `lib.rs` — entry.
- `shaders/`.
- Pan/zoom is a matrix uniform → no data re-fetch on navigate. Click → pixel →
  clump-id grid lookup.

### `web/`

Vanilla HTML/CSS/JS shell driving the WASM (dataset / filter / RGB / clumps).
`web/pkg/` holds the built `viewer_bg.wasm` + glue.

## Ported and golden-tested

FITS read, WCS + separation, both stretches, both RGB composites, clump hull +
pixel grid, manifest. Viewer: camera, image render, clump overlays + selection,
web shell.

## Key divergences from Python jellyscope

- Display params **fixed at bake time** — no live stretch tuning (the Python app
  exposed none either). Re-bake to change stretch parameters.
- Raw f64 not shipped — only baked RGBA textures.
- No FastAPI, no runtime endpoints. Static site.

## Build / dev

- `just build` — bake `data/` → `dist/` and package the WASM into `web/pkg/`.
- `just serve` — static file server on :8000 (python http.server). Open
  `http://localhost:8000/web/index.html`.
- `just check` — fmt + clippy (`-D warnings`) + tests. **The gate.**
- `just bake DATA DIST` / `just build-viewer` / `just wasm` — partial steps.
- Needs Rust stable + `wasm32-unknown-unknown` target + `wasm-bindgen-cli`
  0.2.100 + `just`. `data/` is a dir of datasets (symlink/copy FITS cubes);
  each holds `cut_datacube_nircam*.fits` + `clumps_properties.csv` +
  `clumps_pixels.csv`.

## History note

The porting work has **no surviving conversation** — the one Claude session in
this repo's project dir (`6185025d-…`) is empty (an `/effort` command, never
prompted). The code, commits, and README are the only record. README is the
canonical human doc; this file is the fast map.

[jellyscope]: https://huggingface.co/spaces/EAT-Prototypes/jellyscope
