//! Two-stage compressor detector: level detection + GR smoothing.
//!
//! Supports three detection modes:
//!
//! **Peak mode** (default):
//! 1. Compute instantaneous level: 20*log10(|x|)
//! 2. Apply gain curve → raw GR in dB
//! 3. Transform raw GR to power domain (gr^p), smooth with asymmetric 1-pole
//! 4. Inverse transform smoothed value back to dB
//! Smoothing in gr^p domain (p < 1) reduces Jensen's inequality bias.
//!
//! **Rms mode** (energy-envelope detection):
//! 1. Compute x², smooth with SYMMETRIC 1-pole (τ = attack time)
//! 2. Convert to dB: 10*log10(smoothed_x²)
//! 3. Apply gain curve → raw GR in dB
//! 4. Smooth GR with attack/release in power domain (same as Peak mode)
//! At fast attack, symmetric smoother barely averages → near peak detection.
//! At slow attack, symmetric smoother averages x² → near true RMS.
//! Square wave (constant x²) is unaffected → always peak level.
//! This produces attack-dependent crest-factor sensitivity.
//!
//! **Smooth mode** (optical-style):
//! 1. Smooth |x| with 1-pole lowpass in linear domain
//! 2. Convert smoothed level to dB: 20*log10(smoothed)
//! 3. Apply gain curve → raw GR in dB
//! 4. Smooth GR in dB domain (SMOOTH_POWER=1.0, no Jensen's bias needed)
//! Linear pre-smoothing naturally adapts to crest factor: sine→mean rect,
//! square→peak, making threshold signal-independent.

use fts_dsp::db::{linear_to_db, DB_FLOOR};

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

/// Detection mode for the compressor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetectorMode {
    /// Instantaneous peak detection with power-domain GR smoothing.
    Peak,
    /// Energy-envelope detection: symmetric smoother on x² → 10*log10 → gain curve → GR.
    /// Symmetric τ = attack time: fast attack → near peak, slow attack → near RMS.
    /// GR is then smoothed with normal attack/release power-domain smoother.
    Rms,
    /// Linear-domain pre-smoothing (optical-style) with dB-domain GR smoothing.
    Smooth,
}

// --- Hermite cubic interpolation constants (Pro-C 3 binary extracted) ---

/// Hermite cubic polynomial normalization constant (1/400).
/// From binary address 0x180213180.
const HERMITE_POLYNOMIAL_CONST: f64 = 0.0025;

/// Write filter state denominator scaling factor.
/// From binary address 0x180213c28.
const WRITE_STATE_SCALE: f64 = -2.0;

/// Logarithm safe approximation thresholds and coefficients (π/4, π/2).
/// From binary addresses 0x180216ad0, 0x180216af0.
const LOG_SAFE_ATK_THRESHOLD: f64 = 0.785398163397448;
const LOG_SAFE_PI_HALF: f64 = 1.570796326734126;
const LOG_SAFE_B20_THRESHOLD: f64 = 0.000000007450581;
const LOG_SAFE_B40_COEFF: f64 = 0.68;

// --- Peak mode constants ---

/// Scaling factor for attack time constant (peak mode).
const PEAK_ATTACK_SCALE: f64 = 2.0;
/// Scaling factor for release time constant (peak mode).
const PEAK_RELEASE_SCALE: f64 = 2.0;
/// Power exponent for GR smoothing domain (peak mode).
/// p<1.0 reduces Jensen's bias from oscillating peak-detected GR.
const PEAK_SMOOTH_POWER: f64 = 0.80;

// --- RMS mode constants ---

/// Scaling factor for the symmetric energy smoother time constant.
/// Applied to the attack_s parameter to derive the energy smoother τ.
const RMS_ENERGY_SCALE: f64 = 2.0;
/// Scaling factor for GR attack time constant (RMS mode).
const RMS_ATTACK_SCALE: f64 = 2.0;
/// Scaling factor for GR release time constant (RMS mode).
const RMS_RELEASE_SCALE: f64 = 2.0;
/// Power exponent for GR smoothing domain (RMS mode).
/// Same as Peak mode — power-domain reduces Jensen's bias from residual
/// GR oscillation after the energy smoother.
const RMS_SMOOTH_POWER: f64 = 0.80;

// --- Smooth mode constants ---

