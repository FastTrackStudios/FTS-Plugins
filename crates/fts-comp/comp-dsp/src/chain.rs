//! Compressor chain — full signal path with sidechain EQ and lookahead.
//!
//! Wraps the core Compressor with a sidechain highpass filter
//! (from eq-dsp), lookahead delay, and implements the fts-dsp Processor trait.

use eq_dsp::band::Band;
use eq_dsp::filter_type::{FilterStructure, FilterType};
use fts_dsp::db::{db_to_linear, linear_to_db};
use fts_dsp::{AudioConfig, Processor};

use crate::compressor::{
    Compressor, PEAK_TO_MEAN_DB, RMS_THRESHOLD_OFFSET_DB, SMOOTH_THRESHOLD_OFFSET_DB,
};
use crate::detector::DetectorMode;

// r[impl comp.chain.signal-flow]
// r[impl comp.chain.sidechain-eq]
/// Complete compressor processing chain.
///
/// Signal flow: Input → Lookahead delay → Sidechain HPF → Compressor (detect + reduce + saturate + mix) → Output.
///
/// The sidechain HPF is applied only to the detection path, not the audio.
/// The lookahead delay allows the detector to see transients before they hit the output.
pub struct CompChain {
    pub comp: Compressor,
    sidechain_hpf: Band,
    /// Sidechain HPF frequency. Set to 0 to disable.
    pub sidechain_freq: f64,
    config: AudioConfig,
    // Lookahead delay
    /// Lookahead time in ms.
    pub lookahead_ms: f64,
    /// Lookahead in samples (derived from lookahead_ms and sample rate).
    pub lookahead_samples: usize,
    delay_l: Vec<f64>,
    delay_r: Vec<f64>,
    delay_pos: usize,
}

impl CompChain {
    pub fn new() -> Self {
        let mut sidechain_hpf = Band::new();
        sidechain_hpf.filter_type = FilterType::Highpass;
        sidechain_hpf.structure = FilterStructure::Tdf2;
        sidechain_hpf.freq_hz = 85.0;
        sidechain_hpf.q = 1.0; // Q=1 (0 dB at cutoff, matches Pro-C 3 Versatile)
        sidechain_hpf.order = 2;
        sidechain_hpf.enabled = false;

        Self {
            comp: Compressor::new(),
            sidechain_hpf,
            sidechain_freq: 0.0,
            config: AudioConfig {
                sample_rate: 48000.0,
                max_buffer_size: 512,
            },
            lookahead_ms: 0.0,
            lookahead_samples: 0,
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            delay_pos: 0,
        }
    }

