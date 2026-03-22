//! pYIN (Probabilistic YIN) pitch detector with HMM smoothing.
//!
//! Extends YIN with:
//! 1. Multiple pitch candidates per frame, weighted by a Beta(2,18) distribution
//! 2. Hidden Markov Model with Viterbi decoding for temporal smoothing
//!
//! Reference: Mauch & Dixon, "pYIN: A Fundamental Frequency Estimator
//! Using Probabilistic Threshold Distributions", ICASSP 2014.

use std::f64::consts::TAU;

use crate::detector::PitchEstimate;

// ── Configuration ───────────────────────────────────────────────────────

const N_THRESHOLDS: usize = 100;

/// Number of pitch bins per semitone (resolution = 1/BPS semitones).
const BINS_PER_SEMITONE: usize = 5;

/// Voiced/unvoiced switch probability.
const SWITCH_PROB: f64 = 0.01;

/// Probability mass assigned when no trough is found.
const NO_TROUGH_PROB: f64 = 0.01;

/// Number of frames to buffer before running Viterbi.
const HMM_WINDOW: usize = 8;

/// Minimum detectable frequency (Hz).
const F_MIN: f64 = 60.0;
/// Maximum detectable frequency (Hz).
const F_MAX: f64 = 1000.0;

// ── Beta Distribution CDF (precomputed) ─────────────────────────────────

/// Precompute the Beta(2,18) CDF at 101 evenly-spaced points in [0,1].
/// Returns the probability mass for each of 100 threshold bins.
fn beta_threshold_distribution() -> [f64; N_THRESHOLDS] {
    // Beta(2,18) CDF: I_x(2,18) = 1 - (1-x)^18 * (1 + 18*x)
    // (closed form for integer parameters)
    let cdf = |x: f64| -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        1.0 - (1.0 - x).powi(18) * (1.0 + 18.0 * x)
    };

    let mut dist = [0.0f64; N_THRESHOLDS];
    let mut prev_cdf = cdf(0.0);
    for i in 0..N_THRESHOLDS {
        let x = (i + 1) as f64 / N_THRESHOLDS as f64;
        let cur_cdf = cdf(x);
        dist[i] = cur_cdf - prev_cdf;
        prev_cdf = cur_cdf;
    }
    dist
}

// ── SVF for pre-filtering (same as detector.rs) ────────────────────────

struct Svf {
    ic1eq: f64,
    ic2eq: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

impl Svf {
    fn new() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 1.0,
        }
    }

    fn set_params(&mut self, freq: f64, q: f64, sample_rate: f64) {
        let g = (TAU * freq / sample_rate / 2.0).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    #[inline]
    fn tick_hp(&mut self, input: f64) -> f64 {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        input - self.k * v1 - v2
    }

    #[inline]
    fn tick_lp(&mut self, input: f64) -> f64 {
        let v3 = input - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        v2
    }

    fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }
}

// ── Pitch Candidate ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Candidate {
    freq_hz: f64,
    probability: f64,
}

// ── pYIN Detector ───────────────────────────────────────────────────────

/// pYIN pitch detector with HMM temporal smoothing.
pub struct PyinDetector {
    /// Reference frequency for A4 (default 440.0).
    pub a_freq: f64,
    /// Silence gate in dB.
    pub gate_db: f64,

    sample_rate: f64,
    window_size: usize,
    hop_size: usize,

    // Ring buffer.
    buffer: Vec<f64>,
    write_pos: usize,
    hop_count: usize,

    // YIN scratch.
    diff: Vec<f64>,
    cmndf: Vec<f64>,

    // Pre-filters.
    hp_filter: Svf,
    lp_filter: Svf,

    // Beta distribution weights.
    beta_dist: [f64; N_THRESHOLDS],

    // HMM state.
    n_pitch_bins: usize,
    /// Frame buffer of observation vectors for windowed Viterbi.
    obs_frames: Vec<Vec<f64>>,
    /// Frequency for each pitch bin.
    bin_freqs: Vec<f64>,
    /// Transition bandwidth in bins.
    trans_width: usize,

