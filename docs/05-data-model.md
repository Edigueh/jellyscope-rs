# Data model

Single contract: `manifest.json` + binary sidecar files.

## Manifest schema

```mermaid
classDiagram
    class Manifest {
        datasets: Dataset[]
    }
    class Dataset {
        name: string
        cubes: Cube[]
    }
    class Cube {
        name: string
        nx: usize
        ny: usize
        filters: Filter[]
        rgb_default: RgbFilters
        wcs: Wcs
        clumps: Clump[]
        pixel_clump: string
    }
    class Filter {
        name: string
        wavelength_um: f64?
        texture_flux: string
    }
    class RgbFilters {
        r: string
        g: string
        b: string
    }
    class Wcs {
        crpix: [f64, f64]
        crval: [f64, f64]
        scale: [f64, f64]
        cos_dec: f64
        pixel_scale_arcsec: f64
    }
    class Clump {
        id: i64
        boundary: [f64, f64][]
        x0: f64
        y0: f64
        ra_deg: f64
        dec_deg: f64
        component: string
        area_pix: i64
        area_arcsec2: f64
        r_eff_arcsec: f64
        area_kpc2: f64
        r_eff_kpc: f64
        inside: bool
    }
    Manifest "1" --> "*" Dataset
    Dataset "1" --> "*" Cube
    Cube "1" --> "*" Filter
    Cube "1" --> "1" RgbFilters
    Cube "1" --> "1" Wcs
    Cube "1" --> "*" Clump
```

## File layout

```
dist/
├── manifest.json
└── {dataset}/
    └── {cube_name}/
        ├── {FILTER}.f32    (per filter: nx*ny f32 LE, row-major, NaN = invalid)
        └── pixmap.i32      (nx*ny i32 LE, pixel -> clump id, -1 = none)
```

## Binary encodings

| File | Element | Order | Invalid |
|---|---|---|---|
| `*.f32` | f32 LE | row-major (y outer, x inner) | NaN |
| `pixmap.i32` | i32 LE | row-major | -1 |

## Default-RGB resolution

```mermaid
flowchart TD
    A[filters list] --> B{F200W + F115W + F090W all present?}
    B -->|yes| C[R=F200W G=F115W B=F090W]
    B -->|no| D[sort by wavelength]
    D --> E{>=3 known λ?}
    E -->|yes| F[R=longest, B=shortest<br/>G=nearest midpoint]
    E -->|no| G[R=last, G=middle, B=first by position]
```

## NIRCam wavelength table (µm)

F070W 0.704, F090W 0.901, F115W 1.154, F140M 1.404, F150W 1.501, F162M 1.627,
F182M 1.845, F200W 1.990, F210M 2.093, F250M 2.503, F277W 2.786, F300M 2.996,
F335M 3.365, F356W 3.563, F360M 3.621, F410M 4.092, F430M 4.280, F444W 4.421,
F460M 4.624, F480M 4.834.
