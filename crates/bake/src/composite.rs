//! RGB composites for display, ported 1:1 from Python `rgb_composite.py`.
//! Two recipes: `percentile_asinh` (default, per-band) and `lupton`
//! (color-preserving). Both take three `ny×nx` flux bands (row-major, `NaN`
//! for invalid) and return interleaved `RGB` bytes, `ny×nx×3`.
#![allow(dead_code)] // consumed by main.rs at the orchestration step

use crate::stretch::{default_alpha, estimate_background, percentile};

/// Per-band percentile+asinh composite (`percentile_asinh_composite`), the
/// default. Each band is median-subtracted, percentile-clipped (10, 99.9),
/// asinh-stretched (`scale=0.1`), pedestal-cut (`floor=0.05`), then weighted
/// `(1.0, 1.02, 1.02)`. Pixels invalid in any band become black.
#[must_use]
#[allow(clippy::many_single_char_names)] // r/g/b are the domain vocabulary
pub fn percentile_asinh(r: &[f64], g: &[f64], b: &[f64]) -> Vec<u8> {
    const WEIGHTS: [f64; 3] = [1.0, 1.02, 1.02];
    let rn = normalize_band_asinh(r, 10.0, 99.9, 0.1, 0.05);
    let gn = normalize_band_asinh(g, 10.0, 99.9, 0.1, 0.05);
    let bn = normalize_band_asinh(b, 10.0, 99.9, 0.1, 0.05);

    let n = r.len();
    let mut out = Vec::with_capacity(n * 3);
    for i in 0..n {
        if is_any_nan(r[i], g[i], b[i]) {
            out.extend_from_slice(&[0, 0, 0]);
            continue;
        }
        out.push(to_u8(rn[i] * WEIGHTS[0]));
        out.push(to_u8(gn[i] * WEIGHTS[1]));
        out.push(to_u8(bn[i] * WEIGHTS[2]));
    }
    out
}

/// Lupton et al. (2004) color-preserving composite (`lupton_rgb_composite`):
/// stretch the total intensity, scale each band by `f(I)/(I-m)`, per-pixel
/// renormalize if any channel exceeds 1, then global 99.5th-percentile
/// normalize. `Q = 8`, `alpha` auto-estimated from the intensity background.
#[must_use]
#[allow(clippy::many_single_char_names)] // r/g/b are the domain vocabulary
pub fn lupton(r: &[f64], g: &[f64], b: &[f64]) -> Vec<u8> {
    let n = r.len();
    let intensity: Vec<f64> = (0..n).map(|i| (r[i] + g[i] + b[i]) / 3.0).collect();
    let (m, sigma) = estimate_background(&intensity);
    let alpha = default_alpha(sigma);

    let chans: Vec<[f64; 3]> = (0..n)
        .map(|i| lupton_pixel(r[i], g[i], b[i], intensity[i], m, alpha))
        .collect();

    // Global normalization by the 99.5th percentile of the positive values.
    let positive: Vec<f64> = chans
        .iter()
        .flatten()
        .copied()
        .filter(|&v| v > 0.0)
        .collect();
    let vmax = if positive.is_empty() {
        1.0
    } else {
        percentile(&positive, 99.5)
    };

    let mut out = Vec::with_capacity(n * 3);
    for px in chans {
        for c in px {
            let v = if vmax > 0.0 { c / vmax } else { c };
            out.push(to_u8(v));
        }
    }
    out
}

/// One Lupton pixel: the color-preserving scale `f(I)/(I-m)` applied to each
/// band, clamped ≥ 0, renormalized if any channel exceeds 1. `NaN` → black.
#[allow(clippy::many_single_char_names)] // r/g/b are the domain vocabulary
fn lupton_pixel(r: f64, g: f64, b: f64, intensity: f64, m: f64, alpha: f64) -> [f64; 3] {
    const Q: f64 = 8.0;
    if is_any_nan(r, g, b) {
        return [0.0, 0.0, 0.0];
    }
    let i_shift = intensity - m;
    let ratio = if i_shift > 0.0 {
        (alpha * Q * i_shift).asinh() / Q / i_shift
    } else {
        0.0
    };
    let mut px = [
        ((r - m) * ratio).max(0.0),
        ((g - m) * ratio).max(0.0),
        ((b - m) * ratio).max(0.0),
    ];
    let max = px[0].max(px[1]).max(px[2]);
    if max > 1.0 {
        for c in &mut px {
            *c /= max;
        }
    }
    px
}

/// One band: median-subtract, percentile-clip `[pmin, pmax]`, asinh-stretch by
/// `scale`, pedestal-cut below `floor`. Empty/all-NaN bands map to zeros.
fn normalize_band_asinh(band: &[f64], pmin: f64, pmax: f64, scale: f64, floor: f64) -> Vec<f64> {
    let finite: Vec<f64> = band.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return vec![0.0; band.len()];
    }
    let bkg = median(&finite);
    let shifted: Vec<f64> = band.iter().map(|&v| v - bkg).collect();

    let finite_shift: Vec<f64> = shifted.iter().copied().filter(|v| v.is_finite()).collect();
    let lo = percentile(&finite_shift, pmin);
    let hi = percentile(&finite_shift, pmax);
    let denom = if hi > lo { hi - lo } else { 1.0 };
    let asinh_1 = (1.0 / scale).asinh();

    shifted
        .iter()
        .map(|&x| {
            let y = ((x - lo) / denom).clamp(0.0, 1.0);
            let y = (y / scale).asinh() / asinh_1;
            let y = if y < floor { 0.0 } else { y };
            y.clamp(0.0, 1.0)
        })
        .collect()
}

