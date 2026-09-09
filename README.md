# Jellyscope

Browser viewer for JWST jellyfish-galaxy datacubes. A Rust CLI bakes NIRCam
FITS cubes into static textures; a Rust→WASM WebGL viewer renders them
client-side with pan, zoom, filter/RGB switching, and clump overlays. No server
at runtime — the baked output is a static site.

This is a rewrite of the Python/FastAPI [jellyscope]; the astronomy math
(stretches, RGB composites, WCS) is ported 1:1 and pinned by golden tests
against the original.

## Layout

    crates/bake     native CLI: FITS + clump CSVs -> dist/ textures + manifest.json
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

`bake` reads each cube, applies the display math on the CPU, and writes one
grayscale RGBA texture per filter per stretch (`log`, `asinh`), two RGB
composites (`percentile-asinh`, `lupton`), a pixel→clump-id grid, and a
`manifest.json` describing everything. The viewer uploads a texture to WebGL2
and draws a single quad; pan/zoom is a matrix uniform, so no data is re-fetched
while navigating. Clump boundaries are drawn as line overlays; clicking maps the
cursor to a pixel and looks up the clump in the grid.

Display parameters are fixed at bake time (the original app exposed no live
tuning). The raw f64 is not shipped; re-bake to change stretch parameters.

## License

MIT

[jellyscope]: https://huggingface.co/spaces/EAT-Prototypes/jellyscope
[`wasm-bindgen` CLI]: https://rustwasm.github.io/wasm-bindgen/
[`just`]: https://just.systems/