    /// Process a single stereo sample through the full chain.
    ///
    /// Detection always runs on the current (undelayed) input.
    /// When lookahead > 0, gain reduction is applied to the delayed audio.
    pub fn process_sample(&mut self, left: &mut f64, right: &mut f64) {
        // Lookahead: push current into ring buffer, pull delayed sample for output
        let (audio_l, audio_r) = if self.lookahead_samples > 0 {
            let pos = self.delay_pos;
            let dl = self.delay_l[pos];
            let dr = self.delay_r[pos];
            self.delay_l[pos] = *left;
            self.delay_r[pos] = *right;
            self.delay_pos = (pos + 1) % self.lookahead_samples;
            (dl, dr)
        } else {
            (*left, *right)
        };

        // Use inlined path when HPF or lookahead is active (both require separating
        // detection input from audio output). Fall through to compressor for simple case.
        let need_inline = self.sidechain_hpf.enabled || self.lookahead_samples > 0;

        if need_inline {
            // Detection input: HPF-filtered if enabled, otherwise raw current input
            let (det_l, det_r) = if self.sidechain_hpf.enabled {
                (
                    self.sidechain_hpf.tick(*left, 0),
                    self.sidechain_hpf.tick(*right, 1),
                )
            } else {
                (*left, *right)
            };

            // Detect from (possibly filtered) current input
            let level_l = self.comp.detector.tick(det_l.abs(), self.comp.feedback, 0);
            let level_r = self.comp.detector.tick(det_r.abs(), self.comp.feedback, 1);

            // Compute GR from instantaneous levels
            let inertia_decay = 0.99 + (self.comp.inertia_decay * 0.01);
            let mut gr_db = [0.0_f64; 2];
            for ch in 0..2 {
                let level = if ch == 0 { level_l } else { level_r };
                let threshold_offset = match self.comp.detector.mode() {
                    DetectorMode::Peak => PEAK_TO_MEAN_DB,
                    DetectorMode::Rms => RMS_THRESHOLD_OFFSET_DB,
                    DetectorMode::Smooth => SMOOTH_THRESHOLD_OFFSET_DB,
                };
                let raw_gr = self.comp.gain_computer.compute(
                    level,
                    self.comp.threshold_db + threshold_offset,
                    self.comp.ratio,
                    self.comp.knee_db,
                    self.comp.inertia,
                    inertia_decay,
                    ch,
                );
                gr_db[ch] = self.comp.detector.smooth_gr(raw_gr, ch);
                gr_db[ch] = gr_db[ch].min(self.comp.range_db);
            }

            // Channel linking
            let max_gr = gr_db[0].max(gr_db[1]);
            if self.comp.channel_link > 0.0 {
                for ch in 0..2 {
                    gr_db[ch] = (max_gr * self.comp.channel_link)
                        + (gr_db[ch] * (1.0 - self.comp.channel_link));
                }
            }

            // Apply GR to delayed (or current) audio
            let mut output_gain = db_to_linear(self.comp.output_gain_db);
            if self.comp.auto_makeup && self.comp.ratio > 1.0 {
                let makeup_db = -self.comp.threshold_db * (1.0 - 1.0 / self.comp.ratio) * 0.5;
                output_gain *= db_to_linear(makeup_db);
            }

            let input_gain = db_to_linear(self.comp.input_gain_db);
            let dry = [audio_l, audio_r];
            let audios = [audio_l, audio_r];
            let mut outputs = [0.0_f64; 2];

            for ch in 0..2 {
                let s = audios[ch] * input_gain;
                let input_db = linear_to_db(s.abs());
                let sign = if s < 0.0 { -1.0 } else { 1.0 };

                let output_db = input_db - gr_db[ch];
                let mut out = db_to_linear(output_db) * sign;

                if self.comp.ceiling > 0.0 {
                    out /= self.comp.ceiling;
                    out = out.tanh();
                    out *= self.comp.ceiling;
                }

                out *= output_gain;
                self.comp.detector.set_output(out, ch);

                if self.comp.fold < 1.0 {
                    out = out * self.comp.fold + dry[ch] * (1.0 - self.comp.fold);
                }

                if !out.is_finite() {
                    out = 0.0;
                }

                outputs[ch] = out;
                self.comp.last_gr_db[ch] = gr_db[ch];
            }

            *left = outputs[0];
            *right = outputs[1];
        } else {
            // Simple path: no HPF, no lookahead — delegate to compressor
            self.comp.process_sample(left, right);
        }
    }

    /// Set the sidechain HPF frequency.
    ///
    /// Common values: 0 (off), 60, 90, 150, 300 Hz.
    pub fn set_sidechain_freq(&mut self, freq: f64) {
        self.sidechain_freq = freq;
        if freq > 0.0 {
            self.sidechain_hpf.enabled = true;
            self.sidechain_hpf.freq_hz = freq;
            self.sidechain_hpf.update(self.config);
        } else {
            self.sidechain_hpf.enabled = false;
        }
    }

    /// Set the lookahead time in ms. Reallocates the delay buffer.
    pub fn set_lookahead(&mut self, lookahead_ms: f64) {
        self.lookahead_ms = lookahead_ms;
        let n = (lookahead_ms / 1000.0 * self.config.sample_rate).round() as usize;
        if n != self.lookahead_samples {
            self.lookahead_samples = n;
            self.delay_l = vec![0.0; n.max(1)];
            self.delay_r = vec![0.0; n.max(1)];
            self.delay_pos = 0;
        }
    }
}

impl Processor for CompChain {
    fn reset(&mut self) {
        self.comp.reset();
        self.sidechain_hpf.reset();
        self.delay_l.iter_mut().for_each(|x| *x = 0.0);
        self.delay_r.iter_mut().for_each(|x| *x = 0.0);
        self.delay_pos = 0;
    }

    fn update(&mut self, config: AudioConfig) {
        self.config = config;
        self.comp.update(config.sample_rate);
        if self.sidechain_hpf.enabled {
            self.sidechain_hpf.update(config);
        }
        // Re-compute lookahead buffer size for new sample rate
        let n = (self.lookahead_ms / 1000.0 * config.sample_rate).round() as usize;
        if n != self.lookahead_samples {
            self.lookahead_samples = n;
            self.delay_l = vec![0.0; n.max(1)];
            self.delay_r = vec![0.0; n.max(1)];
            self.delay_pos = 0;
        }
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        for i in 0..left.len() {
            self.process_sample(&mut left[i], &mut right[i]);
        }
    }
}

impl Default for CompChain {
    fn default() -> Self {
        Self::new()
    }
}
