# mc-mod-updater

 A CLI tool to keep Minecraft mods in sync between your local client and a remote server, written in Rust.

Mods are identified automatically via SHA-512 hash (Modrinth) and Murmur2 fingerprint (CurseForge). No manual configuration per-mod required.

## Features

- Automatic mod identification without any per-mod setup
- Update detection filtered by Minecraft version and mod loader
- Interactive selection of which updates to apply
- Bidirectional sync between local client and remote server via SFTP
- Client-only and server-only mods are detected and excluded from sync automatically
- Detection of mismatches, client-only, and server-only discrepancies
- Parallel file hashing, API calls, and downloads for fast execution
- Progress bars for all long-running operations
- CurseForge takes priority over Modrinth when both recognize a mod

## Requirements

- Rust 1.75+
- No system libraries needed — SSH, SFTP and TLS are implemented in pure Rust

## Installation

```bash
git clone https://github.com/youruser/mc-mod-updater
cd mc-mod-updater
cargo build --release
```

Works on Linux, Windows and macOS — no external libraries required.

The binary will be at `target/release/mc-mod-updater`.

## Configuration

Copy the example config and fill in your details:

```bash
cp config.example.toml config.toml
```

```toml
[local]
mods_folder = "/home/user/.minecraft/mods"
minecraft_version = "1.21.1"
loader = "fabric"              # fabric | forge | quilt | neoforge

[curseforge]
# Free API keys: https://console.curseforge.com/
# Remove this section entirely to disable CurseForge lookups.
api_key = "your-key-here"

[sftp]
host = "your.server.host"
port = 22
username = "mcadmin"
password = "your_password"
remote_mods_folder = "/home/mcadmin/server/mods"
```

The tool looks for `config.toml` in the current working directory.

## Usage

```bash
./mc-mod-updater
```

### Phase 1 — Local updates

1. Scans the local `mods_folder` for `.jar` files
2. Identifies each mod via Modrinth (SHA-512) and optionally CurseForge (Murmur2)
3. Checks for newer versions compatible with your `minecraft_version` and `loader`
4. Shows a table of available updates
5. Prompts you to select which ones to apply; downloads and installs them

### Phase 2 — Server sync

1. Connects to the remote server via SFTP
2. Scans the remote `remote_mods_folder` and identifies mods the same way
3. Compares client and server mod sets, reporting:
   - **mismatch** — same mod, different version
   - **client only** — present locally, missing on server
   - **server only** — present on server, missing locally
4. Prompts you to select which discrepancies to resolve
5. Uploads/deletes files on the server accordingly

Mods that cannot be identified (custom or private) are skipped and left untouched.

Mods marked as client-only on Modrinth (e.g. shader mods, minimap mods) are automatically excluded from the server sync and vice versa, so they never show up as false discrepancies.

## Project structure

```
src/
├── main.rs        — entry point, phase 1 & 2 orchestration
├── config.rs      — TOML config loading
├── models.rs      — shared data structures
├── hashing.rs     — SHA-512 and Murmur2 (CurseForge fingerprint)
├── local_mods.rs  — local mod folder scanner
├── modrinth.rs    — Modrinth API client
├── curseforge.rs  — CurseForge API client
├── updater.rs     — mod identification and update downloads
├── sftp.rs        — SSH/SFTP client (russh, pure Rust)
├── sync.rs        — remote scan, diff, and conflict resolution
└── ui.rs          — tables, interactive prompts, progress bars
```

## Concurrency model

The tool uses two parallelism strategies built on Tokio:

- **`tokio::join!`** — fires multiple HTTP requests at the same time on the same thread; while one waits for a network response it yields to the others
- **`JoinSet::spawn`** — one async task per mod for `get_latest_version` calls; all run concurrently and results are collected as they arrive
- **`JoinSet::spawn_blocking`** — SHA-512 / Murmur2 hashing is CPU-bound, so each file is hashed on a dedicated blocking thread pool (separate from the async pool)

```
main thread (tokio runtime)
│
├── tokio::join!  ──→ [Modrinth batch]  +  [CurseForge batch]   (concurrent, same thread)
│
├── JoinSet::spawn ──→ task₁: get_latest_version mod A  ┐
│                  ──→ task₂: get_latest_version mod B  ├─ all in parallel
│                  ──→ task₃: get_latest_version mod C  ┘
│
└── JoinSet::spawn_blocking ──→ thread₁: hash file A  ┐
                             ──→ thread₂: hash file B  ├─ blocking thread pool
                             ──→ thread₃: hash file C  ┘
```

## Credits

Rust rewrite of [Minecraft-Mod-Client-Server-updater](https://github.com/Frenk012/Minecraft-Mod-Client-Server-updater) by [Frenk012](https://github.com/Frenk012).

## License

MIT
