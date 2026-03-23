//! Feedback Delay Network (FDN) — the workhorse of Room and Hall reverbs.
//!
//! N parallel delay lines mixed through a unitary feedback matrix
//! (Householder or Hadamard) with per-line damping filters.

use fts_dsp::delay_line::DelayLine;

use super::householder;
use super::one_pole::Lp1;

/// Mixing matrix type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MixMatrix {
    Householder,
    Hadamard,
}

/// Generic FDN with N delay lines.
pub struct Fdn {
    delays: Vec<DelayLine>,
    delay_samples: Vec<usize>,
    damping: Vec<Lp1>,
    feedback: Vec<f64>, // Per-line state (output of delay -> matrix input)
    decay_gain: f64,    // Overall decay multiplier
    mix_matrix: MixMatrix,
    num_lines: usize,
}

impl Fdn {
    /// Create an FDN with the given delay lengths (in samples).
    pub fn new(delay_lengths: &[usize], matrix: MixMatrix) -> Self {
        let n = delay_lengths.len();
        let delays = delay_lengths
            .iter()
            .map(|&len| DelayLine::new(len + 1))
            .collect();
        let delay_samples = delay_lengths.to_vec();
        let damping = (0..n).map(|_| Lp1::new()).collect();
        let feedback = vec![0.0; n];

        Self {
            delays,
            delay_samples,
            damping,
            feedback,
            decay_gain: 0.85,
            mix_matrix: matrix,
            num_lines: n,
        }
    }

    /// Set all delay lengths (in samples). Must match the number of lines.
    pub fn set_delays(&mut self, lengths: &[usize]) {
        for (i, &len) in lengths.iter().enumerate().take(self.num_lines) {
            self.delay_samples[i] = len.min(self.delays[i].len() - 1);
        }
    }

    /// Set the damping filter cutoff for all lines.
    pub fn set_damping(&mut self, freq_hz: f64, sample_rate: f64) {
        for d in &mut self.damping {
            d.set_freq(freq_hz, sample_rate);
        }
    }

    /// Set the damping coefficient directly (0.0 = no damping, 1.0 = max).
    pub fn set_damping_coeff(&mut self, g: f64) {
        for d in &mut self.damping {
            d.set_coeff(g);
        }
    }

    /// Set the overall decay gain (0.0 = no feedback, 1.0 = infinite).
    pub fn set_decay(&mut self, gain: f64) {
        self.decay_gain = gain.clamp(0.0, 0.999);
    }

    /// Process one mono input sample, return the mixed output of all lines.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        let n = self.num_lines;

        // Read from all delay lines
        for i in 0..n {
            self.feedback[i] = self.delays[i].read(self.delay_samples[i]);
        }

        // Apply mixing matrix
        match self.mix_matrix {
            MixMatrix::Householder => householder::mix(&mut self.feedback[..n]),
            MixMatrix::Hadamard => {
                // Hadamard requires power of 2 — if not, fall back to Householder
                if n.is_power_of_two() {
                    super::hadamard::mix(&mut self.feedback[..n]);
                } else {
                    householder::mix(&mut self.feedback[..n]);
                }
            }
        }

        // Sum output before feeding back (tap from delay outputs)
        let mut output = 0.0;
        let output_scale = 1.0 / (n as f64).sqrt();

        for i in 0..n {
            output += self.delays[i].read(self.delay_samples[i]) * output_scale;

            // Apply damping and decay, then feed back with input injection
            let fb = self.damping[i].tick(self.feedback[i]) * self.decay_gain;
            self.delays[i].write(input + fb);
        }

        output
    }

    pub fn reset(&mut self) {
        for d in &mut self.delays {
            d.clear();
        }
        for d in &mut self.damping {
            d.reset();
        }
        self.feedback.fill(0.0);
    }
}
