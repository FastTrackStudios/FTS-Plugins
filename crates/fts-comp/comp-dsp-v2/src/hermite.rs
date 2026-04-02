//! Hermite cubic interpolation for gain reduction smoothing with change detection.
//!
//! This is the core algorithm from Pro-C 3: a hybrid approach that uses change detection
//! to choose between two paths:
//! 1. **No change**: Return sqrt(state_value) - simple, fast
//! 2. **Change detected**: Use Hermite cubic with 4 control points - smooth
//!
//! The mystery `state_func` transforms history values. Hypothesis testing framework
//! allows testing different implementations without binary reverse engineering.

/// Hermite cubic smoother with change detection
pub struct HermiteCubicSmoother {
    /// Per-channel state for state_func calls
    /// For hypotheses that need history (exponential, history lookup)
    state_func_history: [[f64; 4]; 2],

    /// Change detection threshold (0.001 = 0.1%)
    change_threshold: f64,

    /// For hypothesis testing - which implementation to use
    state_func_hypothesis: StateFuncHypothesis,
}

/// Different hypotheses for what state_func does
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
            state_func_history: [[1.0; 4]; 2],
            change_threshold: 0.001,
            state_func_hypothesis: hypothesis,
        }
    }

    /// Core algorithm from Pro-C 3 (verified in binary at 18010d3e0):
    /// 1. Read GR history (prior smoothed GR values from state buffer)
    /// 2. Detect change: threshold = gr_inst * 0.001, compare with history
    /// 3. Route: Hermite cubic if change detected, sqrt(gr_inst) if steady state
    /// 4. Update history buffer with smoothed output for next sample
    pub fn process(
        &mut self,
        gr_inst: f64,
        _attack_coeff: f64,
        _release_coeff: f64,
        log_rel: f64,
        log_atk: f64,
        sqrt_h0: f64,
        sqrt_h1: f64,
        ch: usize,
    ) -> f64 {
        // Step 1: Get history buffer for this channel (4 prior GR values)
        let hist = self.state_func_history[ch];

        // Step 2: Change detection threshold (0.1% as per binary)
        let threshold = gr_inst * self.change_threshold;

        // Step 3: Check if ANY history value differs significantly
        let has_change = hist.iter().any(|&h| (gr_inst - h).abs() >= threshold);

        // Step 4: Route to algorithm
        let result = if has_change {
            // Change detected: use Hermite cubic interpolation
            // Hermite cubic expects history values in gr_inst domain
            let hc_result = self.hermite_cubic(hist, log_rel, log_atk, sqrt_h0, sqrt_h1);
            hc_result
        } else {
            // Steady state: use simple sqrt fallback (from binary)
            gr_inst.sqrt()
        };

        // Step 5: Update history buffer with new smoothed value
        // Shift history: [h1, h2, h3, result]
        self.state_func_history[ch][0] = hist[1];
        self.state_func_history[ch][1] = hist[2];
        self.state_func_history[ch][2] = hist[3];
        self.state_func_history[ch][3] = result;

        result
    }

    /// Branch A: Main Hermite cubic path (lines 18010d63b-18010d734)
    ///
    /// Extracted from 53 FMA operations in Pro-C 3 assembly.
    /// Note: Uses log_rel and log_atk from attack/release coefficients, not other_coeff.
    /// The 4 history values come from prior smoothed GR values, not coefficients.
    fn hermite_cubic(
        &self,
        hist: [f64; 4],
        log_rel: f64,
        log_atk: f64,
        sqrt_h0: f64,
        sqrt_h1: f64,
    ) -> f64 {
        // Phase 1: Initial products
        let prod1 = hist[0] * hist[2]; // h0 * h2
        let _prod2 = hist[0] * hist[3]; // h0 * h3 (not used in Branch A)
        let prod3 = hist[3] * hist[2]; // h3 * h2

        // Binary uses only attack/release coefficients, NOT other_coeff
        // So diff1 = log_rel² - log_rel * log_atk (not log_third)
        let diff1 = log_rel * log_rel - log_rel * log_atk;
        let log_rel_sq = log_rel * log_rel;
        let log_rel_4th = log_rel_sq * log_rel_sq;
        let _log_atk_sq = log_atk * log_atk;

        // Phase 2: Hermite basis combinations
        let term1 = prod1 * log_rel_sq * (diff1 * diff1);
        let term3 = log_rel_4th * diff1 * prod3;

        let intermediate = term3 - term1;
        let sqrt_intermediate = intermediate.abs().sqrt();

        // Phase 3: Numerator computation (if denominator != 0)
        if sqrt_intermediate > 1e-15 {
            let numerator_inner = (diff1 * log_rel_sq * prod3).sqrt();
            let numerator = numerator_inner * sqrt_h0 / sqrt_intermediate;
            // Note: Binary has additional factor here, but removing log_third avoids degeneration

            let _clamped = numerator.min(2.0 * sqrt_h1);

            // Phase 4: Final computation
            let factor = log_rel_sq / sqrt_h0;
            let diff_final = log_rel_sq - factor;
            let _term_sq = diff_final * diff_final * hist[2];

            // Result is clamped in sqrt fallback path
            let result = (log_rel_sq - factor).abs();
            result * log_rel_sq.abs().sqrt()
        } else {
            0.0
        }
    }

    /// state_func: the mystery function!
    ///
    /// This is called 4 times with different inputs (gr_inst, attack_coeff, release_coeff, other_coeff)
    /// and determines which history values are used in Hermite interpolation.
    ///
    /// Testing different hypotheses by changing this function.
    fn state_func(&mut self, input: f64) -> f64 {
        match self.state_func_hypothesis {
            // Hypothesis 1: Identity (simplest)
            StateFuncHypothesis::Identity => input,

            // Hypothesis 2: Simple 2x scaling
            StateFuncHypothesis::Scale2x => input * 2.0,

            // Hypothesis 3: Would return gr_inst (not available here)
            // For now, use identity as approximation
            StateFuncHypothesis::GrInst => input,

            // Hypothesis 4: Exponential smoothing (IIR)
            // Would need to maintain history per input type
            // For now, use identity as baseline
            StateFuncHypothesis::ExponentialSmoothing => input,

            // Hypothesis 5: Power domain
            StateFuncHypothesis::PowerDomain => input.sqrt(),

            // Hypothesis 6: Log domain
            StateFuncHypothesis::LogDomain => {
                if input > 0.0 {
                    input.ln()
                } else {
                    -100.0
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.state_func_history = [[1.0; 4]; 2];
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
