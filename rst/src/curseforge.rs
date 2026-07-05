use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use crate::models::{ModSource, ModVersion};

const BASE_URL: &str = "https://api.curseforge.com/v1";

fn loader_id(loader: &str) -> u32 {
    match loader {
        "forge" => 1,
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 0,
    }
}

#[derive(Deserialize)]
struct CfFile {
    id: u32,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "gameVersions")]
    game_versions: Vec<String>,
    #[serde(rename = "fileFingerprint", default)]
    file_fingerprint: u64,
}

#[derive(Deserialize)]
struct CfMod {
    id: u32,
    name: String,
}

#[derive(Deserialize)]
struct CfFingerprintMatch {
    id: u32,
    file: CfFile,
}

#[derive(Deserialize)]
struct CfFingerprintData {
    #[serde(rename = "exactMatches")]
    exact_matches: Vec<CfFingerprintMatch>,
}

#[derive(Deserialize)]
struct CfResponse<T> {
    data: T,
}

async fn request_with_retry(client: &Client, api_key: &str, url: &str, body: Option<serde_json::Value>) -> Result<reqwest::Response> {
    loop {
        let req = if let Some(ref b) = body {
            client.post(url).header("x-api-key", api_key).json(b)
        } else {
            client.get(url).header("x-api-key", api_key)
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

fn parse_cf_file(file: CfFile, _mod_id: u32, _loader: &str) -> Option<ModVersion> {
    let url = file.download_url?;
    let loaders: Vec<String> = file
        .game_versions
        .iter()
        .filter(|v| !v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .map(|v| v.to_lowercase())
        .collect();
    let game_versions: Vec<String> = file
        .game_versions
        .iter()
        .filter(|v| v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .cloned()
        .collect();

    Some(ModVersion {
        version_id: file.id.to_string(),
        version_number: file.display_name,
        filename: file.file_name,
        download_url: url,
        game_versions,
        loaders,
        source: ModSource::CurseForge,
    })
}

/// Returns map from murmur2_fingerprint -> (project_id as string, ModVersion).
pub async fn get_versions_by_fingerprint(
    client: &Client,
    api_key: &str,
    fingerprints: &[u32],
    loader: &str,
) -> Result<HashMap<u32, (String, ModVersion)>> {
    if fingerprints.is_empty() {
        return Ok(HashMap::new());
    }
    let url = format!("{BASE_URL}/fingerprints/432");
    let body = json!({ "fingerprints": fingerprints });
    let resp = request_with_retry(client, api_key, &url, Some(body)).await?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }
    let data: CfResponse<CfFingerprintData> = resp.json().await?;
    let mut result = HashMap::new();
    for m in data.data.exact_matches {
        let fingerprint = m.file.file_fingerprint as u32;
        if let Some(mv) = parse_cf_file(m.file, m.id, loader) {
            result.insert(fingerprint, (m.id.to_string(), mv));
        }
    }
    Ok(result)
}

/// Returns map from project_id -> project_name.
pub async fn get_mods_info(
    client: &Client,
    api_key: &str,
    mod_ids: &[u32],
) -> Result<HashMap<u32, String>> {
    if mod_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let url = format!("{BASE_URL}/mods");
    let body = json!({ "modIds": mod_ids });
    let resp = request_with_retry(client, api_key, &url, Some(body)).await?;
    if !resp.status().is_success() {
        return Ok(HashMap::new());
    }
    let data: CfResponse<Vec<CfMod>> = resp.json().await?;
    Ok(data.data.into_iter().map(|m| (m.id, m.name)).collect())
}

/// Returns the latest compatible version for a project.
pub async fn get_latest_version(
    client: &Client,
    api_key: &str,
    mod_id: u32,
    mc_version: &str,
    loader: &str,
) -> Result<Option<ModVersion>> {
    let loader_id = loader_id(loader);
    let url = format!(
        "{BASE_URL}/mods/{mod_id}/files?gameVersion={mc_version}&modLoaderType={loader_id}&pageSize=1"
    );
    let resp = request_with_retry(client, api_key, &url, None).await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    #[derive(Deserialize)]
    struct CfFilesResponse {
        data: Vec<CfFile>,
    }
    let data: CfFilesResponse = resp.json().await.unwrap_or(CfFilesResponse { data: vec![] });
    Ok(data.data.into_iter().next().and_then(|f| parse_cf_file(f, mod_id, loader)))
}