fn median(values: &[f64]) -> f64 {
    let mut xs = values.to_vec();
    xs.sort_unstable_by(f64::total_cmp);
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        f64::midpoint(xs[n / 2 - 1], xs[n / 2])
    }
}

fn is_any_nan(a: f64, b: f64, c: f64) -> bool {
    a.is_nan() || b.is_nan() || c.is_nan()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to [0,255]
fn to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    // 6×6 bands, seed-7 normals with one bright source and a NaN — identical to
    // the array fed to the Python reference; goldens captured from that run.
    #[allow(clippy::too_many_lines)] // literal fixture data, not logic
    fn bands() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let r = vec![
            5.001_230_153_357_483,
            5.298_745_537_508_47,
            4.725_862_144_637_782,
            4.109_408_161_242_726,
            4.545_329_214_828_278,
            4.008_353_445_003_538,
            5.060_143_602_597_439,
            6.340_215_245_554_534,
            4.507_793_481_448_671,
            4.379_525_100_180_06,
            5.489_842_050_185_198_6,
            5.356_887_008_160_061,
            5.105_414_248_997_898,
            4.069_531_955_291_795_5,
            4.970_748_177_536_726,
            200.0,
            3.655_785_452_714_918,
            4.542_384_238_959_782,
            3.098_777_260_199_155_7,
            3.710_462_260_215_024,
            3.158_264_962_208_267_7,
            4.764_908_868_925_318_5,
            3.732_553_518_556_297,
            5.271_264_358_821_702,
            5.156_751_086_624_225,
            f64::NAN,
            2.483_240_289_179_487,
            4.461_307_104_153_363,
            4.951_499_054_598_928,
            5.113_308_986_003_307,
            3.469_864_234_494_606_2,
            4.522_246_723_966_069,
            4.021_480_921_943_36,
            4.191_162_760_574_401,
            6.060_898_623_386_079,
            4.192_465_324_668_103,
        ];
        let g = vec![
            5.967_478_295_054_479,
            6.884_389_867_383_174,
            5.416_399_567_256_698,
            5.888_298_050_415_84,
            6.110_464_143_249_48,
            6.063_781_774_255_062,
            4.774_944_173_582_306,
            6.076_140_230_377_008,
            7.358_823_421_741_538,
            4.452_855_321_871_517_4,
            6.859_382_688_021_598,
            6.119_354_025_696_581,
            5.358_529_605_892_778_5,
            8.000_416_546_342_423,
            6.762_259_712_084_711,
            150.0,
            6.074_516_228_771_463,
            6.576_689_583_670_185,
            5.811_217_874_649_251,
            6.682_910_267_195_206,
            5.933_482_679_850_584,
            6.667_247_560_834_328,
            7.438_522_591_656_152,
            5.324_337_748_994_347,
            6.203_138_610_389_609,
            5.536_692_423_461_584,
            6.127_268_411_225_831,
            4.812_805_472_149_86,
            5.420_698_403_497_327,
            5.803_804_027_195_503,
            6.898_763_872_100_408,
            7.145_222_007_454_132,
            4.676_472_207_515_745,
            5.205_357_634_012_951,
            6.646_903_422_573_422,
            4.007_580_215_825_506,
        ];
        let b = vec![
            3.536_830_135_047_633,
            3.902_713_074_329_911,
            5.257_014_977_286_82,
            4.689_403_900_570_755,
            3.672_786_579_777_802,
            3.631_424_105_900_040_7,
            3.749_804_599_482_075_2,
            5.523_529_400_456_161,
            3.571_975_057_427_133,
            3.696_319_611_635_271,
            4.352_589_067_285_265,
            3.879_229_554_913_545,
            3.802_715_772_034_277_5,
            2.885_932_856_848_943_7,
            3.988_478_531_961_452,
            90.0,
            5.166_127_776_190_223,
            4.653_088_502_701_164,
            3.975_856_386_990_068,
            4.668_381_023_267_344,
            3.660_130_448_286_850_4,
            5.052_126_358_426_947,
            3.994_600_439_328_373_4,
            4.583_382_354_180_414,
            2.709_106_754_676_512_6,
            4.346_680_048_878_429,
            2.311_795_882_633_458_4,
            1.964_671_055_060_067_7,
            3.695_523_122_288_563,
            3.100_072_392_401_404_6,
            4.164_052_795_712_222,
            6.244_756_626_486_049_5,
            3.168_276_818_587_918_4,
            3.376_056_413_556_094,
            4.205_403_946_064_699,
            4.493_013_291_412_356,
        ];
        (r, g, b)
    }

    #[test]
    fn percentile_asinh_matches_python() {
        let (r, g, b) = bands();
        let out = percentile_asinh(&r, &g, &b);
        let px = |y: usize, x: usize| {
            let i = (y * 6 + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        assert_eq!(px(2, 3), [255, 255, 255], "bright source saturates");
        assert_eq!(px(0, 0), [0, 0, 0], "floor cuts faint pixel to black");
        assert_eq!(px(4, 1), [0, 0, 0], "NaN pixel is black");
    }

    #[test]
    fn lupton_matches_python() {
        let (r, g, b) = bands();
        let out = lupton(&r, &g, &b);
        let px = |y: usize, x: usize| {
            let i = (y * 6 + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        assert_eq!(px(2, 3), [255, 197, 115], "bright source keeps color ratio");
        assert_eq!(px(1, 1), [18, 14, 8], "mid-tone color-preserving path");
        assert_eq!(px(4, 1), [0, 0, 0], "NaN pixel is black");
    }
}
