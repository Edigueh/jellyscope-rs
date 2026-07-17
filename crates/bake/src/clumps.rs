//! Clump catalog: properties from `clumps_properties.csv`, per-pixel assignment
//! from `clumps_pixels.csv`. Ported from Python `clumps.py`. Produces convex-hull
//! boundary polygons (via Andrew's monotone chain) and a pixel→clump-id grid.
#![allow(dead_code)]
// consumed by manifest.rs / main.rs at the orchestration step
// Casts here move between clump ids and pixel indices — all bounded by the cube
// size (< 65k), so truncation/sign loss cannot occur in practice.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClumpError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}:{line}: malformed CSV row")]
    BadRow { path: String, line: usize },
    #[error("{path} missing expected column {column}")]
    MissingColumn { path: String, column: String },
}

/// Physical + geometric metadata for one clump (mirrors `ClumpProperties`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClumpProperties {
    pub clump_id: i64,
    pub area_pix: i64,
    pub area_arcsec2: f64,
    pub r_eff_arcsec: f64,
    pub x0: f64,
    pub y0: f64,
    pub area_kpc2: f64,
    pub r_eff_kpc: f64,
    pub inside: bool,
    pub component: String,
}

/// The catalog: properties keyed by id (sorted), a pixel→id grid (`-1` = empty,
/// row-major `ny × nx`), and the pixels assigned to each clump.
#[derive(Debug, Clone)]
pub struct ClumpCatalog {
    pub nx: usize,
    pub ny: usize,
    props: BTreeMap<i64, ClumpProperties>,
    pixel_clump: Vec<i32>,
    pixels: BTreeMap<i64, Vec<(u32, u32)>>,
}

impl ClumpCatalog {
    pub fn load(
        properties_csv: &Path,
        pixels_csv: &Path,
        nx: usize,
        ny: usize,
    ) -> Result<Self, ClumpError> {
        let props = read_properties(properties_csv)?;
        let (pixel_clump, pixels) = read_pixels(pixels_csv, nx, ny)?;
        Ok(Self {
            nx,
            ny,
            props,
            pixel_clump,
            pixels,
        })
    }

    #[must_use]
    pub fn ids(&self) -> Vec<i64> {
        self.props.keys().copied().collect()
    }

    #[must_use]
    pub fn properties(&self, id: i64) -> Option<&ClumpProperties> {
        self.props.get(&id)
    }

    /// Clump id at pixel `(x, y)`, or `None` if empty / out of bounds.
    #[must_use]
    pub fn id_at_pixel(&self, x: usize, y: usize) -> Option<i64> {
        if x >= self.nx || y >= self.ny {
            return None;
        }
        match self.pixel_clump[y * self.nx + x] {
            -1 => None,
            v => Some(i64::from(v)),
        }
    }

    #[must_use]
    pub fn pixel_clump_grid(&self) -> &[i32] {
        &self.pixel_clump
    }

    /// Closed convex-hull boundary polygon in pixel coords (last point repeats
    /// the first). Clumps with < 3 pixels return the points themselves, closed —
    /// matching the Python fallback.
    #[must_use]
    pub fn boundary(&self, id: i64) -> Vec<(f64, f64)> {
        let Some(px) = self.pixels.get(&id) else {
            return Vec::new();
        };
        let points: Vec<(f64, f64)> = px
            .iter()
            .map(|&(x, y)| (f64::from(x), f64::from(y)))
            .collect();
        let mut hull = convex_hull(&points);
        if let Some(&first) = hull.first() {
            hull.push(first); // close the loop
        }
        hull
    }
}

fn read_properties(path: &Path) -> Result<BTreeMap<i64, ClumpProperties>, ClumpError> {
    let text = read_to_string(path)?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let cols = column_index(header);

    let mut out = BTreeMap::new();
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        let get = |c: &str| f.get(cols[c]).copied().unwrap_or_default().trim();
        let bad = || ClumpError::BadRow {
            path: path.display().to_string(),
            line: n + 2,
        };
        let p = ClumpProperties {
            clump_id: get("clump_id").parse().map_err(|_| bad())?,
            area_pix: get("area_pix").parse().map_err(|_| bad())?,
            area_arcsec2: get("area_arcsec2").parse().map_err(|_| bad())?,
            r_eff_arcsec: get("r_eff_arcsec").parse().map_err(|_| bad())?,
            x0: get("x0").parse().map_err(|_| bad())?,
            y0: get("y0").parse().map_err(|_| bad())?,
            area_kpc2: get("area_kpc2").parse().map_err(|_| bad())?,
            r_eff_kpc: get("r_eff_kpc").parse().map_err(|_| bad())?,
            inside: parse_bool(get("inside")),
            component: get("component").to_string(),
        };
        out.insert(p.clump_id, p);
    }
    Ok(out)
}

