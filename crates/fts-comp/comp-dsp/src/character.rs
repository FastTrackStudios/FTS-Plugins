//! Compression character ranges — musically meaningful attack/release zones.
//!
//! These define the perceptual boundaries between different compression behaviors.
//! Used both for UI labeling and as test points for reference capture.

/// Attack time character range.
pub struct AttackRange {
    pub name: &'static str,
    pub min_ms: f64,
    pub max_ms: f64,
}

/// Release time character range.
pub struct ReleaseRange {
    pub name: &'static str,
    pub min_ms: f64,
    pub max_ms: f64,
}

/// Attack character ranges, from fastest to slowest.
pub const ATTACK_RANGES: &[AttackRange] = &[
    AttackRange {
        name: "Distortion",
        min_ms: 0.0,
        max_ms: 2.0,
    },
    AttackRange {
        name: "Transient Erosion",
        min_ms: 3.0,
        max_ms: 5.0,
    },
    AttackRange {
        name: "Clicky",
        min_ms: 6.0,
        max_ms: 15.0,
    },
    AttackRange {
        name: "Punchy",
        min_ms: 16.0,
        max_ms: 24.0,
    },
    AttackRange {
        name: "Mix Bus",
        min_ms: 25.0,
        max_ms: 39.0,
    },
    AttackRange {
        name: "Sluggish",
        min_ms: 40.0,
        max_ms: 59.0,
    },
    AttackRange {
        name: "Very Slow",
        min_ms: 60.0,
        max_ms: 240.0,
    },
];

/// Release character ranges, from fastest to slowest.
pub const RELEASE_RANGES: &[ReleaseRange] = &[
    ReleaseRange {
        name: "Distortion",
        min_ms: 0.0,
        max_ms: 9.0,
    },
    ReleaseRange {
        name: "Loud",
        min_ms: 10.0,
        max_ms: 19.0,
    },
    ReleaseRange {
        name: "Crisp",
        min_ms: 20.0,
        max_ms: 24.0,
    },
    ReleaseRange {
        name: "Gluey",
        min_ms: 35.0,
        max_ms: 49.0,
    },
    ReleaseRange {
        name: "Pumpy",
        min_ms: 50.0,
        max_ms: 79.0,
    },
    ReleaseRange {
        name: "Attack Emphasis",
        min_ms: 80.0,
        max_ms: 119.0,
    },
    ReleaseRange {
        name: "Slow",
        min_ms: 120.0,
        max_ms: 199.0,
    },
    ReleaseRange {
        name: "Very Slow",
        min_ms: 200.0,
        max_ms: 400.0,
    },
];

/// Key attack test points (ms) — boundaries of each character range.
/// These are the values to capture reference data at.
pub const ATTACK_TEST_POINTS_MS: &[f64] = &[
    0.0, 2.0, // Distortion
    3.0, 5.0, // Transient Erosion
    6.0, 15.0, // Clicky
    16.0, 24.0, // Punchy
    25.0, 39.0, // Mix Bus
    40.0, 59.0, // Sluggish
    60.0, 240.0, // Very Slow
];

/// Key release test points (ms) — boundaries of each character range.
pub const RELEASE_TEST_POINTS_MS: &[f64] = &[
    0.0, 9.0, // Distortion
    10.0, 19.0, // Loud
    20.0, 24.0, // Crisp
    35.0, 49.0, // Gluey
    50.0, 79.0, // Pumpy
    80.0, 119.0, // Attack Emphasis
    120.0, 199.0, // Slow
    200.0, 400.0, // Very Slow
];

/// Look up the character name for a given attack time in ms.
pub fn attack_character(ms: f64) -> &'static str {
    for range in ATTACK_RANGES {
        if ms >= range.min_ms && ms <= range.max_ms {
            return range.name;
        }
    }
    if ms > 240.0 {
        "Very Slow"
    } else {
        "Unknown"
    }
}

/// Look up the character name for a given release time in ms.
pub fn release_character(ms: f64) -> &'static str {
    for range in RELEASE_RANGES {
        if ms >= range.min_ms && ms <= range.max_ms {
            return range.name;
        }
    }
    if ms > 400.0 {
        "Very Slow"
    } else {
        "Unknown"
    }
}
