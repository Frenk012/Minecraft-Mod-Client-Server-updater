use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    pub local: LocalConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curseforge: Option<CurseForgeConfig>,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LocalConfig {
    pub mods_folder: String,
    pub minecraft_version: String,
    pub loader: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    #[serde(default)]
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub remote_mods_folder: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CurseForgeConfig {
    pub api_key: String,
}

#[derive(Debug, Clone)]
pub struct LocalMod {
    pub filename: String,
    pub filepath: PathBuf,
    pub sha512: String,
    pub murmur2: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ModVersion {
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    pub download_url: String,
    #[allow(dead_code)]
    pub game_versions: Vec<String>,
    #[allow(dead_code)]
    pub loaders: Vec<String>,
    #[allow(dead_code)]
    pub source: ModSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModSource {
    Modrinth,
    CurseForge,
}

impl ModSource {
    pub fn label(&self) -> &'static str {
        match self {
            ModSource::Modrinth => "Modrinth",
            ModSource::CurseForge => "CurseForge",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Side {
    Both,
    ClientOnly,
    ServerOnly,
}

impl Side {
    pub fn label(&self) -> &'static str {
        match self {
            Side::Both => "both",
            Side::ClientOnly => "client",
            Side::ServerOnly => "server",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModInfo {
    pub project_id: String,
    pub project_name: String,
    pub current_version: ModVersion,
    pub latest_version: Option<ModVersion>,
    pub local_mod: LocalMod,
    pub source: ModSource,
    pub side: Side,
}

impl ModInfo {
    pub fn has_update(&self) -> bool {
        match &self.latest_version {
            Some(latest) => latest.version_id != self.current_version.version_id,
            None => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnknownMod {
    pub local_mod: LocalMod,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscrepancyKind {
    Mismatch,
    ClientOnly,
    ServerOnly,
}

#[derive(Debug, Clone)]
pub struct DiscrepancyRecord {
    pub project_id: String,
    pub project_name: String,
    pub kind: DiscrepancyKind,
    pub client_mod: Option<ModInfo>,
    pub server_mod: Option<ModInfo>,
}
