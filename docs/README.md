# Architecture

Jellyscope. No server at runtime. CLI bakes FITS → static assets; WASM viewer renders in-browser.

## Documents

- [`01-overview.md`](01-overview.md) — system context + top-level flow
- [`02-components.md`](02-components.md) — crates, modules, deps
- [`03-build-pipeline.md`](03-build-pipeline.md) — bake sequence
- [`04-runtime.md`](04-runtime.md) — viewer runtime + interactions
- [`05-data-model.md`](05-data-model.md) — manifest + file layout
- [`06-deployment.md`](06-deployment.md) — docker / static hosting
- [`clump-data-calculations.tex`](clump-data-calculations.tex) — clump fields,
  image math, WCS, separations, units, and precision
- [`clump-data-calculations.pdf`](clump-data-calculations.pdf) — rendered reference
