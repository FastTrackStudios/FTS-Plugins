//! Integration test helpers for FTS plugin testing.
//!
//! This module provides utilities for setting up and running integration tests
//! against the REAPER DAW with fts-plugins.

/// Test setup and teardown utilities.
pub mod setup {
    use std::path::PathBuf;
    use std::time::Duration;

    /// Path to the REAPER executable.
    pub const REAPER_EXECUTABLE: &str =
        "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";

    /// Path to REAPER resources directory.
    pub const REAPER_RESOURCES: &str =
        "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    /// FTS user plugins directory.
    pub fn fx_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/codywright".to_string());
        PathBuf::from(format!(
            "{}/Music/FastTrackStudio/Reaper/FTS-TRACKS/UserPlugins/FX",
            home
        ))
    }

    /// Check if fts-macros plugin is installed.
    pub fn is_macros_plugin_installed() -> bool {
        fx_dir().join("fts-macros.clap").exists()
    }

    /// Check if REAPER executable exists.
    pub fn is_reaper_available() -> bool {
        std::path::Path::new(REAPER_EXECUTABLE).exists()
    }

    /// Verify test environment is set up correctly.
    pub fn verify_environment() -> Result<(), String> {
        let mut errors = Vec::new();

        if !is_reaper_available() {
            errors.push(format!("REAPER not found at {}", REAPER_EXECUTABLE));
        }

        if !is_macros_plugin_installed() {
            errors.push(format!(
                "fts-macros plugin not installed to {}",
                fx_dir().display()
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Print environment check results.
    pub fn print_environment_check() {
        println!("\n=== FTS Plugin Test Environment Check ===\n");
        println!("REAPER executable: {}", REAPER_EXECUTABLE);
        println!("  Available: {}", is_reaper_available());

        let plugin_path = fx_dir().join("fts-macros.clap");
        println!("\nfts-macros plugin: {}", plugin_path.display());
        println!("  Installed: {}", is_macros_plugin_installed());

        println!("\nTo install the plugin:");
        println!("  cd /Users/codywright/Documents/Development/FastTrackStudio/fts-plugins");
        println!("  just install");
    }
}

/// Mock/stub implementations for offline testing.
pub mod mock {
    /// Represents a simulated REAPER track for offline testing.
    pub struct MockTrack {
        pub fx_chain: Vec<MockFx>,
    }

    impl MockTrack {
        pub fn new() -> Self {
            Self {
                fx_chain: Vec::new(),
            }
        }

        pub fn add_fx(&mut self, name: &str) -> usize {
            self.fx_chain.push(MockFx::new(name));
            self.fx_chain.len() - 1
        }
    }

    /// Represents a simulated FX plugin.
    pub struct MockFx {
        pub name: String,
        pub params: Vec<MockParam>,
    }

    impl MockFx {
        pub fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                params: Vec::new(),
            }
        }

        /// Initialize FX with a set of parameters.
        pub fn with_params(name: &str, param_count: usize) -> Self {
            let mut fx = Self::new(name);
            for i in 0..param_count {
                fx.params.push(MockParam {
                    name: format!("Param {}", i),
                    value: 0.0,
                });
            }
            fx
        }
    }

    /// Represents a simulated parameter.
    pub struct MockParam {
        pub name: String,
        pub value: f32,
    }

    impl MockParam {
        pub fn set(&mut self, value: f32) {
            self.value = value.clamp(0.0, 1.0);
        }

        pub fn get(&self) -> f32 {
            self.value
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_mock_track_and_fx() {
            let mut track = MockTrack::new();
            let fx_idx = track.add_fx("fts-macros");
            assert_eq!(fx_idx, 0);

            // Add parameters to the FX
            let mut fx = MockFx::with_params("fts-macros", 8);
            assert_eq!(fx.params.len(), 8);

            // Test parameter setting and getting
            fx.params[0].set(0.5);
            assert_eq!(fx.params[0].get(), 0.5);

            // Test parameter clamping
            fx.params[0].set(1.5);
            assert_eq!(fx.params[0].get(), 1.0);
        }
    }
}

pub mod macros {
    //! Constants and definitions for macro system testing.

    /// Number of macro slots in fts-macros plugin.
    pub const MACRO_COUNT: usize = 8;

    /// Parameter ID prefix for macro parameters.
    pub const MACRO_PARAM_PREFIX: &str = "macro_";

    /// Expected parameter names in order (1-indexed for user display).
    pub fn macro_name(index: usize) -> String {
        format!("Macro {}", index + 1)
    }

    /// Macro parameter ID by index (0-7).
    pub fn macro_param_id(index: usize) -> String {
        format!("{}{}",MACRO_PARAM_PREFIX, index)
    }

    /// Test macro parameter ID format.
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_macro_naming() {
            assert_eq!(macro_name(0), "Macro 1");
            assert_eq!(macro_name(7), "Macro 8");
        }

        #[test]
        fn test_macro_param_ids() {
            assert_eq!(macro_param_id(0), "macro_0");
            assert_eq!(macro_param_id(7), "macro_7");
        }
    }
}
