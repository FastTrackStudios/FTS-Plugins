//! ToTape9 — faithful Rust port of the Airwindows ToTape9 algorithm.
//!
//! Original by Chris Johnson (Airwindows), MIT license.
//! <https://github.com/airwindows/airwindows>
//!
//! Signal flow:
//! ```text
//! Input Gain → Dubly Encode → Flutter → Bias (9-stage slew limiter)
//!   → Hysteresis → TapeHack2 (pre-sat averaging) → TapeHack (saturation)
//!   → Post-sat averaging → Head Bump → Dubly Decode → Output Gain
//!   → ClipOnly3
//! ```

use std::f64::consts::PI;

use fts_dsp::{AudioConfig, Processor};

// ── Slew array layout (9 stages × 3: prevSampL, prevSampR, threshold) ──
// Only the threshold indices are needed at setup time; the per-sample loop
// iterates by stride-3 using the generic x, x+1, x+2 pattern.
const THRESHOLD1: usize = 2;
const THRESHOLD2: usize = 5;
const THRESHOLD3: usize = 8;
const THRESHOLD4: usize = 11;
const THRESHOLD5: usize = 14;
const THRESHOLD6: usize = 17;
const THRESHOLD7: usize = 20;
const THRESHOLD8: usize = 23;
const THRESHOLD9: usize = 26;
const GSLEW_TOTAL: usize = 27;

// ── Head bump biquad layout ────────────────────────────────────────
const HDB_FREQ: usize = 0;
const HDB_RESO: usize = 1;
const HDB_A0: usize = 2;
const HDB_A1: usize = 3;
const HDB_A2: usize = 4;
const HDB_B1: usize = 5;
const HDB_B2: usize = 6;
const HDB_SL1: usize = 7;
const HDB_SL2: usize = 8;
const HDB_SR1: usize = 9;
const HDB_SR2: usize = 10;
const HDB_TOTAL: usize = 11;

// ── Golden ratio ───────────────────────────────────────────────────
const PHI: f64 = 1.618033988749894848204586;

/// Parameters for the ToTape9 algorithm.
/// All values are normalized 0.0–1.0, default 0.5.
#[derive(Clone, Copy)]
pub struct ToTape9Params {
    /// Input gain. Quadratic: `(A*2)^2`, range 0–4×.
    pub input_gain: f64,
    /// Dubly tilt. 0.5 = flat (encode/decode cancel).
    pub tilt: f64,
    /// IIR frequency split for Dubly encode/decode.
    pub shape: f64,
    /// Flutter depth.
    pub flutter_depth: f64,
    /// Flutter LFO speed.
    pub flutter_speed: f64,
    /// Bias. 0.5 = center, <0.5 = under-bias (sticky), >0.5 = over-bias (slew limit).
    pub bias: f64,
    /// Head bump amount.
    pub head_bump: f64,
    /// Head bump frequency. Displayed as `((H*H)*175)+25` Hz.
    pub head_bump_freq: f64,
    /// Output gain. Linear: `I*2`, range 0–2×.
    pub output_gain: f64,
}

impl Default for ToTape9Params {
    fn default() -> Self {
        Self {
            input_gain: 0.5,
            tilt: 0.5,
            shape: 0.5,
            flutter_depth: 0.5,
            flutter_speed: 0.5,
            bias: 0.5,
            head_bump: 0.5,
            head_bump_freq: 0.5,
            output_gain: 0.5,
        }
    }
}

/// Complete ToTape9 processor state.
pub struct ToTape9 {
    pub params: ToTape9Params,
    sample_rate: f64,

    // Dubly encode/decode
    iir_enc_l: f64,
    iir_enc_r: f64,
    iir_dec_l: f64,
    iir_dec_r: f64,
    comp_enc_l: f64,
    comp_enc_r: f64,
    comp_dec_l: f64,
    comp_dec_r: f64,
    avg_enc_l: f64,
    avg_enc_r: f64,
    avg_dec_l: f64,
    avg_dec_r: f64,

    // Flutter
    d_l: [f64; 1002],
    d_r: [f64; 1002],
    sweep_l: f64,
    sweep_r: f64,
    nextmax_l: f64,
    nextmax_r: f64,
    gcount: i32,

    // Bias (9-stage slew limiter)
    gslew: [f64; GSLEW_TOTAL],

    // Hysteresis
    hysteresis_l: f64,
    hysteresis_r: f64,

    // Head bump
    head_bump_l: f64,
    head_bump_r: f64,
    hdb_a: [f64; HDB_TOTAL],
    hdb_b: [f64; HDB_TOTAL],