/// Scaling factor for attack time constant (smooth mode).
const SMOOTH_ATTACK_SCALE: f64 = 2.0;
/// Scaling factor for release time constant (smooth mode).
const SMOOTH_RELEASE_SCALE: f64 = 2.0;
/// Power exponent for GR smoothing domain (smooth mode).
/// 1.0 = plain dB domain. No Jensen's compensation needed because
/// the linear pre-smoother eliminates GR oscillation.
const SMOOTH_SMOOTH_POWER: f64 = 0.80;

/// Fixed time constant for the linear-domain level pre-smoother (seconds).
/// Short enough not to add significant dynamics, long enough to smooth
/// rectified audio oscillations (mean-rectify). 5ms smooths ~1 cycle at 200Hz.
const LEVEL_SMOOTH_TAU_S: f64 = 0.005;

// --- Shared constants ---

/// Minimum release time in seconds to prevent zero-crossing collapse.
/// Pro-C 3's normalized 0.0 release maps to 10ms.
const MIN_RELEASE_S: f64 = 0.010;

/// Minimum attack time in seconds.
/// Set to Pro-C 3's minimum (0.005 ms).
const MIN_ATTACK_S: f64 = 0.000005;

/// Two-stage detector: level detection → gain curve → smoothed GR.
pub struct Detector {
    /// Current detection mode.
    mode: DetectorMode,

    /// Smoothed GR in transformed domain (gr^p) per channel.
    smooth_grp: [f64; MAX_CH],
    /// Previous output sample per channel (for feedback detection).
    prev_output: [f64; MAX_CH],

    /// Smoothed |x| in linear domain per channel (smooth mode only).
    smooth_level: [f64; MAX_CH],
    /// 1-pole coefficient for level pre-smoother attack (smooth mode).
    level_smooth_coeff: f64,

    /// Smoothed x² (energy) per channel (RMS mode only).
    smooth_energy: [f64; MAX_CH],
    /// 1-pole coefficient for symmetric energy smoother (RMS mode).
    rms_energy_coeff: f64,

    /// GR history for Hermite cubic interpolation (4-point: [hist0, hist1, hist2, inst]).
    /// Per-channel history buffer.
    gr_history: [[f64; 4]; MAX_CH],
    /// Hermite cubic filter state outputs (5 values per channel).
    hermite_outputs: [[f64; 5]; MAX_CH],

    // Coefficients
    attack_coeff: f64,
    release_coeff: f64,
    sample_rate: f64,

    // Hold
    /// Duration of hold phase in samples.
    hold_samples: usize,
    /// Per-channel hold countdown: samples remaining before release begins.
    hold_countdown: [usize; MAX_CH],
}

impl Detector {
    pub fn new() -> Self {
        Self {
            mode: DetectorMode::Peak,
            smooth_grp: [0.0; MAX_CH],
            prev_output: [0.0; MAX_CH],
            smooth_level: [0.0; MAX_CH],
            level_smooth_coeff: 0.0,
            smooth_energy: [0.0; MAX_CH],
            rms_energy_coeff: 0.0,
            gr_history: [[0.0; 4]; MAX_CH],
            hermite_outputs: [[0.0; 5]; MAX_CH],
            attack_coeff: 0.0,
            release_coeff: 0.0,
            sample_rate: 48000.0,
            hold_samples: 0,
            hold_countdown: [0; MAX_CH],
        }
    }

    /// Set the detection mode.
    pub fn set_mode(&mut self, mode: DetectorMode) {
        self.mode = mode;
    }

    /// Get the current detection mode.
    pub fn mode(&self) -> DetectorMode {
        self.mode
    }

    /// Update coefficients for new attack/release times or sample rate.
    pub fn set_params(&mut self, attack_s: f64, release_s: f64, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let (attack_scale, release_scale) = match self.mode {
            DetectorMode::Peak => (PEAK_ATTACK_SCALE, PEAK_RELEASE_SCALE),
            DetectorMode::Rms => (RMS_ATTACK_SCALE, RMS_RELEASE_SCALE),
            DetectorMode::Smooth => (SMOOTH_ATTACK_SCALE, SMOOTH_RELEASE_SCALE),
        };
        self.attack_coeff = Self::coeff(attack_s.max(MIN_ATTACK_S), sample_rate, attack_scale);
        self.release_coeff = Self::coeff(release_s.max(MIN_RELEASE_S), sample_rate, release_scale);

        // RMS mode: symmetric energy smoother with τ = attack_s.
        // Uses the attack time as the integration window for x² averaging.
        if self.mode == DetectorMode::Rms {
            self.rms_energy_coeff =
                Self::coeff(attack_s.max(MIN_ATTACK_S), sample_rate, RMS_ENERGY_SCALE);
        }

        // Level pre-smoother: fixed short time constant for mean-rectification.
        // Independent of attack/release — just smooths audio-rate oscillations.
        if self.mode == DetectorMode::Smooth {
            self.level_smooth_coeff =
                Self::coeff(LEVEL_SMOOTH_TAU_S, sample_rate, SMOOTH_ATTACK_SCALE);
        }
    }

