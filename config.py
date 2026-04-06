import sys
import tomllib
from models import AppConfig


def load_config(path: str = "config.toml") -> AppConfig:
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
    except FileNotFoundError:
        print(f"[red]Config error:[/red] '{path}' not found. Copy config.example.toml to config.toml and fill it in.")
        sys.exit(1)
    except tomllib.TOMLDecodeError as e:
        print(f"Config error: invalid TOML in '{path}': {e}")
        sys.exit(1)

    try:
        local = data["local"]
        sftp = data["sftp"]

        mods_folder = local["mods_folder"]
        minecraft_version = local["minecraft_version"]
        loader = local["loader"].lower()

        host = sftp["host"]
        port = int(sftp.get("port", 22))
        username = sftp["username"]
        password = sftp.get("password") or None
        key_path = sftp.get("key_path") or None
        remote_mods_folder = sftp["remote_mods_folder"]
    except KeyError as e:
        print(f"Config error: missing required key {e} in '{path}'.")
        sys.exit(1)

    if not password and not key_path:
        print("Config error: SFTP requires either 'password' or 'key_path' in [sftp] section.")
        sys.exit(1)

    return AppConfig(
        mods_folder=mods_folder,
        minecraft_version=minecraft_version,
        loader=loader,
        sftp_host=host,
        sftp_port=port,
        sftp_username=username,
        sftp_password=password,
        sftp_key_path=key_path,
        remote_mods_folder=remote_mods_folder,
    )
