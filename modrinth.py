import json
import time
import requests
from typing import Optional
from models import ModVersion

BASE_URL = "https://api.modrinth.com/v2"
HEADERS = {"User-Agent": "mods-updater/1.0 (personal-tool)"}


def _request_with_retry(method: str, url: str, **kwargs) -> requests.Response:
    resp = requests.request(method, url, headers=HEADERS, **kwargs)
    if resp.status_code == 429:
        wait = int(resp.headers.get("X-Ratelimit-Reset", 60))
        time.sleep(wait)
        resp = requests.request(method, url, headers=HEADERS, **kwargs)
    resp.raise_for_status()
    return resp


def parse_version_object(raw: dict) -> ModVersion:
    files = raw.get("files", [])
    primary = next((f for f in files if f.get("primary")), files[0] if files else {})
    return ModVersion(
        version_id=raw["id"],
        version_number=raw["version_number"],
        filename=primary.get("filename", ""),
        download_url=primary.get("url", ""),
        game_versions=raw.get("game_versions", []),
        loaders=raw.get("loaders", []),
    )


def get_versions_by_hash(sha512_hashes: list) -> dict:
    if not sha512_hashes:
        return {}
    resp = _request_with_retry(
        "POST",
        f"{BASE_URL}/version_files",
        json={"hashes": sha512_hashes, "algorithm": "sha512"},
    )
    return resp.json()


def get_projects(project_ids: list) -> dict:
    if not project_ids:
        return {}
    resp = _request_with_retry(
        "GET",
        f"{BASE_URL}/projects",
        params={"ids": json.dumps(project_ids)},
    )
    return {p["id"]: p for p in resp.json()}


def get_latest_version(project_id: str, minecraft_version: str, loader: str) -> Optional[ModVersion]:
    try:
        resp = _request_with_retry(
            "GET",
            f"{BASE_URL}/project/{project_id}/version",
            params={
                "game_versions": json.dumps([minecraft_version]),
                "loaders": json.dumps([loader]),
            },
        )
        versions = resp.json()
        if not versions:
            return None
        return parse_version_object(versions[0])
    except requests.HTTPError:
        return None
