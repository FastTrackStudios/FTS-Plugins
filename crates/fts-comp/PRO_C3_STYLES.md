# Pro-C 3 Compressor Styles — Analysis & Reference

Captured 2026-03-30 using `fts-analyzer-cli capture-compressor` with SC EQ disabled
(`--param 32=0 --param 33=0`), Auto Gain off (`--param 19=0`), 196 attack/release
scenarios across 34 frequencies (20 Hz – 20 kHz).

Profiles: `fts-analyzer/profiles/capture/pro-c3-{name}.json`
Reference data: `fts-analyzer/reference/pro-c3-{name}-nosc/`

## Summary Table

| ID | Style | Category | Topology | Freq Response | Steady GR (1kHz) | GR Range | Key Trait |
|----|-------|----------|----------|---------------|-------------------|----------|-----------|
| 0 | Clean | Modern | Feedforward | Flat | -5.63 dB | -5.3 to -0.3 | Low distortion, program dependent |
| 1 | Versatile | Modern | — | Flat | -6.07 dB | -5.5 to -0.5 | Punchy long attack, tight short attack |
| 2 | Smooth | Modern | — | Flat | -4.16 dB | -4.4 to -0.5 | Always smooth, lowest sensitivity |
| 3 | Punch | Modern | — | Flat | -6.28 dB | -5.9 to -1.2 | Traditional analog-like |
| 4 | Upward | Modern | — | Flat | -9.04 dB | -14.1 to -2.2 | Boosts below threshold |
| 5 | TTM | Modern | 3-band multiband | **Multiband** | -10.09 dB | -5.0 to +1.2 | Upward + downward, xover ~90/2500 Hz |
| 6 | Op-El | Classic | Opto-like tube | Gentle HF rolloff | -5.44 dB | -5.9 to -0.9 | Smooth, warm opto character |
| 7 | Vari-Mu | Classic | Feedback, variable-mu | Flat | -4.59 dB | -4.4 to -0.3 | Soft knee from tube curve |
| 8 | Classic | Classic | Feedback | Flat | -5.36 dB | -6.6 to -0.3 | Vintage, very program dependent |
| 9 | Opto | Classic | — | Gentle LF-to-HF slope | -6.60 dB | -5.5 to -0.7 | Slow, very soft knee, linear |
| 10 | Vocal | Utility | Auto knee + ratio | Flat | -9.24 dB | -9.4 to -6.0 | Threshold-only workflow, narrow GR range |
| 11 | Mastering | Utility | — | Flat | -7.10 dB | -7.0 to -4.4 | Maximum transparency |
| 12 | Bus | Utility | — | Flat | -7.13 dB | -6.6 to -4.9 | Glue, narrowest GR range |
| 13 | Pumping | Utility | — | Flat | -7.34 dB | -6.5 to -2.7 | Deep pumping for EDM/drums |

**Test conditions:** Threshold -18 dB, Ratio 4:1, Knee 0, input alternating -6/-20 dBFS at 1kHz.

## Key Finding: All Styles Are Wideband (Except TTM)

With the user-configurable SC EQ disabled, **13 of 14 styles show flat frequency response**.
The 85 Hz HPF rolloff previously observed in Versatile/Smooth/Punch was entirely the
SC EQ sidechain filter (param 32/33), not the algorithm itself.

The styles differ in **time constants, gain sensitivity, knee shape, and dynamic behavior** —
not in frequency response. This means implementing them requires matching dynamic
characteristics, not adding per-band processing (except TTM).

## Classification by Dynamic Behavior

### High Sensitivity (large GR variation across scenarios)
- **Classic** — GR range 6.3 dB (fast attack = -6.4, slow attack = -1.5 dB)
- **Upward** — GR range 11.9 dB (inverted: fast attack = -2.3, slow attack = -12.6 dB)
- **TTM** — GR range 6.2 dB (multiband, upward + downward)

