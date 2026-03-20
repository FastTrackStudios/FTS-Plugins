# Limiter Plugin Specification

Requirements for the FTS Limiter plugin.

## Limiter DSP Engine

r[limiter.chain.signal-flow]
Signal flow: Drive → Slew Rate Limiter (Loud) → Soft Clip (ClipSoftly) → Multi-stage Golden Ratio Clipper (ADClip8) → Ceiling.

### ADClip8 (Multi-stage Clipper)

r[limiter.adclip.cascaded]
Support up to 7 cascaded clipping stages. Each stage applies input gain, slew rate limiting, and golden-ratio peak clipping.

r[limiter.adclip.golden-ratio]
Use golden ratio constants (phi=1.618, 1/phi=0.618, 1-1/phi=0.382) for intersample peak control during clip transitions.

r[limiter.adclip.sample-rate-aware]
Lookahead buffer sizing must scale with sample rate.

### ClipSoftly (Sin Waveshaper)

r[limiter.softclip.history]
ClipSoftly must not be a static waveshaper. It must track previous sample state and blend using speed-dependent smoothing: louder signals get more smoothing.

### Loud (Slew Rate Limiter)

r[limiter.loud.derivative]
Loud limits the derivative (sample-to-sample difference), not the amplitude. This preserves peak level while controlling transient edge.

### BlockParty (Mu-law Limiter)

r[limiter.blockparty.mu-law]
Implement mu-law limiting with variable threshold and golden ratio voicing.

r[limiter.blockparty.independent-channels]
Threshold must adapt independently per channel.

## Limiter Profiles

r[limiter.profile.control]
Full access: drive, ceiling, character (clean/warm/loud), stages (1-7), transient control, soft clip amount.

r[limiter.profile.l2]
L2-style profile: just Input and Ceiling with a large gain reduction meter.

r[limiter.profile.mastering]
Mastering profile: full multi-stage with per-stage metering, ceiling, and dither selection.

## Limiter Plugin

r[limiter.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.limiter`, feature tag `limiter`.

r[limiter.plugin.vst3]
Export as VST3 with subcategory Dynamics.

r[limiter.plugin.true-peak]
Display true-peak metering on the output, not just sample-peak.