    /// Set hold time. Called whenever hold_ms or sample rate changes.
    pub fn set_hold(&mut self, hold_ms: f64, sample_rate: f64) {
        self.hold_samples = (hold_ms / 1000.0 * sample_rate).round() as usize;
    }

    #[inline]
    fn coeff(time_s: f64, sample_rate: f64, scale: f64) -> f64 {
        if time_s > 0.0 {
            (-scale / (sample_rate * time_s)).exp()
        } else {
            0.0
        }
    }

    /// Feed a sample and return the detected level in dB.
    ///
    /// In Peak mode: instantaneous 20*log10(|x|).
    /// In Smooth mode: 1-pole smoothed |x| in linear domain, then to dB.
    #[inline]
    pub fn tick(&mut self, input_abs: f64, feedback: f64, ch: usize) -> f64 {
        let combined = input_abs + self.prev_output[ch].abs() * feedback;

        match self.mode {
            DetectorMode::Peak => linear_to_db(combined).max(DB_FLOOR),
            DetectorMode::Rms => {
                // Symmetric amplitude smoother on |x|.
                // Uses attack_s as τ for BOTH rise and fall directions.
                // - Fast attack: short τ → barely smooths → near instantaneous |x| → peak-like
                // - Slow attack: long τ → averages |x| → mean-rectified level
                // Mean-rectified sine = 2A/π ≈ A - 3.9 dB (stronger than RMS's -3 dB).
                // Square wave (constant |x|) is unaffected → always peak level.
                let c = self.rms_energy_coeff;
                self.smooth_energy[ch] = c * self.smooth_energy[ch] + (1.0 - c) * combined;
                linear_to_db(self.smooth_energy[ch]).max(DB_FLOOR)
            }
            DetectorMode::Smooth => {
                // Smooth |x| in linear domain with fixed short 1-pole.
                // Symmetric smoothing (same τ for rise and fall) acts as
                // mean rectifier, converting peak level to mean level
                // without adding significant attack/release dynamics.
                let c = self.level_smooth_coeff;
                self.smooth_level[ch] = c * self.smooth_level[ch] + (1.0 - c) * combined;
                linear_to_db(self.smooth_level[ch]).max(DB_FLOOR)
            }
        }
    }

