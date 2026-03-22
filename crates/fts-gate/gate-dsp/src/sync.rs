//! Phase-locked multi-instance alignment.
//!
//! A global singleton [`GateSession`] allows multiple gate plugin instances to
//! share onset timing information via a lock-free ring buffer. Each instance
//! can compute its time offset relative to the earliest-arriving mic and align
//! its gate decisions accordingly.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use fts_dsp::delay_line::DelayLine;

use super::classifier::DrumClass;

/// Maximum number of instances per session.
const MAX_INSTANCES: usize = 16;

/// Onset event ring buffer capacity (power of 2).
const RING_CAPACITY: usize = 256;
const RING_MASK: usize = RING_CAPACITY - 1;

// ── Onset Event ──────────────────────────────────────────────────────

/// An onset event published by one gate instance.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct OnsetEvent {
    /// Instance that detected this onset.
    pub instance_id: u32,
    /// Absolute sample position (from DAW transport or internal counter).
    pub sample_position: u64,
    /// Peak amplitude at onset.
    pub amplitude: f32,
    /// Classified drum type.
    pub drum_class_raw: u8,
}

impl OnsetEvent {
    pub fn drum_class(&self) -> DrumClass {
        match self.drum_class_raw {
            0 => DrumClass::Kick,
            1 => DrumClass::Snare,
            2 => DrumClass::HiHat,
            3 => DrumClass::Tom,
            _ => DrumClass::Unknown,
        }
    }
}

impl Default for OnsetEvent {
    fn default() -> Self {
        Self {
            instance_id: 0,
            sample_position: 0,
            amplitude: 0.0,
            drum_class_raw: 255,
        }
    }
}

fn drum_class_to_raw(c: DrumClass) -> u8 {
    match c {
        DrumClass::Kick => 0,
        DrumClass::Snare => 1,
        DrumClass::HiHat => 2,
        DrumClass::Tom => 3,
        DrumClass::Unknown => 255,
    }
}

// ── Lock-Free Ring Buffer ────────────────────────────────────────────

/// Single-producer-multi-consumer ring buffer for onset events.
/// Writers increment `write_pos` atomically. Readers each track their own position.
struct OnsetRing {
    events: Box<[std::cell::UnsafeCell<OnsetEvent>; RING_CAPACITY]>,
    write_pos: AtomicU64,
}

// SAFETY: OnsetEvent is Copy and small. We accept torn reads as benign
// (worst case: a misclassified onset event, which self-corrects next hit).
unsafe impl Send for OnsetRing {}
unsafe impl Sync for OnsetRing {}

impl OnsetRing {
    fn new() -> Self {
        Self {
            events: Box::new(std::array::from_fn(|_| {
                std::cell::UnsafeCell::new(OnsetEvent::default())
            })),
            write_pos: AtomicU64::new(0),
        }
    }

    fn write(&self, event: OnsetEvent) {
        let pos = self.write_pos.fetch_add(1, Ordering::AcqRel);
        let idx = (pos as usize) & RING_MASK;
        // SAFETY: single logical writer per instance_id, and readers tolerate torn reads.
        unsafe {
            *self.events[idx].get() = event;
        }
    }

    /// Read events starting from `reader_pos`. Updates `reader_pos` in place.
    /// Returns a Vec of events read (typically 0-2 per block).
    fn read_new(&self, reader_pos: &mut u64) -> Vec<OnsetEvent> {
        let current = self.write_pos.load(Ordering::Acquire);
        let mut out = Vec::new();

        if *reader_pos >= current {
            return out;
        }

        // If we've been lapped, skip ahead
        if current - *reader_pos > RING_CAPACITY as u64 {
            *reader_pos = current - (RING_CAPACITY as u64 / 2);
        }

        while *reader_pos < current {
            let idx = (*reader_pos as usize) & RING_MASK;
            // SAFETY: reading a potentially torn OnsetEvent is benign.
            let event = unsafe { *self.events[idx].get() };
            out.push(event);
            *reader_pos += 1;
        }

        out
    }
}

// ── Instance Info ────────────────────────────────────────────────────

/// Per-instance metadata in the shared session.
struct InstanceSlot {
    active: AtomicBool,
    id: AtomicU32,
}

impl InstanceSlot {
    fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            id: AtomicU32::new(0),
        }
    }
}

// ── Shared Session ───────────────────────────────────────────────────

