# FTS-Reverb: Algorithm Resources & References

## Goal

All-inclusive reverb plugin covering classic and modern reverb types. Phase 1 focuses on **Room, Hall, Plate, Spring**. Phase 2 adds **Cloud, Bloom, Chorale, Shimmer, Magneto, Non-Linear, Swell, Reflections**.

---

## Target Algorithms (BigSky-Class)

### Phase 1 — Core Reverbs

| Algorithm | DSP Approach | Key Character |
|-----------|-------------|---------------|
| **Room** | FDN with early reflections network | Tight, natural, position-aware |
| **Hall** | Large FDN + diffusion, slow density buildup | Spacious, diffused, long tails |
| **Plate** | Dattorro tank topology | Fast-building, bright, no ER cues |
| **Spring** | Waveguide + dispersion chirp modeling | Frequency-dependent propagation, splashy |

### Phase 2 — Extended Reverbs

| Algorithm | DSP Approach | Key Character |
|-----------|-------------|---------------|
| **Cloud** | Granular/pitch-frozen buffer + diffusion | Floating ambient particles, infinite sustain |
| **Bloom** | Multi-diffusion feeding traditional tank | Reverb that builds and blooms over time |
| **Chorale** | Pitch tracking + formant filtering in reverb tail | Vocal/choral textures |
| **Shimmer** | Pitch-shifted feedback (octave up/down/5ths) | Evolving harmonic tails |
| **Magneto** | Multi-head tape delay + diffusion crossover | Blurred delay/reverb boundary |
| **Non-Linear** | Shaped envelopes (reverse, gate, swell, ramp) | Physics-defying decay shapes |
| **Swell** | Envelope-controlled reverb build | Gradual rise behind dry signal |
| **Reflections** | Geometric ray-tracing ER calculation | Psychoacoustically accurate room modeling |

---

## Open Source Implementations to Study

### CloudSeedCore (MIT)
- **URL:** https://github.com/GhostNoteAudio/CloudSeedCore
- **User Guide:** https://ghostnoteaudio.uk/pages/cloud-seed-user-guide
- **Language:** C++14
- **Architecture:** 3-stage serial pipeline:
  1. **Multitap Delay** — 1-second delay line, up to 256 taps at randomized times/gains
  2. **Early Diffusion** — Up to 12 series allpass reverberators (feedback ~70% natural, 40-50% bloom, 90%+ metallic)
  3. **Late Reverberation** — Up to 12 parallel delay lines, each with LP/HP tone filters and allpass diffusion in feedback path
- **Key DSP files:**
  - `DSP/AllpassDiffuser.h` — Allpass diffusion networks
  - `DSP/ModulatedAllpass.h` — Sine-wave modulated allpass filters
  - `DSP/ModulatedDelay.h` — Delay with feedback and sinusoidal time modulation
  - `DSP/MultitapDelay.h` — Multi-tap delay line
  - `DSP/ReverbChannel.h` / `ReverbController.h` — Signal flow coordination
  - `DSP/Biquad.h` — Second-order filters
  - `DSP/Lp1.h` / `DSP/Hp1.h` — First-order lowpass/highpass
- **Key techniques:** Four independent seed generators for randomization, Cross-Seed parameter for stereo decorrelation
- **Useful for:** Cloud, Bloom, Shimmer algorithms; general late-reverb tail architecture

### Dragonfly Reverb (GPL-3.0)
- **URL:** https://github.com/michaelwillis/dragonfly-reverb
- **Language:** C++
- **DSP Engine:** Freeverb3 library
- **Plugins:** 4 separate reverbs — Early Reflections, Hall, Plate, Room
- **Useful for:** Reference implementations of classic reverb types, parameter ranges

### Nepenthe (Open Source)
- **URL:** https://amalgamatedsignals.com/nepenthe
- **Download:** Source code available from website
- **Algorithm:** Velvet noise reverb — uses predetermined randomly-generated echo sets instead of delay-line networks
- **Parameters:** Time (RT60), Delay (pre-delay), Bass, Treble, Width, Mix
- **Design philosophy:** Transparent, CPU-friendly, SIMD-optimized. Annotated source code.
- **Useful for:** Alternative approach to traditional delay-line reverbs; efficient CPU usage

### Plateau / Valley Audio (GPL-3.0)
- **URL:** https://valleyaudio.github.io/rack/plateau/
- **Algorithm:** Dattorro plate reverb implementation
- **Useful for:** Reference Dattorro tank implementation

### Freeverb (Public Domain)
- **URL:** https://github.com/sinshu/freeverb
- **Architecture per channel:**
  - 8 parallel Schroeder-Moorer filtered-feedback comb filters (lowpass in feedback)
  - 4 series allpass filters
  - Stereo: right channel adds 23 samples to all delay lengths
