//! Compressor chain — full signal path with sidechain EQ.
//!
//! Wraps the core Compressor with a sidechain highpass filter
//! (from eq-dsp) and implements the fts-dsp Processor trait.

use eq_dsp::band::Band;
use eq_dsp::filter_type::{FilterStructure, FilterType};
use fts_dsp::db::{db_to_linear, linear_to_db};
use fts_dsp::{AudioConfig, Processor};

use crate::compressor::Compressor;

// r[impl comp.chain.signal-flow]
// r[impl comp.chain.sidechain-eq]
/// Complete compressor processing chain.
///
/// Signal flow: Input → Sidechain HPF → Compressor (detect + reduce + saturate + mix) → Output.
///
/// The sidechain HPF is applied only to the detection path, not the audio.
/// This prevents bass-heavy content from driving excessive gain reduction.
pub struct CompChain {
    pub comp: Compressor,
    sidechain_hpf: Band,
    /// Sidechain HPF frequency. Set to 0 to disable.
    pub sidechain_freq: f64,
    config: AudioConfig,
}

impl CompChain {
    pub fn new() -> Self {
        let mut sidechain_hpf = Band::new();
        sidechain_hpf.filter_type = FilterType::Highpass;
        sidechain_hpf.structure = FilterStructure::Tdf2;
        sidechain_hpf.freq_hz = 90.0;
        sidechain_hpf.q = 0.707;
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
}

impl Processor for CompChain {
    fn reset(&mut self) {
        self.comp.reset();
        self.sidechain_hpf.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.config = config;
        self.comp.update(config.sample_rate);
        if self.sidechain_hpf.enabled {
            self.sidechain_hpf.update(config);
        }
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let use_sc_hpf = self.sidechain_hpf.enabled;

        for i in 0..left.len() {
            // If sidechain HPF is active, filter the detection signal
            // The compressor's detector will use the filtered version
            if use_sc_hpf {
                // We apply the HPF to copies for detection only.
                // The compressor internally uses the raw input for audio path,
                // and we override the detector level with the filtered version.
                let sc_l = self.sidechain_hpf.tick(left[i], 0);
                let sc_r = self.sidechain_hpf.tick(right[i], 1);

                // Detect from filtered sidechain
                let level_l = self.comp.detector.tick(sc_l.abs(), self.comp.feedback, 0);
                let level_r = self.comp.detector.tick(sc_r.abs(), self.comp.feedback, 1);

                // Compute gain reduction from filtered levels
                let inertia_decay = 0.99 + (self.comp.inertia_decay * 0.01);
                let mut gr_db = [0.0_f64; 2];
                for ch in 0..2 {
                    let level = if ch == 0 { level_l } else { level_r };
                    gr_db[ch] = self.comp.gain_computer.compute(
                        level,
                        self.comp.threshold_db,
                        self.comp.ratio,
                        self.comp.knee_db,
                        self.comp.inertia,
                        inertia_decay,
                        ch,
                    );
                }

                // Channel linking
                let max_gr = gr_db[0].max(gr_db[1]);
                if self.comp.channel_link > 0.0 {
                    for ch in 0..2 {
                        gr_db[ch] = (max_gr * self.comp.channel_link)
                            + (gr_db[ch] * (1.0 - self.comp.channel_link));
                    }
                }

                // Apply gain reduction to original (unfiltered) audio
                let input_gain = db_to_linear(self.comp.input_gain_db);
                let output_gain = db_to_linear(self.comp.output_gain_db);
                let dry = [left[i], right[i]];

                for (ch, sample) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                    let s = *sample * input_gain;
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

                    if self.comp.fold > 0.0 {
                        out = out * (1.0 - self.comp.fold) + dry[ch] * self.comp.fold;
                    }

                    if !out.is_finite() {
                        out = 0.0;
                    }

                    *sample = out;
                    self.comp.last_gr_db[ch] = gr_db[ch];
                }
            } else {
                // No sidechain HPF — use the compressor's built-in processing
                self.comp.process_sample(&mut left[i], &mut right[i]);
            }
        }
    }
}

impl Default for CompChain {
    fn default() -> Self {
        Self::new()
    }
}
