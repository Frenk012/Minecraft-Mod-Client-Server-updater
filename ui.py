import questionary
from rich.console import Console
from rich.table import Table
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, DownloadColumn, TimeRemainingColumn

from models import ModInfo, UnknownMod, DiscrepancyRecord

console = Console()


def show_update_table(mod_infos: list, unknown_mods: list) -> None:
    table = Table(title="Installed Mods", show_lines=True)
    table.add_column("Mod Name", style="bold")
    table.add_column("Current Version")
    table.add_column("Latest Version")
    table.add_column("Status")

    for mod in mod_infos:
        current = mod.current_version.version_number
        if mod.latest_version:
            latest = mod.latest_version.version_number
            status = "[yellow]Update available[/yellow]"
        else:
            latest = "[dim]—[/dim]"
            status = "[green]Up to date[/green]"
        table.add_row(mod.project_name, current, latest, status)

    for u in unknown_mods:
        table.add_row(
            u.local_mod.filename,
            "[dim]unknown[/dim]",
            "[dim]—[/dim]",
            "[dim]Not on Modrinth[/dim]",
        )

    console.print(table)


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

    selected = questionary.checkbox(
        "Select mods to update (space to toggle, enter to confirm):",
        choices=choices,
    ).ask()

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

    selected = questionary.checkbox(
        "Select discrepancies to resolve (pushes client version to server):",
        choices=choices,
    ).ask()

    return selected or []


def make_progress() -> Progress:
    return Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        DownloadColumn(),
        TimeRemainingColumn(),
    )
