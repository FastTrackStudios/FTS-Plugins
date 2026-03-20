//! GateChain — complete gate processing chain with sidechain EQ and lookahead.

use eq_dsp::band::Band;
use eq_dsp::filter_type::{FilterStructure, FilterType};
use fts_dsp::{AudioConfig, Processor};

use crate::detector::GateDetector;
use crate::envelope::GateEnvelope;

// r[impl gate.chain.signal-flow]
// r[impl gate.detector.lookahead]
/// Complete gate processing chain.
///
/// Signal flow: Input → Lookahead delay → gain modulation by envelope.
/// Sidechain: Input → HPF/LPF → detector → envelope.
///
/// The sidechain path is separate from the audio path, so the filters
/// only affect detection, not the audio itself.
pub struct GateChain {
    pub detector: GateDetector,
    pub envelope: GateEnvelope,

    // Sidechain filters
    sc_hpf: Band,
    sc_lpf: Band,

    // Lookahead delay buffer (circular, stereo)
    lookahead_buf: Vec<[f64; 2]>,
    lookahead_pos: usize,
    lookahead_samples: usize,

    // Parameters (public for direct access)
    pub open_threshold_db: f64,
    pub close_threshold_db: f64,
    pub attack_ms: f64,
    pub hold_ms: f64,
    pub release_ms: f64,
    pub range_db: f64,
    pub lookahead_ms: f64,
    pub sc_hpf_freq: f64,
    pub sc_lpf_freq: f64,
    pub sc_listen: bool,

    config: AudioConfig,
    /// Last gate gain per channel (for metering).
    pub last_gain: [f64; 2],
}

impl GateChain {
    pub fn new() -> Self {
        let mut sc_hpf = Band::new();
        sc_hpf.filter_type = FilterType::Highpass;
        sc_hpf.structure = FilterStructure::Tdf2;
        sc_hpf.freq_hz = 100.0;
        sc_hpf.q = 0.707;
        sc_hpf.order = 2;
        sc_hpf.enabled = false;

        let mut sc_lpf = Band::new();
        sc_lpf.filter_type = FilterType::Lowpass;
        sc_lpf.structure = FilterStructure::Tdf2;
        sc_lpf.freq_hz = 10000.0;
        sc_lpf.q = 0.707;
        sc_lpf.order = 2;
        sc_lpf.enabled = false;

        Self {
            detector: GateDetector::new(),
            envelope: GateEnvelope::new(),
            sc_hpf,
            sc_lpf,
            lookahead_buf: Vec::new(),
            lookahead_pos: 0,
            lookahead_samples: 0,
            open_threshold_db: -40.0,
            close_threshold_db: -50.0,
            attack_ms: 0.5,
            hold_ms: 50.0,
            release_ms: 100.0,
            range_db: -80.0,
            lookahead_ms: 0.0,
            sc_hpf_freq: 0.0,
            sc_lpf_freq: 0.0,
            sc_listen: false,
            config: AudioConfig {
                sample_rate: 48000.0,
                max_buffer_size: 512,
            },
            last_gain: [0.0; 2],
        }
    }

    /// Set sidechain HPF frequency (0 = off).
    pub fn set_sc_hpf(&mut self, freq: f64) {
        self.sc_hpf_freq = freq;
        if freq > 0.0 {
            self.sc_hpf.enabled = true;
            self.sc_hpf.freq_hz = freq;
            self.sc_hpf.update(self.config);
        } else {
            self.sc_hpf.enabled = false;
        }
    }

    /// Set sidechain LPF frequency (0 = off).
    pub fn set_sc_lpf(&mut self, freq: f64) {
        self.sc_lpf_freq = freq;
        if freq > 0.0 {
            self.sc_lpf.enabled = true;
            self.sc_lpf.freq_hz = freq;
            self.sc_lpf.update(self.config);
        } else {
            self.sc_lpf.enabled = false;
        }
    }

    /// Update the lookahead buffer size.
    fn update_lookahead(&mut self) {
        let samples = (self.lookahead_ms * 0.001 * self.config.sample_rate) as usize;
        if samples != self.lookahead_samples {
            self.lookahead_samples = samples;
            if samples > 0 {
                self.lookahead_buf = vec![[0.0; 2]; samples];
            } else {
                self.lookahead_buf.clear();
            }
            self.lookahead_pos = 0;
        }
    }

    /// Read from the lookahead delay, write the current sample.
    #[inline]
    fn lookahead_delay(&mut self, left: f64, right: f64) -> (f64, f64) {
        if self.lookahead_samples == 0 {
            return (left, right);
        }
        let delayed = self.lookahead_buf[self.lookahead_pos];
        self.lookahead_buf[self.lookahead_pos] = [left, right];
        self.lookahead_pos = (self.lookahead_pos + 1) % self.lookahead_samples;
        (delayed[0], delayed[1])
    }

    /// Get the latency in samples introduced by lookahead.
    pub fn latency_samples(&self) -> usize {
        self.lookahead_samples
    }
}

impl Processor for GateChain {
    fn reset(&mut self) {
        self.detector.reset();
        self.envelope.reset();
        self.sc_hpf.reset();
        self.sc_lpf.reset();
        for s in &mut self.lookahead_buf {
            *s = [0.0; 2];
        }
        self.lookahead_pos = 0;
        self.last_gain = [0.0; 2];
    }

    fn update(&mut self, config: AudioConfig) {
        self.config = config;
        self.detector.set_sample_rate(config.sample_rate);
        self.envelope.set_params(
            self.attack_ms,
            self.hold_ms,
            self.release_ms,
            self.range_db,
            config.sample_rate,
        );
        if self.sc_hpf.enabled {
            self.sc_hpf.update(config);
        }
        if self.sc_lpf.enabled {
            self.sc_lpf.update(config);
        }
        self.update_lookahead();
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        for i in 0..left.len() {
            // Sidechain path: filter for detection
            let mut sc_l = left[i];
            let mut sc_r = right[i];

            if self.sc_hpf.enabled {
                sc_l = self.sc_hpf.tick(sc_l, 0);
                sc_r = self.sc_hpf.tick(sc_r, 1);
            }
            if self.sc_lpf.enabled {
                sc_l = self.sc_lpf.tick(sc_l, 0);
                sc_r = self.sc_lpf.tick(sc_r, 1);
            }

            // Sidechain listen mode: output the filtered sidechain
            // r[impl gate.plugin.sidechain-listen]
            if self.sc_listen {
                left[i] = sc_l;
                right[i] = sc_r;
                continue;
            }

            // Detection (mono sum for stereo-linked gate)
            let sc_mono = (sc_l + sc_r) * 0.5;
            let gate_open =
                self.detector
                    .tick(sc_mono, self.open_threshold_db, self.close_threshold_db, 0);

            // Envelope shaping
            let gain = self.envelope.tick(gate_open, 0);

            // Apply lookahead delay to audio (detection uses current sample)
            let (delayed_l, delayed_r) = self.lookahead_delay(left[i], right[i]);

            // Apply gain
            left[i] = delayed_l * gain;
            right[i] = delayed_r * gain;

            self.last_gain[0] = gain;
            self.last_gain[1] = gain;
        }
    }
}

impl Default for GateChain {
    fn default() -> Self {
        Self::new()
    }
}
