//! The `manifest.json` contract between `bake` and the WASM viewer, plus the
//! `NIRCam` wavelength table and default-RGB picker (ported from `config.py` and
//! `app.js`). Texture files are raw `RGBA` bytes, `nx*ny*4`, row-major.

use serde::Serialize;

/// `NIRCam` filter central wavelengths (µm), from `config.py::NIRCAM_WAVELENGTHS`.
pub const NIRCAM_WAVELENGTHS: &[(&str, f64)] = &[
    ("F070W", 0.704),
    ("F090W", 0.901),
    ("F115W", 1.154),
    ("F140M", 1.404),
    ("F150W", 1.501),
    ("F162M", 1.627),
    ("F182M", 1.845),
    ("F200W", 1.990),
    ("F210M", 2.093),
    ("F250M", 2.503),
    ("F277W", 2.786),
    ("F300M", 2.996),
    ("F335M", 3.365),
    ("F356W", 3.563),
    ("F360M", 3.621),
    ("F410M", 4.092),
    ("F430M", 4.280),
    ("F444W", 4.421),
    ("F460M", 4.624),
    ("F480M", 4.834),
];

#[must_use]
pub fn wavelength_of(filter: &str) -> Option<f64> {
    NIRCAM_WAVELENGTHS
        .iter()
        .find(|(name, _)| *name == filter)
        .map(|(_, wl)| *wl)
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub datasets: Vec<Dataset>,
}

#[derive(Debug, Serialize)]
pub struct Dataset {
    pub name: String,
    pub cubes: Vec<Cube>,
}

#[derive(Debug, Serialize)]
pub struct Cube {
    pub name: String,
    pub nx: usize,
    pub ny: usize,
    pub filters: Vec<Filter>,
    pub rgb_default: RgbFilters,
    pub wcs: Wcs,
    pub clumps: Vec<Clump>,
    /// Path (relative to the manifest) of the `i32` pixel→clump-id grid.
    pub pixel_clump: String,
}

#[derive(Debug, Serialize)]
pub struct Filter {
    pub name: String,
    pub wavelength_um: Option<f64>,
    /// Grayscale RGBA texture paths, one per stretch.
    pub texture_log: String,
    pub texture_asinh: String,
}

#[derive(Debug, Serialize)]
pub struct RgbFilters {
    pub r: String,
    pub g: String,
    pub b: String,
    /// RGBA composite texture paths, one per recipe.
    pub texture_percentile: String,
    pub texture_lupton: String,
}

#[derive(Debug, Serialize)]
pub struct Wcs {
    pub crpix: [f64; 2],
    pub crval: [f64; 2],
    pub scale: [f64; 2],
    pub cos_dec: f64,
    pub pixel_scale_arcsec: f64,
}

#[derive(Debug, Serialize)]
pub struct Clump {
    pub id: i64,
    pub boundary: Vec<[f64; 2]>,
    pub x0: f64,
    pub y0: f64,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub component: String,
    pub area_pix: i64,
    pub area_arcsec2: f64,
    pub r_eff_arcsec: f64,
    pub area_kpc2: f64,
    pub r_eff_kpc: f64,
    pub inside: bool,
}

/// Pick default R/G/B filter names by wavelength rank, mirroring
/// `app.js::resolveRgbDefaults`: prefer the named F200W/F115W/F090W; otherwise
/// R = longest λ, B = shortest λ, G = nearest to the λ midpoint. Falls back to
/// position (last, middle, first) when fewer than three filters carry a λ.
#[must_use]
pub fn default_rgb(filters: &[String]) -> RgbFilters {
    let named = |want: &str| {
        filters
            .iter()
            .any(|f| f == want && wavelength_of(f).is_some())
    };
    let (mut r, mut g, mut b) = (
        named("F200W").then(|| "F200W".to_string()),
        named("F115W").then(|| "F115W".to_string()),
        named("F090W").then(|| "F090W".to_string()),
    );

    if r.is_none() || g.is_none() || b.is_none() {
        let mut known: Vec<(&String, f64)> = filters
            .iter()
            .filter_map(|f| wavelength_of(f).map(|wl| (f, wl)))
            .collect();
        known.sort_by(|a, b| a.1.total_cmp(&b.1));

        if known.len() >= 3 {
            let (lo, hi) = (known[0], known[known.len() - 1]);
            let mid = f64::midpoint(lo.1, hi.1);
            let mid_pick = known
                .iter()
                .min_by(|a, b| (a.1 - mid).abs().total_cmp(&(b.1 - mid).abs()))
                .unwrap()
                .0;
            r = r.or_else(|| Some(hi.0.clone()));
            b = b.or_else(|| Some(lo.0.clone()));
            g = g.or_else(|| Some(mid_pick.clone()));
        } else {
            // Position fallback: R = last, G = middle, B = first.
            let last = filters.last().cloned().unwrap_or_default();
            let mid = filters.get(filters.len() / 2).cloned().unwrap_or_default();
            let first = filters.first().cloned().unwrap_or_default();
            r = r.or(Some(last));
            g = g.or(Some(mid));
            b = b.or(Some(first));
        }
    }

    RgbFilters {
        r: r.unwrap_or_default(),
        g: g.unwrap_or_default(),
        b: b.unwrap_or_default(),
        texture_percentile: String::new(),
        texture_lupton: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_named_rgb() {
        let filters: Vec<String> = ["F090W", "F115W", "F150W", "F200W"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let rgb = default_rgb(&filters);
        assert_eq!(
            (rgb.r.as_str(), rgb.g.as_str(), rgb.b.as_str()),
            ("F200W", "F115W", "F090W")
        );
    }

    #[test]
    fn falls_back_to_wavelength_rank() {
        // No named triple present; expect R=longest, B=shortest, G≈midpoint.
        let filters: Vec<String> = ["F140M", "F162M", "F444W"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let rgb = default_rgb(&filters);
        assert_eq!(rgb.r, "F444W"); // longest λ
        assert_eq!(rgb.b, "F140M"); // shortest λ
        assert_eq!(rgb.g, "F162M"); // only remaining → nearest midpoint
    }
}
