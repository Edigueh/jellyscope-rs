# Overview

Static site. Zero runtime server. Astronomy math shared f64 between native bake and WASM viewer.

## System context

```mermaid
flowchart LR
    A[FITS cubes<br/>clump CSVs] -->|data/| B[bake CLI<br/>native Rust]
    B -->|dist/| C[Static assets<br/>f32 planes + manifest.json + pixmap.i32]
    C --> D[Static HTTP server<br/>any CDN / nginx / http.server]
    D -->|fetch| E[Browser]
    E --> F[viewer.wasm<br/>WebGL2]
    F --> G[Canvas render]
    U[User] --> E
```

## Top-level flow

```mermaid
sequenceDiagram
    autonumber
    participant Dev
    participant Bake as bake (native)
    participant FS as dist/
    participant Browser
    participant WASM as viewer.wasm

    Dev->>Bake: just build (data/ -> dist/)
    Bake->>FS: f32 flux planes + pixmap.i32 + manifest.json
    Dev->>FS: serve static (just serve)
    Browser->>FS: GET index.html / glue.js / wasm
    Browser->>WASM: new Viewer(canvas)
    Browser->>FS: GET manifest.json
    Browser->>FS: GET *.f32 planes
    Browser->>WASM: loadPlane / renderSingle | renderRgb
    WASM-->>Browser: pixels (WebGL2 draw)
```

## Key properties

- Raw f64 math never shipped: f32 planes shipped, viewer widens to f64.
- Astronomy math (stretch, composite) in `jelly-core`; same code in bake + viewer.
- Pan/zoom = matrix uniform. No data re-fetch while navigating.
- Click → pixmap lookup → clump id.
