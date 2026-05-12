import time
import requests
from typing import Optional
from models import ModVersion

BASE_URL = "https://api.curseforge.com/v1"

_LOADER_TYPE = {
    "forge": 1,
    "fabric": 4,
    "quilt": 5,
    "neoforge": 6,
}

_LOADER_NAME = {v: k for k, v in _LOADER_TYPE.items()}


def _headers(api_key: str) -> dict:
    return {
        "x-api-key": api_key,
        "Accept": "application/json",
    }


def _request_with_retry(method: str, url: str, api_key: str, **kwargs) -> requests.Response:
    resp = requests.request(method, url, headers=_headers(api_key), **kwargs)
    if resp.status_code == 429:
        time.sleep(int(resp.headers.get("Retry-After", 60)))
        resp = requests.request(method, url, headers=_headers(api_key), **kwargs)
    resp.raise_for_status()
    return resp


def parse_file_object(raw: dict) -> ModVersion:
    loader_type = raw.get("modLoaderType", 0)
    loader_name = _LOADER_NAME.get(loader_type, str(loader_type))
    return ModVersion(
        version_id=str(raw["id"]),
        version_number=raw.get("displayName", str(raw["id"])),
        filename=raw.get("fileName", ""),
        download_url=raw.get("downloadUrl") or "",
        game_versions=raw.get("gameVersions", []),
        loaders=[loader_name],
        source="curseforge",
    )


def get_versions_by_fingerprint(fingerprints: list, api_key: str) -> dict:
    """Return dict mapping fingerprint (int) → file dict for exact matches."""
    if not fingerprints:
        return {}
    resp = _request_with_retry(
        "POST",
        f"{BASE_URL}/fingerprints",
        api_key,
        json={"fingerprints": fingerprints},
    )
    data = resp.json().get("data", {})
    result = {}
    for match in data.get("exactMatches", []):
        fp = match.get("id")
        file_data = match.get("file")
        if fp is not None and file_data:
            result[fp] = file_data
    return result


def get_mod_info(mod_id: int, api_key: str) -> dict:
    try:
        resp = _request_with_retry("GET", f"{BASE_URL}/mods/{mod_id}", api_key)
        return resp.json().get("data", {})
    except requests.HTTPError:
        return {}


def get_mods_info(mod_ids: list, api_key: str) -> dict:
    """Batch fetch mod metadata. Returns dict mod_id (int) → mod dict."""
    if not mod_ids:
        return {}
    try:
        resp = _request_with_retry(
            "POST",
            f"{BASE_URL}/mods",
            api_key,
            json={"modIds": mod_ids},
        )
        return {m["id"]: m for m in resp.json().get("data", [])}
    except requests.HTTPError:
        return {}


def get_latest_version(mod_id: int, minecraft_version: str, loader: str, api_key: str) -> Optional[ModVersion]:
    loader_type = _LOADER_TYPE.get(loader.lower(), 0)
    try:
        resp = _request_with_retry(
            "GET",
            f"{BASE_URL}/mods/{mod_id}/files",
            api_key,
            params={
                "gameVersion": minecraft_version,
                "modLoaderType": loader_type,
                "pageSize": 1,
                "sortOrder": "desc",
            },
        )
        files = resp.json().get("data", [])
        if not files:
            return None
        return parse_file_object(files[0])
    except requests.HTTPError:
        return None
