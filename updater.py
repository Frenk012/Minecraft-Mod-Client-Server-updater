import os
import requests
from pathlib import Path
from typing import Optional
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, DownloadColumn, TimeRemainingColumn

import modrinth
import curseforge as cf
from models import LocalMod, ModInfo, ModVersion, UnknownMod


def identify_and_check_updates(
    local_mods: list,
    minecraft_version: str,
    loader: str,
    curseforge_api_key: Optional[str] = None,
    progress: Optional[Progress] = None,
) -> tuple:
    if not local_mods:
        return [], []

    # ── Batch lookups ──────────────────────────────────────────────────────────
    sha512_hashes = [m.sha512 for m in local_mods]
    modrinth_results = modrinth.get_versions_by_hash(sha512_hashes)

    cf_results = {}
    if curseforge_api_key:
        fingerprints = [m.murmur2 for m in local_mods if m.murmur2 is not None]
        if fingerprints:
            cf_results = cf.get_versions_by_fingerprint(fingerprints, curseforge_api_key)

    # ── Identify each mod — CF fingerprint wins unconditionally ───────────────
    # Each entry: (LocalMod, cf_raw_or_None, mr_raw_or_None)
    unknown_mods = []
    identified = []

    for mod in local_mods:
        cf_match = cf_results.get(mod.murmur2) if (curseforge_api_key and mod.murmur2 is not None) else None
        mr_match = modrinth_results.get(mod.sha512)

        if cf_match is not None or mr_match is not None:
            identified.append((mod, cf_match, mr_match))
        else:
            unknown_mods.append(UnknownMod(local_mod=mod))

    if not identified:
        return [], unknown_mods

    # ── Batch-fetch metadata only for what will actually be used ──────────────
    # CF metadata: for all CF-matched mods
    cf_mod_ids = list({entry[1]["modId"] for entry in identified if entry[1] is not None})
    cf_mods_meta = {}
    if cf_mod_ids and curseforge_api_key:
        cf_mods_meta = cf.get_mods_info(cf_mod_ids, curseforge_api_key)

    # Modrinth metadata: only for mods with no CF match (Modrinth-only)
    mr_project_ids = list({
        entry[2]["project_id"]
        for entry in identified
        if entry[1] is None and entry[2] is not None
    })
    mr_projects = modrinth.get_projects(mr_project_ids) if mr_project_ids else {}

    # ── Build ModInfo with latest version check ────────────────────────────────
    mod_infos = []
    task = None
    if progress:
        task = progress.add_task("Checking for updates...", total=len(identified))

    for local_mod, cf_raw, mr_raw in identified:
        if cf_raw is not None:
            # CF identified: use CF for current version AND latest version check
            mod_id = cf_raw["modId"]
            mod_meta = cf_mods_meta.get(mod_id, {})
            current_version = cf.parse_file_object(cf_raw)
            latest_version = cf.get_latest_version(mod_id, minecraft_version, loader, curseforge_api_key)
            project_id = str(mod_id)
            project_slug = mod_meta.get("slug", project_id)
            project_name = mod_meta.get("name", project_id)
        else:
            # Modrinth-only: no CF fingerprint match, use Modrinth exclusively
            project_id = mr_raw["project_id"]
            project_data = mr_projects.get(project_id, {})
            current_version = modrinth.parse_version_object(mr_raw)
            latest_version = modrinth.get_latest_version(project_id, minecraft_version, loader)
            project_slug = project_data.get("slug", project_id)
            project_name = project_data.get("title", project_id)

        if latest_version and latest_version.version_id == current_version.version_id:
            latest_version = None

        mod_infos.append(ModInfo(
            project_id=project_id,
            project_slug=project_slug,
            project_name=project_name,
            current_version=current_version,
            latest_version=latest_version,
            local_mod=local_mod,
        ))

        if progress and task is not None:
            progress.advance(task)

    return mod_infos, unknown_mods


def download_mod(mod_info: ModInfo, destination_folder: str, progress: Progress, task_id) -> str:
    url = mod_info.latest_version.download_url
    filename = mod_info.latest_version.filename
    dest_path = str(Path(destination_folder) / filename)

    with requests.get(url, stream=True, timeout=60) as resp:
        resp.raise_for_status()
        total = int(resp.headers.get("content-length", 0))
        if task_id is not None:
            progress.update(task_id, total=total)
        downloaded = 0
        with open(dest_path, "wb") as f:
            for chunk in resp.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
                    downloaded += len(chunk)
                    if task_id is not None:
                        progress.advance(task_id, len(chunk))

    return dest_path


def apply_updates(selected_mods: list, destination_folder: str) -> list:
    results = []
    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        DownloadColumn(),
        TimeRemainingColumn(),
    ) as progress:
        for mod in selected_mods:
            task_id = progress.add_task(f"Downloading {mod.project_name}...", total=None)
            try:
                new_path = download_mod(mod, destination_folder, progress, task_id)
                old_path = mod.local_mod.filepath
                if os.path.exists(old_path):
                    os.remove(old_path)
                results.append((mod, new_path))
                progress.update(task_id, description=f"[green]✓ {mod.project_name}")
            except Exception as e:
                progress.update(task_id, description=f"[red]✗ {mod.project_name}: {e}")

    return results
