//! Bias — golden-ratio slew chain.
//!
//! Emulates the effect of tape bias on high-frequency saturation.
//! Uses a chain of slew-rate limiters spaced at golden-ratio intervals
//! to create frequency-dependent soft clipping.

// TODO: Implement Bias algorithm from Airwindows
