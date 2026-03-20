//! Vocal rider DSP engine — automatic level riding for vocals and dialog.
//!
//! This crate provides a vocal rider that detects signal level and applies
//! smooth gain adjustments to maintain a target loudness. Depends on `eq-dsp`
//! for sidechain filtering and borrows dynamics detection concepts from
//! `comp-dsp`.
//!
//! # Features
//!
//! - RMS / LUFS level detection with adjustable integration window
//! - Target level with configurable gain range (min/max)
//! - Attack / release smoothing for natural, transparent gain changes
//! - Sidechain input for music-reactive riding (duck when music is louder)
//! - Dynamics detection concepts from `comp-dsp`
//!
//! # Modules
//!
//! - [`detector`] — Level detector (RMS, peak, LUFS window)
//! - [`rider`] — Gain calculation engine
//! - [`chain`] — [`RiderChain`] composable processing chain

pub mod chain;
pub mod detector;
pub mod rider;

pub use chain::RiderChain;