/// Shared state for a group of linked gate instances.
pub struct GateSession {
    ring: OnsetRing,
    slots: Box<[InstanceSlot; MAX_INSTANCES]>,
    instance_counter: AtomicU32,
}

impl GateSession {
    fn new() -> Self {
        Self {
            ring: OnsetRing::new(),
            slots: Box::new(std::array::from_fn(|_| InstanceSlot::new())),
            instance_counter: AtomicU32::new(0),
        }
    }

    /// Register a new instance. Returns the assigned instance ID.
    pub fn register(&self) -> u32 {
        let id = self.instance_counter.fetch_add(1, Ordering::Relaxed);
        // Find a free slot
        for slot in self.slots.iter() {
            if !slot.active.swap(true, Ordering::AcqRel) {
                slot.id.store(id, Ordering::Relaxed);
                return id;
            }
        }
        id // all slots full, still return an ID (just can't track it)
    }

    /// Unregister an instance.
    pub fn unregister(&self, id: u32) {
        for slot in self.slots.iter() {
            if slot.active.load(Ordering::Relaxed) && slot.id.load(Ordering::Relaxed) == id {
                slot.active.store(false, Ordering::Release);
                return;
            }
        }
    }

    /// Publish an onset event.
    pub fn publish_onset(&self, event: OnsetEvent) {
        self.ring.write(event);
    }

    /// Read new onset events since the reader's last position.
    pub fn read_onsets(&self, reader_pos: &mut u64) -> Vec<OnsetEvent> {
        self.ring.read_new(reader_pos)
    }

    /// Count active instances.
    pub fn active_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.active.load(Ordering::Relaxed))
            .count()
    }
}

// ── Global Session Registry ──────────────────────────────────────────

/// Process-global session registry. Sessions are keyed by ID string.
static SESSIONS: OnceLock<Mutex<Vec<(String, Arc<GateSession>)>>> = OnceLock::new();

fn sessions() -> &'static Mutex<Vec<(String, Arc<GateSession>)>> {
    SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Get or create a shared session by ID.
pub fn get_session(session_id: &str) -> Arc<GateSession> {
    let mut sessions = sessions().lock().unwrap();
    for (id, session) in sessions.iter() {
        if id == session_id {
            return session.clone();
        }
    }
    let session = Arc::new(GateSession::new());
    sessions.push((session_id.to_string(), session.clone()));
    session
}

/// Remove a session if no instances remain.
pub fn cleanup_session(session_id: &str) {
    let mut sessions = sessions().lock().unwrap();
    sessions.retain(|(id, session)| id != session_id || session.active_count() > 0);
}

// ── Per-Instance Phase Aligner ───────────────────────────────────────

/// Per-instance alignment logic. Reads peer onsets, computes time offset,
/// and delays audio to align with the earliest-arriving instance.
pub struct PhaseAligner {
    session: Option<Arc<GateSession>>,
    instance_id: u32,

    // Ring buffer reader position
    reader_pos: u64,

    // Alignment delay
    alignment_delay: DelayLine,
    alignment_delay_samples: usize,

    // Onset correlation
    my_onsets: Vec<(u64, f32)>,        // (sample_pos, amplitude)
    peer_onsets: Vec<(u32, u64, f32)>, // (instance_id, sample_pos, amplitude)

    // Absolute sample counter
    pub sample_counter: u64,

    // Configuration
    pub enabled: bool,
    pub max_alignment_ms: f64,
    max_alignment_samples: usize,
    sample_rate: f64,
}

impl PhaseAligner {
    pub fn new() -> Self {
        Self {
            session: None,
            instance_id: 0,
            reader_pos: 0,
            alignment_delay: DelayLine::new(512), // up to ~10ms at 48kHz
            alignment_delay_samples: 0,
            my_onsets: Vec::new(),
            peer_onsets: Vec::new(),
            sample_counter: 0,
            enabled: false,
            max_alignment_ms: 5.0,
            max_alignment_samples: 240,
            sample_rate: 48000.0,
        }
    }

    /// Join a session. Call during plugin initialization.
    pub fn join_session(&mut self, session_id: &str) {
        let session = get_session(session_id);
        self.instance_id = session.register();
        self.session = Some(session);
    }

