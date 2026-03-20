//! Noise gate DSP engine — gating with zero-crossing awareness and sidechain filtering.
//!
//! This crate provides a noise gate with features borrowed from the Airwindows
//! Dynamics plugin. Depends on `eq-dsp` for sidechain HPF/LPF.
//!
//! # Features
//!
//! - Zero-crossing-aware gate open/close from Airwindows Dynamics
//! - Hysteresis — separate open and close thresholds to prevent chatter
//! - Lookahead buffer for transparent gating of transients
//! - Sidechain HPF/LPF via `eq-dsp` filters
//! - Configurable hold time, attack and release envelope shaping
//!
//! # Modules
//!
//! - [`detector`] — Envelope follower with zero-crossing detection
//! - [`envelope`] — Attack / hold / release shaper
//! - [`chain`] — [`GateChain`] composable processing chain

pub mod chain;
pub mod detector;
pub mod envelope;

pub use chain::GateChain;
