# Components

Cargo workspace. Three crates. One web shell.

## Workspace

```mermaid
flowchart TB
    subgraph Workspace[Cargo workspace]
        core[jelly-core<br/>pure f64 math<br/>stretch + composite]
        bake[bake<br/>native CLI<br/>fitrs + serde + thiserror]
        viewer[viewer<br/>cdylib -> WASM<br/>wasm-bindgen + web-sys]
    end
    core --> viewer
    web[web/<br/>index.html + glue.js + style.css] -->|loads| viewer
```

## Module map

```mermaid
flowchart LR
    subgraph bake_crate[crates/bake]
        b_main[main.rs<br/>orchestration]
        b_fits[fits.rs<br/>FITS reader]
        b_wcs[wcs.rs<br/>linear WCS affine]
        b_clumps[clumps.rs<br/>CSV + hull + grid]
        b_manifest[manifest.rs<br/>JSON emit + λ table]
        b_main --> b_fits
        b_main --> b_wcs
        b_main --> b_clumps
        b_main --> b_manifest
    end

    subgraph viewer_crate[crates/viewer]
        v_lib[lib.rs<br/>Viewer struct + wasm_bindgen]
        v_gl[gl.rs<br/>WebGL2: ImageProgram + OverlayProgram]
        v_cam[camera.rs<br/>pan/zoom matrix]
        v_sh[shaders/]
        v_lib --> v_gl
        v_lib --> v_cam
        v_gl --> v_sh
    end

    subgraph core_crate[crates/core]
        c_stretch[stretch.rs<br/>log + lupton-asinh]
        c_comp[composite.rs<br/>percentile + lupton RGB]
    end

    v_lib --> c_stretch
    v_lib --> c_comp
```

## Dependencies

| Crate | Deps | Notes |
|---|---|---|
| `jelly-core` | — | pure std f64 |
| `bake` | `fitrs`, `serde`, `serde_json`, `thiserror` | native only |
| `viewer` | `jelly-core`, `wasm-bindgen =0.2.100`, `web-sys`, `js-sys` | cdylib → wasm32-unknown-unknown |

`wasm-bindgen` pinned to installed CLI version — mismatch breaks runtime bindings.
