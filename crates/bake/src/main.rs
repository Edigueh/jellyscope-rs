#![forbid(unsafe_code)]

//! `bake`: turn a `data/` tree of `NIRCam` cubes + clump CSVs into a static
//! `dist/` of raw f32 flux planes and a `manifest.json` for the WASM viewer.
//! The viewer stretches/composites/colormaps live from these planes.
//!
//! Usage: `bake [DATA_DIR] [DIST_DIR]` (defaults: `data`, `dist`).

mod clumps;
mod fits;
mod manifest;
mod wcs;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use thiserror::Error;

use clumps::ClumpCatalog;
use fits::Cube;
use manifest::{Clump, Dataset, Filter, Manifest, Wcs};
use wcs::Affine;

#[derive(Debug, Error)]
enum BakeError {
    #[error("read {path}: {source}")]
    ReadDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Fits {
        path: PathBuf,
        source: fits::FitsError,
    },
    #[error("{dataset} clumps: {source}")]
    Clumps {
        dataset: String,
        source: clumps::ClumpError,
    },
    #[error("serialize manifest: {0}")]
    SerializeManifest(#[from] serde_json::Error),
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let data_dir = PathBuf::from(args.next().unwrap_or_else(|| "data".into()));
    let dist_dir = PathBuf::from(args.next().unwrap_or_else(|| "dist".into()));

    match run(&data_dir, &dist_dir) {
        Ok(n) => {
            println!("baked {n} cube(s) into {}", dist_dir.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bake failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(data_dir: &Path, dist_dir: &Path) -> Result<usize, BakeError> {
    let mut datasets = Vec::new();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(data_dir)
        .map_err(|source| BakeError::ReadDir {
            path: data_dir.to_path_buf(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    let mut cube_count = 0;
    for dir in entries {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let cubes = bake_dataset(&dir, &name, dist_dir)?;
        cube_count += cubes.len();
        if !cubes.is_empty() {
            datasets.push(Dataset { name, cubes });
        }
    }

    let manifest = Manifest { datasets };
    let json = serde_json::to_string_pretty(&manifest)?;
    let manifest_path = dist_dir.join("manifest.json");
    std::fs::create_dir_all(dist_dir).map_err(|source| BakeError::Write {
        path: dist_dir.to_path_buf(),
        source,
    })?;
    std::fs::write(&manifest_path, json).map_err(|source| BakeError::Write {
        path: manifest_path,
        source,
    })?;
    Ok(cube_count)
}

fn bake_dataset(
    dir: &Path,
    dataset: &str,
    dist_dir: &Path,
) -> Result<Vec<manifest::Cube>, BakeError> {
    let props_csv = dir.join("clumps_properties.csv");
    let pixels_csv = dir.join("clumps_pixels.csv");

    let mut fits_paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| BakeError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "fits"))
        .collect();
    fits_paths.sort();

    let mut cubes = Vec::new();
    for fits_path in fits_paths {
        let cube = fits::read_cube(&fits_path).map_err(|source| BakeError::Fits {
            path: fits_path.clone(),
            source,
        })?;
        let catalog = ClumpCatalog::load(&props_csv, &pixels_csv, cube.nx, cube.ny).map_err(
            |source| BakeError::Clumps {
                dataset: dataset.to_string(),
                source,
            },
        )?;
        let cube_name = fits_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let out_dir = dist_dir.join(dataset).join(&cube_name);
        std::fs::create_dir_all(&out_dir).map_err(|source| BakeError::Write {
            path: out_dir.clone(),
            source,
        })?;

        cubes.push(bake_cube(&cube, &catalog, dataset, &cube_name, &out_dir)?);
    }
    Ok(cubes)
}

fn bake_cube(
    cube: &Cube,
    catalog: &ClumpCatalog,
    dataset: &str,
    cube_name: &str,
    out_dir: &Path,
) -> Result<manifest::Cube, BakeError> {
    let affine = Affine::from_keywords(&cube.wcs);
    let rel = |file: &str| format!("{dataset}/{cube_name}/{file}");

    let filters = write_flux_planes(cube, out_dir, &rel)?;
    let rgb = manifest::default_rgb(&cube.filters);
    write_pixmap(catalog, out_dir)?;

    let clumps = catalog
        .ids()
        .into_iter()
        .filter_map(|id| build_clump(catalog, id, &affine))
        .collect();

    Ok(manifest::Cube {
        name: cube_name.to_string(),
        nx: cube.nx,
        ny: cube.ny,
        filters,
        rgb_default: rgb,
        wcs: Wcs {
            crpix: affine.crpix,
            crval: affine.crval,
            scale: affine.scale,
            cos_dec: affine.cos_dec,
            pixel_scale_arcsec: affine.pixel_scale_arcsec(),
        },
        clumps,
        pixel_clump: rel("pixmap.i32"),
    })
}

// Per-filter raw flux planes (f32 little-endian). The viewer widens these to
// f64 and runs the same stretch/composite code, so display is fully live.
fn write_flux_planes(
    cube: &Cube,
    out_dir: &Path,
    rel: &impl Fn(&str) -> String,
) -> Result<Vec<Filter>, BakeError> {
    let mut filters = Vec::with_capacity(cube.n_filters());
    for (i, fname) in cube.filters.iter().enumerate() {
        write_bytes(
            &out_dir.join(format!("{fname}.f32")),
            &flux_f32_le(cube.slice(i)),
        )?;
        filters.push(Filter {
            name: fname.clone(),
            wavelength_um: manifest::wavelength_of(fname),
            texture_flux: rel(&format!("{fname}.f32")),
        });
    }
    Ok(filters)
}

// Pixel→clump grid (i32 little-endian).
fn write_pixmap(catalog: &ClumpCatalog, out_dir: &Path) -> Result<(), BakeError> {
    let pixmap: Vec<u8> = catalog
        .pixel_clump_grid()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    write_bytes(&out_dir.join("pixmap.i32"), &pixmap)
}

fn build_clump(catalog: &ClumpCatalog, id: i64, affine: &Affine) -> Option<Clump> {
    let p = catalog.properties(id)?;
    let (ra, dec) = affine.pixel_to_radec(p.x0, p.y0);
    let boundary = catalog
        .boundary(id)
        .into_iter()
        .map(|(x, y)| [x, y])
        .collect();
    Some(Clump {
        id,
        boundary,
        x0: p.x0,
        y0: p.y0,
        ra_deg: ra,
        dec_deg: dec,
        component: p.component.clone(),
        area_pix: p.area_pix,
        area_arcsec2: p.area_arcsec2,
        r_eff_arcsec: p.r_eff_arcsec,
        area_kpc2: p.area_kpc2,
        r_eff_kpc: p.r_eff_kpc,
        inside: p.inside,
    })
}

/// Flux plane (row-major f64, `NaN` for invalid) → little-endian f32 bytes.
/// f32 halves the payload; the viewer widens back to f64 before the math, so
/// `NaN` is preserved and output matches a pure-f64 bake within one u8 level.
#[allow(clippy::cast_possible_truncation)] // f64 flux -> f32 storage, by design
fn flux_f32_le(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for &v in values {
        out.extend_from_slice(&(v as f32).to_le_bytes());
    }
    out
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), BakeError> {
    std::fs::write(path, bytes).map_err(|source| BakeError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ponytail: bakes the real sample dataset via the gitignored `data/` symlink
    // into a throwaway dir; skips when the data is absent.
    #[test]
    fn bake_smoke_produces_valid_textures() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/A2744_F1228");
        if !data.join("cut_datacube_nircam.fits").exists() {
            return;
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dist = std::env::temp_dir().join("jellyscope_bake_smoke");
        let _ = std::fs::remove_dir_all(&dist);

        let n = run(&src, &dist).expect("bake runs");
        assert!(n >= 1, "at least one cube baked");

        let json = std::fs::read_to_string(dist.join("manifest.json")).unwrap();
        let m: serde_json::Value = serde_json::from_str(&json).expect("manifest is valid JSON");
        let cube = &m["datasets"][0]["cubes"][0];
        let (nx, ny) = (cube["nx"].as_u64().unwrap(), cube["ny"].as_u64().unwrap());
        let tex = cube["filters"][0]["texture_flux"].as_str().unwrap();
        let bytes = std::fs::metadata(dist.join(tex)).unwrap().len();
        assert_eq!(bytes, nx * ny * 4, "flux plane is nx*ny*4 f32 bytes");

        let _ = std::fs::remove_dir_all(&dist);
    }
}