    // TapeHack2 / post-saturation moving averages
    avg32_l: [f64; 33],
    avg32_r: [f64; 33],
    avg16_l: [f64; 17],
    avg16_r: [f64; 17],
    avg8_l: [f64; 9],
    avg8_r: [f64; 9],
    avg4_l: [f64; 5],
    avg4_r: [f64; 5],
    avg2_l: [f64; 3],
    avg2_r: [f64; 3],
    post32_l: [f64; 33],
    post32_r: [f64; 33],
    post16_l: [f64; 17],
    post16_r: [f64; 17],
    post8_l: [f64; 9],
    post8_r: [f64; 9],
    post4_l: [f64; 5],
    post4_r: [f64; 5],
    post2_l: [f64; 3],
    post2_r: [f64; 3],
    last_dark_l: f64,
    last_dark_r: f64,
    avg_pos: usize,

    // ClipOnly3
    last_sample_l: f64,
    last_sample_r: f64,
    intermediate_l: [f64; 18],
    intermediate_r: [f64; 18],
    slew_l: [f64; 34],
    slew_r: [f64; 34],
    was_pos_clip_l: bool,
    was_pos_clip_r: bool,
    was_neg_clip_l: bool,
    was_neg_clip_r: bool,

    // PRNG (xorshift32)
    fpd_l: u32,
    fpd_r: u32,
}

impl ToTape9 {
    pub fn new() -> Self {
        Self {
            params: ToTape9Params::default(),
            sample_rate: 44100.0,

            iir_enc_l: 0.0,
            iir_enc_r: 0.0,
            iir_dec_l: 0.0,
            iir_dec_r: 0.0,
            comp_enc_l: 1.0,
            comp_enc_r: 1.0,
            comp_dec_l: 1.0,
            comp_dec_r: 1.0,
            avg_enc_l: 0.0,
            avg_enc_r: 0.0,
            avg_dec_l: 0.0,
            avg_dec_r: 0.0,

            d_l: [0.0; 1002],
            d_r: [0.0; 1002],
            sweep_l: PI,
            sweep_r: PI,
            nextmax_l: 0.5,
            nextmax_r: 0.5,
            gcount: 0,

            gslew: [0.0; GSLEW_TOTAL],

            hysteresis_l: 0.0,
            hysteresis_r: 0.0,

            head_bump_l: 0.0,
            head_bump_r: 0.0,
            hdb_a: [0.0; HDB_TOTAL],
            hdb_b: [0.0; HDB_TOTAL],

            avg32_l: [0.0; 33],
            avg32_r: [0.0; 33],
            avg16_l: [0.0; 17],
            avg16_r: [0.0; 17],
            avg8_l: [0.0; 9],
            avg8_r: [0.0; 9],
            avg4_l: [0.0; 5],
            avg4_r: [0.0; 5],
            avg2_l: [0.0; 3],
            avg2_r: [0.0; 3],
            post32_l: [0.0; 33],
            post32_r: [0.0; 33],
            post16_l: [0.0; 17],
            post16_r: [0.0; 17],
            post8_l: [0.0; 9],
            post8_r: [0.0; 9],
            post4_l: [0.0; 5],
            post4_r: [0.0; 5],
            post2_l: [0.0; 3],
            post2_r: [0.0; 3],
            last_dark_l: 0.0,
            last_dark_r: 0.0,
            avg_pos: 0,

            last_sample_l: 0.0,
            last_sample_r: 0.0,
            intermediate_l: [0.0; 18],
            intermediate_r: [0.0; 18],
            slew_l: [0.0; 34],
            slew_r: [0.0; 34],
            was_pos_clip_l: false,
            was_pos_clip_r: false,
            was_neg_clip_l: false,
            was_neg_clip_r: false,

            fpd_l: 17,
            fpd_r: 13,
        }
    }

    /// Advance the xorshift32 PRNG and return the new state.
    #[inline]
    fn xorshift(state: &mut u32) {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
    }

    /// Convert PRNG state to f64 in [0, 1).
    #[inline]
    fn fpd_to_f64(fpd: u32) -> f64 {
        fpd as f64 / u32::MAX as f64
    }
}

