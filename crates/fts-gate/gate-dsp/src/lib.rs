//! Noise gate DSP engine — gating with zero-crossing awareness and sidechain filtering.
//!
//! # Features
//!
//! - Zero-crossing-aware gate open/close from Airwindows Dynamics
//! - Hysteresis — separate open and close thresholds to prevent chatter
//! - Lookahead buffer for transparent gating of transients
//! - Sidechain HPF/LPF via `eq-dsp` filters
//! - Configurable hold time, attack and release envelope shaping
//! - **Timbre-aware drum classification** (kick/snare/hat/tom)
//! - **Adaptive resonant decay** tracking per-hit
//! - **Phase-locked multi-instance alignment**
//!
//! # Modules
//!
//! - [`detector`] — Envelope follower with zero-crossing detection
//! - [`envelope`] — Attack / hold / release shaper
//! - [`classifier`] — 4-band timbre-based drum classifier
//! - [`adaptive_decay`] — Per-hit resonance tracking and adaptive release
//! - [`sync`] — Multi-instance phase alignment via shared memory
//! - [`chain`] — [`GateChain`] composable processing chain

pub mod adaptive_decay;
pub mod chain;
pub mod classifier;
pub mod detector;
pub mod envelope;
pub mod sync;

pub use chain::GateChain;
pub use classifier::DrumClass;
