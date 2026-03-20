//! Complete compressor — the core processing unit.
//!
//! Combines envelope detection, gain reduction computation, channel linking,
//! output saturation, and parallel mix into a single stereo compressor.
//! Based on APComp's versatile architecture.

use fts_dsp::db::{db_to_linear, linear_to_db};

use crate::detector::Detector;
use crate::gain::GainComputer;

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

// r[impl comp.chain.signal-flow]
/// Complete stereo compressor with all APComp features.
///
/// Signal flow per sample:
/// 1. Input gain
/// 2. Level detection (feedforward + optional feedback)
/// 3. Gain reduction computation (threshold/ratio/convexity/inertia)
/// 4. Channel linking
/// 5. Apply gain reduction
/// 6. Output saturation (tanh soft clip)
/// 7. Parallel mix (fold)
/// 8. Output gain
pub struct Compressor {
    pub detector: Detector,
    pub gain_computer: GainComputer,

    // Parameters
    pub threshold_db: f64,
    pub ratio: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub convexity: f64,
    pub feedback: f64,
    pub channel_link: f64,
    pub inertia: f64,
    pub inertia_decay: f64,
    pub ceiling: f64,
    pub fold: f64,
    pub input_gain_db: f64,
    pub output_gain_db: f64,

    // State
    sample_rate: f64,
    /// Last gain reduction in dB per channel (for metering).
    pub last_gr_db: [f64; MAX_CH],
}

impl Compressor {
    pub fn new() -> Self {
        Self {
            detector: Detector::new(),
            gain_computer: GainComputer::new(),

            threshold_db: 0.0,
            ratio: 4.0,
            attack_ms: 90.0,
            release_ms: 400.0,
            convexity: 1.0,
            feedback: 0.0,
            channel_link: 1.0,
            inertia: 0.0,
            inertia_decay: 0.94,
            ceiling: 1.0,
            fold: 0.0,
            input_gain_db: 0.0,
            output_gain_db: 0.0,

            sample_rate: 48000.0,
            last_gr_db: [0.0; MAX_CH],
        }
    }

    /// Update internal coefficients after parameter changes.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let attack_s = linear_to_exponential(self.attack_ms, 0.0, 300.0) / 1000.0;
        let release_s = linear_to_exponential(self.release_ms, 0.0, 3000.0) / 1000.0;
        self.detector.set_params(attack_s, release_s, sample_rate);
    }

    // r[impl comp.chain.signal-flow]
    /// Process a stereo pair of samples in-place.
    ///
    /// This is the per-sample inner loop matching APComp's `doCompressionDSP`.
    #[inline]
    pub fn process_sample(&mut self, left: &mut f64, right: &mut f64) {
        let input_gain = db_to_linear(self.input_gain_db);
        let output_gain = db_to_linear(self.output_gain_db);
        let inertia_decay = 0.99 + (self.inertia_decay * 0.01);

        let dry = [*left, *right];

        // Apply input gain
        let samples = [*left * input_gain, *right * input_gain];

        // Per-channel detection and gain reduction
        let mut gr_db = [0.0_f64; MAX_CH];

        for ch in 0..MAX_CH {
            // Detect envelope level
            let level_db = self.detector.tick(samples[ch].abs(), self.feedback, ch);

            // Compute gain reduction
            gr_db[ch] = self.gain_computer.compute(
                level_db,
                self.threshold_db,
                self.ratio,
                self.convexity,
                self.inertia,
                inertia_decay,
                ch,
            );
        }

        // Channel linking: blend individual GR with max GR
        let max_gr = gr_db[0].max(gr_db[1]);
        if self.channel_link > 0.0 {
            for ch in 0..MAX_CH {
                gr_db[ch] = (max_gr * self.channel_link) + (gr_db[ch] * (1.0 - self.channel_link));
            }
        }

        // Apply gain reduction and output processing
        let mut outputs = [0.0_f64; MAX_CH];
        for ch in 0..MAX_CH {
            let input_db = linear_to_db(samples[ch].abs());
            let sign = if samples[ch] < 0.0 { -1.0 } else { 1.0 };

            // Apply gain reduction
            let output_db = input_db - gr_db[ch];
            let mut out = db_to_linear(output_db) * sign;

            // Output saturation: tanh soft clip with ceiling
            if self.ceiling > 0.0 {
                out /= self.ceiling;
                out = out.tanh();
                out *= self.ceiling;
            }

            // Apply output gain
            out *= output_gain;

            // Store for feedback detection
            self.detector.set_output(out, ch);

            // Parallel mix (fold): blend dry and compressed
            // r[impl comp.chain.parallel-mix]
            if self.fold > 0.0 {
                out = out * (1.0 - self.fold) + dry[ch] * self.fold;
            }

            // NaN safety
            if !out.is_finite() {
                out = 0.0;
            }

            outputs[ch] = out;
            self.last_gr_db[ch] = gr_db[ch];
        }

        *left = outputs[0];
        *right = outputs[1];
    }

    /// Get the current gain reduction in dB for metering.
    pub fn gain_reduction_db(&self) -> f64 {
        self.last_gr_db[0].max(self.last_gr_db[1])
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.gain_computer.reset();
        self.last_gr_db = [0.0; MAX_CH];
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Utility ────────────────────────────────────────────────────────────

/// APComp's exponential parameter scaling.
///
/// Maps a linear 0..max input to an exponential curve for more natural
/// control feel on attack/release/ratio knobs.
#[inline]
fn linear_to_exponential(value: f64, min: f64, max: f64) -> f64 {
    let value = value.clamp(min, max);
    let normalized = (value - min) / (max - min);
    let exponential = normalized * normalized;
    min + exponential * (max - min)
}
