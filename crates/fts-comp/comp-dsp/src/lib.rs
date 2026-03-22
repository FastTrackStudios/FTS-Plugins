//! Compressor DSP engine — dynamics processing for the plugin suite.
//!
//! Based on APComp's versatile compressor architecture, which covers the full
//! range from gentle opto-style compression to hard limiting through its
//! parameter space (knee, feedback, inertia).
//!
//! # Architecture
//!
//! - [`detector`] — Envelope detection with exponential attack/release ballistics
//! - [`gain`] — Gain reduction computation (threshold, ratio, soft knee)
//! - [`compressor`] — Complete single-band compressor with all APComp features
//! - [`chain`] — Full processing chain with sidechain EQ and parallel mix
//!
//! # Features
//!
//! - Feedforward + feedback detection topology
//! - Exponential attack/release envelope follower
//! - Convexity-shaped gain reduction curve
//! - Inertia system (momentum-based smoothing)
//! - Stereo channel linking
//! - Output saturation (tanh soft clip)
//! - Parallel compression (dry/wet fold)
//! - Sidechain HPF via eq-dsp

pub mod chain;
pub mod character;
pub mod compressor;
pub mod detector;
pub mod gain;

pub use chain::CompChain;
pub use compressor::Compressor;
