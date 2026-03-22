//! Gain reduction computation — threshold, ratio, soft knee, inertia.
//!
//! Converts the detected envelope level into the amount of gain reduction
//! to apply, shaped by the compression curve parameters.

/// Maximum number of stereo channels.
const MAX_CH: usize = 2;

// r[impl comp.chain.signal-flow]
/// Gain computer with soft knee and inertia momentum.
pub struct GainComputer {
    /// Current gain reduction in dB per channel.
    pub gr_db: [f64; MAX_CH],
    /// Previous gain reduction for inertia calculation.
    prev_gr: [f64; MAX_CH],
    /// Inertia velocity per channel.
    inertia_vel: [f64; MAX_CH],
}

impl GainComputer {
    pub fn new() -> Self {
        Self {
            gr_db: [0.0; MAX_CH],
            prev_gr: [0.0; MAX_CH],
            inertia_vel: [0.0; MAX_CH],
        }
    }

    /// Compute gain reduction in dB from the detected level.
    ///
    /// Uses a softplus knee function that provides a smooth, C∞-continuous
    /// transition around the threshold:
    ///   GR = slope * k * ln(1 + exp((level - threshold) / k))
    /// where k = knee_db / 10.
    ///
    /// For large signals (level >> threshold): GR ≈ slope * (level - threshold)
    /// For small signals (level << threshold): GR ≈ 0
    /// The transition is controlled by `knee_db` — larger values give a
    /// softer, wider transition zone.
    ///
    /// When `knee_db` is 0, falls back to a hard-knee curve.
    ///
    /// # Parameters
    /// - `level_db`: smoothed envelope level in dB (from detector)
    /// - `threshold_db`: compression threshold in dB
    /// - `ratio`: compression ratio (1.0 = no compression, 20.0 = hard limiting)
    /// - `knee_db`: soft knee width in dB (0 = hard knee, 6 = gentle, 18 = standard)
    /// - `inertia`: momentum coefficient (-1.0 to 0.3, 0 = off)
    /// - `inertia_decay`: decay coefficient for inertia velocity (0.99-1.0 range)
    /// - `ch`: channel index
    ///
    /// Returns gain reduction in dB (positive = reducing).
    #[inline]
    pub fn compute(
        &mut self,
        level_db: f64,
        threshold_db: f64,
        ratio: f64,
        knee_db: f64,
        inertia: f64,
        inertia_decay: f64,
        ch: usize,
    ) -> f64 {
        if ratio <= 1.0 {
            self.gr_db[ch] = 0.0;
            return 0.0;
        }

        let slope = 1.0 - 1.0 / ratio;

        let gr = if knee_db > 0.001 {
            // Softplus knee: GR = slope * k * ln(1 + exp(x / k))
            let k = knee_db * 0.1;
            let x = level_db - threshold_db;
            let z = x / k;
            let softplus = if z > 20.0 {
                z
            } else if z < -20.0 {
                0.0
            } else if z > 0.0 {
                z + (1.0 + (-z).exp()).ln()
            } else {
                (1.0 + z.exp()).ln()
            };
            slope * k * softplus
        } else if level_db > threshold_db {
            // Hard knee: standard compression
            slope * (level_db - threshold_db)
        } else {
            0.0
        };

        self.gr_db[ch] = gr;

        // Inertia system — adds momentum to gain reduction changes
        if inertia.abs() > 1e-6 {
            let gr_linear = db_to_gain(self.gr_db[ch]);

            if inertia > 0.0 {
                // Positive inertia: only apply momentum when gain reduction is increasing
                if self.gr_db[ch] > self.prev_gr[ch] {
                    self.inertia_vel[ch] += inertia * gr_linear * -0.001;
                }
            } else {
                // Negative inertia: always apply momentum
                self.inertia_vel[ch] += inertia * gr_linear * -0.001;
            }

            self.inertia_vel[ch] *= inertia_decay;
            self.inertia_vel[ch] = self.inertia_vel[ch].clamp(-100.0, 100.0);

            let gr_linear = gr_linear + self.inertia_vel[ch];
            let gr_linear = gr_linear.clamp(-1000.0, 1000.0);
            self.gr_db[ch] = gain_to_db(gr_linear);
        }

        self.prev_gr[ch] = self.gr_db[ch];
        self.gr_db[ch]
    }

    pub fn reset(&mut self) {
        self.gr_db = [0.0; MAX_CH];
        self.prev_gr = [0.0; MAX_CH];
        self.inertia_vel = [0.0; MAX_CH];
    }
}

impl Default for GainComputer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Utility ────────────────────────────────────────────────────────────

#[inline]
fn db_to_gain(db: f64) -> f64 {
    if db <= -1000.0 {
        return 0.0;
    }
    10.0_f64.powf(db / 20.0)
}

#[inline]
fn gain_to_db(gain: f64) -> f64 {
    if gain <= 0.0 {
        return -1000.0;
    }
    20.0 * gain.log10()
}
