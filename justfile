# Recipes: `just` to list. Native checks run on the host target; the viewer
# crate also builds for wasm to catch target-specific breakage early.

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

# Static-serve dist/ + web/ for the browser viewer.
serve port="8000":
    python3 -m http.server {{port}}

# Confirm the viewer crate still compiles to wasm.
wasm:
    cargo build -p viewer --target wasm32-unknown-unknown
