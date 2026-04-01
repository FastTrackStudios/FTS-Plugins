//! Offline analysis and evaluation for FTS MIDI Guitar.
//!
//! Provides tools to evaluate pitch detection accuracy against:
//! - **GuitarSet**: Hexaphonic guitar DI recordings with JAMS annotations
//! - **Guitar-TECHS**: Guitar DI recordings with MIDI annotations

pub mod datasets;
pub mod eval;
pub mod jams;
pub mod midi_file;
