//! Timbre-based drum classifier using 4-band energy ratios.
//!
//! Splits the sidechain signal into sub-bands via Linkwitz-Riley crossovers
//! and classifies onsets by their energy distribution across bands.

use fts_dsp::biquad::{Biquad, FilterType};
use fts_dsp::envelope::EnvelopeFollower;

/// Drum type classification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumClass {
    Kick,
    Snare,
    HiHat,
    Tom,
    Unknown,
}

/// Crossover frequencies for the 4-band split.
const CROSS_LO: f64 = 150.0;
const CROSS_MID: f64 = 1000.0;
const CROSS_HI: f64 = 5000.0;

/// Band indices.
const SUB: usize = 0; // < 150 Hz
const LOW_MID: usize = 1; // 150 Hz – 1 kHz
const HI_MID: usize = 2; // 1 kHz – 5 kHz
const HIGH: usize = 3; // > 5 kHz

/// Number of bands.
pub const NUM_BANDS: usize = 4;

/// Timbre classifier with 4-band crossover and onset energy analysis.
pub struct TimbreClassifier {
    // Linkwitz-Riley crossovers (2x 2nd-order = 4th-order LR)
    // 3 crossover points, each needs a LP and HP pair (cascaded twice for LR4)
    xover_lo_lp: [Biquad; 2],
    xover_lo_hp: [Biquad; 2],
    xover_mid_lp: [Biquad; 2],
    xover_mid_hp: [Biquad; 2],
    xover_hi_lp: [Biquad; 2],
    xover_hi_hp: [Biquad; 2],

    // Per-band envelope followers (fast attack for onset energy capture)
    band_env: [EnvelopeFollower; NUM_BANDS],

    // Per-band peak energy captured at onset
    band_energy: [f64; NUM_BANDS],

    // Onset detection: fast/slow envelope ratio
    fast_env: EnvelopeFollower,
    slow_env: EnvelopeFollower,

    // Configuration
    pub target_drum: DrumClass,
    pub strictness: f64,
    pub enabled: bool,

    // State
    onset_active: bool,
    onset_samples: usize,
    last_classification: DrumClass,
    /// Duration (in samples) to accumulate energy after onset.
    capture_samples: usize,

    sample_rate: f64,
}

impl TimbreClassifier {
    pub fn new() -> Self {
        Self {
            xover_lo_lp: [Biquad::new(), Biquad::new()],
            xover_lo_hp: [Biquad::new(), Biquad::new()],
            xover_mid_lp: [Biquad::new(), Biquad::new()],
            xover_mid_hp: [Biquad::new(), Biquad::new()],
            xover_hi_lp: [Biquad::new(), Biquad::new()],
            xover_hi_hp: [Biquad::new(), Biquad::new()],
            band_env: std::array::from_fn(|_| EnvelopeFollower::new(0.0)),
            band_energy: [0.0; NUM_BANDS],
            fast_env: EnvelopeFollower::new(0.0),
            slow_env: EnvelopeFollower::new(0.0),
            target_drum: DrumClass::Unknown,
            strictness: 0.5,
            enabled: false,
            onset_active: false,
            onset_samples: 0,
            last_classification: DrumClass::Unknown,
            capture_samples: 480, // 10ms at 48kHz
            sample_rate: 48000.0,
        }
    }

    /// Update crossover filters and envelope times for the given sample rate.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.capture_samples = (sample_rate * 0.010) as usize; // 10ms

        let q = std::f64::consts::FRAC_1_SQRT_2; // Butterworth Q for LR4

        // Low crossover (150 Hz)
        for bq in &mut self.xover_lo_lp {
            bq.set(FilterType::Lowpass, CROSS_LO, q, sample_rate);
        }
        for bq in &mut self.xover_lo_hp {
            bq.set(FilterType::Highpass, CROSS_LO, q, sample_rate);
        }

        // Mid crossover (1 kHz)
        for bq in &mut self.xover_mid_lp {
            bq.set(FilterType::Lowpass, CROSS_MID, q, sample_rate);
        }
        for bq in &mut self.xover_mid_hp {
            bq.set(FilterType::Highpass, CROSS_MID, q, sample_rate);
        }

        // High crossover (5 kHz)
        for bq in &mut self.xover_hi_lp {
            bq.set(FilterType::Lowpass, CROSS_HI, q, sample_rate);
        }
        for bq in &mut self.xover_hi_hp {
            bq.set(FilterType::Highpass, CROSS_HI, q, sample_rate);
        }

        // Band envelopes: 0.5ms attack, 20ms release
        for env in &mut self.band_env {
            env.set_times_ms(0.5, 20.0, sample_rate);
        }

