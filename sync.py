from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, TransferSpeedColumn

import local_mods as lm
from models import LocalMod, ModInfo, DiscrepancyRecord
from sftp_client import SFTPClient


def scan_remote_mods(sftp: SFTPClient, remote_folder: str) -> list:
    jar_names = sftp.list_remote_jars(remote_folder)
    result = []

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
    ) as progress:
        task = progress.add_task("Scanning remote mods...", total=len(jar_names))
        for name in jar_names:
            remote_path = remote_folder.rstrip("/") + "/" + name
            try:
                data = sftp.read_remote_file_bytes(remote_path)
                sha = lm.hash_bytes(data)
                result.append(LocalMod(filename=name, filepath=remote_path, sha512=sha))
            except Exception as e:
                progress.print(f"[yellow]Warning:[/yellow] Could not read remote file '{name}': {e}")
            finally:
                progress.advance(task)

    return result


def compare_mod_sets(client_mods: list, server_mods: list) -> list:
    client_by_id = {m.project_id: m for m in client_mods}
    server_by_id = {m.project_id: m for m in server_mods}

    all_ids = set(client_by_id) | set(server_by_id)
    discrepancies = []

    for pid in all_ids:
        c = client_by_id.get(pid)
        s = server_by_id.get(pid)

        if c and s:
            if c.current_version.version_id == s.current_version.version_id:
                continue  # in sync
            discrepancies.append(DiscrepancyRecord(
                project_name=c.project_name,
                project_slug=c.project_slug,
                client_version=c.current_version.version_number,
                server_version=s.current_version.version_number,
                client_mod_info=c,
                server_mod_info=s,
                kind="mismatch",
            ))
        elif c and not s:
            discrepancies.append(DiscrepancyRecord(
                project_name=c.project_name,
                project_slug=c.project_slug,
                client_version=c.current_version.version_number,
                server_version=None,
                client_mod_info=c,
                server_mod_info=None,
                kind="client_only",
            ))
        elif s and not c:
            discrepancies.append(DiscrepancyRecord(
                project_name=s.project_name,
                project_slug=s.project_slug,
                client_version=None,
                server_version=s.current_version.version_number,
                client_mod_info=None,
                server_mod_info=s,
                kind="server_only",
            ))

    return discrepancies


def resolve_discrepancies(
    selected: list,
    sftp: SFTPClient,
    local_folder: str,
    remote_folder: str,
) -> None:
    remote_folder = remote_folder.rstrip("/")

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        TransferSpeedColumn(),
    ) as progress:
        for disc in selected:
            if disc.kind in ("mismatch", "client_only"):
                client_info = disc.client_mod_info
                local_path = client_info.local_mod.filepath
                remote_path = remote_folder + "/" + client_info.local_mod.filename
                task = progress.add_task(f"Uploading {disc.project_name}...", total=None)

                def make_callback(t):
                    def cb(transferred, total):
                        progress.update(t, total=total, completed=transferred)
                    return cb

                try:
                    # Delete old server file if it exists (mismatch case)
                    if disc.kind == "mismatch" and disc.server_mod_info:
                        old_remote = disc.server_mod_info.local_mod.filepath
                        try:
                            sftp.delete_remote_file(old_remote)
                        except Exception:
                            pass

                    sftp.upload_file(local_path, remote_path, progress_callback=make_callback(task))
                    progress.update(task, description=f"[green]✓ {disc.project_name}")
                except Exception as e:
                    progress.update(task, description=f"[red]✗ {disc.project_name}: {e}")

            elif disc.kind == "server_only":
                # Caller must have already confirmed deletion
                server_info = disc.server_mod_info
                task = progress.add_task(f"Deleting {disc.project_name} from server...", total=1)
                try:
                    sftp.delete_remote_file(server_info.local_mod.filepath)
                    progress.update(task, completed=1, description=f"[green]✓ Deleted {disc.project_name}")
                except Exception as e:
                    progress.update(task, description=f"[red]✗ {disc.project_name}: {e}")
