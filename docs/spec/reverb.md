# Reverb Plugin Specification

Requirements for the FTS Reverb plugin.

## Reverb DSP Engine

r[reverb.chain.signal-flow]
Signal flow: Predelay → Input Conditioning (HPF + adaptive compressor + saturation) → Early Reflections → Late Reverb (topology-selectable) → Feedback Processing → Output.

### kPlateA (Plate Reverb)

r[reverb.kplate.allpass-matrix]
Implement a Hadamard-like allpass mixing matrix: 6 initial allpasses (3 per channel, cross-mixed) feeding 6 more with matrix mixing (`outDL = ((outBL + outCL) - outAL)`).

r[reverb.kplate.distributed-biquads]
Place 4 biquad bandpass filters (at different frequencies) distributed between reverb stages — not just on input/output. This shapes the spectral envelope of the reverb tail over time.

r[reverb.kplate.input-compress]
Apply adaptive input compression: `gainIn += sin(fabs(input*4)) * pow(input, 4)`.

### Galactic (Shimmer)

r[reverb.galactic.hadamard]
Implement three blocks of four parallel delay lines with Hadamard unitary cross-mixing: `aA = (outI - (outJ + outK + outL))`.

r[reverb.galactic.cross-channel]
L feedback must come from R delay outputs and vice versa, creating natural stereo decay.

r[reverb.galactic.vibrato-predelay]
Predelay must include sinusoidal vibrato with drift modulation.

### Verbity2 (Hall/Room)

r[reverb.verbity.prime-delays]
Implement a 5-stage cascaded delay network using prime-number delay lengths.

r[reverb.verbity.chrome-oxide]
Feedback softening must use per-sample randomized interpolation between current and previous feedback values, adding tape-like quality.

r[reverb.verbity.self-limiting]
Feedback must self-limit: `regen * (1.0 - fabs(feedback * regen))`.

## Reverb Profiles

r[reverb.profile.control]
Full access: type (plate/hall/room/shimmer), size, decay, predelay, damping, tone, modulation, stereo width, mix.

r[reverb.profile.emt-140]
EMT 140 profile: maps to kPlateA topology, Decay + Damping + Mix only.

r[reverb.profile.lexicon-480]
Lexicon 480 profile: algorithm selector (Hall/Room/Plate/Chamber), Size, Decay, Pre-delay.

## Reverb Plugin

r[reverb.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.reverb`, feature tag `reverb`.

r[reverb.plugin.vst3]
Export as VST3 with subcategory Reverb.
