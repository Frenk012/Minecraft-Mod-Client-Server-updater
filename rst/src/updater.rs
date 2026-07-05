use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use tokio::task::JoinSet;
use crate::models::{LocalMod, ModInfo, ModSource, ModVersion, Side, UnknownMod};
use crate::progress::Progress;
use crate::{curseforge, modrinth};

#[derive(Clone)]
pub struct IdentifyResult {
    pub known: Vec<ModInfo>,
    pub unknown: Vec<UnknownMod>,
}

struct PendingMod {
    local_mod: LocalMod,
    project_id: String,
    project_name: String,
    current_version: ModVersion,
    source: ModSource,
    side: Side,
    cf_mod_id: Option<u32>,
}

pub async fn identify_and_check_updates(
    client: &Client,
    local_mods: Vec<LocalMod>,
    mc_version: &str,
    loader: &str,
    cf_api_key: Option<&str>,
    progress: &Progress,
) -> Result<IdentifyResult> {
    let hashes: Vec<String> = local_mods.iter().map(|m| m.sha512.clone()).collect();

    progress.set("Querying CurseForge/Modrinth by hash…");
    let mr_fut = modrinth::get_versions_by_hash_full(client, &hashes);
    let cf_fut = async {
        if let Some(key) = cf_api_key {
            let fps: Vec<u32> = local_mods.iter().filter_map(|m| m.murmur2).collect();
            curseforge::get_versions_by_fingerprint(client, key, &fps, loader).await
        } else {
            Ok(HashMap::new())
        }
    };
    let (mr_results, cf_results) = tokio::join!(mr_fut, cf_fut);
    let (mr_results, cf_results) = (mr_results?, cf_results?);

    // sha512 -> CF match (CurseForge checked first, Modrinth as fallback)
    let mut sha_to_cf: HashMap<String, (String, ModVersion)> = HashMap::new();
    for m in &local_mods {
        if let Some(fp) = m.murmur2 {
            if let Some(cf_match) = cf_results.get(&fp) {
                sha_to_cf.insert(m.sha512.clone(), cf_match.clone());
            }
        }
    }

    let mr_project_ids: Vec<String> = mr_results
        .values()
        .filter(|(pid, _)| !sha_to_cf.values().any(|(cpid, _)| cpid == pid))
        .map(|(pid, _)| pid.clone())
        .collect();
    let cf_mod_ids: Vec<u32> = sha_to_cf
        .values()
        .filter_map(|(pid, _)| pid.parse::<u32>().ok())
        .collect();

    progress.set("Fetching project names…");
    let mr_names_fut = modrinth::get_projects(client, &mr_project_ids);
    let cf_names_fut = async {
        if let Some(key) = cf_api_key {
            curseforge::get_mods_info(client, key, &cf_mod_ids).await
        } else {
            Ok(HashMap::new())
        }
    };
    let (project_names, cf_names) = tokio::join!(mr_names_fut, cf_names_fut);
    let (project_names, cf_names) = (project_names?, cf_names?);

    let mut pending: Vec<PendingMod> = Vec::new();
    let mut unknown: Vec<UnknownMod> = Vec::new();

    for local_mod in local_mods {
        let sha = &local_mod.sha512;

        if let Some((project_id, current_version)) = sha_to_cf.get(sha) {
            let mod_id: u32 = project_id.parse().unwrap_or(0);
            let project_name = cf_names
                .get(&mod_id)
                .cloned()
                .unwrap_or_else(|| current_version.filename.clone());
            pending.push(PendingMod {
                local_mod,
                project_id: project_id.clone(),
                project_name,
                current_version: current_version.clone(),
                source: ModSource::CurseForge,
                side: Side::Both, // CurseForge does not expose side info
                cf_mod_id: Some(mod_id),
            });
        } else if let Some((project_id, current_version)) = mr_results.get(sha) {
            let (project_name, side) = project_names
                .get(project_id)
                .cloned()
                .unwrap_or_else(|| (current_version.filename.clone(), Side::Both));
            pending.push(PendingMod {
                local_mod,
                project_id: project_id.clone(),
                project_name,
                current_version: current_version.clone(),
                source: ModSource::Modrinth,
                side,
                cf_mod_id: None,
            });
        } else {
            unknown.push(UnknownMod { local_mod });
        }
    }

    let total = pending.len();
    progress.set(format!("Checking updates 0/{total}"));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut set: JoinSet<Result<ModInfo>> = JoinSet::new();
    for p in pending {
        let client = client.clone();
        let mc_version = mc_version.to_string();
        let loader = loader.to_string();
        let cf_key = cf_api_key.map(|s| s.to_string());
        let progress = progress.clone();
        let done = done.clone();

        set.spawn(async move {
            let latest_version = match p.source {
                ModSource::CurseForge => {
                    if let (Some(key), Some(mod_id)) = (&cf_key, p.cf_mod_id) {
                        curseforge::get_latest_version(&client, key, mod_id, &mc_version, &loader)
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    }
                }
                ModSource::Modrinth => {
                    modrinth::get_latest_version(&client, &p.project_id, &mc_version, &loader)
                        .await
                        .ok()
                        .flatten()
                }
            };
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            progress.set(format!("Checking updates {n}/{total}"));
            Ok(ModInfo {
                project_id: p.project_id,
                project_name: p.project_name,
                current_version: p.current_version,
                latest_version,
                local_mod: p.local_mod,
                source: p.source,
                side: p.side,
            })
        });
    }

    let mut known = Vec::new();
    while let Some(res) = set.join_next().await {
        known.push(res??);
    }
    known.sort_by(|a, b| a.project_name.to_lowercase().cmp(&b.project_name.to_lowercase()));

    Ok(IdentifyResult { known, unknown })
}

pub async fn download_mod_bytes(client: &Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

pub async fn apply_updates(
    client: &Client,
    updates: &[ModInfo],
    mods_folder: &str,
    progress: &Progress,
) -> Result<()> {
    let mut set: JoinSet<Result<String>> = JoinSet::new();

    for info in updates {
        let latest = match &info.latest_version {
            Some(v) => v.clone(),
            None => continue,
        };
        let client = client.clone();
        let dest = Path::new(mods_folder).join(&latest.filename);
        let old_path = info.local_mod.filepath.clone();

        set.spawn(async move {
            let data = download_mod_bytes(&client, &latest.download_url).await?;
            tokio::fs::write(&dest, &data).await?;
            if old_path != dest {
                let _ = tokio::fs::remove_file(&old_path).await;
            }
            Ok(latest.filename)
        });
    }

    while let Some(res) = set.join_next().await {
        let name = res??;
        progress.set(format!("Updated {name}"));
    }
    Ok(())
}
