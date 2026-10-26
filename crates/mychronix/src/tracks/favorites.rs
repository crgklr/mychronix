//! Tracks marked as favorites.

use std::path::PathBuf;

/// Tracks marked as favorites, by track guid, saved in `~/Mychronix/favorite-tracks.json`.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Favorites {
    pub guids: std::collections::BTreeSet<String>,
}

impl Favorites {
    fn path() -> Option<PathBuf> {
        crate::paths::data("favorite-tracks.json")
    }

    pub fn load() -> Self {
        crate::storage::load_or_default(Self::path())
    }

    pub fn contains(&self, guid: &str) -> bool {
        self.guids.contains(guid)
    }

    /// Adds or removes `guid`, then saves.
    pub fn toggle(&mut self, guid: &str) -> std::io::Result<()> {
        if !self.guids.remove(guid) {
            self.guids.insert(guid.to_string());
        }
        let Some(path) = Self::path() else { return Ok(()) };
        crate::storage::save(&path, self)
    }
}