- **Parameters:** Room Size, Damping, Width, Wet/Dry
- **Useful for:** Simple starting point, understanding Schroeder architecture

### Freeverb3
- **URL:** https://freeverb3-vst.sourceforge.io/
- **Library:** Multiple reverb algorithm implementations in C++
- **Used by:** Dragonfly Reverb
- **Useful for:** Multiple algorithm reference implementations

---

## Foundational Papers

### The 10 Essential Papers (per Sean Costello)

1. **Schroeder (1961)** — "Colorless Artificial Reverberation"
   - Introduced digital reverb and allpass delays

2. **Schroeder (1962)** — "Natural Sounding Artificial Reverberation"
   - Two architectures: parallel combs + series allpass (well-known), nested allpass (forgotten but superior)
   - Nested allpass produces echo density that **increases with time** (like real rooms)

3. **Gerzon (1971)** — "Synthetic Stereo Reverberation, Part 1"
   - Established FDNs with unitary feedback matrices

4. **Gerzon (1972)** — "Synthetic Studio Reverberation, Part 2"
   - Allpass feedback networks, frequency-dependent decay

5. **Moorer (1979)** — "About This Reverberation Business"
   - Efficient 2-multiply allpass, lowpass comb filters

6. **Stautner & Puckette (1982)** — "Designing Multi-Channel Reverberators"
   - Popularized FDNs, introduced delay modulation to reduce metallic artifacts

7. **Smith (1985)** — "A New Approach to Digital Reverberation using Closed Waveguide Networks"
   - Waveguide reverb — bidirectional delay lines

8. **Jot & Chaigne (1991)** — "Digital Delay Networks for Designing Artificial Reverberators"
   - Per-line damping filters for frequency-dependent decay in FDNs

9. **Gardner (1992)** — "A Realtime Multichannel Room Simulator"
   - First public allpass loop reverberator

10. **Dattorro (1997)** — "Effect Design, Part 1: Reverberator and Other Filters"
    - **PDF:** https://ccrma.stanford.edu/~dattorro/EffectDesignPart1.pdf
    - Comprehensive allpass loop (plate) design with specific coefficients
    - Architecture: Pre-delay/EQ → Input Diffuser (cascaded allpass) → Tank (two cross-fed filter chains with allpass delays, damping, decay) → Multi-tap output

---

## Key Online Resources

