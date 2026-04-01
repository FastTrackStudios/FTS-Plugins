//! SaturateEngine — dispatches processing to the active style's processor.

use fts_dsp::{AudioConfig, Processor};

use crate::density::Density;
use crate::hard_vacuum::HardVacuum;
use crate::interstage::Interstage;
use crate::mojo;
use crate::purest_drive::PurestDrive;
use crate::style::{Category, Style};
use crate::to_tape9::ToTape9;
use crate::tube2::Tube2;
use crate::waveshaper::{Curve, WaveshaperProcessor};

/// Universal parameters that apply to all styles.
#[derive(Clone, Copy)]
pub struct EngineParams {
    /// Drive amount (0–1). Maps to input_gain for tape, waveshaping drive for others.
    pub drive: f64,
    /// Dry/wet mix (0–1).
    pub mix: f64,
    /// Output gain (0–1, maps to 0–2×).
    pub output: f64,
    /// Tone control (0–1). 0.5 = neutral. < 0.5 = darker, > 0.5 = brighter.
    pub tone: f64,
    /// Body control (0–1). Low-end character — head bump for tape, weight for others.
    pub body: f64,

    // ── Tape-specific params ────────────────────────────────────────
    /// Flutter depth (0–1). Tape only.
    pub flutter: f64,
    /// Flutter speed (0–1). Tape only.
    pub flutter_speed: f64,
    /// Bias (0–1). Tape only. 0.5 = center.
    pub bias: f64,
}

impl Default for EngineParams {
    fn default() -> Self {
        Self {
            drive: 0.5,
            mix: 1.0,
            output: 0.5,
            tone: 0.5,
            body: 0.5,
            flutter: 0.5,
            flutter_speed: 0.5,
            bias: 0.5,
        }
    }
}

/// Which processor backend the engine should dispatch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Tape,
    Tube2,
    Density,
    PurestDrive,
    Mojo,
    HardVacuum,
    Interstage,
    Waveshaper,
}

impl Backend {
    fn from_style(style: &Style) -> Self {
        match style.category {
            Category::Tape => Self::Tape,
            Category::Tube => Self::Tube2,
            Category::Saturation => match style.variant {
                0 => Self::Mojo,        // Mojo
                1 => Self::PurestDrive, // PurestDrive
                _ => Self::Density,     // Density
            },
            Category::Amp => Self::HardVacuum,
            Category::Transformer => Self::Interstage,
            Category::FX => Self::Waveshaper,
        }
    }
}

/// Main saturation engine. Holds all processor variants and dispatches
/// to the one selected by the current style.
pub struct SaturateEngine {
    pub params: EngineParams,
    pub style: Style,

    backend: Backend,
    tape: ToTape9,
    tube2: Tube2,
    density: Density,
    purest_drive: PurestDrive,
    hard_vacuum: HardVacuum,
    interstage: Interstage,
    waveshaper: WaveshaperProcessor,
    sample_rate: f64,
}

impl SaturateEngine {
    pub fn new() -> Self {
        Self {
            params: EngineParams::default(),
            style: Style::default(),
            backend: Backend::Tape,
            tape: ToTape9::new(),
            tube2: Tube2::new(),
            density: Density::new(),
            purest_drive: PurestDrive::new(),
            hard_vacuum: HardVacuum::new(),
            interstage: Interstage::new(),
            waveshaper: WaveshaperProcessor::new(),
            sample_rate: 44100.0,
        }
    }

    /// Set the active style. If the backend changed, reset the new processor.
    pub fn set_style(&mut self, style: Style) {
        let new_backend = Backend::from_style(&style);
        let backend_changed = self.backend != new_backend;
        self.style = style;
        self.backend = new_backend;

        if new_backend == Backend::Waveshaper {
            self.waveshaper.set_curve(Curve::from_style(&style));
        }

        if backend_changed {
            match new_backend {
                Backend::Tape => self.tape.reset(),
                Backend::Tube2 => self.tube2.reset(),
                Backend::Density => self.density.reset(),
                Backend::PurestDrive => self.purest_drive.reset(),
                Backend::Mojo => {} // stateless
                Backend::HardVacuum => self.hard_vacuum.reset(),
                Backend::Interstage => self.interstage.reset(),
                Backend::Waveshaper => self.waveshaper.reset(),
            }
        }
    }

