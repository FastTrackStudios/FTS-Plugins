//! FTS Chorus — multi-engine chorus/flanger/vibrato.
//!
//! Four distinct chorus engines covering clean to experimental:
//! - **Cubic**: Clean Catmull-Rom interpolation (default, transparent)
//! - **BBD**: Bucket-brigade device emulation (vintage analog)
//! - **Tape**: Wow/flutter/saturation (Roland Juno-like warmth)
//! - **Orbit**: Dual-tap elliptical orbital modulation (experimental, spatial)
//!
//! Each engine can operate in Chorus, Flanger, or Vibrato mode.
//!
//! Credits:
//! - Cubic interpolation: standard Catmull-Rom (fts-dsp)
//! - BBD topology: Choroboros (EsotericShadow), clock-driven S&H chain
//! - Tape modulation: ChowDSP AnalogTapeModel (wow/flutter), qdelay (tiagolr)
//! - Orbit modulation: Choroboros (EsotericShadow), elliptical 2D LFO

pub mod chain;
pub mod engine;