    /// Smooth a raw GR value with asymmetric attack/hold/release.
    ///
    /// In Peak mode: Hermite cubic spline interpolation on 4-point GR history.
    /// In Smooth/Rms mode: power-domain smoothing (SMOOTH_POWER <= 1.0).
    #[inline]
    pub fn smooth_gr(&mut self, raw_gr_db: f64, ch: usize) -> f64 {
        // Shift history buffer: [hist0, hist1, hist2, inst]
        self.gr_history[ch][0] = self.gr_history[ch][1];
        self.gr_history[ch][1] = self.gr_history[ch][2];
        self.gr_history[ch][2] = self.gr_history[ch][3];
        self.gr_history[ch][3] = raw_gr_db;

        if self.mode == DetectorMode::Peak {
            // Peak mode: Apply Hermite cubic interpolation
            let raw_gr_clamped = raw_gr_db.max(0.0);
            let smoothed_gr = Self::apply_hermite_cubic_interpolation(
                raw_gr_clamped,
                &[
                    self.gr_history[ch][0],
                    self.gr_history[ch][1],
                    self.gr_history[ch][2],
                ],
                self.attack_coeff,
                self.release_coeff,
            );

            // Apply hold logic
            if smoothed_gr >= self.smooth_grp[ch] {
                // GR increasing → attack; reset hold countdown
                self.hold_countdown[ch] = self.hold_samples;
            } else if self.hold_countdown[ch] > 0 {
                // Hold phase: GR wants to decrease but hold timer hasn't expired
                self.hold_countdown[ch] -= 1;
                // Return current smoothed value (hold)
                return self.smooth_grp[ch];
            }

            self.smooth_grp[ch] = smoothed_gr;
            smoothed_gr
        } else {
            // Smooth/RMS mode: Use power-domain 1-pole IIR (original behavior)
            let p = match self.mode {
                DetectorMode::Peak => PEAK_SMOOTH_POWER,
                DetectorMode::Rms => RMS_SMOOTH_POWER,
                DetectorMode::Smooth => SMOOTH_SMOOTH_POWER,
            };

            // Transform to power domain: gr^p (p<1 reduces Jensen's bias)
            let raw_grp = raw_gr_db.max(0.0).powf(p);

            if raw_grp >= self.smooth_grp[ch] {
                // GR increasing → attack; reset hold countdown
                self.hold_countdown[ch] = self.hold_samples;
                let c = self.attack_coeff;
                self.smooth_grp[ch] = c * self.smooth_grp[ch] + (1.0 - c) * raw_grp;
            } else if self.hold_countdown[ch] > 0 {
                // Hold phase: GR wants to decrease but hold timer hasn't expired
                self.hold_countdown[ch] -= 1;
                // smooth_grp unchanged — hold at current level
            } else {
                // Release: GR decreasing, hold expired
                let c = self.release_coeff;
                self.smooth_grp[ch] = c * self.smooth_grp[ch] + (1.0 - c) * raw_grp;
            }

            // Inverse transform: gr = smoothed^(1/p)
            self.smooth_grp[ch].max(0.0).powf(1.0 / p)
        }
    }

    /// Store the output sample for feedback detection on next tick.
    #[inline]
    pub fn set_output(&mut self, output: f64, ch: usize) {
        self.prev_output[ch] = output;
    }

    /// Get the current smoothed GR in dB.
    pub fn level_db(&self, ch: usize) -> f64 {
        let p = match self.mode {
            DetectorMode::Peak => PEAK_SMOOTH_POWER,
            DetectorMode::Rms => RMS_SMOOTH_POWER,
            DetectorMode::Smooth => SMOOTH_SMOOTH_POWER,
        };
        self.smooth_grp[ch].max(0.0).powf(1.0 / p)
    }

    pub fn reset(&mut self) {
        self.smooth_grp = [0.0; MAX_CH];
        self.prev_output = [0.0; MAX_CH];
        self.smooth_level = [0.0; MAX_CH];
        self.smooth_energy = [0.0; MAX_CH];
        self.gr_history = [[0.0; 4]; MAX_CH];
        self.hermite_outputs = [[0.0; 5]; MAX_CH];
        self.hold_countdown = [0; MAX_CH];
    }

    /// Safe logarithm with piecewise approximation for boundary values.
    /// Handles denormals, zero, and infinity gracefully.
    #[inline]
    fn log_safe_approx(x: f64) -> f64 {
        // Prevent log(0) → -inf
        if x <= MIN_ATTACK_S {
            return 0.0;
        }

        let abs_x = x.abs();

        // Use natural logarithm
        let mut result = x.ln();

        // Apply threshold-based adjustments for very small values
        if abs_x < LOG_SAFE_B20_THRESHOLD {
            // Cubic approximation blend for very small values
            result = x * x * x * 0.1 + x;
        }

        result
    }

    /// Write smoothing filter state: converts raw polynomial outputs to filter state.
    /// Two paths depending on mode_flag: main path (SQRTPD-based) or alternative (simple sqrt).
    #[inline]
    fn write_smoothing_filter_state(
        param_2: f64,
        param_3: f64,
        param_4: f64,
        param_5: f64,
        param_6: f64,
        mode_flag: i32,
    ) -> [f64; 5] {
        let mut outputs = [0.0; 5];

        if mode_flag == 0 {
            // Main path: SQRTPD operations on scaling coefficients
            let sqrt_5 = param_5.sqrt();
            let sqrt_6 = param_6.sqrt();

            let dvar3 = param_4 + 1.0;
            let dvar2 = param_2 * param_4;
            let dvar4 = (1.0 - param_4) * WRITE_STATE_SCALE;
            let dvar1 = 1.0 / (dvar3 + sqrt_6);

            outputs[0] = (dvar2 + param_3 + sqrt_5) * dvar1;
            outputs[1] = (param_3 - dvar2) * WRITE_STATE_SCALE * dvar1;
            outputs[2] = ((param_3 - sqrt_5) + dvar2) * dvar1;
            outputs[3] = dvar4 * dvar1;
            outputs[4] = (dvar3 - sqrt_6) * dvar1;
        } else {
            // Alternative path: simpler sqrt-based computation
            let dvar2 = param_4.sqrt();

            outputs[0] = dvar2;
            outputs[2] = param_2;
            outputs[4] = param_3;

            if dvar2 != 0.0 {
                let dvar1 = 1.0 / dvar2;
                let sqrt_6 = param_6.sqrt();
                let sqrt_5 = param_5.sqrt();

                outputs[1] = dvar1 * sqrt_6 + dvar2;
                outputs[3] = dvar1 * sqrt_5;
            }
        }

        outputs
    }