    /// Sync universal params to the active processor.
    fn sync_params(&mut self) {
        let p = &self.params;

        match self.backend {
            Backend::Tape => {
                let t = &mut self.tape.params;
                t.input_gain = p.drive;
                t.tilt = p.tone;
                t.shape = p.tone;
                t.flutter_depth = p.flutter;
                t.flutter_speed = p.flutter_speed;
                t.bias = p.bias;
                t.head_bump = p.body;
                t.head_bump_freq = p.body;
                t.output_gain = p.output;
            }
            Backend::Tube2 => {
                self.tube2.input_pad = p.drive;
                self.tube2.iterations = p.drive;
            }
            Backend::Density => {
                // Drive 0–1 maps to density 0–4
                self.density.density = 1.0 + p.drive * 3.0;
                self.density.highpass = p.tone.max(0.0).min(0.5) * 2.0; // tone < 0.5 adds HP
                self.density.output = p.output * 2.0;
                self.density.mix = p.mix;
            }
            Backend::PurestDrive => {
                self.purest_drive.intensity = p.drive;
            }
            Backend::Mojo => {
                // Mojo is a stateless fn — params synced in process()
            }
            Backend::HardVacuum => {
                self.hard_vacuum.multistage = p.drive;
                self.hard_vacuum.warmth = p.body;
                self.hard_vacuum.aura = p.tone;
                self.hard_vacuum.output = p.output * 2.0;
                self.hard_vacuum.mix = p.mix;
            }
            Backend::Interstage => {
                self.interstage.intensity = p.drive;
            }
            Backend::Waveshaper => {
                self.waveshaper.drive = p.drive;
                self.waveshaper.mix = p.mix;
                self.waveshaper.output_gain = p.output * 2.0;
                self.waveshaper.tone = p.tone;
            }
        }
    }

    /// Apply mix and output for backends that don't handle it internally.
    #[inline]
    fn apply_mix_output(
        left: &mut [f64],
        right: &mut [f64],
        dry_l: &[f64],
        dry_r: &[f64],
        mix: f64,
        output: f64,
    ) {
        let len = left.len().min(right.len());
        for i in 0..len {
            left[i] = (dry_l[i] * (1.0 - mix) + left[i] * mix) * output;
            right[i] = (dry_r[i] * (1.0 - mix) + right[i] * mix) * output;
        }
    }
}

impl Default for SaturateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for SaturateEngine {
    fn reset(&mut self) {
        self.tape.reset();
        self.tube2.reset();
        self.density.reset();
        self.purest_drive.reset();
        self.hard_vacuum.reset();
        self.interstage.reset();
        self.waveshaper.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.sample_rate = config.sample_rate;
        self.tape.update(config);
        self.tube2.update(config);
        self.density.update(config);
        self.purest_drive.update(config);
        self.hard_vacuum.update(config);
        self.interstage.update(config);
        self.waveshaper.update(config);
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        self.sync_params();
        let mix = self.params.mix;
        let output = self.params.output * 2.0;

        match self.backend {
            Backend::Tape => {
                // Tape handles output internally; apply mix externally
                if mix >= 0.999 {
                    self.tape.process(left, right);
                } else {
                    let len = left.len().min(right.len());
                    let dry_l: Vec<f64> = left[..len].to_vec();
                    let dry_r: Vec<f64> = right[..len].to_vec();
                    self.tape.process(left, right);
                    for i in 0..len {
                        left[i] = dry_l[i] * (1.0 - mix) + left[i] * mix;
                        right[i] = dry_r[i] * (1.0 - mix) + right[i] * mix;
                    }
                }
            }
            Backend::Tube2 => {
                let len = left.len().min(right.len());
                let dry_l: Vec<f64> = left[..len].to_vec();
                let dry_r: Vec<f64> = right[..len].to_vec();
                self.tube2.process(left, right);
                Self::apply_mix_output(left, right, &dry_l, &dry_r, mix, output);
            }
            Backend::Density => {
                // Density handles mix/output internally
                self.density.process(left, right);
            }
            Backend::PurestDrive => {
                let len = left.len().min(right.len());
                let dry_l: Vec<f64> = left[..len].to_vec();
                let dry_r: Vec<f64> = right[..len].to_vec();
                self.purest_drive.process(left, right);
                Self::apply_mix_output(left, right, &dry_l, &dry_r, mix, output);
            }
            Backend::Mojo => {
                let drive = self.params.drive;
                let len = left.len().min(right.len());
                for i in 0..len {
                    let dry_l = left[i];
                    let dry_r = right[i];
                    let wet_l = mojo::mojo(left[i], drive);
                    let wet_r = mojo::mojo(right[i], drive);
                    left[i] = (dry_l * (1.0 - mix) + wet_l * mix) * output;
                    right[i] = (dry_r * (1.0 - mix) + wet_r * mix) * output;
                }
            }
            Backend::HardVacuum => {
                // HardVacuum handles mix/output internally
                self.hard_vacuum.process(left, right);
            }
            Backend::Interstage => {
                let len = left.len().min(right.len());
                let dry_l: Vec<f64> = left[..len].to_vec();
                let dry_r: Vec<f64> = right[..len].to_vec();
                self.interstage.process(left, right);
                Self::apply_mix_output(left, right, &dry_l, &dry_r, mix, output);
            }
            Backend::Waveshaper => {
                // WaveshaperProcessor handles mix/output internally
                self.waveshaper.process(left, right);
            }
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

    fn sine_buffer(len: usize, freq: f64, amp: f64) -> Vec<f64> {
        (0..len)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / SR).sin() * amp)
            .collect()
    }

