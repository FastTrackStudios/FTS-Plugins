//! Drum trigger DSP engine — transient detection and sample triggering.
//!
//! This crate provides a drum trigger with onset detection, velocity
//! extraction, and sample playback. Depends on `eq-dsp` for sidechain
//! filtering to isolate drum frequency ranges.
//!
//! # Features
//!
//! - Transient / onset detection
//! - Velocity extraction from transient energy
//! - Retrigger prevention with configurable minimum interval
//! - Sidechain filtering to isolate drum frequency ranges (via `eq-dsp`)
//! - Sample playback engine for triggered samples with round-robin
//!
//! # Modules
//!
//! - [`detector`] — Onset / transient detection
//! - [`velocity`] — Energy-to-velocity mapping
//! - [`sampler`] — Sample playback with round-robin support
//! - [`chain`] — [`TriggerChain`] composable processing chain

pub mod chain;
pub mod detector;
pub mod sampler;
pub mod velocity;

pub use chain::TriggerChain;
