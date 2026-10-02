# Build pipeline (bake)

CLI entry: `bake [DATA_DIR] [DIST_DIR]`. Default `data` → `dist`.

## Directory walk

```mermaid
flowchart TD
    A[data/] --> B{dir entry}
    B -->|dataset dir| C[bake_dataset]
    C --> D{*.fits files}
    D --> E[read_cube]
    E --> F[ClumpCatalog::load<br/>properties.csv + pixels.csv]
    F --> G[bake_cube]
    G --> H[write f32 planes<br/>per filter]
    G --> I[write pixmap.i32]
    G --> J[build Clump list<br/>hull + WCS RA/Dec]
    H --> K[Dataset entry]
    I --> K
    J --> K
    K --> L[Manifest]
    L --> M[dist/manifest.json]
```

## Per-cube bake

```mermaid
sequenceDiagram
    autonumber
    participant main
    participant fits as fits.rs
    participant clumps as clumps.rs
    participant wcs as wcs.rs
    participant out as dist/{dataset}/{cube}/

    main->>fits: read_cube(path)
    fits-->>main: Cube{nx, ny, filters, wcs, data}
    main->>clumps: ClumpCatalog::load(props, pixels, nx, ny)
    clumps-->>main: catalog (hull + pixel grid)
    main->>wcs: Affine::from_keywords(cube.wcs)
    loop per filter
        main->>out: write {FILTER}.f32 (f64 -> f32 LE)
    end
    main->>out: write pixmap.i32 (i32 LE row-major)
    main->>main: build_clump per id (RA/Dec via affine)
    main-->>main: Cube manifest entry
```

## Outputs per cube

```
dist/{dataset}/{cube}/
├── F090W.f32      nx*ny*4 bytes, f32 LE, row-major (NaN = invalid)
├── F115W.f32
├── ...
└── pixmap.i32     nx*ny*4 bytes, i32 LE, pixel -> clump id (-1 = none)
```

Plus top-level `dist/manifest.json`.
