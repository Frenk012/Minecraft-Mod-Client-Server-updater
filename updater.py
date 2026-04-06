import os
import requests
from pathlib import Path
from typing import Optional
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, DownloadColumn, TimeRemainingColumn

import modrinth
from models import LocalMod, ModInfo, ModVersion, UnknownMod


def identify_and_check_updates(
    local_mods: list,
    minecraft_version: str,
    loader: str,
    progress: Optional[Progress] = None,
) -> tuple:
    if not local_mods:
        return [], []

    hashes = [m.sha512 for m in local_mods]
    hash_to_mod = {m.sha512: m for m in local_mods}

    hash_results = modrinth.get_versions_by_hash(hashes)

    identified = {}   # sha512 -> (LocalMod, raw_version)
    unknown_mods = []

    for mod in local_mods:
        if mod.sha512 in hash_results:
            identified[mod.sha512] = (mod, hash_results[mod.sha512])
        else:
            unknown_mods.append(UnknownMod(local_mod=mod))

    if not identified:
        return [], unknown_mods

    project_ids = list({raw["project_id"] for _, raw in identified.values()})
    projects = modrinth.get_projects(project_ids)

    mod_infos = []
    items = list(identified.values())

    task = None
    if progress:
        task = progress.add_task("Checking for updates...", total=len(items))

    for local_mod, raw_version in items:
        project_id = raw_version["project_id"]
        project_data = projects.get(project_id, {})
        current_version = modrinth.parse_version_object(raw_version)

        latest_version = modrinth.get_latest_version(project_id, minecraft_version, loader)

        if latest_version and latest_version.version_id == current_version.version_id:
            latest_version = None

        mod_infos.append(ModInfo(
            project_id=project_id,
            project_slug=project_data.get("slug", project_id),
            project_name=project_data.get("title", project_id),
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
