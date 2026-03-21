//! LPC (Linear Predictive Coding) spectral envelope estimator.
//!
//! Models the spectral envelope of speech/voice signals as an all-pole filter,
//! capturing formant structure more accurately than cepstral methods for small
//! model orders.
//!
//! Algorithm:
//! 1. Autocorrelation method: compute autocorrelation of windowed signal frame
//! 2. Levinson-Durbin recursion: solve for LPC coefficients from autocorrelation
//! 3. Spectral envelope: evaluate `1/|A(e^{jw})|^2` at each FFT bin frequency

use std::f64::consts::PI;

/// Default LPC order (number of coefficients).
/// For speech: `2 + sample_rate / 1000` (e.g., 50 at 48 kHz).
/// Lower orders (16-24) yield smoother envelopes.
const DEFAULT_LPC_ORDER: usize = 24;

/// LPC spectral envelope estimator using the autocorrelation method and
/// Levinson-Durbin recursion.
pub struct LpcEnvelope {
    /// LPC order (number of poles).
    pub order: usize,

    /// LPC coefficients `[a1, a2, ..., a_order]`.
    coeffs: Vec<f64>,
    /// Autocorrelation values `r[0..=order]`.
    autocorr: Vec<f64>,
    /// Reflection coefficients (PARCOR) `k[1..=order]`.
    reflection: Vec<f64>,
    /// Scratch buffer for Levinson-Durbin update step.
    scratch: Vec<f64>,
    /// Prediction error energy after the last `analyze()`.
    error: f64,
}

impl LpcEnvelope {
    /// Create a new `LpcEnvelope` with the default order (24).
    pub fn new() -> Self {
        Self::with_order(DEFAULT_LPC_ORDER)
    }

    /// Create a new `LpcEnvelope` with the given order.
    ///
    /// # Panics
    /// Panics if `order` is zero.
    pub fn with_order(order: usize) -> Self {
        assert!(order > 0, "LPC order must be at least 1");
        Self {
            order,
            coeffs: vec![0.0; order],
            autocorr: vec![0.0; order + 1],
            reflection: vec![0.0; order],
            scratch: vec![0.0; order],
            error: 0.0,
        }
    }

    /// Analyze a windowed frame and compute LPC coefficients via the
    /// autocorrelation method followed by Levinson-Durbin recursion.
    ///
    /// `frame` should be pre-windowed (Hamming/Hann). If the frame is shorter
    /// than `order + 1`, the missing lags are treated as zero.
    pub fn analyze(&mut self, frame: &[f64]) {
        let n = frame.len();

        // --- Autocorrelation ---
        for lag in 0..=self.order {
            let mut sum = 0.0;
            if lag < n {
                for i in 0..n - lag {
                    sum += frame[i] * frame[i + lag];
                }
            }
            self.autocorr[lag] = sum;
        }

        // Early out: silence or DC-only frame.
        if self.autocorr[0] <= 0.0 {
            self.coeffs.iter_mut().for_each(|c| *c = 0.0);
            self.reflection.iter_mut().for_each(|k| *k = 0.0);
            self.error = 0.0;
            return;
        }

        // --- Levinson-Durbin recursion ---
        let r = &self.autocorr;
        let a = &mut self.coeffs;
        let refl = &mut self.reflection;
        let tmp = &mut self.scratch;

        // Initialise
        a.iter_mut().for_each(|v| *v = 0.0);
        let mut e = r[0];

        for m in 1..=self.order {
            // Compute lambda = r[m] + sum_{k=1}^{m-1} a[k] * r[m-k]
            let mut lambda = r[m];
            for k in 1..m {
                lambda += a[k - 1] * r[m - k];
            }

            let km = -lambda / e;
            refl[m - 1] = km;

            // Save old coefficients into scratch.
            tmp[..m - 1].copy_from_slice(&a[..m - 1]);

            // Update coefficients.
            a[m - 1] = km;
            for k in 1..m {
                a[k - 1] = tmp[k - 1] + km * tmp[m - 1 - k];
            }

            e *= 1.0 - km * km;

            if e <= 0.0 {
                // Numerical instability guard — stop recursion.
                e = e.max(1e-30);
                break;
            }
        }

        self.error = e;
    }

