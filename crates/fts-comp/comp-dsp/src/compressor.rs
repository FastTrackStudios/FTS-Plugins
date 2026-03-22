//! Complete compressor — the core processing unit.
//!
//! Two-stage architecture (DAFX textbook):
//! 1. Compute instantaneous level in dB
//! 2. Apply gain curve (threshold/ratio/knee) → raw GR
//! 3. Smooth GR with attack/release
//! 4. Apply smoothed GR, saturation, mix

use fts_dsp::db::{db_to_linear, linear_to_db};

use crate::detector::Detector;
use crate::gain::GainComputer;

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

/// Threshold offset — set to 0 for 2-stage architecture.
///
/// The 2-stage approach (instant level → gain curve → smooth GR)
/// naturally handles peak-vs-mean weighting through the nonlinear
/// averaging of the gain curve over the waveform cycle.
pub const PEAK_TO_MEAN_DB: f64 = 0.0;

// r[impl comp.chain.signal-flow]
/// Complete stereo compressor with all APComp features.
///
/// Signal flow per sample:
/// 1. Input gain
/// 2. Level detection (feedforward + optional feedback)
/// 3. Gain reduction computation (threshold/ratio/knee/inertia)
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
    pub knee_db: f64,
    pub feedback: f64,
    pub channel_link: f64,
    pub inertia: f64,
    pub inertia_decay: f64,
    pub ceiling: f64,
    pub fold: f64,
    pub input_gain_db: f64,
    pub output_gain_db: f64,
    pub auto_makeup: bool,

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
            knee_db: 6.0,
            feedback: 0.0,
            channel_link: 1.0,
            inertia: 0.0,
            inertia_decay: 0.94,
            ceiling: 1.0,
            fold: 0.0,
            input_gain_db: 0.0,
            output_gain_db: 0.0,
            auto_makeup: false,

            sample_rate: 48000.0,
            last_gr_db: [0.0; MAX_CH],
        }
    }

    /// Update internal coefficients after parameter changes.
    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let attack_s = self.attack_ms / 1000.0;
        let release_s = self.release_ms / 1000.0;
        self.detector.set_params(attack_s, release_s, sample_rate);
    }

    // r[impl comp.chain.signal-flow]
    /// Process a stereo pair of samples in-place.
    ///
    /// This is the per-sample inner loop matching APComp's `doCompressionDSP`.
    #[inline]
    pub fn process_sample(&mut self, left: &mut f64, right: &mut f64) {
        let input_gain = db_to_linear(self.input_gain_db);
        let mut output_gain = db_to_linear(self.output_gain_db);
        let inertia_decay = 0.99 + (self.inertia_decay * 0.01);

        // Auto makeup gain: compensate for expected GR at threshold
        if self.auto_makeup && self.ratio > 1.0 {
            let makeup_db = -self.threshold_db * (1.0 - 1.0 / self.ratio) * 0.5;
            output_gain *= db_to_linear(makeup_db);
        }

        let dry = [*left, *right];

        // Apply input gain
        let samples = [*left * input_gain, *right * input_gain];

        // Per-channel: smoothed level → gain curve → GR
        let mut gr_db = [0.0_f64; MAX_CH];

        for ch in 0..MAX_CH {
            // Step 1: Instantaneous level in dB
            let level_db = self.detector.tick(samples[ch].abs(), self.feedback, ch);

            // Step 2: Apply gain curve to get raw (instantaneous) GR
            let raw_gr = self.gain_computer.compute(
                level_db,
                self.threshold_db + PEAK_TO_MEAN_DB,
                self.ratio,
                self.knee_db,
                self.inertia,
                inertia_decay,
                ch,
            );

            // Step 3: Smooth the GR with attack/release
            gr_db[ch] = self.detector.smooth_gr(raw_gr, ch);
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
