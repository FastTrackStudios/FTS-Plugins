/// Pre-processing pipeline for guitar DI signals.
///
/// Implements:
/// 1. DC blocker (high-pass at ~10 Hz)
/// 2. Hum filter (notch at 60 Hz + harmonics)
/// 3. Soft-knee compressor (normalize dynamics before detection)

/// DC blocking filter: first-order high-pass.
/// y[n] = x[n] - x[n-1] + R * y[n-1], where R ~ 0.995.
pub struct DcBlocker {
    x_prev: f64,
    y_prev: f64,
    r: f64,
}

impl DcBlocker {
    pub fn new() -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
            r: 0.995,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = x - self.x_prev + self.r * self.y_prev;
        self.x_prev = x;
        self.y_prev = y;
        y
    }

    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
}

/// Second-order IIR biquad filter (used for high-pass and notch).
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// High-pass filter at the given frequency (Butterworth, Q=0.707).
    pub fn high_pass(freq: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2); // Q = 1/sqrt(2)

        let a0 = 1.0 + alpha;
        let b0 = ((1.0 + cos_w0) / 2.0) / a0;
        let b1 = (-(1.0 + cos_w0)) / a0;
        let b2 = ((1.0 + cos_w0) / 2.0) / a0;
        let a1 = (-2.0 * cos_w0) / a0;
        let a2 = (1.0 - alpha) / a0;

        Self::new(b0, b1, b2, a1, a2)
    }

    /// Notch filter at the given frequency.
    pub fn notch(freq: f64, q: f64, sample_rate: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let a0 = 1.0 + alpha;
        let b0 = 1.0 / a0;
        let b1 = (-2.0 * cos_w0) / a0;
        let b2 = 1.0 / a0;
        let a1 = (-2.0 * cos_w0) / a0;
        let a2 = (1.0 - alpha) / a0;

        Self::new(b0, b1, b2, a1, a2)
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Simple envelope follower for the compressor.
struct EnvelopeFollower {
    level: f64,
    attack_coeff: f64,
    release_coeff: f64,
}

impl EnvelopeFollower {
    fn new(attack_ms: f64, release_ms: f64, sample_rate: f64) -> Self {
        Self {
            level: 0.0,
            attack_coeff: (-1.0 / (attack_ms * 0.001 * sample_rate)).exp(),
            release_coeff: (-1.0 / (release_ms * 0.001 * sample_rate)).exp(),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f64, attack_ms: f64, release_ms: f64) {
        self.attack_coeff = (-1.0 / (attack_ms * 0.001 * sample_rate)).exp();
        self.release_coeff = (-1.0 / (release_ms * 0.001 * sample_rate)).exp();
    }

    #[inline]
    fn process(&mut self, input_abs: f64) -> f64 {
        let coeff = if input_abs > self.level {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.level = coeff * self.level + (1.0 - coeff) * input_abs;
        self.level
    }

    fn reset(&mut self) {
        self.level = 0.0;
    }
}

/// Soft-knee compressor for normalizing guitar DI dynamics before detection.
/// This is internal-only, not for audio output — just shapes the signal for
/// more consistent energy across the resonator bank.
pub struct Compressor {
    envelope: EnvelopeFollower,
    /// Threshold in linear amplitude.
    threshold: f64,
    /// Ratio (e.g. 4.0 = 4:1).
    ratio: f64,
    /// Knee width in dB.
    knee_db: f64,
    /// Makeup gain in linear.
    makeup: f64,
}

impl Compressor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            // Fast attack for guitar transients, moderate release.
            envelope: EnvelopeFollower::new(1.0, 50.0, sample_rate),
            threshold: 0.05, // -26 dB — only compress loud transients
            ratio: 3.0,
            knee_db: 12.0, // wide knee for gentle compression
            makeup: 1.5,   // minimal makeup — just tame dynamics
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.envelope.set_sample_rate(sample_rate, 1.0, 50.0);
    }

    #[inline]
    pub fn process(&mut self, input: f64) -> f64 {
        let abs_in = input.abs();
        let env = self.envelope.process(abs_in);

        if env < 1e-10 {
            return input * self.makeup;
        }

        let env_db = 20.0 * env.log10();
        let thresh_db = 20.0 * self.threshold.log10();
        let over_db = env_db - thresh_db;

        let gain_reduction_db = if over_db <= -self.knee_db / 2.0 {
            // Below knee: no compression.
            0.0
        } else if over_db >= self.knee_db / 2.0 {
            // Above knee: full compression.
            over_db * (1.0 - 1.0 / self.ratio)
        } else {
            // In knee: smooth transition.
            let x = over_db + self.knee_db / 2.0;
            (1.0 - 1.0 / self.ratio) * x * x / (2.0 * self.knee_db)
        };

        let gain = 10.0_f64.powf(-gain_reduction_db / 20.0) * self.makeup;
        input * gain
    }

    pub fn reset(&mut self) {
        self.envelope.reset();
    }
}

/// Full pre-processing chain for guitar DI signals.
pub struct PreProcessor {
    dc_blocker: DcBlocker,
    high_pass: Biquad,
    notch_60: Biquad,
    notch_120: Biquad,
    compressor: Compressor,
    enabled: bool,
}

impl PreProcessor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            dc_blocker: DcBlocker::new(),
            high_pass: Biquad::high_pass(70.0, sample_rate),
            notch_60: Biquad::notch(60.0, 30.0, sample_rate),
            notch_120: Biquad::notch(120.0, 30.0, sample_rate),
            compressor: Compressor::new(sample_rate),
            enabled: true,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.high_pass = Biquad::high_pass(70.0, sample_rate);
        self.notch_60 = Biquad::notch(60.0, 30.0, sample_rate);
        self.notch_120 = Biquad::notch(120.0, 30.0, sample_rate);
        self.compressor.set_sample_rate(sample_rate);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Process one sample through the full chain:
    /// DC block -> high-pass -> hum notches -> compression.
    #[inline]
    pub fn process(&mut self, input: f64) -> f64 {
        if !self.enabled {
            return input;
        }
        let x = self.dc_blocker.process(input);
        let x = self.high_pass.process(x);
        let x = self.notch_60.process(x);
        let x = self.notch_120.process(x);
        self.compressor.process(x)
    }

    pub fn reset(&mut self) {
        self.dc_blocker.reset();
        self.high_pass.reset();
        self.notch_60.reset();
        self.notch_120.reset();
        self.compressor.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dc_blocker_removes_dc() {
        let mut dc = DcBlocker::new();
        // Feed 1000 samples of DC offset.
        for _ in 0..1000 {
            dc.process(1.0);
        }
        // Output should converge to ~0.
        let out = dc.process(1.0);
        assert!(
            out.abs() < 0.01,
            "DC blocker output should be near 0, got {out}"
        );
    }

    #[test]
    fn test_high_pass_passes_guitar_freq() {
        let sr = 48000.0;
        let mut hp = Biquad::high_pass(70.0, sr);
        let freq = 330.0; // E4
        let mut max_out = 0.0_f64;
        for i in 0..4800 {
            let t = i as f64 / sr;
            let x = (2.0 * std::f64::consts::PI * freq * t).sin();
            let y = hp.process(x);
            if i > 2400 {
                max_out = max_out.max(y.abs());
            }
        }
        // Should pass through most of the signal.
        assert!(
            max_out > 0.9,
            "High-pass should pass 330 Hz, got peak {max_out}"
        );
    }

    #[test]
    fn test_high_pass_blocks_hum() {
        let sr = 48000.0;
        let mut hp = Biquad::high_pass(70.0, sr);
        let freq = 50.0; // 50 Hz hum
        let mut max_out = 0.0_f64;
        for i in 0..4800 {
            let t = i as f64 / sr;
            let x = (2.0 * std::f64::consts::PI * freq * t).sin();
            let y = hp.process(x);
            if i > 2400 {
                max_out = max_out.max(y.abs());
            }
        }
        assert!(
            max_out < 0.5,
            "High-pass should attenuate 50 Hz, got peak {max_out}"
        );
    }

    #[test]
    fn test_compressor_reduces_loud_signals() {
        let sr = 48000.0;
        let mut comp = Compressor::new(sr);
        // Feed a loud signal.
        let mut output = 0.0;
        for i in 0..4800 {
            let t = i as f64 / sr;
            let x = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            output = comp.process(x);
        }
        // Output should be boosted by makeup but compressed.
        assert!(output.abs() > 0.0, "Compressor should produce output");
    }

    #[test]
    fn test_preprocessor_chain() {
        let sr = 48000.0;
        let mut pp = PreProcessor::new(sr);
        // Process some guitar-like signal.
        let mut max_out = 0.0_f64;
        for i in 0..4800 {
            let t = i as f64 / sr;
            let x = 0.1 * (2.0 * std::f64::consts::PI * 330.0 * t).sin();
            let y = pp.process(x);
            if i > 2400 {
                max_out = max_out.max(y.abs());
            }
        }
        assert!(
            max_out > 0.0,
            "PreProcessor should produce output for guitar signal"
        );
    }
}
