import sys
import socket
import paramiko
import questionary
from rich.console import Console
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, TextColumn

import config as cfg
import local_mods
import updater
import sync as sync_module
import ui
from sftp_client import SFTPClient

console = Console()


def main() -> None:
    console.print(Panel.fit(
        "[bold cyan]Minecraft Mod Updater[/bold cyan]\n"
        "[dim]Powered by Modrinth API[/dim]",
        border_style="cyan",
    ))

    # ── Load config ──────────────────────────────────────────────────────────
    config = cfg.load_config("config.toml")
    console.print(f"[dim]Minecraft {config.minecraft_version} · {config.loader}[/dim]\n")

    # ── Phase 1: Local Update ─────────────────────────────────────────────────
    console.rule("[bold]Phase 1 — Local Mods[/bold]")

    try:
        local_mod_list = local_mods.scan_local_folder(config.mods_folder)
    except FileNotFoundError as e:
        console.print(f"[red]Error:[/red] {e}")
        sys.exit(1)

    console.print(f"Found [bold]{len(local_mod_list)}[/bold] mod(s) in local folder.\n")

    if not local_mod_list:
        console.print("[yellow]No mods found. Skipping update check.[/yellow]")
        mod_infos, unknown_mods = [], []
    else:
        with Progress(SpinnerColumn(), TextColumn("{task.description}")) as progress:
            task = progress.add_task("Querying Modrinth API...", total=None)
            mod_infos, unknown_mods = updater.identify_and_check_updates(
                local_mod_list,
                config.minecraft_version,
                config.loader,
                progress=progress,
            )

    ui.show_update_table(mod_infos, unknown_mods)

    updatable = [m for m in mod_infos if m.latest_version is not None]

    if not updatable:
        console.print("\n[green]All identified mods are up to date.[/green]")
    else:
        console.print(f"\n[yellow]{len(updatable)}[/yellow] update(s) available.\n")
        selected_updates = ui.prompt_select_updates(updatable)

        if not selected_updates:
            console.print("[dim]No updates selected.[/dim]")
        else:
            console.print()
            updated = updater.apply_updates(selected_updates, config.mods_folder)
            console.print(f"\n[green]✓ Updated {len(updated)} mod(s) successfully.[/green]")

    # ── Phase 2: Server Sync ──────────────────────────────────────────────────
    console.print()
    console.rule("[bold]Phase 2 — Server Sync[/bold]")

    if not questionary.confirm("Connect to SFTP server to sync mods?", default=True).ask():
        console.print("[dim]Skipping server sync.[/dim]")
        return

    try:
        with SFTPClient(config) as sftp:
            console.print(f"\nConnected to [bold]{config.sftp_host}[/bold].\n")

            # Scan remote mods
            remote_local_mods = sync_module.scan_remote_mods(sftp, config.remote_mods_folder)
            console.print(f"Found [bold]{len(remote_local_mods)}[/bold] mod(s) on server.\n")

            # Identify server mods via Modrinth
            with Progress(SpinnerColumn(), TextColumn("{task.description}")) as progress:
                progress.add_task("Identifying server mods...", total=None)
                server_mod_infos, server_unknown = updater.identify_and_check_updates(
                    remote_local_mods,
                    config.minecraft_version,
                    config.loader,
                    progress=progress,
                )

            # Re-scan client mods (may have changed after Phase 1 updates)
            fresh_local = local_mods.scan_local_folder(config.mods_folder)
            with Progress(SpinnerColumn(), TextColumn("{task.description}")) as progress:
                progress.add_task("Re-scanning client mods...", total=None)
                fresh_client_infos, _ = updater.identify_and_check_updates(
                    fresh_local,
                    config.minecraft_version,
                    config.loader,
                    progress=progress,
                )

            # Compare
            discrepancies = sync_module.compare_mod_sets(fresh_client_infos, server_mod_infos)

            if not discrepancies:
                console.print("[green]Client and server are in sync.[/green]")
                return

            ui.show_discrepancy_table(discrepancies)

            # Separate server-only mods for explicit confirmation
            server_only = [d for d in discrepancies if d.kind == "server_only"]
            actionable = [d for d in discrepancies if d.kind != "server_only"]

            if server_only:
                console.print(
                    f"\n[red]{len(server_only)}[/red] mod(s) exist only on the server. "
                    "Include them for deletion?"
                )
                if questionary.confirm("Delete server-only mods?", default=False).ask():
                    actionable.extend(server_only)

            selected_disc = ui.prompt_select_discrepancies(actionable)

            if not selected_disc:
                console.print("[dim]No changes selected.[/dim]")
                return

            console.print()
            sync_module.resolve_discrepancies(
                selected_disc, sftp, config.mods_folder, config.remote_mods_folder
            )
            console.print(f"\n[green]✓ Server sync complete. {len(selected_disc)} item(s) resolved.[/green]")

    except paramiko.AuthenticationException:
        console.print("[red]SFTP Error:[/red] Authentication failed. Check username/password or key_path in config.toml.")
        sys.exit(1)
    except paramiko.SSHException as e:
        console.print(f"[red]SFTP Error:[/red] SSH connection failed: {e}")
        sys.exit(1)
    except socket.gaierror as e:
        console.print(f"[red]SFTP Error:[/red] Cannot reach host '{config.sftp_host}': {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