    /// Leave the session. Call during plugin drop.
    pub fn leave_session(&mut self) {
        if let Some(session) = self.session.take() {
            session.unregister(self.instance_id);
            // Cleanup handled lazily
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.max_alignment_samples = (self.max_alignment_ms * 0.001 * sample_rate) as usize;
        self.alignment_delay = DelayLine::new(self.max_alignment_samples.max(1) + 64);
    }

    /// Publish an onset from this instance.
    pub fn publish_onset(&self, amplitude: f32, drum_class: DrumClass) {
        if let Some(session) = &self.session {
            session.publish_onset(OnsetEvent {
                instance_id: self.instance_id,
                sample_position: self.sample_counter,
                amplitude,
                drum_class_raw: drum_class_to_raw(drum_class),
            });
        }
    }

    /// Process one stereo sample pair through the alignment delay.
    /// Returns the delayed sample pair.
    pub fn tick(&mut self, left: f64, right: f64) -> (f64, f64) {
        if !self.enabled || self.session.is_none() {
            self.sample_counter += 1;
            return (left, right);
        }

        // Read new peer onsets periodically (every 64 samples to reduce overhead)
        if self.sample_counter % 64 == 0 {
            self.poll_onsets();
        }

        // Apply alignment delay (mono for simplicity — same delay both channels)
        self.alignment_delay.write(left);
        let delayed_l = if self.alignment_delay_samples > 0 {
            self.alignment_delay.read(self.alignment_delay_samples)
        } else {
            left
        };

        // For right channel, we'd need a second delay line in a real stereo implementation.
        // For now, apply the same delay logic — the actual delay value is the same for both.
        let delayed_r = right; // TODO: add second delay line for true stereo alignment

        self.sample_counter += 1;
        (delayed_l, delayed_r)
    }

    /// Poll the session for new onset events and update alignment.
    fn poll_onsets(&mut self) {
        let session = match &self.session {
            Some(s) => s,
            None => return,
        };

        let events = session.read_onsets(&mut self.reader_pos);
        let cutoff = self
            .sample_counter
            .saturating_sub((self.sample_rate * 2.0) as u64);

        for event in events {
            if event.sample_position < cutoff {
                continue; // too old
            }
            if event.instance_id == self.instance_id {
                self.my_onsets
                    .push((event.sample_position, event.amplitude));
            } else {
                self.peer_onsets
                    .push((event.instance_id, event.sample_position, event.amplitude));
            }
        }

        // Prune old onsets
        self.my_onsets.retain(|&(pos, _)| pos >= cutoff);
        self.peer_onsets.retain(|&(_, pos, _)| pos >= cutoff);

        // Compute alignment from onset correlation
        self.compute_alignment();
    }

    /// Cross-correlate onset times to find consistent delay offset.
    fn compute_alignment(&mut self) {
        if self.my_onsets.len() < 2 || self.peer_onsets.is_empty() {
            return;
        }

        let window = self.max_alignment_samples as u64;
        let mut delays: Vec<i64> = Vec::new();

        // For each of our onsets, find the closest peer onset
        for &(my_pos, _) in &self.my_onsets {
            let mut best_dist = i64::MAX;
            let mut best_delay: i64 = 0;

            for &(_, peer_pos, _) in &self.peer_onsets {
                let diff = my_pos as i64 - peer_pos as i64;
                if diff.unsigned_abs() < window && diff.abs() < best_dist.abs() {
                    best_dist = diff.abs();
                    best_delay = diff;
                }
            }

            if best_dist < i64::MAX {
                delays.push(best_delay);
            }
        }

        if delays.len() >= 2 {
            delays.sort();
            let median = delays[delays.len() / 2];

            // If median > 0, our onset arrives LATER than peer → we need LESS delay
            // If median < 0, our onset arrives EARLIER → we need MORE delay
            // Alignment goal: all instances open at the same time
            // The earliest instance gets max delay, latest gets zero
            if median > 0 {
                // We're late — reduce our delay
                self.alignment_delay_samples = 0;
            } else {
                // We're early — add delay so we match the latest
                self.alignment_delay_samples = ((-median) as usize).min(self.max_alignment_samples);
            }
        }
    }

    /// Current alignment delay in samples.
    pub fn alignment_samples(&self) -> usize {
        if self.enabled {
            self.alignment_delay_samples
        } else {
            0
        }
    }

    pub fn reset(&mut self) {
        self.alignment_delay.clear();
        self.alignment_delay_samples = 0;
        self.my_onsets.clear();
        self.peer_onsets.clear();
        self.sample_counter = 0;
        self.reader_pos = 0;
    }
}

impl Default for PhaseAligner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PhaseAligner {
    fn drop(&mut self) {
        self.leave_session();
    }
}
