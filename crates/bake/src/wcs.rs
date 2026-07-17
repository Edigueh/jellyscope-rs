//! The linear pixel-to-sky transform for a rotation-free, axis-aligned TAN WCS,
//! ported from the Python `wcs_affine_params` / `skycoord_separation_arcsec`.
//!
//! Exact only when the WCS has no rotation/skew (diagonal PC, no CD/SIP); the
//! cut cubes satisfy that to ~16 mas over the field, which the viewer tolerates.
#![allow(dead_code)] // consumed by manifest.rs / main.rs at the orchestration step

use crate::fits::WcsKeywords;

/// Linear pixel-to-RA/Dec parameters with `crpix` already 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub crpix: [f64; 2],
    pub crval: [f64; 2],
    pub scale: [f64; 2],
    pub cos_dec: f64,
}

impl Affine {
    /// Build from raw FITS keywords: shift `crpix` to 0-based and precompute
    /// `cos(dec0)` (the RA-axis foreshortening factor).
    #[must_use]
    pub fn from_keywords(k: &WcsKeywords) -> Self {
        Self {
            crpix: [k.crpix[0] - 1.0, k.crpix[1] - 1.0],
            crval: k.crval,
            scale: k.scale,
            cos_dec: k.crval[1].to_radians().cos(),
        }
    }

    /// Pixel (0-based) to (RA, Dec) in degrees.
    /// `RA = crval_ra + scale_x*(x - crpix_x)/cos(dec0)`, `Dec` analogous.
    #[must_use]
    pub fn pixel_to_radec(&self, x: f64, y: f64) -> (f64, f64) {
        let ra = self.crval[0] + self.scale[0] * (x - self.crpix[0]) / self.cos_dec;
        let dec = self.crval[1] + self.scale[1] * (y - self.crpix[1]);
        (ra, dec)
    }

    /// Mean linear pixel scale in arcsec/pixel — matches astropy's
    /// `proj_plane_pixel_scales` (the raw scale, before the cos(dec) division).
    #[must_use]
    pub fn pixel_scale_arcsec(&self) -> f64 {
        f64::midpoint(self.scale[0].abs(), self.scale[1].abs()) * 3600.0
    }
}

/// Great-circle angular separation between two sky points, in arcsec.
/// Haversine form — numerically stable for the small separations here.
#[must_use]
pub fn separation_arcsec(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (r1, d1) = (ra1.to_radians(), dec1.to_radians());
    let (r2, d2) = (ra2.to_radians(), dec2.to_radians());
    let (dr, dd) = (r2 - r1, d2 - d1);
    let a = (dd / 2.0).sin().powi(2) + d1.cos() * d2.cos() * (dr / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    c.to_degrees() * 3600.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_wcs() -> WcsKeywords {
        WcsKeywords {
            crpix: [-2710.5, -2097.5],
            crval: [3.5875, -30.396_666_7],
            scale: [-5.555_555_555_555_5e-6, 5.555_555_555_555_5e-6],
        }
    }

    #[test]
    fn affine_matches_python_golden() {
        let a = Affine::from_keywords(&sample_wcs());
        // Golden RA/Dec captured from the Python wcs_affine_params transform.
        let cases = [
            (0.0, 0.0, 3.570_035_494_4, -30.385_008_366_7),
            (100.0, 50.0, 3.569_391_404_3, -30.384_730_588_9),
            (85.0, 110.0, 3.569_488_017_8, -30.384_397_255_6),
        ];
        for (x, y, ra_want, dec_want) in cases {
            let (ra, dec) = a.pixel_to_radec(x, y);
            assert!(
                (ra - ra_want).abs() < 1e-9,
                "RA pix({x},{y}): {ra} vs {ra_want}"
            );
            assert!(
                (dec - dec_want).abs() < 1e-9,
                "Dec pix({x},{y}): {dec} vs {dec_want}"
            );
        }
    }

    #[test]
    fn pixel_scale_is_20_mas() {
        let a = Affine::from_keywords(&sample_wcs());
        assert!((a.pixel_scale_arcsec() - 0.02).abs() < 1e-9);
    }

    #[test]
    fn separation_matches_astropy_golden() {
        // Same two points as the Python separation golden (1.2369407 arcsec).
        let sep = separation_arcsec(
            3.569_488_017_8,
            -30.384_397_255_6,
            3.569_391_404_3,
            -30.384_730_588_9,
        );
        assert!((sep - 1.236_940_7).abs() < 1e-4, "sep {sep}");
    }
}