type PixelMap = (Vec<i32>, BTreeMap<i64, Vec<(u32, u32)>>);

fn read_pixels(path: &Path, nx: usize, ny: usize) -> Result<PixelMap, ClumpError> {
    let text = read_to_string(path)?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let cols = column_index(header);
    let (ci, xi, yi) = (cols["clump_id"], cols["x"], cols["y"]);

    let mut grid = vec![-1_i32; nx * ny];
    let mut pixels: BTreeMap<i64, Vec<(u32, u32)>> = BTreeMap::new();
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').collect();
        let bad = || ClumpError::BadRow {
            path: path.display().to_string(),
            line: n + 2,
        };
        let cid: i64 = f
            .get(ci)
            .ok_or_else(bad)?
            .trim()
            .parse()
            .map_err(|_| bad())?;
        let x: i64 = f
            .get(xi)
            .ok_or_else(bad)?
            .trim()
            .parse()
            .map_err(|_| bad())?;
        let y: i64 = f
            .get(yi)
            .ok_or_else(bad)?
            .trim()
            .parse()
            .map_err(|_| bad())?;
        if x < 0 || y < 0 || x as usize >= nx || y as usize >= ny {
            continue; // out-of-bounds pixels are dropped, as in the Python
        }
        let (xu, yu) = (x as usize, y as usize);
        grid[yu * nx + xu] = i32::try_from(cid).unwrap_or(-1);
        pixels.entry(cid).or_default().push((xu as u32, yu as u32));
    }
    Ok((grid, pixels))
}

/// Convex hull via Andrew's monotone chain, returning vertices counter-clockwise.
/// Degenerate inputs (< 3 points, or all collinear) return the sorted points.
fn convex_hull(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }

    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn read_to_string(path: &Path) -> Result<String, ClumpError> {
    std::fs::read_to_string(path).map_err(|source| ClumpError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn column_index(header: &str) -> BTreeMap<String, usize> {
    header
        .split(',')
        .enumerate()
        .map(|(i, name)| (name.trim().to_string(), i))
        .collect()
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim(), "True" | "true" | "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/A2744_F1228")
    }

    fn load() -> Option<ClumpCatalog> {
        // Cube is 172×221; clump pixels index into that grid.
        ClumpCatalog::load(
            &dir().join("clumps_properties.csv"),
            &dir().join("clumps_pixels.csv"),
            172,
            221,
        )
        .ok()
    }

    #[test]
    fn loads_properties() {
        let Some(cat) = load() else { return };
        let c0 = cat.properties(0).expect("clump 0");
        assert_eq!(c0.area_pix, 121);
        assert_eq!(c0.component, "outside");
        assert!(!c0.inside);
        let c1 = cat.properties(1).expect("clump 1");
        assert_eq!(c1.component, "disk");
        assert!(c1.inside);
    }

    #[test]
    fn hull_matches_scipy_as_a_set() {
        let Some(cat) = load() else { return };
        // scipy ConvexHull of clump 0: 11 vertices, area 130.0.
        let want: std::collections::BTreeSet<(i64, i64)> = [
            (63, 17),
            (63, 13),
            (67, 13),
            (70, 14),
            (74, 17),
            (77, 20),
            (78, 22),
            (80, 27),
            (80, 29),
            (77, 29),
            (64, 18),
        ]
        .iter()
        .map(|&(x, y)| (x, y))
        .collect();

        let mut boundary = cat.boundary(0);
        boundary.pop(); // drop the repeated closing point
        let got: std::collections::BTreeSet<(i64, i64)> = boundary
            .iter()
            .map(|&(x, y)| (x as i64, y as i64))
            .collect();
        assert_eq!(got, want, "hull vertex set must match scipy");
        assert!((polygon_area(&cat.boundary(0)) - 130.0).abs() < 1e-6);
    }

    #[test]
    fn pixel_lookup_hits_and_misses() {
        let Some(cat) = load() else { return };
        assert_eq!(cat.id_at_pixel(63, 13), Some(0)); // first pixel of clump 0
        assert_eq!(cat.id_at_pixel(0, 0), None); // empty corner
        assert_eq!(cat.id_at_pixel(9999, 9999), None); // out of bounds
    }

    // Shoelace area of a closed polygon, for the hull-area check.
    fn polygon_area(poly: &[(f64, f64)]) -> f64 {
        let mut a = 0.0;
        for w in poly.windows(2) {
            a += w[0].0 * w[1].1 - w[1].0 * w[0].1;
        }
        a.abs() / 2.0
    }
}
