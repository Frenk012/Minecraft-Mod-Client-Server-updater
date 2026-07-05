use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::path::Path;
use crate::hashing::hash_bytes;
use crate::models::{DiscrepancyKind, DiscrepancyRecord, LocalMod, ModInfo, Side};
use crate::progress::Progress;
use crate::sftp::SftpClient;
use crate::updater::download_mod_bytes;

pub async fn scan_remote_mods(
    sftp: &SftpClient,
    remote_folder: &str,
    progress: &Progress,
) -> Result<Vec<LocalMod>> {
    let jars = sftp.list_remote_jars(remote_folder).await?;
    let total = jars.len();

    let mut mods = Vec::new();
    for (i, filename) in jars.into_iter().enumerate() {
        progress.set(format!("Reading remote mods {}/{total}", i + 1));
        let remote_path = format!("{remote_folder}/{filename}");
        let bytes = sftp.read_remote_file_bytes(&remote_path).await?;
        let sha512 = hash_bytes(&bytes);
        let murmur2 = Some(crate::hashing::murmur2(&bytes));
        mods.push(LocalMod {
            filename,
            filepath: Path::new(&remote_path).to_path_buf(),
            sha512,
            murmur2,
        });
    }
    Ok(mods)
}

pub fn compare_mod_sets(
    client_mods: &[ModInfo],
    server_mods: &[ModInfo],
) -> Vec<DiscrepancyRecord> {
    let client_map: HashMap<&str, &ModInfo> =
        client_mods.iter().map(|m| (m.project_id.as_str(), m)).collect();
    let server_map: HashMap<&str, &ModInfo> =
        server_mods.iter().map(|m| (m.project_id.as_str(), m)).collect();

    let mut records = Vec::new();

    for (pid, cm) in &client_map {
        // Client-only mods don't belong on the server, skip
        if cm.side == Side::ClientOnly {
            continue;
        }
        match server_map.get(pid) {
            Some(sm) => {
                if cm.current_version.version_id != sm.current_version.version_id {
                    records.push(DiscrepancyRecord {
                        project_id: pid.to_string(),
                        project_name: cm.project_name.clone(),
                        kind: DiscrepancyKind::Mismatch,
                        client_mod: Some((*cm).clone()),
                        server_mod: Some((*sm).clone()),
                    });
                }
            }
            None => {
                records.push(DiscrepancyRecord {
                    project_id: pid.to_string(),
                    project_name: cm.project_name.clone(),
                    kind: DiscrepancyKind::ClientOnly,
                    client_mod: Some((*cm).clone()),
                    server_mod: None,
                });
            }
        }
    }

    for (pid, sm) in &server_map {
        // Server-only mods don't belong on the client, skip
        if sm.side == Side::ServerOnly {
            continue;
        }
        if !client_map.contains_key(pid) {
            records.push(DiscrepancyRecord {
                project_id: pid.to_string(),
                project_name: sm.project_name.clone(),
                kind: DiscrepancyKind::ServerOnly,
                client_mod: None,
                server_mod: Some((*sm).clone()),
            });
        }
    }

    records.sort_by(|a, b| a.project_name.to_lowercase().cmp(&b.project_name.to_lowercase()));
    records
}

pub async fn resolve_discrepancies(
    sftp: &SftpClient,
    selected: &[DiscrepancyRecord],
    remote_folder: &str,
    progress: &Progress,
) -> Result<()> {
    for record in selected {
        match record.kind {
            DiscrepancyKind::Mismatch | DiscrepancyKind::ClientOnly => {
                let cm = record.client_mod.as_ref().unwrap();
                let local_path = &cm.local_mod.filepath;
                let remote_path = format!("{remote_folder}/{}", cm.local_mod.filename);

                if record.kind == DiscrepancyKind::Mismatch {
                    let sm = record.server_mod.as_ref().unwrap();
                    let old_remote = format!("{remote_folder}/{}", sm.local_mod.filename);
                    let _ = sftp.delete_remote_file(&old_remote).await;
                }

                progress.set(format!("Uploading {}", cm.local_mod.filename));
                sftp.upload_file(local_path, &remote_path).await?;
            }
            DiscrepancyKind::ServerOnly => {
                let sm = record.server_mod.as_ref().unwrap();
                let remote_path = format!("{remote_folder}/{}", sm.local_mod.filename);
                progress.set(format!("Deleting {}", sm.local_mod.filename));
                sftp.delete_remote_file(&remote_path).await?;
            }
        }
    }
    Ok(())
}

/// Update mods directly on the server (works for server-only mods too):
/// download the new jar and upload it via SFTP, then delete the old one.
pub async fn update_server_mods(
    client: &Client,
    sftp: &SftpClient,
    updates: &[ModInfo],
    remote_folder: &str,
    progress: &Progress,
) -> Result<()> {
    for info in updates {
        let latest = match &info.latest_version {
            Some(v) => v,
            None => continue,
        };
        progress.set(format!("Downloading {}", latest.filename));
        let data = download_mod_bytes(client, &latest.download_url).await?;

        let new_remote = format!("{remote_folder}/{}", latest.filename);
        progress.set(format!("Uploading {}", latest.filename));
        sftp.upload_bytes(&data, &new_remote).await?;

        if info.local_mod.filename != latest.filename {
            let old_remote = format!("{remote_folder}/{}", info.local_mod.filename);
            let _ = sftp.delete_remote_file(&old_remote).await;
        }
    }
    Ok(())
}
