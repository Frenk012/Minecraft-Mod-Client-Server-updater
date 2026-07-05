use anyhow::{bail, Result};
use std::path::Path;
use tokio::task::JoinSet;
use crate::hashing::{hash_file, murmur2_file};
use crate::models::LocalMod;

pub async fn scan_local_folder(folder: &str) -> Result<Vec<LocalMod>> {
    let path = Path::new(folder);
    if !path.exists() {
        bail!("Mods folder not found: {folder}");
    }

    let entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("jar"))
                .unwrap_or(false)
        })
        .collect();

    let mut set: JoinSet<Result<LocalMod>> = JoinSet::new();
    for entry in entries {
        set.spawn_blocking(move || {
            let filepath = entry.path();
            let filename = filepath.file_name().unwrap().to_string_lossy().to_string();
            let sha512 = hash_file(&filepath)?;
            let murmur2 = murmur2_file(&filepath).ok();
            Ok(LocalMod { filename, filepath, sha512, murmur2 })
        });
    }

    let mut mods = Vec::new();
    while let Some(res) = set.join_next().await {
        mods.push(res??);
    }
    Ok(mods)
}
