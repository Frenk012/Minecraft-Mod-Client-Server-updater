from dataclasses import dataclass, field
from typing import Optional


@dataclass
class AppConfig:
    mods_folder: str
    minecraft_version: str
    loader: str
    sftp_host: str
    sftp_port: int
    sftp_username: str
    sftp_password: Optional[str]
    sftp_key_path: Optional[str]
    remote_mods_folder: str


@dataclass
class LocalMod:
    filename: str
    filepath: str
    sha512: str


@dataclass
class ModVersion:
    version_id: str
    version_number: str
    filename: str
    download_url: str
    game_versions: list
    loaders: list


@dataclass
class ModInfo:
    project_id: str
    project_slug: str
    project_name: str
    current_version: ModVersion
    latest_version: Optional[ModVersion]
    local_mod: LocalMod


@dataclass
class UnknownMod:
    local_mod: LocalMod


@dataclass
class DiscrepancyRecord:
    project_name: str
    project_slug: str
    client_version: Optional[str]
    server_version: Optional[str]
    client_mod_info: Optional[ModInfo]
    server_mod_info: Optional[ModInfo]
    kind: str  # "mismatch" | "client_only" | "server_only"
