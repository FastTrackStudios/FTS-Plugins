/// A constant-Q complex resonator implemented as a first-order IIR filter.
///
/// Transfer function: `y[n] = b0 * x[n] + a1 * y[n-1]`
///
/// Coefficient calculation from the reference implementation
/// (luciamarock/Polyphonic-Pitch-Detector-for-guitars).
#[derive(Debug, Clone)]
pub struct Resonator {
    /// Complex coefficient a1 (feedback): (real, imag).
    a1: (f64, f64),
    /// Real gain coefficient b0 (feedforward).
    b0: f64,
    /// Complex state y[n-1]: (real, imag).
    state: (f64, f64),
    /// Accumulated energy (|y|^2) over the current analysis window.
    energy: f64,
}

impl Resonator {
    pub fn new() -> Self {
        Self {
            a1: (0.0, 0.0),
            b0: 0.0,
            state: (0.0, 0.0),
            energy: 0.0,
        }
    }

    /// Initialize coefficients for the given frequency and sample rate.
    ///
    /// Uses the reference implementation's coefficient formula:
    /// - d = 2^(1/24) (half-semitone ratio)
    /// - c = (2d - 2) / (d + 1) (constant-Q bandwidth factor)
    /// - r_omega = 2 * frequency * c (decay rate)
    /// - a1 = exp((-r_omega + j*omega) / sample_rate)
    /// - b0 = ((1 - r²) / r) / √2
    pub fn init(&mut self, frequency_hz: f64, sample_rate: f64) {
        let d = 2.0_f64.powf(1.0 / 24.0);
        let c = (2.0 * d - 2.0) / (d + 1.0);

        let omega = 2.0 * std::f64::consts::PI * frequency_hz;
        let r_omega = 2.0 * frequency_hz * c;

        // Pole radius: r = exp(-r_omega / sample_rate)
        let r = (-r_omega / sample_rate).exp();

        // Complex pole at r * e^(j*omega/SR)
        let angle = omega / sample_rate;
        self.a1 = (r * angle.cos(), r * angle.sin());

        // Feedforward gain from reference: ((1 - r²) / r) / √2
        self.b0 = ((1.0 - r * r) / r) / std::f64::consts::SQRT_2;
    }

    /// Process one input sample, returning the squared magnitude of the output.
    ///
    /// Accumulates energy internally for later retrieval.
    #[inline]
    pub fn process_sample(&mut self, input: f64) -> f64 {
        // y[n] = b0 * x[n] + a1 * y[n-1]
        let feedback_re = self.a1.0 * self.state.0 - self.a1.1 * self.state.1;
        let feedback_im = self.a1.0 * self.state.1 + self.a1.1 * self.state.0;

        let y_re = self.b0 * input + feedback_re;
        let y_im = feedback_im;

        self.state = (y_re, y_im);

        let mag_sq = y_re * y_re + y_im * y_im;
        self.energy += mag_sq;
        mag_sq
    }

    /// Return accumulated energy and reset the accumulator.
    #[inline]
    pub fn take_energy(&mut self) -> f64 {
        let e = self.energy;
        self.energy = 0.0;
        e
    }

    /// Reset internal state (but not coefficients).
    pub fn reset(&mut self) {
        self.state = (0.0, 0.0);
        self.energy = 0.0;
    }
}