    last_estimate: PitchEstimate,
}

impl PyinDetector {
    pub fn new() -> Self {
        let beta_dist = beta_threshold_distribution();
        Self {
            a_freq: 440.0,
            gate_db: -60.0,
            sample_rate: 48000.0,
            window_size: 1024,
            hop_size: 256,
            buffer: vec![0.0; 2048],
            write_pos: 0,
            hop_count: 0,
            diff: vec![0.0; 1024],
            cmndf: vec![0.0; 1024],
            hp_filter: Svf::new(),
            lp_filter: Svf::new(),
            beta_dist,
            n_pitch_bins: 0,
            obs_frames: Vec::new(),
            bin_freqs: Vec::new(),
            trans_width: 0,
            last_estimate: PitchEstimate::unvoiced(),
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.window_size = if sample_rate >= 88200.0 { 2048 } else { 1024 };
        self.hop_size = self.window_size / 4;
        self.buffer = vec![0.0; self.window_size * 2];
        self.diff = vec![0.0; self.window_size];
        self.cmndf = vec![0.0; self.window_size];
        self.write_pos = 0;
        self.hop_count = 0;

        self.hp_filter.set_params(F_MIN, 0.5, sample_rate);
        self.lp_filter.set_params(F_MAX, 0.5, sample_rate);

        // Compute pitch bins.
        let n_semitones = (12.0 * (F_MAX / F_MIN).log2()).ceil() as usize;
        self.n_pitch_bins = n_semitones * BINS_PER_SEMITONE + 1;
        self.bin_freqs = (0..self.n_pitch_bins)
            .map(|i| F_MIN * 2.0f64.powf(i as f64 / (12.0 * BINS_PER_SEMITONE as f64)))
            .collect();

        // Transition width: ~10 semitones per frame at 35.92 oct/s.
        let max_st_per_frame = (35.92 * 12.0 * self.hop_size as f64 / sample_rate).round() as usize;
        self.trans_width = max_st_per_frame * BINS_PER_SEMITONE + 1;

        self.obs_frames.clear();
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.hop_count = 0;
        self.hp_filter.reset();
        self.lp_filter.reset();
        self.obs_frames.clear();
        self.last_estimate = PitchEstimate::unvoiced();
    }

    #[inline]
    pub fn tick(&mut self, input: f64) -> PitchEstimate {
        let filtered = self.lp_filter.tick_lp(self.hp_filter.tick_hp(input));
        let buf_len = self.buffer.len();
        self.buffer[self.write_pos] = filtered;
        self.write_pos = (self.write_pos + 1) % buf_len;
        self.hop_count += 1;

        if self.hop_count >= self.hop_size {
            self.hop_count = 0;
            self.analyze_frame();
        }

        self.last_estimate
    }

    pub fn current(&self) -> PitchEstimate {
        self.last_estimate
    }

    pub fn latency(&self) -> usize {
        self.window_size + self.hop_size * HMM_WINDOW
    }

    // ── YIN candidate extraction ────────────────────────────────────────

    fn analyze_frame(&mut self) {
        let w = self.window_size;
        let buf_len = self.buffer.len();

        // Gate check.
        let mut rms = 0.0f64;
        for i in 0..w {
            let idx = (self.write_pos + buf_len - w + i) % buf_len;
            let s = self.buffer[idx];
            rms += s * s;
        }
        rms = (rms / w as f64).sqrt();
        let db = if rms > 1e-20 {
            20.0 * rms.log10()
        } else {
            -120.0
        };

        if db < self.gate_db {
            // Unvoiced observation.
            let n_bins = self.n_pitch_bins;
            let mut obs = vec![0.0f64; 2 * n_bins];
            let unvoiced_prob = 1.0 / n_bins as f64;
            for i in n_bins..2 * n_bins {
                obs[i] = unvoiced_prob;
            }
            self.push_observation(obs);
            return;
        }

        // Compute CMNDF.
        let min_lag = (self.sample_rate / F_MAX).floor() as usize;
        let max_lag = ((self.sample_rate / F_MIN).ceil() as usize).min(w - 1);
        if min_lag >= max_lag {
            self.push_unvoiced_observation();
            return;
        }

        self.compute_cmndf(max_lag);

        // Extract candidates using probabilistic thresholds.
        let candidates = self.extract_candidates(min_lag, max_lag);

        // Build HMM observation vector.
        self.build_observation(candidates);
    }

    fn compute_cmndf(&mut self, max_lag: usize) {
        let w = self.window_size;
        let buf_len = self.buffer.len();

        self.diff[0] = 0.0;
        for tau in 1..=max_lag {
            let mut sum = 0.0;
            for j in 0..(w - tau) {
                let a = (self.write_pos + buf_len - w + j) % buf_len;
                let b = (self.write_pos + buf_len - w + j + tau) % buf_len;
                let d = self.buffer[a] - self.buffer[b];
                sum += d * d;
            }
            self.diff[tau] = sum;
        }

        self.cmndf[0] = 1.0;
        let mut running = 0.0;
        for tau in 1..=max_lag {
            running += self.diff[tau];
            self.cmndf[tau] = if running > 1e-20 {
                self.diff[tau] * tau as f64 / running
            } else {
                1.0
            };
        }
    }

    fn extract_candidates(&self, min_lag: usize, max_lag: usize) -> Vec<Candidate> {
        // Find all troughs (local minima) in CMNDF, sorted by lag (ascending).
        let mut troughs: Vec<(usize, f64)> = Vec::new();
        for tau in (min_lag + 1)..max_lag {
            if self.cmndf[tau] < self.cmndf[tau - 1] && self.cmndf[tau] <= self.cmndf[tau + 1] {
                troughs.push((tau, self.cmndf[tau]));
            }
        }

        // If no troughs, use global minimum with low probability.
        if troughs.is_empty() {
            let mut best_tau = min_lag;
            let mut best_val = f64::MAX;
            for tau in min_lag..=max_lag {
                if self.cmndf[tau] < best_val {
                    best_val = self.cmndf[tau];
                    best_tau = tau;
                }
            }
            let period = self.parabolic_interp(best_tau, max_lag);
            if period > 0.0 {
                return vec![Candidate {
                    freq_hz: self.sample_rate / period,
                    probability: NO_TROUGH_PROB,
                }];
            }
            return Vec::new();
        }

        // pYIN probabilistic thresholds: for each threshold level,
        // find the FIRST trough whose CMNDF is below that threshold.
        // Only that trough gets the beta weight for that threshold.
        // This naturally favors the fundamental (shortest lag).
        let mut trough_probs = vec![0.0f64; troughs.len()];

        for (i, &beta_w) in self.beta_dist.iter().enumerate() {
            let threshold = (i + 1) as f64 / N_THRESHOLDS as f64;

            // Find the first trough below this threshold (troughs are lag-sorted).
            for (t_idx, &(_tau, cmndf_val)) in troughs.iter().enumerate() {
                if cmndf_val < threshold {
                    trough_probs[t_idx] += beta_w;
                    break;
                }
            }
        }

        let mut candidates = Vec::with_capacity(troughs.len());
        for (t_idx, &(tau, _cmndf_val)) in troughs.iter().enumerate() {
            let prob = trough_probs[t_idx];
            if prob > 1e-10 {
                let period = self.parabolic_interp(tau, max_lag);
                if period > 0.0 {
                    candidates.push(Candidate {
                        freq_hz: self.sample_rate / period,
                        probability: prob,
                    });
                }
            }
        }

        // Normalize if total exceeds 1.
        let total: f64 = candidates.iter().map(|c| c.probability).sum();
        if total > 1.0 {
            for c in &mut candidates {
                c.probability /= total;
            }
        }

        candidates
    }

    fn parabolic_interp(&self, tau: usize, max_lag: usize) -> f64 {
        if tau > 0 && tau < max_lag {
            let a = self.cmndf[tau - 1];
            let b = self.cmndf[tau];
            let c = self.cmndf[tau + 1];
            let denom = 2.0 * (2.0 * b - a - c);
            if denom.abs() > 1e-20 {
                return tau as f64 + (a - c) / denom;
            }
        }
        tau as f64
    }

    // ── HMM observation + Viterbi ───────────────────────────────────────

    fn build_observation(&mut self, candidates: Vec<Candidate>) {
        let n = self.n_pitch_bins;
        let mut obs = vec![0.0f64; 2 * n];

        // Map candidates to pitch bins.
        for c in &candidates {
            if c.freq_hz >= F_MIN && c.freq_hz <= F_MAX {
                let bin =
                    (12.0 * BINS_PER_SEMITONE as f64 * (c.freq_hz / F_MIN).log2()).round() as usize;
                if bin < n {
                    obs[bin] += c.probability;
                }
            }
        }

        // Voiced probability = sum of voiced bins.
        let voiced_prob: f64 = obs[..n].iter().sum::<f64>().min(1.0);

        // Unvoiced bins get uniform share of remaining probability.
        let unvoiced_prob = (1.0 - voiced_prob) / n as f64;
        for i in n..2 * n {
            obs[i] = unvoiced_prob;
        }

        self.push_observation(obs);
    }

    fn push_unvoiced_observation(&mut self) {
        let n = self.n_pitch_bins;
        let mut obs = vec![0.0f64; 2 * n];
        let p = 1.0 / n as f64;
        for i in n..2 * n {
            obs[i] = p;
        }
        self.push_observation(obs);
    }

    fn push_observation(&mut self, obs: Vec<f64>) {
        self.obs_frames.push(obs);

        // Run Viterbi on the buffered window.
        if self.obs_frames.len() >= HMM_WINDOW {
            let result = self.viterbi_decode();
            self.last_estimate = result;
            // Keep last frame for continuity.
            let last = self.obs_frames.pop().unwrap();
            self.obs_frames.clear();
            self.obs_frames.push(last);
        }
    }

    fn viterbi_decode(&self) -> PitchEstimate {
        let n = self.n_pitch_bins;
        let n_states = 2 * n;
        let t_len = self.obs_frames.len();
        if t_len == 0 || n == 0 {
            return PitchEstimate::unvoiced();
        }

        let tw = self.trans_width.min(n);
        let eps = 1e-300;

        // Initialize (log space).
        let init_log = -(n_states as f64).ln();
        let mut prev = vec![f64::NEG_INFINITY; n_states];
        for j in 0..n_states {
            let obs_p = self.obs_frames[0].get(j).copied().unwrap_or(0.0);
            prev[j] = init_log + (obs_p + eps).ln();
        }

        let mut backptr = vec![vec![0usize; n_states]; t_len];

        // Precompute triangle transition weights.
        let mut tri_weights = vec![0.0f64; 2 * tw + 1];
        let total_area = (tw + 1) as f64;
        for d in 0..=tw {
            tri_weights[tw + d] = (tw + 1 - d) as f64 / total_area;
            tri_weights[tw - d] = (tw + 1 - d) as f64 / total_area;
        }

        let self_trans = 1.0 - SWITCH_PROB;

        // Forward pass.
        for t in 1..t_len {
            let mut curr = vec![f64::NEG_INFINITY; n_states];

            for j in 0..n_states {
                let obs_log = (self.obs_frames[t].get(j).copied().unwrap_or(0.0) + eps).ln();
                let j_voiced = j < n;
                let j_pitch = if j_voiced { j } else { j - n };

                let mut best_val = f64::NEG_INFINITY;
                let mut best_from = 0;

                // Only search within transition bandwidth.
                let pitch_lo = j_pitch.saturating_sub(tw);
                let pitch_hi = (j_pitch + tw).min(n - 1);

                for k_pitch in pitch_lo..=pitch_hi {
                    let d = if k_pitch > j_pitch {
                        k_pitch - j_pitch
                    } else {
                        j_pitch - k_pitch
                    };
                    let pitch_trans = tri_weights[tw + d];

                    // Same voicing state.
                    let k_same = if j_voiced { k_pitch } else { k_pitch + n };
                    let trans_log = (self_trans * pitch_trans + eps).ln();
                    let val = prev[k_same] + trans_log;
                    if val > best_val {
                        best_val = val;
                        best_from = k_same;
                    }

                    // Switch voicing state.
                    let k_switch = if j_voiced { k_pitch + n } else { k_pitch };
                    let trans_log_sw = (SWITCH_PROB * pitch_trans + eps).ln();
                    let val_sw = prev[k_switch] + trans_log_sw;
                    if val_sw > best_val {
                        best_val = val_sw;
                        best_from = k_switch;
                    }
                }

                curr[j] = best_val + obs_log;
                backptr[t][j] = best_from;
            }

            prev = curr;
        }

        // Backtrack: find best final state.
        let mut best_state = 0;
        let mut best_val = f64::NEG_INFINITY;
        for j in 0..n_states {
            if prev[j] > best_val {
                best_val = prev[j];
                best_state = j;
            }
        }

        // Trace back to get the last decoded frame.
        let mut state = best_state;
        for t in (1..t_len).rev() {
            state = backptr[t][state];
        }
        // We want the most recent frame, so trace forward from the best path.
        // Actually, we want the LAST frame's state.
        let final_state = best_state;

        if final_state < n {
            // Voiced.
            let freq = self.bin_freqs[final_state];
            let semitones = 12.0 * (freq / self.a_freq).log2();
            let midi_note = 69.0 + semitones;
            // Confidence from the observation probability.
            let confidence = self
                .obs_frames
                .last()
                .and_then(|obs| obs.get(final_state))
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            // Scale confidence: pYIN voiced probabilities tend to be small.
            let confidence = (confidence * 5.0).min(1.0);

            PitchEstimate {
                freq_hz: freq,
                semitones,
                midi_note,
                confidence,
            }
        } else {
            PitchEstimate::unvoiced()
        }
    }
}

impl Default for PyinDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_detector() -> PyinDetector {
        let mut d = PyinDetector::new();
        d.update(SR);
        d
    }