### Medium Sensitivity
- **Clean** — GR range 5.0 dB
- **Versatile** — GR range 5.0 dB
- **Punch** — GR range 4.7 dB
- **Op-El** — GR range 5.0 dB
- **Opto** — GR range 4.8 dB
- **Pumping** — GR range 3.8 dB

### Low Sensitivity (compressed GR range, glue behavior)
- **Smooth** — GR range 3.9 dB (lowest among modern)
- **Vari-Mu** — GR range 4.1 dB
- **Vocal** — GR range 3.4 dB (auto knee/ratio)
- **Mastering** — GR range 2.6 dB
- **Bus** — GR range 1.7 dB (narrowest — constant glue)

---

## Per-Style Details

### Style 0: Clean (Modern)

**FabFilter description:** "An allround, low distortion, feedforward, program dependent style."

**FTS-Comp status:** 99.97% parity (6662/6664 scenarios at 1.0 dB tolerance).

**Topology:** Feedforward. Detection on input signal.

**Measured behavior:**
- Flat frequency response (Low -5.55, Mid -5.61, High -5.54 dB)
- Medium sensitivity: fast_atk -4.1, slow_atk -1.3 dB at 1kHz
- 2 remaining failures at 20/30 Hz with extreme atk-0.01ms/rel-0ms (power-domain smoothing limit)

**Key constants (FTS-Comp):** PEAK_TO_MEAN_DB=4.2, SMOOTH_POWER=0.80, ATTACK_SCALE=2.0, RELEASE_SCALE=2.0

**Compare profile:** `fts-analyzer/profiles/compare/fts-comp-clean.json`

---

### Style 1: Versatile (Modern, new in Pro-C 3)

**FabFilter description:** "As the name implies, works great on any material. It's punchy at longer attack times but tight and smooth at shorter times."

**Measured behavior:**
- Flat frequency response (Low -6.03, Mid -6.07, High -5.98 dB)
- ~0.5 dB more sensitive than Clean at the same settings
- Slightly higher GR range (fast_atk -4.5, slow_atk -1.7 dB)
- Behaves like Clean with a sensitivity offset (~0.45 dB more compression)

**Implementation notes:** The "punchy at long attack, smooth at short attack" character
suggests attack-dependent knee or program-dependent time constants. With SC EQ enabled,
the default 85 Hz HPF on the sidechain makes it less sensitive to low-frequency content,
which is the "versatile" character — but the core algorithm is wideband.

**Reference data:** `fts-analyzer/reference/pro-c3-versatile-nosc/`

---

### Style 2: Smooth (Modern, new in Pro-C 3)

**FabFilter description:** "Designed to stay smooth at all times, especially suitable for gluing with low ratio and longer times."

