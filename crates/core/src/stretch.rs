//! Flux stretches for single-filter display, ported 1:1 from the Python
//! `image_viewer.py`. Each returns values in `[0, 1]` with `NaN` preserved for
//! invalid pixels. Parameters are fixed (as in the source); callers (`bake`
//! natively, `viewer` live in WASM) run the same code so output matches.

/// Sigma-clipped `(median, std)` over the finite values, matching astropy's
/// `sigma_clipped_stats(sigma=3, maxiters=5)`: center on the median, clip at
/// `median ± 3·std` (population std, ddof=0), iterate until nothing is clipped.
#[must_use]
pub fn estimate_background(data: &[f64]) -> (f64, f64) {
    let mut kept: Vec<f64> = data.iter().copied().filter(|v| v.is_finite()).collect();
    if kept.is_empty() {
        return (0.0, 1.0);
    }
    for _ in 0..5 {
        let med = median(&mut kept);
        let std = std_pop(&kept, mean(&kept));
        let (lo, hi) = (med - 3.0 * std, med + 3.0 * std);
        let before = kept.len();
        kept.retain(|&v| v >= lo && v <= hi);
        if kept.len() == before {
            break;
        }
    }
    (median(&mut kept), std_pop(&kept, mean(&kept)))
}

/// Default Lupton linear stretch factor from the background noise sigma.
#[must_use]
pub fn default_alpha(sigma: f64) -> f64 {
    0.02 / (sigma + 1e-10)
}

/// Log stretch (`image_viewer._log_stretch`): asymmetric-percentile clip
/// (10, 99.98), normalize, then `log(a·x + 1) / log(a + 1)` with `a = 200`.
/// `NaN`/non-positive pixels are excluded from the limits and map to 0.
#[must_use]
pub fn log_stretch(data: &[f64]) -> Vec<f64> {
    const A: f64 = 200.0;
    let valid: Vec<f64> = data
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if valid.is_empty() {
        return data.to_vec();
    }
    let vmin = percentile(&valid, 10.0);
    let vmax = percentile(&valid, 99.98);
    let span = vmax - vmin + 1e-10;
    let denom = (A + 1.0).ln();
    data.iter()
        .map(|&v| {
            let clipped = v.clamp(vmin, vmax);
            let norm = (clipped - vmin) / span;
            (A * norm + 1.0).ln() / denom
        })
        .collect()
}