### Sean Costello / Valhalla DSP Blog
- **Blog:** https://valhalladsp.com/blog/
- [Schroeder Reverbs: The Forgotten Algorithm](https://valhalladsp.com/2009/05/30/schroeder-reverbs-the-forgotten-algorithm/) — Nested allpass architecture
- [Getting Started With Reverb Design, Part 1: Dev Environments](https://valhalladsp.com/2021/09/20/getting-started-with-reverb-design-part-1-dev-environments/)
- [Getting Started With Reverb Design, Part 2: The Foundations](https://valhalladsp.com/2021/09/22/getting-started-with-reverb-design-part-2-the-foundations/) — Lists the 10 foundational papers
- [Reverbs: Diffusion, Allpass Delays, and Metallic Artifacts](https://valhalladsp.com/2011/01/21/reverbs-diffusion-allpass-delays-and-metallic-artifacts/)
- [The Philosophy of ValhallaSupermassive](https://valhalladsp.com/2020/05/06/the-philosophy-of-valhallasupermassive/)

### Julius O. Smith / CCRMA Stanford
- **Home:** https://ccrma.stanford.edu/~jos/
- [Artificial Reverberation (MUS420)](https://ccrma.stanford.edu/~jos/Reverb/) — Free lecture notes (CC)
- [Physical Audio Signal Processing](https://ccrma.stanford.edu/~jos/pasp/) — Free online book, covers waveguides and reverb
- [Freeverb Analysis](https://ccrma.stanford.edu/~jos/pasp/Freeverb.html)
- [FDN Reverberation](https://www.dsprelated.com/freebooks/pasp/FDN_Reverberation.html)

### Signalsmith — "Let's Write a Reverb"
- **URL:** https://signalsmith-audio.co.uk/writing/2021/lets-write-a-reverb/
- Excellent modern tutorial separating reverb into two problems:
  1. **Diffuser** — Multi-channel delays + Hadamard mixing matrix, rapidly reaching 2000-4000 echoes/sec
  2. **FDN feedback loop** — Multi-channel delay with Householder matrix mixing and gain control
- Key insight: separate diffusion from decay; use energy-preserving operations

### Faust Reverb Libraries
- **URL:** https://faustlibraries.grame.fr/libs/reverbs/
- Includes FDN, zita-rev1, and other algorithm implementations

### FDN Optimization via Gradient Descent
- **URL:** https://arxiv.org/html/2402.11216v1
- Modern approach to FDN parameter optimization (used in Strymon BigSky MX)

---

## Algorithm-Specific Design Notes

### Room Reverb
- **Core:** FDN (4-8 delay lines) with Householder or Hadamard feedback matrix
- **Early reflections:** Separate ER network from late reverb; position-dependent tap patterns
- **Key params:** Room size (delay lengths), damping (LP in feedback), diffusion, ER/late balance
- **FDN rules:** Delay lengths mutually prime; mode density M >= 0.15 * RT60 * sample_rate
- **Feedback matrices:** Householder `A = I - (2/N)uu^T` (only 2N-1 additions) or Hadamard (no multiplies for N = power of 4)

### Hall Reverb
- **Core:** Large FDN (8-16 delay lines) with slower diffusion buildup
- **Longer delay lines** than Room for spaciousness
- **Modulated allpass delays** to prevent metallic artifacts (sine LFO, ~0.5-2 Hz, small depth)
- **Two sizes** worth supporting: Concert hall and Arena/Cathedral
- **Key difference from Room:** Higher echo density, longer pre-delay, more diffusion stages

### Plate Reverb (Dattorro)
- **Core:** Dattorro 1997 topology — the gold standard
- **Architecture:**
  1. Input conditioning (pre-delay + EQ)
  2. Input diffuser: 4 cascaded allpass filters
  3. Tank: Two cross-coupled chains, each with modulated allpass + delay + damping LP + decay gain
  4. Output: Multi-tap extraction from tank for stereo decorrelation
- **Character:** Fast density buildup, bright, no early reflection cues (no room shape)
- **Key coefficients:** Published in Dattorro's paper with specific delay lengths and gains
- **Modulation:** Small sinusoidal modulation of allpass delay times (~1 Hz) prevents ringing

### Spring Reverb
- **Core:** Digital waveguide model with dispersion
- **Key characteristics:**
  - **Dispersion chirp** — Frequency-dependent propagation velocity in helical springs (higher frequencies travel faster)
  - **Multiple reflections** — Signal bounces between fixed endpoints
  - **Characteristic "drip"** — From transient excitation of dispersive spring
- **Modeling approaches:**
  1. Waveguide (bidirectional delay lines) — Efficient, captures reflections naturally
  2. Allpass cascade for dispersion — Chain of allpass filters simulates frequency-dependent delay
  3. Mass-spring Newtonian simulation — Most accurate but expensive
- **Practical:** Allpass cascade for dispersion + waveguide for reflections is the sweet spot
- **References:**
  - "Efficient Dispersion Generation Structures for Spring Reverb Emulation" (ResearchGate)
  - "Parametric Spring Reverberation Effect" (ResearchGate)
  - "Numerical Simulation of Spring Reverberation" (ResearchGate)

---

## Architecture Insights

### Common Building Blocks
All reverb types share these primitives:
- **Delay line** — Ring buffer with interpolation (linear or allpass)
- **Allpass filter** — `y[n] = -g*x[n] + x[n-M] + g*y[n-M]` — Passes all frequencies but smears phase
- **Modulated allpass** — Allpass with LFO-modulated delay time (prevents metallic buildup)
- **Damping filter** — Usually 1-pole LP in feedback path: `y[n] = (1-d)*x[n] + d*y[n-1]`
- **Feedback matrix** — Householder or Hadamard for energy-preserving mixing in FDNs
- **Pre-delay** — Simple delay before reverb input

### Stereo Techniques
- **Freeverb approach:** Offset all delay lengths by fixed amount (23 samples) for right channel
- **Dattorro approach:** Multi-tap output from different points in the tank
- **FDN approach:** Extract L/R from different delay line outputs
- **Cross-seed** (CloudSeed): Independent random seeds for L/R channel parameters

### CPU Optimization
- Hadamard matrix: No multiplies needed (additions only) for N = power of 4
- Householder: Only 2N-1 additions
- Velvet noise (Nepenthe): Sparse impulse response, SIMD-friendly
- Process blocks, not samples, where possible

---

## Strymon BigSky Technical Details

- **Hardware:** SHARC DSP 366MHz (original), tri-core 800MHz ARM (MX)
- **Processing:** 32-bit floating-point
- **Core DSP:** FDN, allpass-delay-filter loops, Schroeder sections, multi-tap delay lines
- **MX innovation:** Gradient descent optimization to minimize ringing artifacts
- **Common controls per algorithm:** Decay, Pre-Delay, Tone (LP damping), Diffusion, Mod (chorus in tail), Low Damp, Mix
