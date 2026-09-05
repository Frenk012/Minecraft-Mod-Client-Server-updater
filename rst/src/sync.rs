use anyhow::{Context, Result};
use futures_util::{stream, StreamExt};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    let done = AtomicUsize::new(0);
    progress.set(format!("Reading remote mods 0/{total}"));

    // Several files in parallel (each already pipelined); hashing off the reactor.
    let mut mods = stream::iter(jars)
        .map(|filename| {
            let done = &done;
            async move {
                let remote_path = format!("{remote_folder}/{filename}");
                let bytes = sftp.read_remote_file_bytes(&remote_path).await?;
                let (sha512, murmur2) = tokio::task::spawn_blocking(move || {
                    (hash_bytes(&bytes), crate::hashing::murmur2(&bytes))
                })
                .await?;
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress.set(format!("Reading remote mods {n}/{total}"));
                anyhow::Ok(LocalMod {
                    filename,
                    filepath: Path::new(&remote_path).to_path_buf(),
                    sha512,
                    murmur2: Some(murmur2),
                })
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    mods.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(mods)
}

/// Compare a source mod set against a destination (always a server).
/// Files are compared by sha512 first (ground truth), then version_id —
/// same jar identified via different APIs must not count as a mismatch.
/// `dst_unknown_hashes`: sha512 of destination jars that could not be
/// identified — a source mod whose exact file is among them is NOT missing.
/// `skip_server_only_extras`: true when the source is the local client
/// (server-only mods legitimately absent from the client are not "extra").
pub fn compare_mod_sets(
    src_mods: &[ModInfo],
    dst_mods: &[ModInfo],
    dst_unknown_hashes: &HashSet<String>,
    skip_server_only_extras: bool,
) -> Vec<DiscrepancyRecord> {
    let mut dst_by_pid: HashMap<&str, Vec<&ModInfo>> = HashMap::new();
    for m in dst_mods {
        dst_by_pid.entry(m.project_id.as_str()).or_default().push(m);
    }
    let src_ids: HashSet<&str> = src_mods.iter().map(|m| m.project_id.as_str()).collect();

    let mut records = Vec::new();
    let dup_delete = |sm: &ModInfo, e: &ModInfo| DiscrepancyRecord {
        project_id: sm.project_id.clone(),
        project_name: format!("{} (duplicate: {})", sm.project_name, e.local_mod.filename),
        kind: DiscrepancyKind::ServerOnly,
        client_mod: None,
        server_mod: Some(e.clone()),
    };

    for sm in src_mods {
        // Client-only mods don't belong on a server, never push them
        if sm.side == Side::ClientOnly {
            continue;
        }
        match dst_by_pid.get(sm.project_id.as_str()) {
            Some(entries) => {
                let matched = entries.iter().find(|e| {
                    e.local_mod.sha512 == sm.local_mod.sha512
                        || e.current_version.version_id == sm.current_version.version_id
                });
                match matched {
                    Some(m) => {
                        // In sync; any other jar of the same project is a stale duplicate
                        for e in entries.iter().filter(|e| e.local_mod.filename != m.local_mod.filename) {
                            records.push(dup_delete(sm, e));
                        }
                    }
                    None => {
                        records.push(DiscrepancyRecord {
                            project_id: sm.project_id.clone(),
                            project_name: sm.project_name.clone(),
                            kind: DiscrepancyKind::Mismatch,
                            client_mod: Some(sm.clone()),
                            server_mod: Some(entries[0].clone()),
                        });
                        for e in &entries[1..] {
                            records.push(dup_delete(sm, e));
                        }
                    }
                }
            }
            None => {
                // File physically present but unidentified by the APIs: not missing
                if dst_unknown_hashes.contains(&sm.local_mod.sha512) {
                    continue;
                }
                records.push(DiscrepancyRecord {
                    project_id: sm.project_id.clone(),
                    project_name: sm.project_name.clone(),
                    kind: DiscrepancyKind::ClientOnly,
                    client_mod: Some(sm.clone()),
                    server_mod: None,
                });
            }
        }
    }

    for (pid, entries) in &dst_by_pid {
        if src_ids.contains(pid) {
            continue;
        }
        for e in entries {
            if skip_server_only_extras && e.side == Side::ServerOnly {
                continue;
            }
            records.push(DiscrepancyRecord {
                project_id: pid.to_string(),
                project_name: e.project_name.clone(),
                kind: DiscrepancyKind::ServerOnly,
                client_mod: None,
                server_mod: Some((*e).clone()),
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
                    if sm.local_mod.filename != cm.local_mod.filename {
                        let old_remote = format!("{remote_folder}/{}", sm.local_mod.filename);
                        sftp.delete_remote_file(&old_remote)
                            .await
                            .with_context(|| format!("Failed to delete old jar {old_remote}"))?;
                    }
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
            sftp.delete_remote_file(&old_remote)
                .await
                .with_context(|| format!("Failed to delete old jar {old_remote} — remove it manually"))?;
        }
    }
    Ok(())
}

/// Sync selected discrepancies from a source server to a destination server:
/// jars are streamed source → local memory → destination over SFTP.
/// In records, `client_mod` is the source-server mod, `server_mod` the destination one.
pub async fn sync_between_servers(
    src: &SftpClient,
    dst: &SftpClient,
    selected: &[DiscrepancyRecord],
    src_folder: &str,
    dst_folder: &str,
    progress: &Progress,
) -> Result<()> {
    for record in selected {
        match record.kind {
            DiscrepancyKind::Mismatch | DiscrepancyKind::ClientOnly => {
                let sm = record.client_mod.as_ref().unwrap();
                let name = &sm.local_mod.filename;
                progress.set(format!("Transferring {name}"));
                let data = src.read_remote_file_bytes(&format!("{src_folder}/{name}")).await
                    .with_context(|| format!("Failed to read {name} from source server"))?;
                dst.upload_bytes(&data, &format!("{dst_folder}/{name}")).await
                    .with_context(|| format!("Failed to upload {name} to destination server"))?;

                if record.kind == DiscrepancyKind::Mismatch {
                    let old = &record.server_mod.as_ref().unwrap().local_mod.filename;
                    if old != name {
                        let old_remote = format!("{dst_folder}/{old}");
                        dst.delete_remote_file(&old_remote)
                            .await
                            .with_context(|| format!("Failed to delete old jar {old_remote}"))?;
                    }
                }
            }
            DiscrepancyKind::ServerOnly => {
                let sm = record.server_mod.as_ref().unwrap();
                let remote_path = format!("{dst_folder}/{}", sm.local_mod.filename);
                progress.set(format!("Deleting {}", sm.local_mod.filename));
                dst.delete_remote_file(&remote_path)
                    .await
                    .with_context(|| format!("Failed to delete {remote_path}"))?;
            }
        }
    }
    Ok(())
}
