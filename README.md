# Mods Updater

A lightweight CLI tool to keep Minecraft mods up to date on both your local client and a remote server.

It uses the [Modrinth API](https://docs.modrinth.com/) to identify installed mods by file hash, checks for newer compatible versions, and syncs selected updates to your server over SFTP — all from a single interactive terminal session.

## Features

- Identifies mods by SHA-512 hash (no manual mod ID configuration needed)
- Checks for updates against Modrinth filtered by your Minecraft version and loader
- Interactive multi-select for choosing which mods to update
- Downloads new JARs and deletes old ones automatically
- Connects to your server via SFTP and compares mod versions
- Shows discrepancies (mismatches, client-only, server-only mods)
- Syncs selected changes to the server

## Requirements

- Python 3.11+
- A Modrinth-hosted modpack (mods not on Modrinth are shown as "Unknown" and skipped)

## Installation

```bash
# Clone or download the project, then install dependencies
pip install -r requirements.txt
```

## Configuration

Copy the example config and fill in your details:

```bash
cp config.example.toml config.toml
```

Edit `config.toml`:

```toml
[local]
mods_folder = "C:/Users/YourName/AppData/Roaming/.minecraft/mods"
minecraft_version = "1.21.1"
loader = "fabric"   # fabric | forge | neoforge

[sftp]
host = "play.example.com"
port = 22
username = "mcadmin"
password = "your-password"   # or use key_path instead
key_path = ""
remote_mods_folder = "/home/mcadmin/server/mods"
```

> Use either `password` or `key_path`, not both.

## Usage

```bash
python main.py
```

The tool runs in two phases:

**Phase 1 — Local update**
1. Scans your local mods folder
2. Queries Modrinth to identify each mod and check for updates
3. Shows a table of all mods with their current and latest versions
4. Prompts you to select which mods to update
5. Downloads new JARs and removes old ones

**Phase 2 — Server sync**
1. Asks whether to connect to your SFTP server
2. Scans the remote mods folder and identifies versions
3. Compares client vs server and shows any discrepancies
4. Prompts you to select which discrepancies to resolve
5. Uploads updated JARs to the server and removes outdated ones

## Notes

- Mods not found on Modrinth (private/custom mods) are displayed but never modified
- Server-only mods require explicit confirmation before deletion
- Requires Python 3.11+ for the built-in `tomllib` TOML parser
