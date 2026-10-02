# Jellyscope

Browser viewer for JWST jellyfish-galaxy datacubes. A Rust CLI bakes NIRCam
FITS cubes into static f32 planes; a Rust→WASM WebGL viewer renders them
client-side with pan, zoom, filter/RGB switching, and clump overlays. No server
at runtime — the baked output is a static site.

This is a rewrite of the Python/FastAPI [jellyscope]. The image stretch and RGB
math is ported 1:1 into `jelly-core` and pinned by golden tests against the
original; the bake CLI preserves raw flux and derives manifest geometry.

## Layout

    crates/bake     native CLI: FITS + clump CSVs -> dist/ f32 planes + manifest.json
    crates/viewer   cdylib compiled to WASM: WebGL2 rendering + camera
    web             HTML/CSS/JS shell that drives the viewer (no framework)

## Prerequisites

- Rust stable with the `wasm32-unknown-unknown` target
- [`wasm-bindgen` CLI] at 0.2.100 (`cargo install wasm-bindgen-cli --version 0.2.100`)
- [`just`]
- A Python 3 for `just serve` (only `http.server`)

## Build and run

    just build      # bake data/ -> dist/ and package the WASM viewer into web/pkg/
    just serve      # static file server on :8000
    # open http://localhost:8000/web/index.html

`just build` expects a `data/` directory of datasets (symlink or copy your FITS
cubes there). Each dataset directory holds one or two
`cut_datacube_nircam*.fits` cubes plus `clumps_properties.csv` and
`clumps_pixels.csv`.

## Development

    just check          # fmt + clippy (-D warnings) + tests — the gate
    just bake DATA DIST # bake only
    just build-viewer   # (re)build web/pkg from the viewer crate
    just wasm           # check the viewer still compiles to wasm

## How it works

`bake` reads each cube and writes raw little-endian f32 flux planes, a
pixel→clump-id grid, clump metadata, and a `manifest.json`. The viewer widens
the planes to f64, applies the live `log`/`asinh` stretch or RGB composite, and
uploads the resulting RGBA image to WebGL2. Pan/zoom is a matrix uniform, so no
data is re-fetched while navigating. Clump boundaries are drawn as line
overlays; clicking maps the cursor to a pixel and looks up the clump in the grid.

The raw f64 is not shipped: f32 planes are widened to f64 before display math.
Stretch constants are fixed in `jelly-core`; the Lupton RGB Q value is live in
the viewer. See [`docs/clump-data-calculations.tex`](docs/clump-data-calculations.tex)
for the formulas and units.

## License

MIT

[jellyscope]: https://huggingface.co/spaces/EAT-Prototypes/jellyscope
[`wasm-bindgen` CLI]: https://rustwasm.github.io/wasm-bindgen/
[`just`]: https://just.systems/
