//! Reading `NIRCam` datacubes: a primary-HDU f64 cube with `FILTERn` names and
//! a rotation-free celestial WCS in the header. Header WCS keywords are pulled
//! raw here; the affine pixel-to-sky transform is derived in the `wcs` module.

use std::path::Path;

use fitrs::{Fits, FitsData, HeaderValue};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FitsError {
    #[error("cannot open FITS file {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },
    #[error("FITS file has no primary HDU")]
    NoPrimaryHdu,
    #[error("missing or non-integer header key {0}")]
    MissingInt(&'static str),
    #[error("missing celestial WCS keyword {0}")]
    MissingWcs(&'static str),
    #[error("expected {expected} filter names (NAXIS3), found {found}")]
    FilterCount { expected: usize, found: usize },
    #[error("primary HDU is not a 64-bit float cube (BITPIX must be -64)")]
    NotF64Cube,
    #[error("data length {len} does not match {nx}×{ny}×{n_filters}")]
    ShapeMismatch {
        len: usize,
        nx: usize,
        ny: usize,
        n_filters: usize,
    },
}

/// Raw celestial WCS keywords for a rotation-free, axis-aligned TAN projection.
/// `crpix` is 1-based as stored in FITS; the `wcs` module converts to 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WcsKeywords {
    pub crpix: [f64; 2],
    pub crval: [f64; 2],
    /// Diagonal `PCi_i * CDELTi` in degrees/pixel (the linear scale per axis).
    pub scale: [f64; 2],
}

/// A datacube: `n_filters` slices of `ny × nx`, flat row-major
/// (`data[f*nx*ny + y*nx + x]`), plus filter names and the celestial WCS.
#[derive(Debug, Clone)]
pub struct Cube {
    pub nx: usize,
    pub ny: usize,
    pub filters: Vec<String>,
    pub wcs: WcsKeywords,
    data: Vec<f64>,
}

impl Cube {
    /// One filter slice as `ny × nx` row-major, `data[y*nx + x]`.
    #[must_use]
    pub fn slice(&self, filter_index: usize) -> &[f64] {
        let plane = self.nx * self.ny;
        let start = filter_index * plane;
        &self.data[start..start + plane]
    }

    #[must_use]
    pub fn n_filters(&self) -> usize {
        self.filters.len()
    }
}

pub fn read_cube(path: &Path) -> Result<Cube, FitsError> {
    let fits = Fits::open(path).map_err(|source| FitsError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let hdu = fits.get(0).ok_or(FitsError::NoPrimaryHdu)?;

    let nx = header_int(&hdu, "NAXIS1")?;
    let ny = header_int(&hdu, "NAXIS2")?;
    let n_filters = header_int(&hdu, "NAXIS3")?;

    let filters = read_filter_names(&hdu, n_filters)?;
    let wcs = read_wcs(&hdu)?;

    let data = match hdu.read_data() {
        FitsData::FloatingPoint64(arr) => arr.data,
        _ => return Err(FitsError::NotF64Cube),
    };
    let expected = nx * ny * n_filters;
    if data.len() != expected {
        return Err(FitsError::ShapeMismatch {
            len: data.len(),
            nx,
            ny,
            n_filters,
        });
    }

    Ok(Cube {
        nx,
        ny,
        filters,
        wcs,
        data,
    })
}

fn read_filter_names(hdu: &fitrs::Hdu, n_filters: usize) -> Result<Vec<String>, FitsError> {
    let mut filters = Vec::with_capacity(n_filters);
    for i in 1..=n_filters {
        let key = format!("FILTER{i}");
        match hdu.value(&key) {
            Some(HeaderValue::CharacterString(s)) => filters.push(s.trim().to_string()),
            _ => break,
        }
    }
    if filters.len() != n_filters {
        return Err(FitsError::FilterCount {
            expected: n_filters,
            found: filters.len(),
        });
    }
    Ok(filters)
}

fn read_wcs(hdu: &fitrs::Hdu) -> Result<WcsKeywords, FitsError> {
    Ok(WcsKeywords {
        crpix: [header_wcs(hdu, "CRPIX1")?, header_wcs(hdu, "CRPIX2")?],
        crval: [header_wcs(hdu, "CRVAL1")?, header_wcs(hdu, "CRVAL2")?],
        // CDELTi is 1.0 in these cubes; the real scale lives in the PC diagonal.
        scale: [
            header_wcs(hdu, "PC1_1")? * header_wcs(hdu, "CDELT1")?,
            header_wcs(hdu, "PC2_2")? * header_wcs(hdu, "CDELT2")?,
        ],
    })
}

fn header_int(hdu: &fitrs::Hdu, key: &'static str) -> Result<usize, FitsError> {
    match hdu.value(key) {
        Some(HeaderValue::IntegerNumber(n)) => usize::try_from(*n).ok(),
        _ => None,
    }
    .ok_or(FitsError::MissingInt(key))
}

fn header_wcs(hdu: &fitrs::Hdu, key: &'static str) -> Result<f64, FitsError> {
    match hdu.value(key) {
        Some(HeaderValue::RealFloatingNumber(v)) => Ok(*v),
        Some(HeaderValue::IntegerNumber(n)) => Ok(f64::from(*n)),
        _ => Err(FitsError::MissingWcs(key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ponytail: reads the sample cube via the gitignored `data/` symlink; test
    // skips cleanly when absent. Add a checked-in tiny fixture cube if CI needs it.
    fn sample() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/A2744_F1228/cut_datacube_nircam.fits")
    }

    #[test]
    fn reads_shape_filters_and_a_known_value() {
        let Ok(cube) = read_cube(&sample()) else {
            eprintln!("sample cube unavailable; skipping");
            return;
        };
        assert_eq!((cube.nx, cube.ny, cube.n_filters()), (172, 221, 20));
        assert_eq!(cube.filters[7], "F200W");

        // Value pinned from astropy: data[7,110,85] in FITS (filter,row,col) order.
        let f200w = cube.slice(7);
        let v = f200w[110 * cube.nx + 85];
        assert!((v - 0.292_062_729_597_511_84).abs() < 1e-12, "got {v}");
    }

    #[test]
    fn extracts_rotation_free_wcs() {
        let Ok(cube) = read_cube(&sample()) else {
            return;
        };
        let w = cube.wcs;
        assert!((w.crval[0] - 3.5875).abs() < 1e-9);
        assert!((w.crval[1] - (-30.396_666_7)).abs() < 1e-9);
        assert!((w.scale[0] - (-5.555_555_555_555_5e-6)).abs() < 1e-18);
    }
}
