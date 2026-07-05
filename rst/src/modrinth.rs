use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use crate::models::{ModSource, ModVersion, Side};

const BASE_URL: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize)]
struct MrVersionFile {
    hashes: HashMap<String, String>,
    url: String,
    filename: String,
}

#[derive(Deserialize)]
struct MrVersion {
    id: String,
    version_number: String,
    files: Vec<MrVersionFile>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
}

#[derive(Deserialize)]
struct MrProject {
    id: String,
    title: String,
    client_side: String,
    server_side: String,
}

fn parse_side(client_side: &str, server_side: &str) -> Side {
    match (client_side, server_side) {
        (_, "unsupported") => Side::ClientOnly,
        ("unsupported", _) => Side::ServerOnly,
        _ => Side::Both,
    }
}

async fn request_with_retry(client: &Client, url: &str, body: Option<serde_json::Value>) -> Result<reqwest::Response> {
    loop {
        let req = if let Some(ref b) = body {
            client.post(url).json(b)
        } else {
            client.get(url).into()
        };
        let resp = req.send().await?;
        if resp.status().as_u16() == 429 {
            let retry_after = resp
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1);
            tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
            continue;
        }
        return Ok(resp);
    }
}

fn parse_version(v: MrVersion) -> Option<ModVersion> {
    let file = v.files.into_iter().find(|f| {
        f.hashes.contains_key("sha512")
    })?;
    Some(ModVersion {
        version_id: v.id,
        version_number: v.version_number,
        filename: file.filename,
        download_url: file.url,
        game_versions: v.game_versions,
        loaders: v.loaders,
        source: ModSource::Modrinth,
    })
}

/// Returns map from sha512 -> (project_id, ModVersion) with project_id filled.
pub async fn get_versions_by_hash_full(
    client: &Client,
    hashes: &[String],
) -> Result<HashMap<String, (String, ModVersion)>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let url = format!("{BASE_URL}/version_files");
    let body = json!({ "hashes": hashes, "algorithm": "sha512" });
    let resp = request_with_retry(client, &url, Some(body)).await?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }

    #[derive(Deserialize)]
    struct MrVersionFull {
        id: String,
        project_id: String,
        version_number: String,
        files: Vec<MrVersionFile>,
        game_versions: Vec<String>,
        loaders: Vec<String>,
    }

    let data: HashMap<String, MrVersionFull> = resp.json().await?;
    let mut result = HashMap::new();
    for (hash, v) in data {
        let file = v.files.into_iter().find(|f| f.hashes.contains_key("sha512"));
        if let Some(file) = file {
            let mv = ModVersion {
                version_id: v.id,
                version_number: v.version_number,
                filename: file.filename,
                download_url: file.url,
                game_versions: v.game_versions,
                loaders: v.loaders,
                source: ModSource::Modrinth,
            };
            result.insert(hash, (v.project_id, mv));
        }
    }
    Ok(result)
}

/// Returns map from project_id -> (name, Side).
pub async fn get_projects(
    client: &Client,
    project_ids: &[String],
) -> Result<HashMap<String, (String, Side)>> {
    if project_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids_json = serde_json::to_string(project_ids)?;
    let url = format!("{BASE_URL}/projects?ids={ids_json}");
    let resp = request_with_retry(client, &url, None).await?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }
    let projects: Vec<MrProject> = resp.json().await?;
    Ok(projects
        .into_iter()
        .map(|p| {
            let side = parse_side(&p.client_side, &p.server_side);
            (p.id, (p.title, side))
        })
        .collect())
}

/// Returns the latest compatible version for a project.
pub async fn get_latest_version(
    client: &Client,
    project_id: &str,
    mc_version: &str,
    loader: &str,
) -> Result<Option<ModVersion>> {
    let url = format!(
        "{BASE_URL}/project/{project_id}/version?game_versions=[\"{mc_version}\"]&loaders=[\"{loader}\"]"
    );
    let resp = request_with_retry(client, &url, None).await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let versions: Vec<MrVersion> = resp.json().await.unwrap_or_default();
    Ok(versions.into_iter().next().and_then(parse_version))
}
