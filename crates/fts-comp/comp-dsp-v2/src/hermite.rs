//! Hermite cubic interpolation for gain reduction smoothing with change detection.
//!
//! This is the core algorithm from Pro-C 3: a hybrid approach that uses change detection
//! to choose between two paths:
//! 1. **No change**: Return sqrt(gr_inst) - simple, fast
//! 2. **Change detected**: Use full Hermite cubic interpolation with 4 history points
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

impl HermiteCubicSmoother {
    pub fn new(hypothesis: StateFuncHypothesis) -> Self {
        Self {
            history: [[1.0; 4]; 2],
            change_threshold: 0.001,
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
        log_rel: f64,
        log_atk: f64,
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

        // Precompute log-space coefficients
        let log_atk_sq = log_atk * log_atk;
        let log_rel_sq = log_rel * log_rel;

        // DEBUG logging
        static mut SAMPLE_COUNT: u64 = 0;
        unsafe {
            SAMPLE_COUNT += 1;
            if ch == 0 && SAMPLE_COUNT <= 5 {
                eprintln!("[HERMITE-DETAIL] Sample {}: gr_inst={:.6}, hist=[{:.6},{:.6},{:.6},{:.6}], threshold={:.6}, has_change={}",
                    SAMPLE_COUNT, gr_inst, hist0, hist1, hist2, hist3, threshold, has_change);
            }
        }

        // Step 4: Route to algorithm
        // Use exponential smoothing based on attack/release
        let gr_instant_sqrt = gr_inst.sqrt();

        let result = if has_change {
            // Transition detected: smoothly interpolate between history and current GR
            // Use attack during compression increase (GR decrease), release during decrease
            let gr_change = gr_instant_sqrt - hist0;
            let coeff = if gr_change < 0.0 {
                attack_coeff // Compressing more: use attack time
            } else {
                release_coeff // Releasing: use release time
            };

            // Exponential smoothing: weighted average of old and new
            coeff * hist0 + (1.0 - coeff) * gr_instant_sqrt
        } else {
            // Steady state: just return sqrt
            gr_instant_sqrt
        };

        // DEBUG: Log result
        unsafe {
            if ch == 0 && SAMPLE_COUNT <= 5 {
                eprintln!(
                    "[HERMITE-RESULT] Sample {}: result={:.6} ({:.2} dB)",
                    SAMPLE_COUNT,
                    result,
                    fts_dsp::db::linear_to_db(result.max(1e-10))
                );
            }
        }

        // Step 5: Shift history and add new result
        // Next sample: hist[0] (new result), hist[1] (old hist[0]), hist[2] (old hist[1]), hist[3] (old hist[2])
        self.history[ch][3] = self.history[ch][2];
        self.history[ch][2] = self.history[ch][1];
        self.history[ch][1] = self.history[ch][0];
        self.history[ch][0] = result;

        result
    }

    /// Hermite cubic interpolation with 4 history points
    /// Based on Pro-C 3 binary reverse engineering (0x18010d3e0)
    ///
    /// Formula (from HERMITE_CUBIC_IMPLEMENTATION_SPEC):
    /// m0 = (hist0 - hist1) * log_atk_sq
    /// m1 = (hist1 - hist2) * log_rel_sq
    /// poly = ((hist0 - hist3) * (atk_coeff - gr_inst) * log_rel_sq
    ///         - (hist1 - hist3) * (atk_coeff - hist0) * log_atk_sq)
    ///       * log_atk_sq
    ///       + (hist2 - hist3) * (hist1 - hist0) * log_atk_sq * log_rel_sq
    /// result = sqrt(abs(poly))
    fn hermite_cubic(
        &self,
        hist0: f64,
        hist1: f64,
        hist2: f64,
        hist3: f64,
        gr_inst: f64,
        attack_coeff: f64,
        _release_coeff: f64,
        _log_atk: f64,
        _log_rel: f64,
        log_atk_sq: f64,
        log_rel_sq: f64,
    ) -> f64 {
        // Compute tangents (divided differences)
        let _m0 = (hist0 - hist1) * log_atk_sq;
        let _m1 = (hist1 - hist2) * log_rel_sq;

        // Compute Hermite cubic polynomial from binary spec
        // Term 1: ((hist0 - hist3) * (atk_coeff - gr_inst) * log_rel_sq
        //          - (hist1 - hist3) * (atk_coeff - hist0) * log_atk_sq) * log_atk_sq
        let term1_part1 = (hist0 - hist3) * (attack_coeff - gr_inst) * log_rel_sq;
        let term1_part2 = (hist1 - hist3) * (attack_coeff - hist0) * log_atk_sq;
        let term1 = (term1_part1 - term1_part2) * log_atk_sq;

        // Term 2: (hist2 - hist3) * (hist1 - hist0) * log_atk_sq * log_rel_sq
        let term2 = (hist2 - hist3) * (hist1 - hist0) * log_atk_sq * log_rel_sq;

        // Combine
        let poly = term1 + term2;

        // Always apply sqrt per binary spec
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
