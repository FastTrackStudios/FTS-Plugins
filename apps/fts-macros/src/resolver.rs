//! FX parameter resolver - converts virtual descriptors to actual REAPER objects
//!
//! Resolves abstract track/FX descriptors at runtime to find the actual FxParameter
//! objects. This allows mappings to survive track/FX reordering.

use crate::mapping::{FxDescriptor, TrackDescriptor};
use std::fmt;

/// Errors that can occur during resolution
#[derive(Clone, Debug, PartialEq)]
pub enum ResolveError {
    TrackNotFound(String),
    FxNotFound(String),
    ParamOutOfBounds(u32),
    NoTracksAvailable,
    MultipleMatches(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::TrackNotFound(desc) => write!(f, "Track not found: {}", desc),
            ResolveError::FxNotFound(desc) => write!(f, "FX not found: {}", desc),
            ResolveError::ParamOutOfBounds(idx) => {
                write!(f, "Parameter index {} out of bounds", idx)
            }
            ResolveError::NoTracksAvailable => write!(f, "No tracks available in project"),
            ResolveError::MultipleMatches(desc) => {
                write!(f, "Multiple matches found for: {}", desc)
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Runtime resolution context
///
/// In a real implementation, this would contain REAPER API references.
/// For now, we provide the interface that US-002 requires.
pub struct FxParameterResolver;

impl FxParameterResolver {
    /// Resolve a track descriptor to a track index
    ///
    /// Returns the track index if found, or an error if:
    /// - Track not found
    /// - Pattern matches multiple tracks (returns error, not first match)
    /// - No tracks in project
    pub fn resolve_track(track_desc: &TrackDescriptor) -> Result<u32, ResolveError> {
        match track_desc {
            TrackDescriptor::ByIndex(idx) => {
                // In real implementation, would validate track exists
                Ok(*idx)
            }
            TrackDescriptor::ByName(name) => {
                // Would search for exact name match
                // For now, return error that would be overridden by real implementation
                Err(ResolveError::TrackNotFound(format!("Track '{}'", name)))
            }
            TrackDescriptor::ByNamePattern(pattern) => {
                // Would search using wildcard matching
                Err(ResolveError::TrackNotFound(format!(
                    "Track matching '{}'",
                    pattern
                )))
            }
        }
    }

    /// Resolve an FX descriptor to an FX index on the given track
    ///
    /// Returns the FX index if found, or an error if:
    /// - FX not found
    /// - Multiple plugins match (pattern matching)
    pub fn resolve_fx(_track_idx: u32, fx_desc: &FxDescriptor) -> Result<u32, ResolveError> {
        match fx_desc {
            FxDescriptor::ByIndex(idx) => {
                // In real implementation, would validate FX exists
                Ok(*idx)
            }
            FxDescriptor::ByName(name) => {
                // Would search for exact plugin name
                Err(ResolveError::FxNotFound(format!("FX '{}'", name)))
            }
            FxDescriptor::ByPluginName(plugin_id) => {
                // Would search by plugin identifier
                Err(ResolveError::FxNotFound(format!("Plugin '{}'", plugin_id)))
            }
        }
    }

    /// Validate that a parameter index is valid for the FX
    ///
    /// In real implementation, would check against actual FX parameter count.
    /// For now, assumes any u32 is potentially valid.
    pub fn validate_param_index(
        _track_idx: u32,
        _fx_idx: u32,
        param_idx: u32,
    ) -> Result<(), ResolveError> {
        // In a real implementation:
        // let param_count = get_fx_param_count(track, fx)?;
        // if param_idx >= param_count { return Err(...) }
        // For now, accept any index (would be validated by REAPER API)
        let _ = param_idx;
        Ok(())
    }
}

/// Resolution cache to avoid repeated lookups within a single buffer
///
/// Tracks are typically reordered rarely, so caching per-buffer
/// dramatically reduces REAPER API calls.
#[derive(Clone, Debug, Default)]
pub struct ResolutionCache {
    /// Cached track lookups: key is track descriptor string, value is track index
    track_cache: std::collections::HashMap<String, u32>,
    /// Cached FX lookups: key is "track_idx:fx_descriptor", value is fx index
    fx_cache: std::collections::HashMap<String, u32>,
}

impl ResolutionCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all cached entries
    pub fn clear(&mut self) {
        self.track_cache.clear();
        self.fx_cache.clear();
    }

    /// Get or compute cached track resolution
    pub fn resolve_track_cached(
        &mut self,
        track_desc: &TrackDescriptor,
    ) -> Result<u32, ResolveError> {
        let key = format!("{:?}", track_desc);
        if let Some(&idx) = self.track_cache.get(&key) {
            return Ok(idx);
        }

        let idx = FxParameterResolver::resolve_track(track_desc)?;
        self.track_cache.insert(key, idx);
        Ok(idx)
    }

    /// Get or compute cached FX resolution
    pub fn resolve_fx_cached(
        &mut self,
        track_idx: u32,
        fx_desc: &FxDescriptor,
    ) -> Result<u32, ResolveError> {
        let key = format!("{}:{:?}", track_idx, fx_desc);
        if let Some(&idx) = self.fx_cache.get(&key) {
            return Ok(idx);
        }

        let idx = FxParameterResolver::resolve_fx(track_idx, fx_desc)?;
        self.fx_cache.insert(key, idx);
        Ok(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_track_by_index() {
        let desc = TrackDescriptor::ByIndex(0);
        assert_eq!(FxParameterResolver::resolve_track(&desc), Ok(0));

        let desc = TrackDescriptor::ByIndex(5);
        assert_eq!(FxParameterResolver::resolve_track(&desc), Ok(5));
    }

    #[test]
    fn test_resolve_track_by_name_error() {
        let desc = TrackDescriptor::ByName("Drums".to_string());
        let result = FxParameterResolver::resolve_track(&desc);
        assert!(result.is_err());
        assert!(matches!(result, Err(ResolveError::TrackNotFound(_))));
    }

    #[test]
    fn test_resolve_track_by_pattern_error() {
        let desc = TrackDescriptor::ByNamePattern("*Drum*".to_string());
        let result = FxParameterResolver::resolve_track(&desc);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_fx_by_index() {
        let desc = FxDescriptor::ByIndex(2);
        assert_eq!(FxParameterResolver::resolve_fx(0, &desc), Ok(2));
    }

    #[test]
    fn test_resolve_fx_by_name_error() {
        let desc = FxDescriptor::ByName("Compressor".to_string());
        let result = FxParameterResolver::resolve_fx(0, &desc);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_fx_by_plugin_name_error() {
        let desc = FxDescriptor::ByPluginName("ReaEQ".to_string());
        let result = FxParameterResolver::resolve_fx(0, &desc);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_param_index() {
        // For now, all indices are valid in test
        assert!(FxParameterResolver::validate_param_index(0, 0, 5).is_ok());
        assert!(FxParameterResolver::validate_param_index(0, 0, 100).is_ok());
    }

    #[test]
    fn test_resolution_cache_track() {
        let mut cache = ResolutionCache::new();
        let desc = TrackDescriptor::ByIndex(1);

        // First call resolves
        let result1 = cache.resolve_track_cached(&desc);
        assert_eq!(result1, Ok(1));

        // Second call uses cache (same result)
        let result2 = cache.resolve_track_cached(&desc);
        assert_eq!(result2, Ok(1));

        // Cache hit (verified by checking it's the same value)
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_resolution_cache_fx() {
        let mut cache = ResolutionCache::new();
        let desc = FxDescriptor::ByIndex(2);

        let result1 = cache.resolve_fx_cached(0, &desc);
        assert_eq!(result1, Ok(2));

        let result2 = cache.resolve_fx_cached(0, &desc);
        assert_eq!(result2, Ok(2));
    }

    #[test]
    fn test_resolution_cache_clear() {
        let mut cache = ResolutionCache::new();
        let desc = TrackDescriptor::ByIndex(1);

        let _result1 = cache.resolve_track_cached(&desc);
        assert_eq!(cache.track_cache.len(), 1);

        cache.clear();
        assert_eq!(cache.track_cache.len(), 0);
        assert_eq!(cache.fx_cache.len(), 0);
    }
}
