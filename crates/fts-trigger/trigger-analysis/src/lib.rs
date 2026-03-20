//! Offline trigger analysis — reads audio via AudioAccessor, detects transients
//! and extracts velocity, can write MIDI notes to the DAW or create trigger
//! automation.
//!
//! Enables batch drum replacement on entire tracks with accurate onset
//! detection and velocity mapping.

pub mod offline;
