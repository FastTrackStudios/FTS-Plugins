# Offline Analysis Specification

Requirements for offline audio analysis and DAW automation writing.

## AudioAccessor Integration

r[offline.accessor.track]
Read audio from a REAPER track using `AudioAccessorService.create_track_accessor()` and `get_samples()` from the daw crate.

r[offline.accessor.full-read]
Read the entire track duration in a single pass for full-context analysis. The analyzer must handle tracks of arbitrary length by processing in chunks if needed.

r[offline.accessor.sample-rate]
Request audio at the project's native sample rate to avoid resampling artifacts in analysis.

## Automation Writing

r[offline.automation.envelope-write]
Write analysis results as automation envelope points via `AutomationService.add_point()`.

r[offline.automation.clear-range]
Before writing new automation, clear existing points in the analyzed range via `delete_points_in_range()`.

r[offline.automation.curve-shapes]
Use appropriate curve shapes for each plugin type:
- Gate: Square (instant on/off) with optional Linear fade times
- Trigger: Square pulses at detected onset times
- Rider: Bezier curves with tension for smooth gain riding

r[offline.automation.undo]
All automation writing must be wrapped in a single REAPER undo block so the user can undo the entire analysis result in one step.

## Analysis Quality

r[offline.analysis.lookahead]
Offline analysis must exploit the fact that the entire audio is available. Use bidirectional processing (forward + backward pass) where applicable for superior results compared to real-time.

r[offline.analysis.deterministic]
Running the same analysis on the same audio with the same parameters must produce identical results every time.

r[offline.analysis.preview]
All offline analyzers must support a preview mode that applies the analysis result to the audio for auditioning before committing automation to the DAW.

## Per-Plugin Analysis

r[offline.gate.analysis]
Gate offline analysis detects open/close regions and writes mute automation. With full lookahead, the gate can open before the transient arrives (zero pre-ring).

r[offline.trigger.analysis]
Trigger offline analysis detects all transients in the track, extracts velocity, and writes either MIDI notes or velocity automation.

r[offline.rider.analysis]
Rider offline analysis computes the ideal gain curve using bidirectional smoothing (forward and backward pass), then writes volume automation as smooth bezier curves.