impl Default for ToTape9 {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for ToTape9 {
    fn reset(&mut self) {
        self.iir_enc_l = 0.0;
        self.iir_enc_r = 0.0;
        self.iir_dec_l = 0.0;
        self.iir_dec_r = 0.0;
        self.comp_enc_l = 1.0;
        self.comp_enc_r = 1.0;
        self.comp_dec_l = 1.0;
        self.comp_dec_r = 1.0;
        self.avg_enc_l = 0.0;
        self.avg_enc_r = 0.0;
        self.avg_dec_l = 0.0;
        self.avg_dec_r = 0.0;

        self.d_l = [0.0; 1002];
        self.d_r = [0.0; 1002];
        self.sweep_l = PI;
        self.sweep_r = PI;
        self.nextmax_l = 0.5;
        self.nextmax_r = 0.5;
        self.gcount = 0;

        self.gslew = [0.0; GSLEW_TOTAL];

        self.hysteresis_l = 0.0;
        self.hysteresis_r = 0.0;

        self.head_bump_l = 0.0;
        self.head_bump_r = 0.0;
        self.hdb_a = [0.0; HDB_TOTAL];
        self.hdb_b = [0.0; HDB_TOTAL];

        self.avg32_l = [0.0; 33];
        self.avg32_r = [0.0; 33];
        self.avg16_l = [0.0; 17];
        self.avg16_r = [0.0; 17];
        self.avg8_l = [0.0; 9];
        self.avg8_r = [0.0; 9];
        self.avg4_l = [0.0; 5];
        self.avg4_r = [0.0; 5];
        self.avg2_l = [0.0; 3];
        self.avg2_r = [0.0; 3];
        self.post32_l = [0.0; 33];
        self.post32_r = [0.0; 33];
        self.post16_l = [0.0; 17];
        self.post16_r = [0.0; 17];
        self.post8_l = [0.0; 9];
        self.post8_r = [0.0; 9];
        self.post4_l = [0.0; 5];
        self.post4_r = [0.0; 5];
        self.post2_l = [0.0; 3];
        self.post2_r = [0.0; 3];
        self.last_dark_l = 0.0;
        self.last_dark_r = 0.0;
        self.avg_pos = 0;

        self.last_sample_l = 0.0;
        self.last_sample_r = 0.0;
        self.intermediate_l = [0.0; 18];
        self.intermediate_r = [0.0; 18];
        self.slew_l = [0.0; 34];
        self.slew_r = [0.0; 34];
        self.was_pos_clip_l = false;
        self.was_pos_clip_r = false;
        self.was_neg_clip_l = false;
        self.was_neg_clip_r = false;
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        let len = left.len().min(right.len());
        let p = &self.params;

        let overallscale = self.sample_rate / 44100.0;

        let spacing = (overallscale.floor() as usize).clamp(1, 16);
        let slewsing = ((overallscale * 2.0).floor() as usize).clamp(2, 32);

        let input_gain = (p.input_gain * 2.0).powi(2);

        let dubly_amount = p.tilt * 2.0;
        let outly_amount = ((1.0 - p.tilt) * -2.0).max(-1.0);
        let iir_enc_freq = (1.0 - p.shape) / overallscale;
        let iir_dec_freq = p.shape / overallscale;

        let flut_depth = (p.flutter_depth.powi(6) * overallscale * 50.0).min(498.0);
        let flut_frequency = (0.02 * p.flutter_speed.powi(3)) / overallscale;
        let bias = (p.bias * 2.0) - 1.0;
        let under_bias = if bias > 0.0 {
            0.0
        } else {
            (bias.powi(4) * 0.25) / overallscale
        };
        let mut over_bias = if bias < 0.0 {
            1.0 / overallscale
        } else {
            (1.0 - bias).powi(3) / overallscale
        };

        // Set up 9-stage golden-ratio slew thresholds (finest to coarsest)
        self.gslew[THRESHOLD9] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD8] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD7] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD6] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD5] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD4] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD3] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD2] = over_bias;
        over_bias *= PHI;
        self.gslew[THRESHOLD1] = over_bias;

        let head_bump_drive = (p.head_bump * 0.1) / overallscale;
        let head_bump_mix = p.head_bump * 0.5;

        // Head bump biquad coefficients
        self.hdb_a[HDB_FREQ] =
            ((p.head_bump_freq * p.head_bump_freq) * 175.0 + 25.0) / self.sample_rate;
        self.hdb_b[HDB_FREQ] = self.hdb_a[HDB_FREQ] * 0.9375;
        self.hdb_a[HDB_RESO] = PHI.recip(); // 0.618...
        self.hdb_b[HDB_RESO] = self.hdb_a[HDB_RESO];
        self.hdb_a[HDB_A1] = 0.0;
        self.hdb_b[HDB_A1] = 0.0;

        // Biquad A
        let k = (PI * self.hdb_a[HDB_FREQ]).tan();
        let norm = 1.0 / (1.0 + k / self.hdb_a[HDB_RESO] + k * k);
        self.hdb_a[HDB_A0] = k / self.hdb_a[HDB_RESO] * norm;
        self.hdb_a[HDB_A2] = -self.hdb_a[HDB_A0];
        self.hdb_a[HDB_B1] = 2.0 * (k * k - 1.0) * norm;
        self.hdb_a[HDB_B2] = (1.0 - k / self.hdb_a[HDB_RESO] + k * k) * norm;

        // Biquad B (slightly lower freq)
        let k = (PI * self.hdb_b[HDB_FREQ]).tan();
        let norm = 1.0 / (1.0 + k / self.hdb_b[HDB_RESO] + k * k);
        self.hdb_b[HDB_A0] = k / self.hdb_b[HDB_RESO] * norm;
        self.hdb_b[HDB_A2] = -self.hdb_b[HDB_A0];
        self.hdb_b[HDB_B1] = 2.0 * (k * k - 1.0) * norm;
        self.hdb_b[HDB_B2] = (1.0 - k / self.hdb_b[HDB_RESO] + k * k) * norm;

        let output_gain = p.output_gain * 2.0;

        for i in 0..len {
            let mut sample_l = left[i];
            let mut sample_r = right[i];

            // Denormal protection via noise injection
            if sample_l.abs() < 1.18e-23 {
                sample_l = Self::fpd_to_f64(self.fpd_l) * 1.18e-17;
            }
            if sample_r.abs() < 1.18e-23 {
                sample_r = Self::fpd_to_f64(self.fpd_r) * 1.18e-17;
            }

            // ── Input gain ──────────────────────────────────────────
            if input_gain != 1.0 {
                sample_l *= input_gain;
                sample_r *= input_gain;
            }

            // ── Dubly encode ────────────────────────────────────────
            self.iir_enc_l = (self.iir_enc_l * (1.0 - iir_enc_freq)) + (sample_l * iir_enc_freq);
            let mut high_part = (sample_l - self.iir_enc_l) * 2.848;
            high_part += self.avg_enc_l;
            self.avg_enc_l = (sample_l - self.iir_enc_l) * 1.152;
            high_part = high_part.clamp(-1.0, 1.0);
            let mut dubly = high_part.abs();
            if dubly > 0.0 {
                let adjust = (1.0 + 255.0 * dubly).ln() / 2.40823996531;
                if adjust > 0.0 {
                    dubly /= adjust;
                }
                self.comp_enc_l = (self.comp_enc_l * (1.0 - iir_enc_freq)) + (dubly * iir_enc_freq);
                sample_l += (high_part * self.comp_enc_l) * dubly_amount;
            }

            self.iir_enc_r = (self.iir_enc_r * (1.0 - iir_enc_freq)) + (sample_r * iir_enc_freq);
            let mut high_part = (sample_r - self.iir_enc_r) * 2.848;
            high_part += self.avg_enc_r;
            self.avg_enc_r = (sample_r - self.iir_enc_r) * 1.152;
            high_part = high_part.clamp(-1.0, 1.0);
            let mut dubly = high_part.abs();
            if dubly > 0.0 {
                let adjust = (1.0 + 255.0 * dubly).ln() / 2.40823996531;
                if adjust > 0.0 {
                    dubly /= adjust;
                }
                self.comp_enc_r = (self.comp_enc_r * (1.0 - iir_enc_freq)) + (dubly * iir_enc_freq);
                sample_r += (high_part * self.comp_enc_r) * dubly_amount;
            }

            // ── Flutter ─────────────────────────────────────────────
            if flut_depth > 0.0 {
                if self.gcount < 0 || self.gcount > 999 {
                    self.gcount = 999;
                }
                self.d_l[self.gcount as usize] = sample_l;
                let mut count = self.gcount as usize;
                let offset = flut_depth + (flut_depth * self.sweep_l.sin());
                self.sweep_l += self.nextmax_l * flut_frequency;
                if self.sweep_l > PI * 2.0 {
                    self.sweep_l -= PI * 2.0;
                    let flut_a = 0.24 + (Self::fpd_to_f64(self.fpd_l) * 0.74);
                    Self::xorshift(&mut self.fpd_l);
                    let flut_b = 0.24 + (Self::fpd_to_f64(self.fpd_l) * 0.74);
                    if (flut_a - (self.sweep_r + self.nextmax_r).sin()).abs()
                        < (flut_b - (self.sweep_r + self.nextmax_r).sin()).abs()
                    {
                        self.nextmax_l = flut_a;
                    } else {
                        self.nextmax_l = flut_b;
                    }
                }
                count += offset.floor() as usize;
                let frac = offset - offset.floor();
                let idx0 = if count > 999 { count - 1000 } else { count };
                let idx1 = if count + 1 > 999 {
                    count + 1 - 1000
                } else {
                    count + 1
                };
                sample_l = self.d_l[idx0] * (1.0 - frac) + self.d_l[idx1] * frac;

                self.d_r[self.gcount as usize] = sample_r;
                count = self.gcount as usize;
                let offset = flut_depth + (flut_depth * self.sweep_r.sin());
                self.sweep_r += self.nextmax_r * flut_frequency;
                if self.sweep_r > PI * 2.0 {
                    self.sweep_r -= PI * 2.0;
                    let flut_a = 0.24 + (Self::fpd_to_f64(self.fpd_r) * 0.74);
                    Self::xorshift(&mut self.fpd_r);
                    let flut_b = 0.24 + (Self::fpd_to_f64(self.fpd_r) * 0.74);
                    if (flut_a - (self.sweep_l + self.nextmax_l).sin()).abs()
                        < (flut_b - (self.sweep_l + self.nextmax_l).sin()).abs()
                    {
                        self.nextmax_r = flut_a;
                    } else {
                        self.nextmax_r = flut_b;
                    }
                }
                count += offset.floor() as usize;
                let frac = offset - offset.floor();
                let idx0 = if count > 999 { count - 1000 } else { count };
                let idx1 = if count + 1 > 999 {
                    count + 1 - 1000
                } else {
                    count + 1
                };
                sample_r = self.d_r[idx0] * (1.0 - frac) + self.d_r[idx1] * frac;

                self.gcount -= 1;
            }

            // ── Bias (9-stage slew limiter) ─────────────────────────
            if bias.abs() > 0.001 {
                let mut x = 0;
                while x < GSLEW_TOTAL {
                    if under_bias > 0.0 {
                        let stuck = (sample_l - (self.gslew[x] / 0.975)).abs() / under_bias;
                        if stuck < 1.0 {
                            sample_l =
                                (sample_l * stuck) + ((self.gslew[x] / 0.975) * (1.0 - stuck));
                        }
                        let stuck = (sample_r - (self.gslew[x + 1] / 0.975)).abs() / under_bias;
                        if stuck < 1.0 {
                            sample_r =
                                (sample_r * stuck) + ((self.gslew[x + 1] / 0.975) * (1.0 - stuck));
                        }
                    }
                    if (sample_l - self.gslew[x]) > self.gslew[x + 2] {
                        sample_l = self.gslew[x] + self.gslew[x + 2];
                    }
                    if -(sample_l - self.gslew[x]) > self.gslew[x + 2] {
                        sample_l = self.gslew[x] - self.gslew[x + 2];
                    }
                    self.gslew[x] = sample_l * 0.975;
                    if (sample_r - self.gslew[x + 1]) > self.gslew[x + 2] {
                        sample_r = self.gslew[x + 1] + self.gslew[x + 2];
                    }
                    if -(sample_r - self.gslew[x + 1]) > self.gslew[x + 2] {
                        sample_r = self.gslew[x + 1] - self.gslew[x + 2];
                    }
                    self.gslew[x + 1] = sample_r * 0.975;
                    x += 3;
                }
            }

            // ── Hysteresis ──────────────────────────────────────────
            let apply_hyst_l = (1.0 - sample_l.abs()) * (1.0 - sample_l.abs()) * 0.012;
            self.hysteresis_l = (self.hysteresis_l + (sample_l * sample_l.abs()))
                .clamp(-0.011449, 0.011449)
                * 0.999;
            sample_l += self.hysteresis_l * apply_hyst_l;

            let apply_hyst_r = (1.0 - sample_r.abs()) * (1.0 - sample_r.abs()) * 0.012;
            self.hysteresis_r = (self.hysteresis_r + (sample_r * sample_r.abs()))
                .clamp(-0.011449, 0.011449)
                * 0.999;
            sample_r += self.hysteresis_r * apply_hyst_r;

            // ── TapeHack2 (pre-saturation adaptive averaging) ───────
            let mut dark_l = sample_l;
            let mut dark_r = sample_r;
            if self.avg_pos > 31 {
                self.avg_pos = 0;
            }
            if slewsing > 31 {
                self.avg32_l[self.avg_pos] = dark_l;
                self.avg32_r[self.avg_pos] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..32 {
                    dark_l += self.avg32_l[x];
                    dark_r += self.avg32_r[x];
                }
                dark_l /= 32.0;
                dark_r /= 32.0;
            }
            if slewsing > 15 {
                self.avg16_l[self.avg_pos % 16] = dark_l;
                self.avg16_r[self.avg_pos % 16] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..16 {
                    dark_l += self.avg16_l[x];
                    dark_r += self.avg16_r[x];
                }
                dark_l /= 16.0;
                dark_r /= 16.0;
            }
            if slewsing > 7 {
                self.avg8_l[self.avg_pos % 8] = dark_l;
                self.avg8_r[self.avg_pos % 8] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..8 {
                    dark_l += self.avg8_l[x];
                    dark_r += self.avg8_r[x];
                }
                dark_l /= 8.0;
                dark_r /= 8.0;
            }
            if slewsing > 3 {
                self.avg4_l[self.avg_pos % 4] = dark_l;
                self.avg4_r[self.avg_pos % 4] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..4 {
                    dark_l += self.avg4_l[x];
                    dark_r += self.avg4_r[x];
                }
                dark_l /= 4.0;
                dark_r /= 4.0;
            }
            if slewsing > 1 {
                self.avg2_l[self.avg_pos % 2] = dark_l;
                self.avg2_r[self.avg_pos % 2] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..2 {
                    dark_l += self.avg2_l[x];
                    dark_r += self.avg2_r[x];
                }
                dark_l /= 2.0;
                dark_r /= 2.0;
            }
            // Slew-adaptive blend: more smoothing on fast transients
            let mut avg_slew_l =
                ((self.last_dark_l - sample_l).abs() * 0.12 * overallscale).min(1.0);
            avg_slew_l = 1.0 - (1.0 - avg_slew_l - avg_slew_l);
            sample_l = (sample_l * (1.0 - avg_slew_l)) + (dark_l * avg_slew_l);
            self.last_dark_l = dark_l;

            let mut avg_slew_r =
                ((self.last_dark_r - sample_r).abs() * 0.12 * overallscale).min(1.0);
            avg_slew_r = 1.0 - (1.0 - avg_slew_r - avg_slew_r);
            sample_r = (sample_r * (1.0 - avg_slew_r)) + (dark_r * avg_slew_r);
            self.last_dark_r = dark_r;

            // ── TapeHack (saturation — modified sin() Taylor) ───────
            sample_l = sample_l.clamp(-2.305929007734908, 2.305929007734908);
            let mut addtwo = sample_l * sample_l;
            let mut empower = sample_l * addtwo;
            sample_l -= empower / 6.0;
            empower *= addtwo;
            sample_l += empower / 69.0;
            empower *= addtwo;
            sample_l -= empower / 2530.08;
            empower *= addtwo;
            sample_l += empower / 224985.6;
            empower *= addtwo;
            sample_l -= empower / 9979200.0;

            sample_r = sample_r.clamp(-2.305929007734908, 2.305929007734908);
            addtwo = sample_r * sample_r;
            empower = sample_r * addtwo;
            sample_r -= empower / 6.0;
            empower *= addtwo;
            sample_r += empower / 69.0;
            empower *= addtwo;
            sample_r -= empower / 2530.08;
            empower *= addtwo;
            sample_r += empower / 224985.6;
            empower *= addtwo;
            sample_r -= empower / 9979200.0;

            // ── Post-saturation averaging (same structure, post buffers)
            dark_l = sample_l;
            dark_r = sample_r;
            if self.avg_pos > 31 {
                self.avg_pos = 0;
            }
            if slewsing > 31 {
                self.post32_l[self.avg_pos] = dark_l;
                self.post32_r[self.avg_pos] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..32 {
                    dark_l += self.post32_l[x];
                    dark_r += self.post32_r[x];
                }
                dark_l /= 32.0;
                dark_r /= 32.0;
            }
            if slewsing > 15 {
                self.post16_l[self.avg_pos % 16] = dark_l;
                self.post16_r[self.avg_pos % 16] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..16 {
                    dark_l += self.post16_l[x];
                    dark_r += self.post16_r[x];
                }
                dark_l /= 16.0;
                dark_r /= 16.0;
            }
            if slewsing > 7 {
                self.post8_l[self.avg_pos % 8] = dark_l;
                self.post8_r[self.avg_pos % 8] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..8 {
                    dark_l += self.post8_l[x];
                    dark_r += self.post8_r[x];
                }
                dark_l /= 8.0;
                dark_r /= 8.0;
            }
            if slewsing > 3 {
                self.post4_l[self.avg_pos % 4] = dark_l;
                self.post4_r[self.avg_pos % 4] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..4 {
                    dark_l += self.post4_l[x];
                    dark_r += self.post4_r[x];
                }
                dark_l /= 4.0;
                dark_r /= 4.0;
            }
            if slewsing > 1 {
                self.post2_l[self.avg_pos % 2] = dark_l;
                self.post2_r[self.avg_pos % 2] = dark_r;
                dark_l = 0.0;
                dark_r = 0.0;
                for x in 0..2 {
                    dark_l += self.post2_l[x];
                    dark_r += self.post2_r[x];
                }
                dark_l /= 2.0;
                dark_r /= 2.0;
            }
            self.avg_pos += 1;
            sample_l = (sample_l * (1.0 - avg_slew_l)) + (dark_l * avg_slew_l);
            sample_r = (sample_r * (1.0 - avg_slew_r)) + (dark_r * avg_slew_r);

            // ── Head Bump ───────────────────────────────────────────
            let mut head_bump_sample_l = 0.0;
            let mut head_bump_sample_r = 0.0;
            if head_bump_mix > 0.0 {
                self.head_bump_l += sample_l * head_bump_drive;
                self.head_bump_l -= self.head_bump_l
                    * self.head_bump_l
                    * self.head_bump_l
                    * (0.0618 / overallscale.sqrt());
                self.head_bump_r += sample_r * head_bump_drive;
                self.head_bump_r -= self.head_bump_r
                    * self.head_bump_r
                    * self.head_bump_r
                    * (0.0618 / overallscale.sqrt());

                // Biquad A — left
                let head_biq_l = (self.head_bump_l * self.hdb_a[HDB_A0]) + self.hdb_a[HDB_SL1];
                self.hdb_a[HDB_SL1] = (self.head_bump_l * self.hdb_a[HDB_A1])
                    - (head_biq_l * self.hdb_a[HDB_B1])
                    + self.hdb_a[HDB_SL2];
                self.hdb_a[HDB_SL2] =
                    (self.head_bump_l * self.hdb_a[HDB_A2]) - (head_biq_l * self.hdb_a[HDB_B2]);
                // Biquad B — left
                head_bump_sample_l = (head_biq_l * self.hdb_b[HDB_A0]) + self.hdb_b[HDB_SL1];
                self.hdb_b[HDB_SL1] = (head_biq_l * self.hdb_b[HDB_A1])
                    - (head_bump_sample_l * self.hdb_b[HDB_B1])
                    + self.hdb_b[HDB_SL2];
                self.hdb_b[HDB_SL2] =
                    (head_biq_l * self.hdb_b[HDB_A2]) - (head_bump_sample_l * self.hdb_b[HDB_B2]);

                // Biquad A — right
                let head_biq_r = (self.head_bump_r * self.hdb_a[HDB_A0]) + self.hdb_a[HDB_SR1];
                self.hdb_a[HDB_SR1] = (self.head_bump_r * self.hdb_a[HDB_A1])
                    - (head_biq_r * self.hdb_a[HDB_B1])
                    + self.hdb_a[HDB_SR2];
                self.hdb_a[HDB_SR2] =
                    (self.head_bump_r * self.hdb_a[HDB_A2]) - (head_biq_r * self.hdb_a[HDB_B2]);
                // Biquad B — right
                head_bump_sample_r = (head_biq_r * self.hdb_b[HDB_A0]) + self.hdb_b[HDB_SR1];
                self.hdb_b[HDB_SR1] = (head_biq_r * self.hdb_b[HDB_A1])
                    - (head_bump_sample_r * self.hdb_b[HDB_B1])
                    + self.hdb_b[HDB_SR2];
                self.hdb_b[HDB_SR2] =
                    (head_biq_r * self.hdb_b[HDB_A2]) - (head_bump_sample_r * self.hdb_b[HDB_B2]);
            }

            sample_l += head_bump_sample_l * head_bump_mix;
            sample_r += head_bump_sample_r * head_bump_mix;

            // ── Dubly decode ────────────────────────────────────────
            self.iir_dec_l = (self.iir_dec_l * (1.0 - iir_dec_freq)) + (sample_l * iir_dec_freq);
            let mut high_part = (sample_l - self.iir_dec_l) * 2.628;
            high_part += self.avg_dec_l;
            self.avg_dec_l = (sample_l - self.iir_dec_l) * 1.372;
            high_part = high_part.clamp(-1.0, 1.0);
            let mut dubly = high_part.abs();
            if dubly > 0.0 {
                let adjust = (1.0 + 255.0 * dubly).ln() / 2.40823996531;
                if adjust > 0.0 {
                    dubly /= adjust;
                }
                self.comp_dec_l = (self.comp_dec_l * (1.0 - iir_dec_freq)) + (dubly * iir_dec_freq);
                sample_l += (high_part * self.comp_dec_l) * outly_amount;
            }

            self.iir_dec_r = (self.iir_dec_r * (1.0 - iir_dec_freq)) + (sample_r * iir_dec_freq);
            let mut high_part = (sample_r - self.iir_dec_r) * 2.628;
            high_part += self.avg_dec_r;
            self.avg_dec_r = (sample_r - self.iir_dec_r) * 1.372;
            high_part = high_part.clamp(-1.0, 1.0);
            let mut dubly = high_part.abs();
            if dubly > 0.0 {
                let adjust = (1.0 + 255.0 * dubly).ln() / 2.40823996531;
                if adjust > 0.0 {
                    dubly /= adjust;
                }
                self.comp_dec_r = (self.comp_dec_r * (1.0 - iir_dec_freq)) + (dubly * iir_dec_freq);
                sample_r += (high_part * self.comp_dec_r) * outly_amount;
            }

            // ── Output gain ─────────────────────────────────────────
            if output_gain != 1.0 {
                sample_l *= output_gain;
                sample_r *= output_gain;
            }

            // ── ClipOnly3 ───────────────────────────────────────────
            let noise = 1.0 - (Self::fpd_to_f64(self.fpd_l) * 0.076);

            if self.was_pos_clip_l {
                if sample_l < self.last_sample_l {
                    self.last_sample_l = (0.9085097 * noise) + (sample_l * (1.0 - noise));
                } else {
                    self.last_sample_l = 0.94;
                }
            }
            self.was_pos_clip_l = false;
            if sample_l > 0.9085097 {
                self.was_pos_clip_l = true;
                sample_l = (0.9085097 * noise) + (self.last_sample_l * (1.0 - noise));
            }
            if self.was_neg_clip_l {
                if sample_l > self.last_sample_l {
                    self.last_sample_l = (-0.9085097 * noise) + (sample_l * (1.0 - noise));
                } else {
                    self.last_sample_l = -0.94;
                }
            }
            self.was_neg_clip_l = false;
            if sample_l < -0.9085097 {
                self.was_neg_clip_l = true;
                sample_l = (-0.9085097 * noise) + (self.last_sample_l * (1.0 - noise));
            }
            self.slew_l[spacing * 2] = (self.last_sample_l - sample_l).abs();
            for x in (1..=spacing * 2).rev() {
                self.slew_l[x - 1] = self.slew_l[x];
            }
            self.intermediate_l[spacing] = sample_l;
            sample_l = self.last_sample_l;
            for x in (1..=spacing).rev() {
                self.intermediate_l[x - 1] = self.intermediate_l[x];
            }
            self.last_sample_l = self.intermediate_l[0];
            if self.was_pos_clip_l || self.was_neg_clip_l {
                for x in 1..=spacing {
                    self.last_sample_l += self.intermediate_l[x];
                }
                self.last_sample_l /= spacing as f64;
            }
            let mut final_slew = 0.0_f64;
            for x in (0..=spacing * 2).rev() {
                if final_slew < self.slew_l[x] {
                    final_slew = self.slew_l[x];
                }
            }
            let postclip = 0.94 / (1.0 + (final_slew * 1.3986013));
            sample_l = sample_l.clamp(-postclip, postclip);

            // ClipOnly3 — right channel
            let noise = 1.0 - (Self::fpd_to_f64(self.fpd_r) * 0.076);

            if self.was_pos_clip_r {
                if sample_r < self.last_sample_r {
                    self.last_sample_r = (0.9085097 * noise) + (sample_r * (1.0 - noise));
                } else {
                    self.last_sample_r = 0.94;
                }
            }
            self.was_pos_clip_r = false;
            if sample_r > 0.9085097 {
                self.was_pos_clip_r = true;
                sample_r = (0.9085097 * noise) + (self.last_sample_r * (1.0 - noise));
            }
            if self.was_neg_clip_r {
                if sample_r > self.last_sample_r {
                    self.last_sample_r = (-0.9085097 * noise) + (sample_r * (1.0 - noise));
                } else {
                    self.last_sample_r = -0.94;
                }
            }
            self.was_neg_clip_r = false;
            if sample_r < -0.9085097 {
                self.was_neg_clip_r = true;
                sample_r = (-0.9085097 * noise) + (self.last_sample_r * (1.0 - noise));
            }
            self.slew_r[spacing * 2] = (self.last_sample_r - sample_r).abs();
            for x in (1..=spacing * 2).rev() {
                self.slew_r[x - 1] = self.slew_r[x];
            }
            self.intermediate_r[spacing] = sample_r;
            sample_r = self.last_sample_r;
            for x in (1..=spacing).rev() {
                self.intermediate_r[x - 1] = self.intermediate_r[x];
            }
            self.last_sample_r = self.intermediate_r[0];
            if self.was_pos_clip_r || self.was_neg_clip_r {
                for x in 1..=spacing {
                    self.last_sample_r += self.intermediate_r[x];
                }
                self.last_sample_r /= spacing as f64;
            }
            final_slew = 0.0;
            for x in (0..=spacing * 2).rev() {
                if final_slew < self.slew_r[x] {
                    final_slew = self.slew_r[x];
                }
            }
            let postclip = 0.94 / (1.0 + (final_slew * 1.3986013));
            sample_r = sample_r.clamp(-postclip, postclip);

            // ── Dither (advance PRNG) ───────────────────────────────
            Self::xorshift(&mut self.fpd_l);
            Self::xorshift(&mut self.fpd_r);

            left[i] = sample_l;
            right[i] = sample_r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44100.0;

    fn config() -> AudioConfig {
        AudioConfig {
            sample_rate: SR,
            max_buffer_size: 512,
        }
    }

    #[test]
    fn silence_in_silence_out() {
        let mut t = ToTape9::new();
        t.update(config());
        let mut l = vec![0.0; 4410];
        let mut r = vec![0.0; 4410];
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.abs() < 1e-6, "Non-silent output at sample {i}: {s}");
        }
    }

    #[test]
    fn no_nan_or_inf() {
        let mut t = ToTape9::new();
        t.update(config());
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at L[{i}]: {s}");
        }
        for (i, &s) in r.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at R[{i}]: {s}");
        }
    }

    #[test]
    fn saturates_loud_signal() {
        let mut t = ToTape9::new();
        t.params.input_gain = 1.0; // 4x gain
        t.update(config());
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin())
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        let max = l.iter().fold(0.0_f64, |a, &b| a.max(b.abs()));
        assert!(max < 1.0, "Should be clipped below 1.0 but max is {max}");
        assert!(max > 0.5, "Should still have signal, max is {max}");
    }

    #[test]
    fn default_params_passthrough_character() {
        let mut t = ToTape9::new();
        t.update(config());
        // At default params (all 0.5), signal should pass through with tape character
        let mut l: Vec<f64> = (0..44100)
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / SR).sin() * 0.3)
            .collect();
        let mut r = l.clone();
        let input_rms: f64 = (l.iter().map(|s| s * s).sum::<f64>() / l.len() as f64).sqrt();
        t.process(&mut l, &mut r);
        let output_rms: f64 = (l.iter().map(|s| s * s).sum::<f64>() / l.len() as f64).sqrt();
        // Output should be similar level (within 6dB)
        let ratio = output_rms / input_rms;
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "Level ratio {ratio} out of expected range"
        );
    }

    #[test]
    fn high_sample_rate_no_panic() {
        let mut t = ToTape9::new();
        t.update(AudioConfig {
            sample_rate: 192000.0,
            max_buffer_size: 512,
        });
        let mut l: Vec<f64> = (0..19200)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / 192000.0).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "Non-finite at 192k L[{i}]");
        }
    }
}
