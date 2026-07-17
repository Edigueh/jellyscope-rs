#![forbid(unsafe_code)]

//! `bake`: turn a `data/` tree of `NIRCam` cubes + clump CSVs into a static
//! `dist/` of raw RGBA textures and a `manifest.json` for the WASM viewer.
//!
//! Usage: `bake [DATA_DIR] [DIST_DIR]` (defaults: `data`, `dist`).

mod clumps;
mod composite;
mod fits;
mod manifest;
mod stretch;
mod wcs;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clumps::ClumpCatalog;
use fits::Cube;
use manifest::{Clump, Dataset, Filter, Manifest, Wcs};
use wcs::Affine;

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

fn run(data_dir: &Path, dist_dir: &Path) -> Result<usize, String> {
    let mut datasets = Vec::new();
    let mut cube_count = 0;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(data_dir)
        .map_err(|e| format!("read {}: {e}", data_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    for dir in entries {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let cubes = bake_dataset(&dir, &name, dist_dir, &mut cube_count)?;
        if !cubes.is_empty() {
            datasets.push(Dataset { name, cubes });
        }
    }

    let manifest = Manifest { datasets };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dist_dir).map_err(|e| e.to_string())?;
    std::fs::write(dist_dir.join("manifest.json"), json)
        .map_err(|e| format!("write manifest: {e}"))?;
    Ok(cube_count)
}

fn bake_dataset(
    dir: &Path,
    dataset: &str,
    dist_dir: &Path,
    cube_count: &mut usize,
) -> Result<Vec<manifest::Cube>, String> {
    let props_csv = dir.join("clumps_properties.csv");
    let pixels_csv = dir.join("clumps_pixels.csv");

    let mut fits_paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "fits"))
        .collect();
    fits_paths.sort();

    let mut cubes = Vec::new();
    for fits_path in fits_paths {
        let cube =
            fits::read_cube(&fits_path).map_err(|e| format!("{}: {e}", fits_path.display()))?;
        let catalog = ClumpCatalog::load(&props_csv, &pixels_csv, cube.nx, cube.ny)
            .map_err(|e| format!("{dataset} clumps: {e}"))?;
        let cube_name = fits_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let out_dir = dist_dir.join(dataset).join(&cube_name);
        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

        cubes.push(bake_cube(&cube, &catalog, dataset, &cube_name, &out_dir)?);
        *cube_count += 1;
    }
    Ok(cubes)
}

fn bake_cube(
    cube: &Cube,
    catalog: &ClumpCatalog,
    dataset: &str,
    cube_name: &str,
    out_dir: &Path,
) -> Result<manifest::Cube, String> {
    let affine = Affine::from_keywords(&cube.wcs);
    let rel = |file: &str| format!("{dataset}/{cube_name}/{file}");

    // Per-filter grayscale textures, one per stretch.
    let mut filters = Vec::with_capacity(cube.n_filters());
    for (i, fname) in cube.filters.iter().enumerate() {
        let slice = cube.slice(i);
        write_bytes(
            &out_dir.join(format!("{fname}_log.rgba")),
            &gray_rgba(&stretch::log_stretch(slice)),
        )?;
        write_bytes(
            &out_dir.join(format!("{fname}_asinh.rgba")),
            &gray_rgba(&stretch::lupton_asinh_stretch(slice)),
        )?;
        filters.push(Filter {
            name: fname.clone(),
            wavelength_um: manifest::wavelength_of(fname),
            texture_log: rel(&format!("{fname}_log.rgba")),
            texture_asinh: rel(&format!("{fname}_asinh.rgba")),
        });
    }

    // RGB composite textures from the default R/G/B filters, one per recipe.
    let names = cube.filters.clone();
    let mut rgb = manifest::default_rgb(&names);
    let idx = |n: &str| names.iter().position(|f| f == n).unwrap_or(0);
    let (r, g, b) = (
        cube.slice(idx(&rgb.r)),
        cube.slice(idx(&rgb.g)),
        cube.slice(idx(&rgb.b)),
    );
    write_bytes(
        &out_dir.join("rgb_percentile.rgba"),
        &rgba(&composite::percentile_asinh(r, g, b)),
    )?;
    write_bytes(
        &out_dir.join("rgb_lupton.rgba"),
        &rgba(&composite::lupton(r, g, b)),
    )?;
    rgb.texture_percentile = rel("rgb_percentile.rgba");
    rgb.texture_lupton = rel("rgb_lupton.rgba");

    // Pixel→clump grid (i32 little-endian).
    let pixmap: Vec<u8> = catalog
        .pixel_clump_grid()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    write_bytes(&out_dir.join("pixmap.i32"), &pixmap)?;

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

/// Stretched scalar `[0,1]` (NaN = invalid) → grayscale RGBA. Viewer applies a
/// colormap; NaN pixels get alpha 0 so they read as transparent.
fn gray_rgba(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for &v in values {
        if v.is_nan() {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let g = (v.clamp(0.0, 1.0) * 255.0) as u8;
            out.extend_from_slice(&[g, g, g, 255]);
        }
    }
    out
}

/// Interleaved `RGB` bytes → `RGBA` (opaque; composites already blacken NaN).
fn rgba(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for chunk in rgb.chunks_exact(3) {
        out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
    }
    out
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
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
        let tex = cube["filters"][0]["texture_log"].as_str().unwrap();
        let bytes = std::fs::metadata(dist.join(tex)).unwrap().len();
        assert_eq!(bytes, nx * ny * 4, "texture is nx*ny*4 RGBA bytes");

        let _ = std::fs::remove_dir_all(&dist);
    }
}
