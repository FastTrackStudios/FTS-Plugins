# Tape Machine Plugin Specification

Requirements for the FTS Tape Machine plugin.

## Tape DSP Engine

r[tape.chain.signal-flow]
Signal flow: Input → Dubly Encode → Flutter → Bias → Saturation (mid/side split) → Head Bump → Dubly Decode → ClipOnly2 Output.

### Dubly (Noise Reduction)

r[tape.dubly.encode]
Dubly encode separates highs via IIR, applies logarithmic compression (`log(1 + 255*amount) / 2.408`), and boosts HF before tape processing.

r[tape.dubly.decode]
Dubly decode restores the original HF balance after tape processing. Encode and decode must use different multiplier coefficients (2.848 vs 2.628).

### Flutter

r[tape.flutter.modulated-delay]
Implement flutter as a 1000-sample circular delay buffer with sinusoidal sweep modulation.

r[tape.flutter.random-rate]
Each modulation cycle must have a randomized rate (`nextmax = 0.24 + random * 0.74`), matching ToTape8's natural wow/flutter behavior.

r[tape.flutter.stereo-crosscouple]
L/R channels must have cross-coupled sweep randomization (R channel's random seed is influenced by L's state).

### Bias

r[tape.bias.golden-slew]
Implement the 9-stage golden-ratio-spaced slew limiter chain from ToTape8. Under-bias creates sample "stickiness" (attracted to previous values). Over-bias is slew-rate clipping.

### Saturation

r[tape.saturation.mid-side]
Split signal at a mid frequency using IIR. Apply sin() waveshaping to lows (tape saturation curve) and cos() thinning to highs (HF compression).

r[tape.saturation.sub-cutoff]
Include a sub-bass cutoff to prevent DC buildup from the saturation stage.

### Head Bump

r[tape.headbump.biquad]
Implement as dual cascaded biquad bandpass filters with user-controlled center frequency (25-200Hz).

r[tape.headbump.cubic-limit]
Use cubic self-limiting (`x -= x^3 * k`) for natural resonance decay rather than hard clipping or ringing.

## Tape Profiles

r[tape.profile.control]
Full access: input gain, tape speed, bias, flutter depth/rate, head bump amount/frequency, saturation drive, noise reduction amount, output gain.

r[tape.profile.studer]
Studer A800 profile: Input/Output, Speed selector (7.5/15/30 ips), Bias trim, EQ selector, Repro/Sync head.

r[tape.profile.ampex]
Ampex ATR-102 profile: emphasis on mastering use, different saturation character mapping.

## Tape Plugin

r[tape.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.tape`.

r[tape.plugin.vst3]
Export as VST3 with subcategory Fx.
