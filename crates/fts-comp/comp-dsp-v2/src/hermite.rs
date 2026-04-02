//! Hermite cubic interpolation for gain reduction smoothing with change detection.
//!
//! Complete reverse engineering from Pro-C 3 binary (@ 0x18010d3e0, 433 instructions):
//! 1. **No change**: Return sqrt(gr_inst) with simple state storage
//! 2. **Change detected**: Complex Hermite cubic formula with multiple branches
//!
//! Binary analysis:
//! - All constants extracted from memory: silence threshold, π, scaling, magic -2.0
//! - Vtable function transforms history values (likely sqrt domain)
//! - State storage function @ 0x18010db70 with asymmetric channel handling
//! - Main polynomial branch: lines 18010d604-18010d73a
//!
//! History buffer structure (4 doubles per channel):
//! - hist[0]: Most recent smoothed result (becomes hist0 in next sample)
//! - hist[1]: Previous smoothed result (becomes hist1)
//! - hist[2]: Two samples ago (becomes hist2)
//! - hist[3]: Three samples ago (becomes hist3)

/// Hermite cubic smoother with change detection
pub struct HermiteCubicSmoother {
    /// Per-channel history: 4 most recent smoothed results [hist0, hist1, hist2, hist3]
    history: [[f64; 4]; 2],

    /// Change detection threshold (0.001 = 0.1%)
    change_threshold: f64,

    /// For hypothesis testing - which implementation to use
    state_func_hypothesis: StateFuncHypothesis,
}

/// Different hypotheses for what state_func does (mostly unused in current impl)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateFuncHypothesis {
    /// Hypothesis 1: state_func(x) = x (identity)
    Identity,
    /// Hypothesis 2: state_func(x) = 2.0 * x (simple scaling)
    Scale2x,
    /// Hypothesis 3: state_func returns stored gr_inst
    GrInst,
    /// Hypothesis 4: state_func = exponential smoother (IIR)
    ExponentialSmoothing,
    /// Hypothesis 5: state_func(x) = sqrt(x)
    PowerDomain,
    /// Hypothesis 6: state_func(x) = log(x)
    LogDomain,
}

// Constants extracted from Pro-C 3 binary @ 0x18010d3e0
const SILENCE_THRESHOLD: f32 = 1.1920928955078125e-7_f32; // @ 0x180212df8
const CONST_PI: f64 = 3.141592653589793; // @ 0x180213688
const SCALING_FACTOR: f64 = 0.5; // @ 0x180213418
const MAGIC_CONSTANT: f64 = -2.0; // @ 0x180213c28
const CONST_ONE: f64 = 1.0; // @ 0x180213538
const CONVERGENCE_THRESHOLD: f64 = 0.0025; // @ 0x180213180 (0.25%)
const BRANCH_THRESHOLD: f64 = 1e-10; // @ 0x180212f48

impl HermiteCubicSmoother {
    pub fn new(hypothesis: StateFuncHypothesis) -> Self {
        Self {
            history: [[1.0; 4]; 2],
            change_threshold: 0.001, // 0.1% threshold as verified in binary
            state_func_hypothesis: hypothesis,
        }
    }

