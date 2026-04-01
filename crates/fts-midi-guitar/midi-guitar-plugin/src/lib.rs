//! FTS MIDI Guitar — nih-plug entry point with polyphonic pitch detection and MIDI output.

use atomic_float::AtomicF32;
use fts_plugin_core::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use midi_guitar_dsp::MidiGuitarDetector;

mod editor;

// ── Shared UI State ──────────────────────────────────────────────────

/// Audio-thread -> UI metering data.
pub struct MidiGuitarUiState {
    pub params: Arc<FtsMidiGuitarParams>,
    /// Peak input level in dB.
    pub input_peak_db: AtomicF32,
    /// Number of currently active MIDI notes.
    pub active_note_count: AtomicU32,
    /// Bitfield of active MIDI notes (128 bits = two u64s).
    /// Bit N = MIDI note N is active.
    pub active_notes_lo: std::sync::atomic::AtomicU64,
    pub active_notes_hi: std::sync::atomic::AtomicU64,
}

impl MidiGuitarUiState {
    fn new(params: Arc<FtsMidiGuitarParams>) -> Self {
        Self {
            params,
            input_peak_db: AtomicF32::new(-100.0),
            active_note_count: AtomicU32::new(0),
            active_notes_lo: std::sync::atomic::AtomicU64::new(0),
            active_notes_hi: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Update the active notes bitfield from the detector state.
    fn update_active_notes(&self, active: &[bool], note_map: &[u8]) {
        let mut lo: u64 = 0;
        let mut hi: u64 = 0;
        let mut count: u32 = 0;

        for (i, &is_active) in active.iter().enumerate() {
            if is_active {
                let note = note_map[i] as u64;
                if note < 64 {
                    lo |= 1 << note;
                } else {
                    hi |= 1 << (note - 64);
                }
                count += 1;
            }
        }

        self.active_notes_lo.store(lo, Ordering::Relaxed);
        self.active_notes_hi.store(hi, Ordering::Relaxed);
        self.active_note_count.store(count, Ordering::Relaxed);
    }
}

// ── Parameters ───────────────────────────────────────────────────────

#[derive(Params)]
pub struct FtsMidiGuitarParams {
    /// Energy threshold for note detection (dB).
    #[id = "threshold"]
    pub threshold: FloatParam,

    /// Velocity sensitivity scaling.
    #[id = "sensitivity"]
    pub sensitivity: FloatParam,

    /// Analysis window size in ms.
    #[id = "window_ms"]
    pub window_ms: FloatParam,

    /// MIDI output channel (1-16).
    #[id = "channel"]
    pub channel: IntParam,

    /// Lowest detectable MIDI note.
    #[id = "lowest_note"]
    pub lowest_note: IntParam,

    /// Highest detectable MIDI note.
    #[id = "highest_note"]
    pub highest_note: IntParam,

    /// Suppress harmonics of detected fundamentals.
    #[id = "harmonic_suppression"]
    pub harmonic_suppression: BoolParam,
}

impl Default for FtsMidiGuitarParams {
    fn default() -> Self {
        Self {
            threshold: FloatParam::new(
                "Threshold",
                -35.0, // 0.0003 linear — optimal from grid search
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            sensitivity: FloatParam::new(
                "Sensitivity",
                50.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            window_ms: FloatParam::new(
                "Window",
                20.0,
                FloatRange::Linear {
                    min: 5.0,
                    max: 50.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            channel: IntParam::new("Channel", 1, IntRange::Linear { min: 1, max: 16 }),

            lowest_note: IntParam::new("Low Note", 39, IntRange::Linear { min: 28, max: 60 })
                .with_value_to_string(Arc::new(|v| midi_note_name(v as u8)))
                .with_string_to_value(Arc::new(|s| s.trim().parse().ok())),

            highest_note: IntParam::new("High Note", 89, IntRange::Linear { min: 60, max: 96 })
                .with_value_to_string(Arc::new(|v| midi_note_name(v as u8)))
                .with_string_to_value(Arc::new(|s| s.trim().parse().ok())),

            harmonic_suppression: BoolParam::new("Harmonics", true),
        }
    }
}

/// Convert MIDI note number to name (e.g. 69 -> "A4").
fn midi_note_name(note: u8) -> String {
    const NAMES: &[&str] = &[
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = (note as i32 / 12) - 1;
    let name = NAMES[(note % 12) as usize];
    format!("{name}{octave}")
}

// ── Plugin ───────────────────────────────────────────────────────────

struct FtsMidiGuitar {
    params: Arc<FtsMidiGuitarParams>,
    ui_state: Arc<MidiGuitarUiState>,
    editor_state: Arc<DioxusState>,
    detector: MidiGuitarDetector,
    sample_rate: f64,
    /// Track current sample index within buffer for MIDI event timing.
    sample_index: u32,
}

impl Default for FtsMidiGuitar {
    fn default() -> Self {
        let params = Arc::new(FtsMidiGuitarParams::default());
        let ui_state = Arc::new(MidiGuitarUiState::new(params.clone()));
        Self {
            params,
            ui_state,
            editor_state: DioxusState::new(|| (560, 380)),
            detector: MidiGuitarDetector::new(),
            sample_rate: 48000.0,
            sample_index: 0,
        }
    }
}

impl FtsMidiGuitar {
    /// Sync nih-plug params -> DSP detector.
    fn sync_params(&mut self) {
        // Convert dB threshold to linear energy threshold.
        let threshold_db = self.params.threshold.value() as f64;
        let threshold_linear = 10.0_f64.powf(threshold_db / 10.0);
        self.detector.set_threshold(threshold_linear);

        self.detector
            .set_sensitivity(self.params.sensitivity.value() as f64 / 100.0);

        // Convert ms to samples.
        let window_samples =
            (self.params.window_ms.value() as f64 * self.sample_rate / 1000.0) as usize;
        self.detector.set_window_size(window_samples.max(1));

        self.detector.set_note_range(
            self.params.lowest_note.value() as u8,
            self.params.highest_note.value() as u8,
        );

        self.detector
            .set_harmonic_suppression(self.params.harmonic_suppression.value());

        // Optimal config from grid search (F1=66.3% on GuitarSet).
        self.detector.set_peak_picking(true);
        self.detector.set_hysteresis_ratio(1.0);
        self.detector.set_whitening(true);
        // Preprocessing for DI signals (HPF removes hum, compression tames dynamics).
        self.detector.set_preprocessing(true);
    }
}

impl Plugin for FtsMidiGuitar {
    const NAME: &'static str = "FTS MIDI Guitar";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        create_dioxus_editor_with_state(
            self.editor_state.clone(),
            self.ui_state.clone(),
            editor::App,
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        self.detector.set_sample_rate(self.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.detector.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_params();

        let midi_channel = (self.params.channel.value() - 1) as u8; // 0-indexed

        self.sample_index = 0;

        for mut frame in buffer.iter_samples() {
            let mut channels = frame.iter_mut();
            let left = channels.next().unwrap();
            let right = channels.next().unwrap();

            // Metering: input peak.
            let input_peak = (*left).abs().max((*right).abs());
            let prev_in = self.ui_state.input_peak_db.load(Ordering::Relaxed);
            let in_db = if input_peak > 0.0 {
                20.0 * input_peak.log10()
            } else {
                -100.0
            };
            let new_in = if in_db > prev_in {
                in_db
            } else {
                prev_in - 0.005
            };
            self.ui_state.input_peak_db.store(new_in, Ordering::Relaxed);

            // Sum to mono for pitch detection.
            let mono = (*left as f64 + *right as f64) * 0.5;

            // Feed to detector.
            if let Some(events) = self.detector.process_sample(mono) {
                for event in &events {
                    if event.is_on {
                        nih_log!("NOTE ON: {} vel={:.2}", event.note, event.velocity);
                        context.send_event(NoteEvent::NoteOn {
                            timing: self.sample_index,
                            voice_id: None,
                            channel: midi_channel,
                            note: event.note,
                            velocity: event.velocity,
                        });
                    } else {
                        context.send_event(NoteEvent::NoteOff {
                            timing: self.sample_index,
                            voice_id: None,
                            channel: midi_channel,
                            note: event.note,
                            velocity: 0.0,
                        });
                    }
                }

                // Update UI active notes state.
                self.ui_state.update_active_notes(
                    self.detector.inner().active_notes(),
                    self.detector.inner().note_map(),
                );
            }

            // Silence the audio output — this plugin is a MIDI generator.
            *left = 0.0;
            *right = 0.0;

            self.sample_index += 1;
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for FtsMidiGuitar {
    const CLAP_ID: &'static str = "com.fasttrackstudio.midi-guitar";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Polyphonic audio-to-MIDI converter for guitar");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::NoteEffect,
        ClapFeature::AudioEffect,
        ClapFeature::Custom("note-detector"),
        ClapFeature::Mono,
    ];
}

impl Vst3Plugin for FtsMidiGuitar {
    const VST3_CLASS_ID: [u8; 16] = *b"FtsMidiGuitr0001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Analyzer];
}

nih_export_clap!(FtsMidiGuitar);
nih_export_vst3!(FtsMidiGuitar);