    #[test]
    fn all_styles_no_nan() {
        for cat_idx in 0..Category::COUNT {
            let cat = Category::from_index(cat_idx);
            for var_idx in 0..cat.variant_count() {
                let style = Style::new(cat, var_idx);
                let mut engine = SaturateEngine::new();
                engine.set_style(style);
                engine.update(config());

                let mut l = sine_buffer(4410, 440.0, 0.5);
                let mut r = l.clone();
                engine.process(&mut l, &mut r);

                for (i, &s) in l.iter().enumerate() {
                    assert!(
                        s.is_finite(),
                        "NaN/Inf at L[{i}] for {}",
                        style.display_name()
                    );
                }
            }
        }
    }

    #[test]
    fn style_switching_no_panic() {
        let mut engine = SaturateEngine::new();
        engine.update(config());

        let mut l = sine_buffer(1024, 440.0, 0.5);
        let mut r = l.clone();

        // Switch through all styles rapidly
        for cat_idx in 0..Category::COUNT {
            let cat = Category::from_index(cat_idx);
            for var_idx in 0..cat.variant_count() {
                engine.set_style(Style::new(cat, var_idx));
                engine.process(&mut l, &mut r);
            }
        }
    }

    #[test]
    fn mix_zero_is_dry() {
        let mut engine = SaturateEngine::new();
        engine.params.mix = 0.0;
        engine.params.drive = 1.0;
        engine.set_style(Style::new(Category::Tube, 0));
        engine.update(config());

        let mut l = sine_buffer(4410, 440.0, 0.5);
        let mut r = l.clone();
        let dry = l.clone();

        engine.process(&mut l, &mut r);

        for (i, (&wet, &d)) in l.iter().zip(dry.iter()).enumerate() {
            assert!(
                (wet - d).abs() < 1e-10,
                "Mix=0 should be dry at {i}: wet={wet}, dry={d}"
            );
        }
    }

    #[test]
    fn tape_mix_zero_is_dry() {
        let mut engine = SaturateEngine::new();
        engine.params.mix = 0.0;
        engine.params.drive = 1.0;
        engine.set_style(Style::new(Category::Tape, 2)); // Tape Warm
        engine.update(config());

        let mut l = sine_buffer(4410, 440.0, 0.5);
        let mut r = l.clone();
        let dry = l.clone();

        engine.process(&mut l, &mut r);

        for (i, (&wet, &d)) in l.iter().zip(dry.iter()).enumerate() {
            assert!(
                (wet - d).abs() < 1e-10,
                "Tape mix=0 should be dry at {i}: wet={wet}, dry={d}"
            );
        }
    }

    #[test]
    fn drive_affects_output() {
        for cat_idx in 0..Category::COUNT {
            let cat = Category::from_index(cat_idx);
            let style = Style::new(cat, 0);

            let mut engine_lo = SaturateEngine::new();
            engine_lo.params.drive = 0.1;
            engine_lo.set_style(style);
            engine_lo.update(config());

            let mut engine_hi = SaturateEngine::new();
            engine_hi.params.drive = 1.0;
            engine_hi.set_style(style);
            engine_hi.update(config());

            let mut l_lo = sine_buffer(4410, 440.0, 0.5);
            let mut r_lo = l_lo.clone();
            engine_lo.process(&mut l_lo, &mut r_lo);

            let mut l_hi = sine_buffer(4410, 440.0, 0.5);
            let mut r_hi = l_hi.clone();
            engine_hi.process(&mut l_hi, &mut r_hi);

            let mut diff_count = 0;
            for i in 0..4410 {
                if (l_lo[i] - l_hi[i]).abs() > 1e-6 {
                    diff_count += 1;
                }
            }
            assert!(
                diff_count > 100,
                "Drive should affect output for {}: only {diff_count} samples differ",
                style.display_name()
            );
        }
    }
}