        // Onset detection envelopes
        self.fast_env.set_times_ms(0.1, 5.0, sample_rate);
        self.slow_env.set_times_ms(5.0, 100.0, sample_rate);
    }

    /// Process one mono sidechain sample.
    ///
    /// Returns `true` if the current signal should be passed (matches target
    /// drum or classifier is disabled), `false` if it should be rejected.
    ///
    /// Also returns the detected drum class.
    pub fn tick(&mut self, sample: f64) -> (bool, DrumClass) {
        if !self.enabled {
            return (true, DrumClass::Unknown);
        }

        let abs_sample = sample.abs();

        // ── Onset detection via fast/slow ratio ──
        let fast = self.fast_env.tick(abs_sample);
        let slow = self.slow_env.tick(abs_sample);

        let onset_ratio = if slow > 1e-10 { fast / slow } else { 0.0 };
        let onset_threshold = 3.0; // Fast envelope must be 3x slow

        if !self.onset_active && onset_ratio > onset_threshold && abs_sample > 1e-6 {
            // New onset detected — start energy capture
            self.onset_active = true;
            self.onset_samples = 0;
            self.band_energy = [0.0; NUM_BANDS];
        }

        // ── 4-band crossover split ──
        // Stage 1: Split at low crossover (150 Hz)
        let lo1 = self.xover_lo_lp[0].tick(sample, 0);
        let lo = self.xover_lo_lp[1].tick(lo1, 0);
        let hi1 = self.xover_lo_hp[0].tick(sample, 0);
        let above_lo = self.xover_lo_hp[1].tick(hi1, 0);

        // Stage 2: Split above_lo at mid crossover (1 kHz)
        let mid1 = self.xover_mid_lp[0].tick(above_lo, 0);
        let mid = self.xover_mid_lp[1].tick(mid1, 0);
        let hi2 = self.xover_mid_hp[0].tick(above_lo, 0);
        let above_mid = self.xover_mid_hp[1].tick(hi2, 0);

        // Stage 3: Split above_mid at high crossover (5 kHz)
        let himid1 = self.xover_hi_lp[0].tick(above_mid, 0);
        let himid = self.xover_hi_lp[1].tick(himid1, 0);
        let hihi1 = self.xover_hi_hp[0].tick(above_mid, 0);
        let hi = self.xover_hi_hp[1].tick(hihi1, 0);

        let bands = [lo, mid, himid, hi];

        // ── Accumulate band energies during onset window ──
        for (i, &b) in bands.iter().enumerate() {
            let env = self.band_env[i].tick(b.abs());
            if self.onset_active {
                self.band_energy[i] += env * env; // RMS-like accumulation
            }
        }

        if self.onset_active {
            self.onset_samples += 1;
            if self.onset_samples >= self.capture_samples {
                // Classify based on accumulated energy
                self.last_classification = self.classify_from_energy();
                self.onset_active = false;
            }
        }

        // ── Gate decision ──
        let pass = match self.target_drum {
            DrumClass::Unknown => true, // No target = pass all
            target => {
                self.last_classification == target || self.last_classification == DrumClass::Unknown
            }
        };

        (pass, self.last_classification)
    }

    /// Classify drum type from accumulated band energy ratios.
    fn classify_from_energy(&self) -> DrumClass {
        let total: f64 = self.band_energy.iter().sum();
        if total < 1e-20 {
            return DrumClass::Unknown;
        }

        let r: [f64; NUM_BANDS] = std::array::from_fn(|i| self.band_energy[i] / total);

        // Strictness adjusts thresholds: higher = stricter matching
        let s = self.strictness;

        // Hi-hat: dominant high-frequency energy
        if r[HIGH] > 0.35 + s * 0.15 {
            return DrumClass::HiHat;
        }

        // Kick: dominant sub energy, minimal highs
        if r[SUB] > 0.30 + s * 0.10 && r[HIGH] < 0.20 - s * 0.05 {
            // Distinguish kick from tom: kicks have less mid content
            if r[LOW_MID] > 0.25 + s * 0.05 {
                return DrumClass::Tom;
            }
            return DrumClass::Kick;
        }

        // Snare: bimodal — mids + highs, with body in low-mid
        if r[HI_MID] > 0.15 + s * 0.05 && r[HIGH] > 0.10 + s * 0.05 && r[LOW_MID] > 0.10 + s * 0.05
        {
            return DrumClass::Snare;
        }

        // Tom: mid-range dominant
        if r[LOW_MID] > 0.30 + s * 0.10 && r[SUB] > 0.15 {
            return DrumClass::Tom;
        }

        DrumClass::Unknown
    }

    /// Get the last classification result.
    pub fn last_class(&self) -> DrumClass {
        self.last_classification
    }

    /// Get current band energy ratios (for UI metering).
    pub fn band_ratios(&self) -> [f64; NUM_BANDS] {
        let total: f64 = self.band_energy.iter().sum();
        if total < 1e-20 {
            return [0.0; NUM_BANDS];
        }
        std::array::from_fn(|i| self.band_energy[i] / total)
    }

    pub fn reset(&mut self) {
        for bq in self
            .xover_lo_lp
            .iter_mut()
            .chain(self.xover_lo_hp.iter_mut())
            .chain(self.xover_mid_lp.iter_mut())
            .chain(self.xover_mid_hp.iter_mut())
            .chain(self.xover_hi_lp.iter_mut())
            .chain(self.xover_hi_hp.iter_mut())
        {
            bq.reset();
        }
        for env in &mut self.band_env {
            env.reset(0.0);
        }
        self.fast_env.reset(0.0);
        self.slow_env.reset(0.0);
        self.band_energy = [0.0; NUM_BANDS];
        self.onset_active = false;
        self.onset_samples = 0;
        self.last_classification = DrumClass::Unknown;
    }
}

impl Default for TimbreClassifier {
    fn default() -> Self {
        Self::new()
    }
}
