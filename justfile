# Recipes: `just` to list. Native checks run on the host target; the viewer
# crate also builds for wasm to catch target-specific breakage early.

# Docker recipes: `just docker`.
mod docker

_default:
    @just --list

# fmt + clippy (-D warnings) + tests. The gate everything must pass.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

fmt:
    cargo fmt

# Bake FITS + CSV from data/ into dist/.
bake *ARGS:
    cargo run -p bake -- {{ARGS}}

# Static-serve the repo root so the browser can reach web/ and dist/.
serve port="8000":
    python3 -m http.server {{port}}

# Confirm the viewer crate still compiles to wasm.
wasm:
    cargo build -p viewer --target wasm32-unknown-unknown

# Build the WASM viewer and generate JS bindings into web/pkg/ (no wasm-pack).
build-viewer:
    cargo build -p viewer --target wasm32-unknown-unknown --release
    wasm-bindgen target/wasm32-unknown-unknown/release/viewer.wasm \
        --out-dir web/pkg --target web --no-typescript

# Full local build: bake data then package the viewer.
build: build-viewer
    cargo run -p bake -- data dist
