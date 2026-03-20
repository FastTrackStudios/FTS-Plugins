//! NamSlot — single NAM model slot with sample-rate resampling and
//! loudness normalization.
//!
//! Each slot loads a `.nam` model file and handles:
//! - Automatic resampling when the model's expected sample rate differs from the host
//! - Optional loudness normalization (target: -18 dB, matching Ratatouille)

use neural_amp_modeler::NamModel;

use crate::resampler::LinearResampler;

/// Target loudness for normalization (dB).
const NORMALIZE_TARGET_DB: f64 = -18.0;

/// A single NAM model slot with resampling support.
pub struct NamSlot {
    model: Option<NamModel>,
    /// Input gain in linear amplitude.
    pub input_gain: f64,
    /// Whether to normalize output level based on model loudness metadata.
    pub normalize: bool,

    // Resampling state
    upsample: LinearResampler,
    downsample: LinearResampler,
    resample_buf: Vec<f64>,
    resample_out: Vec<f64>,
    needs_resample: bool,
    model_rate: f64,
    host_rate: f64,

    // Normalization gain (computed from model loudness metadata)
    norm_gain: f64,
}

impl NamSlot {
    pub fn new() -> Self {
        Self {
            model: None,
            input_gain: 1.0,
            normalize: false,
            upsample: LinearResampler::new(),
            downsample: LinearResampler::new(),
            resample_buf: Vec::new(),
            resample_out: Vec::new(),
            needs_resample: false,
            model_rate: 48000.0,
            host_rate: 48000.0,
            norm_gain: 1.0,
        }
    }

    /// Load a `.nam` model file. Returns an error string on failure.
    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let model = NamModel::load(path)?;

        // Compute normalization gain from loudness metadata
        self.norm_gain = match model.loudness() {
            Some(loudness) => 10.0_f64.powf((NORMALIZE_TARGET_DB - loudness) / 20.0),
            None => 1.0,
        };

        // Check if resampling is needed
        self.model_rate = model.expected_sample_rate().unwrap_or(48000.0);
        self.needs_resample = (self.model_rate - self.host_rate).abs() > 1.0;

        if self.needs_resample {
            // Host → model rate (before inference)
            self.upsample.set_rates(self.host_rate, self.model_rate);
            self.upsample.reset();
            // Model → host rate (after inference)
            self.downsample.set_rates(self.model_rate, self.host_rate);
            self.downsample.reset();
        }

        self.model = Some(model);
        self.reset_model();
        Ok(())
    }

    /// Unload the current model.
    pub fn unload(&mut self) {
        self.model = None;
        self.norm_gain = 1.0;
        self.needs_resample = false;
    }

    /// Whether a model is loaded.
    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    /// Reset for new sample rate / buffer size.
    pub fn update(&mut self, host_rate: f64, max_buffer_size: usize) {
        self.host_rate = host_rate;

        if let Some(ref model) = self.model {
            self.model_rate = model.expected_sample_rate().unwrap_or(48000.0);
            self.needs_resample = (self.model_rate - self.host_rate).abs() > 1.0;

            if self.needs_resample {
                self.upsample.set_rates(self.host_rate, self.model_rate);
                self.upsample.reset();
                self.downsample.set_rates(self.model_rate, self.host_rate);
                self.downsample.reset();

                // Allocate resample buffers with headroom
                let max_resampled = (max_buffer_size as f64 * self.model_rate / self.host_rate)
                    .ceil() as usize
                    + 16;
                self.resample_buf.resize(max_resampled, 0.0);
                self.resample_out.resize(max_resampled, 0.0);
            }
        }

        self.reset_model();
    }

    fn reset_model(&mut self) {
        if let Some(ref mut model) = self.model {
            let rate = if self.needs_resample {
                self.model_rate
            } else {
                self.host_rate
            };
            model.reset(rate, 4096);
        }
    }

    /// Process a mono buffer through this slot. Writes output into `output`.
    /// If no model is loaded, copies input to output with input gain.
    pub fn process(&mut self, input: &[f64], output: &mut [f64]) {
        let n = input.len().min(output.len());

        let model = match self.model.as_mut() {
            Some(m) => m,
            None => {
                // No model — passthrough with input gain
                for i in 0..n {
                    output[i] = input[i] * self.input_gain;
                }
                return;
            }
        };

        if !self.needs_resample {
            // Direct processing — apply input gain first
            // Use output as scratch for gained input
            for i in 0..n {
                output[i] = input[i] * self.input_gain;
            }
            // Process in-place (output serves as both input and output)
            let (inp, out) = unsafe {
                // Safe: NAM reads input fully before writing output
                let ptr = output.as_mut_ptr();
                (
                    std::slice::from_raw_parts(ptr, n),
                    std::slice::from_raw_parts_mut(ptr, n),
                )
            };
            model.process(inp, out);

            // Apply normalization
            if self.normalize && self.norm_gain != 1.0 {
                for s in output[..n].iter_mut() {
                    *s *= self.norm_gain;
                }
            }
        } else {
            // Resample: host rate → model rate → inference → model rate → host rate

            // 1. Apply input gain
            let gained: Vec<f64> = input[..n].iter().map(|&s| s * self.input_gain).collect();

            // 2. Upsample to model rate
            let max_resampled = (n as f64 * self.model_rate / self.host_rate).ceil() as usize + 16;
            if self.resample_buf.len() < max_resampled {
                self.resample_buf.resize(max_resampled, 0.0);
            }
            if self.resample_out.len() < max_resampled {
                self.resample_out.resize(max_resampled, 0.0);
            }

            let resampled_n = self.upsample.process(&gained, &mut self.resample_buf);

            // 3. Run inference at model rate
            model.process(
                &self.resample_buf[..resampled_n],
                &mut self.resample_out[..resampled_n],
            );

            // 4. Downsample back to host rate
            let written = self
                .downsample
                .process(&self.resample_out[..resampled_n], &mut output[..n]);

            // Zero any remaining output
            for s in output[written..n].iter_mut() {
                *s = 0.0;
            }

            // Apply normalization
            if self.normalize && self.norm_gain != 1.0 {
                for s in output[..n].iter_mut() {
                    *s *= self.norm_gain;
                }
            }
        }
    }

    /// Get model metadata, if loaded.
    pub fn metadata(&self) -> Option<neural_amp_modeler::ModelMetadata> {
        self.model.as_ref().map(|m| m.metadata())
    }
}

impl Default for NamSlot {
    fn default() -> Self {
        Self::new()
    }
}