    /// Hermite cubic spline interpolation on 4-point GR history.
    /// Returns the smoothed GR value in dB domain.
    #[inline]
    fn apply_hermite_cubic_interpolation(
        gr_inst: f64,
        gr_hist: &[f64; 3],
        alpha_atk: f64,
        alpha_rel: f64,
    ) -> f64 {
        const MIN_DENOMINATOR_CHECK: f64 = 1e-15;
        const CHANGE_DETECTION_THRESHOLD: f64 = 0.01;

        // Step 1: Transform coefficients via log_safe_approx
        let dVar6 = Self::log_safe_approx(alpha_atk);
        let dVar7 = Self::log_safe_approx(alpha_rel);

        // Step 2: Change detection
        let threshold = gr_inst * CHANGE_DETECTION_THRESHOLD;
        let change_detected = threshold <= (gr_inst - gr_hist[0]).abs()
            || threshold <= (gr_inst - gr_hist[1]).abs()
            || threshold <= (gr_inst - gr_hist[2]).abs();

        if !change_detected {
            // No significant change: return instantaneous GR
            return gr_inst;
        }

        // Step 3: Hermite cubic interpolation (main polynomial computation)

        // Squared terms
        let dVar3 = dVar6 * dVar6; // log_atk²
        let dVar2 = dVar7 * dVar7; // log_rel²
        let dVar26 = dVar3;
        let dVar12 = gr_inst.sqrt();
        let _dVar20_raw = gr_inst.sqrt();

        // Main polynomial
        let dVar23 = dVar3 - dVar2; // atk² - rel²

        // Numerator denominator (Hermite basis)
        let dVar36_numerator = ((gr_hist[1] - gr_hist[2]) * (alpha_rel - gr_inst) * dVar2
            - (gr_inst - gr_hist[2]) * (alpha_rel - gr_hist[1]) * dVar3)
            * dVar26
            + (alpha_rel - gr_hist[2]) * (gr_inst - gr_hist[1]) * dVar3 * dVar2;

        if dVar36_numerator.abs() < MIN_DENOMINATOR_CHECK {
            return gr_inst;
        }

        // Intermediate values
        let dVar13 = dVar23 * gr_hist[2];
        let dVar27 = dVar2 - dVar26;
        let dVar14 = (dVar26 - dVar3) * gr_hist[1];
        let dVar15 = gr_hist[2] * gr_hist[1];
        let dVar16 = HERMITE_POLYNOMIAL_CONST / (dVar6 * dVar7);

        // Primary polynomial evaluation
        let dVar8_num = ((dVar27 * gr_inst + dVar23 * gr_hist[1] + (dVar26 - dVar3) * gr_hist[2])
            * gr_inst
            + (dVar14 + dVar13) * gr_inst
            + dVar27 * dVar15)
            / dVar36_numerator;

        let dVar12_final = if dVar8_num < dVar16 {
            // Fallback polynomial
            let numerator = (((dVar3 * gr_hist[2] - dVar3 * gr_hist[1]) + dVar2 * gr_hist[1])
                - dVar26 * gr_hist[2])
                * gr_inst
                + (dVar26 - dVar2) * dVar15
                + dVar16 * dVar36_numerator;

            let denominator = dVar27 * gr_inst + dVar13 + dVar14;

            if denominator.abs() < MIN_DENOMINATOR_CHECK {
                dVar12
            } else {
                (numerator / denominator).max(0.0).sqrt()
            }
        } else {
            dVar12
        };

        // Return smoothed GR: dVar12_final² converts back from sqrt domain to dB
        dVar12_final * dVar12_final
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