/// Lupton asinh stretch (`image_viewer._lupton_asinh_stretch`):
/// `arcsinh(alpha·Q·(x - m)) / Q`, then normalized by the 99.5th percentile of
/// the stretched finite values and clipped to `[0, 1]`. `NaN` is preserved.
#[must_use]
pub fn lupton_asinh_stretch(data: &[f64]) -> Vec<f64> {
    const Q: f64 = 8.0;
    let (m, sigma) = estimate_background(data);
    let alpha = default_alpha(sigma);

    let stretched: Vec<f64> = data
        .iter()
        .map(|&v| (alpha * Q * (v - m)).asinh() / Q)
        .collect();

    let finite: Vec<f64> = stretched
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect();
    if finite.is_empty() {
        return vec![f64::NAN; data.len()];
    }
    let vmax = percentile(&finite, 99.5);
    stretched
        .iter()
        .zip(data)
        .map(|(&s, &orig)| {
            if orig.is_nan() {
                f64::NAN
            } else if vmax > 0.0 {
                (s / vmax).clamp(0.0, 1.0)
            } else {
                s.clamp(0.0, 1.0)
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // sample counts are small; f64 is exact here
fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

#[allow(clippy::cast_precision_loss)] // sample counts are small; f64 is exact here
fn std_pop(xs: &[f64], mean: f64) -> f64 {
    let var = xs.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    var.sqrt()
}

fn median(xs: &mut [f64]) -> f64 {
    xs.sort_unstable_by(f64::total_cmp);
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        f64::midpoint(xs[n / 2 - 1], xs[n / 2])
    }
}

/// numpy-style linear-interpolation percentile (type 7) over already-finite
/// values. `p` is a percentage in `[0, 100]`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
pub(crate) fn percentile(values: &[f64], p: f64) -> f64 {
    let mut xs = values.to_vec();
    xs.sort_unstable_by(f64::total_cmp);
    let n = xs.len();
    if n == 1 {
        return xs[0];
    }
    let rank = (p / 100.0) * (n - 1) as f64; // rank ∈ [0, n-1]
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    xs[lo] + (xs[hi] - xs[lo]) * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    // 8×8 fixture with two bright sources and a NaN — identical to the array
    // fed to the Python reference; goldens captured from that run.
    fn fixture() -> Vec<f64> {
        vec![
            f64::NAN,
            7.920_031_787_519_009,
            11.500_902_391_612_914,
            11.881_129_432_782_428,
            6.097_929_622_692_327,
            7.395_640_986_275_364,
            10.255_680_806_334_57,
            9.367_514_815_312_836,
            9.966_397_684_991_422,
            8.293_912_144_852_84,
            11.758_795_949_725_657,
            11.555_583_870_857_896,
            10.132_061_395_122_433,
            12.254_482_413_936_065,
            10.935_018_684_504_092,
            8.281_415_074_233_523,
            10.737_501_568_164_998,
            8.082_234_798_342_002,
            11.756_900_602_614_545,
            9.900_148_178_027_495,
            9.630_275_272_909_48,
            8.638_140_911_192_117,
            12.445_082_677_348_061,
            9.690_941_035_862_396,
            9.143_344_355_673_786,
            9.295_732_899_023_541,
            11.064_618_371_106_697,
            10.730_888_128_728_157,
            500.0,
            10.861_642_006_015_765,
            14.283_295_201_740_923,
            9.187_169_967_230_77,
            8.975_514_541_856_926,
            8.372_454_543_504_244,
            11.231_958_845_150_992,
            12.257_944_585_441_784,
            9.772_105_084_690_25,
            8.319_687_046_074_943,
            8.351_037_568_617_521,
            11.301_185_575_649_402,
            11.486_508_342_406_884,
            11.086_308_536_610_39,
            8.668_980_585_422_611,
            10.464_322_646_133_44,
            10.233_371_618_281_456,
            300.0,
            11.742_857_555_896_38,
            10.447_191_097_549_364,
            11.357_827_126_143_789,
            10.135_158_138_977_783,
            10.578_238_797_379_969,
            11.262_576_451_677_08,
            7.085_688_360_288_667_6,
            9.360_657_567_285_397,
            9.059_254_691_414_41,
            8.722_244_303_513_316,
            9.449_715_497_546_633,
            12.989_882_622_468_79,
            8.268_337_768_613_513,
            11.936_556_709_182_963,
            6.634_260_456_768_390_5,
            9.330_229_940_028_45,
            10.325_506_130_210_01,
            11.172_444_662_718_556,
        ]
    }

    #[test]
    fn background_matches_astropy() {
        let (m, s) = estimate_background(&fixture());
        assert!((m - 10.135_158_138_977_783).abs() < 1e-9, "median {m}");
        assert!((s - 1.612_009_809_212_507).abs() < 1e-9, "std {s}");
        assert!((default_alpha(s) - 0.012_406_872_392_749_048).abs() < 1e-12);
    }

    #[test]
    fn log_stretch_matches_python() {
        let out = log_stretch(&fixture());
        assert!((out[3 * 8 + 4] - 0.999_999_999_999_961_7).abs() < 1e-9);
        assert!((out[1] - 0.0).abs() < 1e-9);
        assert!((out[5 * 8 + 5] - 0.903_139_007_209_256_3).abs() < 1e-9);
    }

    #[test]
    fn lupton_asinh_matches_python() {
        let out = lupton_asinh_stretch(&fixture());
        assert!((out[3 * 8 + 4] - 1.0).abs() < 1e-9);
        assert!((out[1] - 0.0).abs() < 1e-9);
        assert!((out[5 * 8 + 5] - 0.918_020_176_090_109_8).abs() < 1e-9);
        assert!(out[0].is_nan(), "NaN input must stay NaN");
    }

    // The viewer stores flux as f32 and widens back to f64 before running this
    // exact code. Pin the accepted precision loss: stretched scalars stay within
    // 1e-5 of the f64 path (NaN stays NaN).
    #[test]
    #[allow(clippy::cast_possible_truncation)] // deliberate f64->f32 round-trip
    fn f32_widen_matches_f64_within_tolerance() {
        let d = fixture();
        let d32: Vec<f64> = d.iter().map(|&v| f64::from(v as f32)).collect();
        for pair in [
            (log_stretch(&d), log_stretch(&d32)),
            (lupton_asinh_stretch(&d), lupton_asinh_stretch(&d32)),
        ] {
            let (a, b) = pair;
            for (x, y) in a.iter().zip(&b) {
                if x.is_nan() && y.is_nan() {
                    continue;
                }
                assert!((x - y).abs() < 1e-5, "{x} vs {y}");
            }
        }
    }
}
