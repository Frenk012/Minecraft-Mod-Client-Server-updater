import hashlib
import os
from pathlib import Path
from models import LocalMod


def hash_file(filepath: str) -> str:
    h = hashlib.sha512()
    with open(filepath, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


def hash_bytes(data: bytes) -> str:
    return hashlib.sha512(data).hexdigest()


def scan_local_folder(folder_path: str) -> list:
    folder = Path(folder_path)
    if not folder.exists():
        raise FileNotFoundError(f"Mods folder not found: {folder_path}")

    mods = []
    for entry in os.listdir(folder):
        if entry.lower().endswith(".jar"):
            filepath = str(folder / entry)
            sha = hash_file(filepath)
            mods.append(LocalMod(filename=entry, filepath=filepath, sha512=sha))

    return mods