**Measured behavior:**
- Flat frequency response (Low -4.09, Mid -4.16, High -4.15 dB)
- Lowest steady-state sensitivity of all modern styles (-4.16 dB vs Clean's -5.63 dB)
- Narrow GR range (fast_atk -3.6, slow_atk -1.4 dB) — very consistent compression

**Implementation notes:** The reduced sensitivity and narrow GR range suggest a soft knee
curve and/or RMS-based level detection (vs peak). The "always smooth" character implies
longer minimum time constants internally or program-dependent smoothing. PEAK_TO_MEAN_DB
would be lower than Clean's 4.2 (less peak sensitivity).

**Reference data:** `fts-analyzer/reference/pro-c3-smooth-nosc/`

---

### Style 3: Punch (Modern)

**FabFilter description:** "Traditional, analog-like compression behavior, sounds good on anything!"

**Measured behavior:**
- Flat frequency response (Low -6.31, Mid -6.28, High -6.23 dB)
- Highest steady-state GR of non-utility modern styles (-6.28 dB)
- Wide GR range (fast_atk -5.0, slow_atk -2.4 dB)

**Implementation notes:** Higher sensitivity than Clean (+0.65 dB more GR). The "analog-like"
descriptor and "punch" name suggest the attack shape preserves transient peaks (fast initial
attack, then settling). May have different attack/release curve shapes vs Clean.

**Reference data:** `fts-analyzer/reference/pro-c3-punch-nosc/`

---

### Style 4: Upward (Modern, new in Pro-C 3)

**FabFilter description:** "Pumping, upward compression (increasing the level when it drops below the threshold), like Saturn's much praised Dynamics knob, but with more control over its behavior."

**Measured behavior:**
- Flat frequency response (Low -9.04, Mid -9.04, High -8.90 dB)
- **Inverted attack behavior:** fast_atk = -2.3, slow_atk = -12.6 dB
  - Fast attack → less GR (opposite of downward compression)
  - Slow attack → massive GR (more upward boost during quiet parts)
- Highest GR range of any style: 11.9 dB

**Implementation notes:** This is fundamentally different from downward compression.
The gain curve inverts: signal below threshold gets boosted. The attack/release controls
how quickly the boost responds. Requires a separate gain calculation path (gain > 1.0
when level < threshold). Saturn-style dynamics = similar concept.

**Reference data:** `fts-analyzer/reference/pro-c3-upward-nosc/`

---

### Style 5: TTM — To The Max (Modern, new in Pro-C 3)

**FabFilter description:** "To The Max multiband mayhem. The TTM style combines upwards and downwards compression on multiple bands, making the input signal louder when it's quiet and quieter when it's loud. In this style the threshold effectively becomes a target level. The knee then controls the blending between the two stages."

**Measured behavior:**
- **Multiband frequency response** — the only non-flat style
  - Low band (<~90 Hz): -8.67 dB (downward compression dominant)
  - Mid band (~90 Hz – 2.5 kHz): -9.05 dB (strongest downward compression)
  - High band (>~2.5 kHz): -2.48 dB average, with upward compression (+5.0 dB at 16 kHz)
- Crossovers at approximately **90 Hz** and **2.5 kHz** (confirmed by per-freq analysis)
- Wild per-frequency variation in high band: alternates between -8.7 and +5.0 dB
- Has both upward and downward compression simultaneously

**Implementation notes:** This requires a full 3-band multiband architecture with
independent compressor instances per band. The threshold acts as a target level —
upward compression below, downward above. The knee parameter blends the two stages.
Crossover frequencies are fixed (~90 Hz, ~2.5 kHz). Each band operates independently,
which is why noise-based tests show massive GR variance (bands fighting each other).

**Reference data:** `fts-analyzer/reference/pro-c3-ttm-nosc/`

---

### Style 6: Op-El (Classic, new in Pro-C 3)

**FabFilter description:** "Effortless opto-like tube compression, smooth and warm."

**Measured behavior:**
- **Gentle HF rolloff:** Low -5.51, Mid -5.49, High -4.69 dB
  - GR decreases smoothly above 1 kHz: -5.44 @ 1k → -2.89 @ 20k (2.5 dB HF reduction)
  - This is not a sidechain filter — it's inherent to the algorithm
- Medium sensitivity (fast_atk -4.9, slow_atk -2.1 dB)

**Implementation notes:** The gradual HF GR reduction is characteristic of optical
compressor behavior where the photocell's response is slower/less sensitive to
high-frequency content. The "tube" aspect adds harmonic coloring. Modeling this
requires either frequency-dependent detection smoothing or a gentle lowpass on
the sidechain path (different from the user SC EQ). The rolloff is gradual
(not a sharp filter), suggesting an inherent smoothing mechanism.

**Reference data:** `fts-analyzer/reference/pro-c3-op-el-nosc/`

---

### Style 7: Vari-Mu (Classic, new in Pro-C 3)

**FabFilter description:** "A classic variable mu topology, offering smooth and colorful feedback compression."

**FabFilter notes:** Feedback algorithm — detection on compressor output, not input.
Uses vacuum tube (remote cutoff tube) modeling. Effective ratio varies with input level
(built-in soft knee). Ratio knob controls tube drive, not direct ratio. Sweet spot -12 to +3 dBFS.

**Measured behavior:**
- Flat frequency response (Low -4.52, Mid -4.59, High -4.58 dB)
- Low sensitivity (-4.59 dB, similar to Smooth)
- Narrow GR range (fast_atk -3.7, slow_atk -1.5 dB)

**Implementation notes:** Feedback topology means the detector sees the compressed output,
creating a natural soft-knee behavior (more compression → less detected level → less
compression). The variable-mu characteristic means the gain element itself has a nonlinear
transfer function — not a static ratio. This requires a feedback loop in the compressor
chain: `output = input * gain(detect(output))`.

**Reference data:** `fts-analyzer/reference/pro-c3-vari-mu-nosc/`

---

### Style 8: Classic (Classic)

**FabFilter description:** "A vintage, feedback, very program dependent style."

**Measured behavior:**
- Flat frequency response (Low -5.33, Mid -5.35, High -5.29 dB)
- **Highest sensitivity to attack time** of all styles: fast_atk -6.4, slow_atk -1.5 dB (range 4.9 dB)
- At fastest attack (0.01ms): -6.4 dB — the most aggressive of the non-utility styles

**Implementation notes:** "Very program dependent" + feedback topology = the compression
character changes significantly with input dynamics. The wide attack sensitivity range
(4.9 dB) confirms strong program dependence. Feedback detection means the compressor
responds to its own output, creating level-dependent behavior that varies with the
signal's dynamic range.

**Reference data:** `fts-analyzer/reference/pro-c3-classic-nosc/`

---

### Style 9: Opto (Classic)

**FabFilter description:** "A relatively slow, very soft knee, more linear opto style."

**Measured behavior:**
- **Gentle LF-to-HF slope:** Low -5.92, Mid -6.40, High -6.59 dB
  - GR increases with frequency: -5.66 @ 20 Hz → -6.76 @ 5 kHz (+1.1 dB)
  - Opposite direction from Op-El's HF rolloff
- Medium-high sensitivity (fast_atk -4.3, slow_atk -2.2 dB)

**Implementation notes:** The increasing GR with frequency is subtle but consistent,
suggesting the opto model has slightly faster response to higher frequencies (photocell
responds quicker to HF content in the sidechain). "Very soft knee" means a wide
transition zone around the threshold. "More linear" = closer to 1:1 below threshold
(no premature compression). This is distinct from Op-El which has tube coloring.

**Reference data:** `fts-analyzer/reference/pro-c3-opto-nosc/`

---

### Style 10: Vocal (Utility)

**FabFilter description:** "A very effective algorithm to bring vocals to the front of your mix. It works with automatic knee and ratio settings, so compressing your lead vocal is as easy as choosing the right threshold."

**Measured behavior:**
- Flat frequency response (Low -9.29, Mid -9.26, High -8.90 dB)
- Very high GR (-9.24 dB at 1 kHz — second highest after TTM's mid band)
- **Extremely narrow GR range:** 3.4 dB (fast -9.3, slow -6.5)
- Automatic knee and ratio override the user controls

**Implementation notes:** The high base GR + narrow range means this style aggressively
compresses everything with very consistent output level regardless of attack/release settings.
The "automatic knee and ratio" means the user's Ratio and Knee knob positions are
partially or fully overridden. This is a "set threshold and forget" design. The narrow
GR range (3.4 dB across 196 scenarios) confirms the auto-ratio is keeping compression consistent.

**Reference data:** `fts-analyzer/reference/pro-c3-vocal-nosc/`

---

### Style 11: Mastering (Utility)

**FabFilter description:** "Designed to be as transparent as possible, introducing as little harmonic distortion as possible, while still being able to catch those fast transients."

**Measured behavior:**
- Flat frequency response (Low -7.07, Mid -7.09, High -6.91 dB)
- High GR (-7.10 dB) but narrow range (2.6 dB)
- Very consistent: fast_atk -6.7, slow_atk -5.2 dB

**Implementation notes:** Transparency + fast transient catching suggests lookahead
or a very clean peak detector with minimal overshoot. The narrow GR range (2.6 dB)
means the time constants have less effect — the compressor maintains consistent
behavior. Minimal distortion = clean gain element with no saturation modeling.

**Reference data:** `fts-analyzer/reference/pro-c3-mastering-nosc/`

---

### Style 12: Bus (Utility)

**FabFilter description:** "Especially great for bus processing, or for adding a pleasant glue to your drums, mixes or tracks."

**Measured behavior:**
- Near-flat frequency response (Low -6.84, Mid -7.13, High -6.99 dB)
- **Narrowest GR range of all styles:** 1.7 dB (fast -6.1, slow -5.4)
- Almost no variation across attack/release settings

**Implementation notes:** The 1.7 dB GR range across 196 scenarios is remarkable —
this means the bus compressor applies essentially the same amount of compression
regardless of time constant settings. This is "glue" behavior: constant, gentle
compression that doesn't pump or breathe. The attack/release controls likely
affect the transient shape rather than the overall GR amount. May use RMS detection
with long internal averaging.

**Reference data:** `fts-analyzer/reference/pro-c3-bus-nosc/`

---

### Style 13: Pumping (Utility)

**FabFilter description:** "Deep and over-the-top pumping, great for drum processing or EDM."

**Measured behavior:**
- Flat frequency response (Low -7.12, Mid -7.18, High -7.07 dB)
- Moderate GR range (3.8 dB: fast -5.1, slow -3.9)
- Notable: at atk-60ms/rel-400ms, GR is -6.1 dB (deepest at slow settings)

**Implementation notes:** "Pumping" comes from the release behavior — fast release
causes the gain to snap back quickly after compression, creating the audible
pumping/breathing effect. The time constant controls likely have exaggerated
curves compared to other styles. The deeper GR at slow settings suggests the
compressor accumulates more compression with longer time constants (opposite of
most styles where fast attack = more GR).

**Reference data:** `fts-analyzer/reference/pro-c3-pumping-nosc/`

---

## Measurement Methodology

### Capture command
```bash
fts-analyzer-cli capture-compressor \
  --profile profiles/capture/pro-c3-{style}.json \
  --threads 1
```

### Profile structure
Each profile sets:
- `"0": <style_id>` — Style selector (0-13)
- `"19": 0.0` — Auto Gain off
- `"32": 0.0` — SC EQ Band 1 Used = false (disables user HPF)
- `"33": 0.0` — SC EQ Band 1 Enabled = false

### Test signal
- Alternating -6 / -20 dBFS pure tone pulses (240 ms high, 240 ms low)
- 34 frequencies: 20, 30, 40, 50, 60, 80, 100 ... 20000 Hz (log-spaced)
- 196 attack/release combinations from the standard scenario set
- 48 kHz sample rate, 512 block size, 3s duration per scenario

### Pro-C 3 fixed settings for all captures
- Threshold: -18.0 dB (default)
- Ratio: 4:1 (default)
- Knee: 0 dB
- Auto Gain: Off

### Binary format
`.bin` files: `[u32 num_freqs][u32 samples_per_freq][u8 GR data...]`
GR encoding: `gr_db = -48.0 + (val / 255.0) * 54.0` (range -48 to +6 dB, ~0.21 dB resolution)

## Implementation Priority

Based on analysis, suggested implementation order:

1. **Clean** — Done (99.97% parity)
2. **Versatile** — Very similar to Clean, just needs sensitivity offset and optional SC HPF
3. **Smooth** — Lower sensitivity variant, likely RMS-based detection
4. **Punch** — Higher sensitivity variant, likely different attack curve shape
5. **Mastering** — Transparent, narrow GR range, clean detection
6. **Bus** — Constant glue, narrowest GR range
7. **Classic** — Feedback topology (new architecture needed)
8. **Vari-Mu** — Feedback + tube gain curve
9. **Op-El** — Opto model with gentle HF rolloff
10. **Opto** — Different opto model, LF-to-HF slope
11. **Vocal** — Auto knee/ratio
12. **Pumping** — Exaggerated release curves
13. **Upward** — Inverted gain curve (boost below threshold)
14. **TTM** — Full 3-band multiband architecture