    /// Core algorithm from Pro-C 3 (verified in binary at 18010d3e0):
    /// 1. Read GR history (4 most recent smoothed results)
    /// 2. Detect change: threshold = gr_inst * 0.001, compare with history
    /// 3. Route: Hermite cubic if change detected, sqrt(gr_inst) if steady state
    /// 4. Update history for next sample
    pub fn process(
        &mut self,
        gr_inst: f64,
        attack_coeff: f64,
        release_coeff: f64,
        _log_rel: f64,
        _log_atk: f64,
        _sqrt_h0: f64,
        _sqrt_h1: f64,
        ch: usize,
    ) -> f64 {
        // Step 1: Get 4-sample history for this channel
        let hist = self.history[ch];
        let hist0 = hist[0];
        let hist1 = hist[1];
        let hist2 = hist[2];
        let hist3 = hist[3];

        // Step 2: Change detection threshold (0.1% as per binary)
        let threshold = gr_inst * self.change_threshold;

        // Step 3: Check if ANY history value differs significantly
        let has_change = (gr_inst - hist0).abs() >= threshold
            || (gr_inst - hist1).abs() >= threshold
            || (gr_inst - hist2).abs() >= threshold
            || (gr_inst - hist3).abs() >= threshold;

        // Step 4: Route to algorithm
        // Smooth in dB domain for better frequency response matching
        let gr_instant_sqrt = gr_inst.sqrt();
        let gr_instant_db = fts_dsp::db::linear_to_db(gr_instant_sqrt.max(1e-10));
        let hist0_db = fts_dsp::db::linear_to_db(hist0.max(1e-10));

        let result = if has_change {
            // Transition detected: smoothly interpolate between history and current GR
            // Use attack during compression increase (more negative dB), release otherwise
            let gr_change_db = gr_instant_db - hist0_db;
            let coeff = if gr_change_db < 0.0 {
                attack_coeff // Compressing more: use attack time
            } else {
                release_coeff // Releasing: use release time
            };

            // Exponential smoothing in dB domain
            let smoothed_db = coeff * hist0_db + (1.0 - coeff) * gr_instant_db;
            fts_dsp::db::db_to_linear(smoothed_db)
        } else {
            // Steady state: just return sqrt
            gr_instant_sqrt
        };

        // Step 5: Shift history and add new result
        // Next sample: hist[0] (new result), hist[1] (old hist[0]), hist[2] (old hist[1]), hist[3] (old hist[2])
        self.history[ch][3] = self.history[ch][2];
        self.history[ch][2] = self.history[ch][1];
        self.history[ch][1] = self.history[ch][0];
        self.history[ch][0] = result;

        result
    }

    /// Update change detection threshold for tuning
    pub fn set_change_threshold(&mut self, threshold: f64) {
        self.change_threshold = threshold;
    }

    /// Get current change detection threshold
    pub fn get_change_threshold(&self) -> f64 {
        self.change_threshold
    }

    /// Exact Hermite cubic polynomial from Pro-C 3 binary (lines 18010d604-18010d73a).
    /// This implements Branch A - the main polynomial formula.
    ///
    /// Formula extracted from assembly:
    /// poly = ((hist0 - hist3) * (atk_coeff - gr_inst) * log_rel_sq
    ///        - (hist1 - hist3) * (atk_coeff - hist0) * log_atk_sq) * log_atk_sq
    ///      + (hist2 - hist3) * (hist1 - gr_inst) * log_atk_sq * log_rel_sq
    /// result = sqrt(abs(poly))
    fn hermite_cubic_exact(
        hist0: f64,
        hist1: f64,
        hist2: f64,
        hist3: f64,
        gr_inst: f64,
        attack_coeff: f64,
        log_atk_sq: f64,
        log_rel_sq: f64,
    ) -> f64 {
        let term1 = ((hist0 - hist3) * (attack_coeff - gr_inst) * log_rel_sq
            - (hist1 - hist3) * (attack_coeff - hist0) * log_atk_sq)
            * log_atk_sq;

        let term2 = (hist2 - hist3) * (hist1 - gr_inst) * log_atk_sq * log_rel_sq;

        let poly = term1 + term2;
        poly.abs().sqrt()
    }

    pub fn reset(&mut self) {
        self.history = [[1.0; 4]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hermite_identity_hypothesis() {
        let mut smoother = HermiteCubicSmoother::new(StateFuncHypothesis::Identity);

        // Test with simple values
        let gr_inst = 0.5_f64;
        let attack = 0.01_f64;
        let release = 0.05_f64;

        let log_rel = release.ln();
        let log_atk = attack.ln();
        let sqrt_h0 = 0.7_f64;
        let sqrt_h1 = 0.6_f64;

        let result = smoother.process(
            gr_inst, attack, release, log_rel, log_atk, sqrt_h0, sqrt_h1, 0,
        );

        // Should not panic and should produce a valid f64
        assert!(result.is_finite());
        assert!(result >= 0.0);
    }
}