    fn generate_sine(freq: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * 0.8)
            .collect()
    }

    #[test]
    fn detects_a4_440hz() {
        let mut d = make_detector();
        let signal = generate_sine(440.0, 96000); // 2 seconds for HMM warmup

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            last.confidence > 0.1,
            "Should detect A4, confidence={}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 440.0).abs() < 15.0,
            "Should detect ~440Hz, got {}",
            last.freq_hz
        );
    }

    #[test]
    fn detects_low_e_82hz() {
        let mut d = make_detector();
        let signal = generate_sine(82.41, 96000);

        let mut last = PitchEstimate::unvoiced();
        for &s in &signal {
            last = d.tick(s);
        }

        assert!(
            last.confidence > 0.1,
            "Should detect low E, confidence={}",
            last.confidence
        );
        assert!(
            (last.freq_hz - 82.41).abs() < 5.0,
            "Should detect ~82Hz, got {}",
            last.freq_hz
        );
    }

    #[test]
    fn silence_is_unvoiced() {
        let mut d = make_detector();
        let mut last = PitchEstimate::unvoiced();
        for _ in 0..48000 {
            last = d.tick(0.0);
        }
        assert!(last.confidence < 0.1, "Silence should be unvoiced");
    }

    #[test]
    fn no_nan() {
        let mut d = make_detector();
        let signal = generate_sine(220.0, 48000);
        for &s in &signal {
            let est = d.tick(s);
            assert!(est.freq_hz.is_finite());
            assert!(est.semitones.is_finite());
            assert!(est.confidence.is_finite());
        }
    }
}