    /// Evaluate the LPC spectral envelope at `num_bins` equally-spaced
    /// frequencies from 0 to Nyquist. Returns magnitude values in linear scale
    /// (not dB).
    ///
    /// Call after [`analyze()`](Self::analyze).
    ///
    /// # Panics
    /// Panics if `num_bins` is less than 2.
    pub fn envelope(&self, num_bins: usize) -> Vec<f64> {
        assert!(num_bins >= 2, "num_bins must be at least 2");

        let mut out = Vec::with_capacity(num_bins);
        let denom = (num_bins - 1) as f64;

        for i in 0..num_bins {
            let omega = PI * (i as f64) / denom;
            out.push(self.eval_at(omega));
        }

        out
    }

    /// Evaluate the spectral envelope at a single normalised frequency `omega`
    /// (0 to pi). Returns the magnitude (linear, not dB).
    pub fn eval_at(&self, omega: f64) -> f64 {
        // A(e^{j*omega}) = 1 + sum_{k=1}^{order} a[k] * e^{-j*k*omega}
        // Re part: 1 + sum a[k] cos(k*omega)
        // Im part:   - sum a[k] sin(k*omega)
        let mut re = 1.0;
        let mut im = 0.0;
        for k in 1..=self.order {
            let angle = (k as f64) * omega;
            let (sin_a, cos_a) = angle.sin_cos();
            re += self.coeffs[k - 1] * cos_a;
            im -= self.coeffs[k - 1] * sin_a;
        }
        let mag_sq = re * re + im * im;
        1.0 / mag_sq.max(1e-20)
    }

    /// Get the current LPC coefficients `[a1, a2, ..., a_order]`.
    pub fn coefficients(&self) -> &[f64] {
        &self.coeffs
    }

    /// Get the prediction error (residual energy) from the last analysis.
    pub fn prediction_error(&self) -> f64 {
        self.error
    }

    /// Get the reflection coefficients (PARCOR) from the last analysis.
    pub fn reflection_coefficients(&self) -> &[f64] {
        &self.reflection
    }

    /// Reset all internal state to zero.
    pub fn reset(&mut self) {
        self.coeffs.iter_mut().for_each(|c| *c = 0.0);
        self.autocorr.iter_mut().for_each(|r| *r = 0.0);
        self.reflection.iter_mut().for_each(|k| *k = 0.0);
        self.scratch.iter_mut().for_each(|s| *s = 0.0);
        self.error = 0.0;
    }
}

