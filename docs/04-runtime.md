# Runtime (viewer)

JS shell loads manifest + glue.js, instantiates WASM Viewer, pushes planes, drives render.

## Boot

```mermaid
sequenceDiagram
    autonumber
    participant HTML as index.html
    participant JS as glue.js
    participant W as Viewer (WASM)
    participant GL as WebGL2
    participant Net as dist/

    HTML->>JS: load module
    JS->>W: new Viewer("canvas")
    W->>GL: get context, compile programs, create VAOs
    JS->>Net: GET manifest.json
    JS->>JS: populate dataset/cube selects
    JS->>Net: GET {FILTER}.f32 per filter
    JS->>W: beginCube(nx, ny, n_filters)
    JS->>W: loadPlane(i, nx, ny, bytes)
    JS->>W: renderSingle | renderRgb
    W->>GL: upload RGBA texture + draw quad
```

## Render path

```mermaid
flowchart LR
    subgraph CPU[WASM / CPU, f64]
        P[flux plane] --> S{mode?}
        S -->|single| ST[stretch::log or lupton_asinh]
        S -->|rgb| CO[composite::percentile_asinh or lupton]
        ST --> GRAY[gray_rgba]
        CO --> RGBA[rgb_to_rgba]
    end
    GRAY --> TEX[WebGL2 texture]
    RGBA --> TEX
    TEX --> IMG[ImageProgram<br/>colormap in shader]
    CAM[Camera<br/>pan/zoom matrix] --> IMG
    IMG --> FB[canvas]
    OV[OverlayProgram<br/>centroid POINTS] --> FB
    JS2D[JS Canvas 2D<br/>clump boundaries] -.overlay.-> FB
```

## Interaction

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Panning: mousedown + drag
    Panning --> Idle: mouseup
    Idle --> Zooming: wheel
    Zooming --> Idle
    Idle --> HitTest: click
    HitTest --> Idle: canvasToImage -> pixmap[y*nx+x] -> clumpId
    Idle --> Resize: window resize
    Resize --> Idle: Viewer.resize -> camera.fit
    Idle --> ReParam: filter / stretch / RGB change
    ReParam --> Idle: renderSingle | renderRgb (re-stretch, no refetch planes)
```

## FFI surface (wasm_bindgen)

| JS name | Rust | Purpose |
|---|---|---|
| `new Viewer(id)` | `Viewer::new` | bind canvas, compile GL |
| `beginCube` | `begin_cube` | reset plane cache |
| `loadPlane` | `load_plane` | f32 LE → f64 plane |
| `renderSingle` | `render_single` | stretch + colormap |
| `renderRgb` | `render_rgb` | composite |
| `setCentroids` | `set_centroids` | upload POINTS |
| `setShowCentroids` | `set_show_centroids` | toggle |
| `resize` | `resize` | viewport + camera fit |
| `pan` / `zoom` | `pan` / `zoom_about` | camera |
| `canvasToImage` / `imageToCanvas` | `canvas_to_image` / `image_to_canvas` | hit-test + overlay anchor |
| `render` | `render` | redraw |
