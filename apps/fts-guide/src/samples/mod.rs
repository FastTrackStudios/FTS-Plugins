//! Sample loading and caching management
//!
//! This module handles loading audio samples (click, count, guide) and managing their cache.

mod cache;
mod click_loader;
mod guide_loader;
pub mod loader;

pub use cache::SampleCache;
pub use click_loader::ClickSampleLoader;
pub use guide_loader::{GuideSampleLoader, get_guide_key, section_to_guide_filename};
pub use loader::SampleLoader;

/// Resolve the FTS home directory.
/// Checks `$FTS_HOME`, falls back to `$HOME/Music/FastTrackStudio`.
pub fn fts_home() -> String {
    std::env::var("FTS_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/codywright".into());
        format!("{home}/Music/FastTrackStudio")
    })
}
