# Rider Plugin Specification

Requirements for the FTS Rider (vocal rider) plugin.

## Rider DSP Engine

r[rider.chain.signal-flow]
Signal flow: Input → Level Detector → Gain Calculator (target vs actual) → Smoothing (attack/release) → Gain Stage → Output.

### Level Detection

r[rider.detector.rms]
Support RMS level detection with configurable window size (10ms-300ms).

r[rider.detector.lufs]
Support LUFS-weighted level detection for perceptually accurate vocal level tracking.

r[rider.detector.sidechain]
Support sidechain input for music-reactive riding: when the music bus is louder, the vocal is attenuated less aggressively (or boosted more).

### Gain Calculation

r[rider.gain.target]
Calculate gain adjustment to bring the detected level toward a user-defined target level.

r[rider.gain.range]
Constrain gain adjustment to a configurable range (e.g., +/-6dB, +/-12dB, +/-24dB) to prevent extreme corrections.

r[rider.gain.smoothing]
Apply independent attack and release smoothing to gain changes for natural-sounding level rides. Attack should be faster than release for transparent operation.

### Vocal Detection

r[rider.vocal.activity]
Implement voice activity detection to distinguish vocal phrases from silence/bleed. The rider should not attempt to boost silent passages.

## Rider Profiles

r[rider.profile.control]
Full access: target level, gain range (min/max), attack, release, detection mode (RMS/LUFS), window size, sidechain input, voice activity threshold.

r[rider.profile.vocal-rider]
Simplified vocal rider mode: Target Level knob, Range knob, Speed (maps attack/release), and a large gain trace display.

## Rider Plugin

r[rider.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.rider`.

r[rider.plugin.vst3]
Export as VST3 with subcategory Dynamics.

r[rider.plugin.gain-trace]
Display a real-time gain trace showing the rider's gain adjustments over time.

## Rider Offline Analysis

r[rider.offline.full-track]
Read an entire track via AudioAccessor and compute the ideal gain ride curve with perfect lookahead (future-aware smoothing).

r[rider.offline.write-automation]
Write the computed gain curve as volume automation (smooth bezier curves) to the DAW via AutomationService.

r[rider.offline.lookahead-advantage]
Offline mode must produce superior results to real-time by using bidirectional smoothing (forward and backward pass) since the entire audio is available.

r[rider.offline.preview]
Allow previewing the ridden result before committing automation, with adjustable target level and range that re-analyze immediately.
