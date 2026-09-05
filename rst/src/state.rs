use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn state_file() -> std::path::PathBuf {
    crate::config::app_dir().join("state.json")
}

/// Persistent app state: survives crashes/restarts.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppState {
    /// project_id -> project_name. Blacklisted mods are never updated.
    #[serde(default)]
    pub blacklist: BTreeMap<String, String>,
}

impl AppState {
    pub fn load() -> Self {
        std::fs::read_to_string(state_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::write(state_file(), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn is_blacklisted(&self, project_id: &str) -> bool {
        self.blacklist.contains_key(project_id)
    }

    pub fn toggle_blacklist(&mut self, project_id: &str, name: &str) {
        if self.blacklist.remove(project_id).is_none() {
            self.blacklist.insert(project_id.to_string(), name.to_string());
        }
        let _ = self.save();
    }
}