impl Default for LpcEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Helper: generate a Hamming-windowed frame of `len` samples from `signal`.
    fn hamming_window(signal: &[f64]) -> Vec<f64> {
        let n = signal.len();
        signal
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let w = 0.54 - 0.46 * (2.0 * PI * i as f64 / (n - 1) as f64).cos();
                s * w
            })
            .collect()
    }

    #[test]
    fn levinson_durbin_identity() {
        // All-zero autocorrelation (silent frame) -> coefficients stay zero.
        let mut lpc = LpcEnvelope::with_order(8);
        let silence = vec![0.0; 256];
        lpc.analyze(&silence);

        for &c in lpc.coefficients() {
            assert_eq!(c, 0.0, "coefficient should be zero for silent frame");
        }
    }

    #[test]
    fn white_noise_flat_envelope() {
        // White noise should produce a roughly flat spectral envelope.
        let mut lpc = LpcEnvelope::with_order(12);
        let n = 2048;

        // Deterministic pseudo-random noise (simple LCG).
        let mut rng: u64 = 12345;
        let mut noise = Vec::with_capacity(n);
        for _ in 0..n {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let val = ((rng >> 33) as f64 / (1u64 << 31) as f64) - 1.0;
            noise.push(val);
        }

        let windowed = hamming_window(&noise);
        lpc.analyze(&windowed);

        let env = lpc.envelope(128);
        // Skip the first and last few bins (DC and Nyquist edges can have
        // large model artefacts). Interior bins should be within 10x of mean.
        let skip = 4;
        let interior = &env[skip..env.len() - skip];
        let int_mean: f64 = interior.iter().sum::<f64>() / interior.len() as f64;
        for (j, &v) in interior.iter().enumerate() {
            let i = j + skip;
            assert!(
                v > int_mean * 0.1 && v < int_mean * 10.0,
                "bin {} = {} deviates too far from mean {} for white noise",
                i,
                v,
                int_mean,
            );
        }
    }

    #[test]
    fn sine_wave_peaks() {
        // A pure sine should produce an envelope that peaks near the sine
        // frequency.
        let mut lpc = LpcEnvelope::with_order(24);
        let n = 2048;
        let sample_rate = 48000.0;
        let freq = 1000.0; // 1 kHz sine

        let sine: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / sample_rate).sin())
            .collect();
        let windowed = hamming_window(&sine);
        lpc.analyze(&windowed);

        let num_bins = 512;
        let env = lpc.envelope(num_bins);

        // Find the bin with the maximum envelope value.
        let peak_bin = env
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        // Convert bin index to frequency.
        let peak_freq = peak_bin as f64 / (num_bins - 1) as f64 * (sample_rate / 2.0);

        // The peak should be within 200 Hz of the sine frequency.
        let diff = (peak_freq - freq).abs();
        assert!(
            diff < 200.0,
            "peak at {} Hz, expected near {} Hz (diff = {})",
            peak_freq,
            freq,
            diff,
        );
    }

    #[test]
    fn prediction_error_positive() {
        let mut lpc = LpcEnvelope::with_order(16);
        let n = 512;
        let signal: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 44100.0).sin())
            .collect();
        let windowed = hamming_window(&signal);
        lpc.analyze(&windowed);

        assert!(
            lpc.prediction_error() > 0.0,
            "prediction error should be positive, got {}",
            lpc.prediction_error(),
        );
    }

    #[test]
    fn coefficients_stable() {
        // All reflection coefficients should satisfy |k_m| < 1 for a stable
        // filter.
        let mut lpc = LpcEnvelope::with_order(20);
        let n = 1024;

        // Multi-sine signal.
        let signal: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / 44100.0;
                0.5 * (2.0 * PI * 300.0 * t).sin()
                    + 0.3 * (2.0 * PI * 1200.0 * t).sin()
                    + 0.2 * (2.0 * PI * 3500.0 * t).sin()
            })
            .collect();
        let windowed = hamming_window(&signal);
        lpc.analyze(&windowed);

        for (i, &k) in lpc.reflection_coefficients().iter().enumerate() {
            assert!(
                k.abs() < 1.0,
                "reflection coefficient k[{}] = {} is unstable (|k| >= 1)",
                i + 1,
                k,
            );
        }
    }

    #[test]
    fn no_nan() {
        // No NaN values should appear in the envelope output, even for
        // pathological inputs.
        let mut lpc = LpcEnvelope::with_order(16);

        // Constant DC signal.
        let dc = vec![1.0; 256];
        lpc.analyze(&dc);
        let env = lpc.envelope(64);
        for (i, &v) in env.iter().enumerate() {
            assert!(!v.is_nan(), "NaN at bin {} for DC signal", i);
            assert!(!v.is_infinite(), "Inf at bin {} for DC signal", i);
        }

        // Near-silent signal.
        let quiet = vec![1e-18; 128];
        lpc.analyze(&quiet);
        let env2 = lpc.envelope(64);
        for (i, &v) in env2.iter().enumerate() {
            assert!(!v.is_nan(), "NaN at bin {} for near-silent signal", i);
            assert!(!v.is_infinite(), "Inf at bin {} for near-silent signal", i);
        }
    }

    #[test]
    fn order_affects_resolution() {
        // Higher LPC order should produce sharper spectral peaks, measured by
        // the ratio of peak-to-mean envelope value.
        let n = 2048;
        let sample_rate = 48000.0;
        let freq = 2000.0;

        let sine: Vec<f64> = (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / sample_rate).sin())
            .collect();
        let windowed = hamming_window(&sine);

        let num_bins = 256;

        let peak_ratio = |order: usize| -> f64 {
            let mut lpc = LpcEnvelope::with_order(order);
            lpc.analyze(&windowed);
            let env = lpc.envelope(num_bins);
            let peak = env.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mean = env.iter().sum::<f64>() / env.len() as f64;
            peak / mean
        };

        let low = peak_ratio(8);
        let high = peak_ratio(32);

        assert!(
            high > low,
            "higher order should give sharper peaks: order 32 ratio = {}, order 8 ratio = {}",
            high,
            low,
        );
    }
}
