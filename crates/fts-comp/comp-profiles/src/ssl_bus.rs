//! SSL bus compressor profile.
//!
//! Maps the SSL G-series bus compressor controls:
//! - Threshold
//! - Ratio (stepped: 2:1, 4:1, 10:1)
//! - Attack (stepped: 0.1, 0.3, 1, 3, 10, 30 ms)
//! - Release (stepped: 0.1, 0.3, 0.6, 1.2 s + Auto)
//! - Makeup gain

// TODO: Implement SSL bus profile with Logical4 mapping
