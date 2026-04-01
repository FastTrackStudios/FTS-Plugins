/// A detected note event from the polyphonic pitch detector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedNote {
    /// MIDI note number (0..127).
    pub note: u8,
    /// Velocity in 0.0..1.0.
    pub velocity: f32,
    /// `true` for note-on, `false` for note-off.
    pub is_on: bool,
}
