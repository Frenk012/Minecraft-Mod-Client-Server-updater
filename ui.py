import questionary
from prompt_toolkit.output.win32 import NoConsoleScreenBufferError
from rich.console import Console
from rich.table import Table
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, DownloadColumn, TimeRemainingColumn

from models import ModInfo, UnknownMod, DiscrepancyRecord

console = Console()


_SOURCE_LABEL = {
    "curseforge": "[orange3]CurseForge[/orange3]",
    "modrinth": "[green]Modrinth[/green]",
}


def show_update_table(mod_infos: list, unknown_mods: list) -> None:
    table = Table(title="Installed Mods", show_lines=True)
    table.add_column("Mod Name", style="bold")
    table.add_column("Source")
    table.add_column("Current Version")
    table.add_column("Latest Version")
    table.add_column("Status")

    for mod in mod_infos:
        source_label = _SOURCE_LABEL.get(mod.current_version.source, mod.current_version.source)
        current = mod.current_version.version_number
        if mod.latest_version:
            latest = mod.latest_version.version_number
            status = "[yellow]Update available[/yellow]"
        else:
            latest = "[dim]—[/dim]"
            status = "[green]Up to date[/green]"
        table.add_row(mod.project_name, source_label, current, latest, status)

    for u in unknown_mods:
        table.add_row(
            u.local_mod.filename,
            "[dim]—[/dim]",
            "[dim]unknown[/dim]",
            "[dim]—[/dim]",
            "[dim]Not identified[/dim]",
        )

    console.print(table)


def confirm(message: str, default: bool = True) -> bool:
    try:
        return bool(questionary.confirm(message, default=default).ask())
    except NoConsoleScreenBufferError:
        console.print(f"[yellow]No interactive console — assuming '{default}' for: {message}[/yellow]")
        return default


def prompt_select_updates(updatable: list) -> list:
    if not updatable:
        return []

    choices = [
        questionary.Choice(
            title=f"{m.project_name}  {m.current_version.version_number} → {m.latest_version.version_number}",
            value=m,
            checked=True,
        )
        for m in updatable
    ]

    try:
        selected = questionary.checkbox(
            "Select mods to update (space to toggle, enter to confirm):",
            choices=choices,
        ).ask()
    except NoConsoleScreenBufferError:
        console.print("[yellow]No interactive console detected — run from a real terminal (not IDE redirected output). Selecting all updates.[/yellow]")
        return updatable

    return selected or []


def show_discrepancy_table(discrepancies: list) -> None:
    table = Table(title="Client / Server Discrepancies", show_lines=True)
    table.add_column("Mod Name", style="bold")
    table.add_column("Client Version")
    table.add_column("Server Version")
    table.add_column("Kind")

    kind_colors = {
        "mismatch": "cyan",
        "client_only": "yellow",
        "server_only": "red",
    }

    for d in discrepancies:
        color = kind_colors.get(d.kind, "white")
        table.add_row(
            d.project_name,
            d.client_version or "[dim]MISSING[/dim]",
            d.server_version or "[dim]MISSING[/dim]",
            f"[{color}]{d.kind}[/{color}]",
        )

    console.print(table)


def prompt_select_discrepancies(discrepancies: list) -> list:
    if not discrepancies:
        return []

    choices = [
        questionary.Choice(
            title=(
                f"{d.project_name}  "
                f"(client: {d.client_version or 'MISSING'} | "
                f"server: {d.server_version or 'MISSING'})  [{d.kind}]"
            ),
            value=d,
            checked=True,
        )
        for d in discrepancies
    ]

    try:
        selected = questionary.checkbox(
            "Select discrepancies to resolve (pushes client version to server):",
            choices=choices,
        ).ask()
    except NoConsoleScreenBufferError:
        console.print("[yellow]No interactive console detected — run from a real terminal. Selecting all.[/yellow]")
        return discrepancies

    return selected or []


def make_progress() -> Progress:
    return Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        DownloadColumn(),
        TimeRemainingColumn(),
    )
