//! Compressor chain — wrapper with lookahead delay and sidechain EQ.

use fts_dsp::AudioConfig;

/// Complete compressor processing chain.
pub struct CompChain {
    pub comp: super::ProC3Compressor,
    // Sidechain EQ would go here in full implementation
    // For now, just provide the interface
    pub sidechain_freq: f64,
    lookahead_ms: f64,
    pub lookahead_samples: usize,
    delay_l: Vec<f64>,
    delay_r: Vec<f64>,
    delay_pos: usize,
    sample_rate: f64,
}

impl CompChain {
    pub fn new() -> Self {
        Self {
            comp: super::ProC3Compressor::new(48000.0),
            sidechain_freq: 0.0,
            lookahead_ms: 0.0,
            lookahead_samples: 0,
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            delay_pos: 0,
            sample_rate: 48000.0,
        }
    }

    /// Process a single stereo sample through the full chain.
    pub fn process_sample(&mut self, left: &mut f64, right: &mut f64) {
        // Handle lookahead delay buffer
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

        // Process stereo pair through compressor
        let out_l = self.comp.process(audio_l, 0);
        let out_r = self.comp.process(audio_r, 1);

        *left = out_l;
        *right = out_r;
    }

    /// Set the sidechain HPF frequency (not implemented in v2).
    pub fn set_sidechain_freq(&mut self, freq: f64) {
        self.sidechain_freq = freq;
        // TODO: Implement sidechain HPF using eq-dsp
    }

    /// Set the lookahead time in ms.
    pub fn set_lookahead(&mut self, lookahead_ms: f64) {
        self.lookahead_ms = lookahead_ms;
        let n = (lookahead_ms / 1000.0 * self.sample_rate).round() as usize;
        if n != self.lookahead_samples {
            self.lookahead_samples = n;
            self.delay_l = vec![0.0; n.max(1)];
            self.delay_r = vec![0.0; n.max(1)];
            self.delay_pos = 0;
        }
    }

    /// Update sample rate (used when format changes).
    pub fn update_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        // Rebuild lookahead buffers if needed
        if self.lookahead_ms > 0.0 {
            self.set_lookahead(self.lookahead_ms);
        }
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        self.comp.reset();
        self.delay_l.iter_mut().for_each(|x| *x = 0.0);
        self.delay_r.iter_mut().for_each(|x| *x = 0.0);
        self.delay_pos = 0;
    }

    /// Update to new audio config (called when sample rate or buffer size changes).
    pub fn update(&mut self, config: AudioConfig) {
        self.comp.update(config.sample_rate);
        self.update_sample_rate(config.sample_rate);
    }
}

impl Default for CompChain {
    fn default() -> Self {
        Self::new()
    }
}
