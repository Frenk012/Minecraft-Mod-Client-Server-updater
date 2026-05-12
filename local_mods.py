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


def _murmur2(data: bytes) -> int:
    """CurseForge Murmur2 fingerprint: strip whitespace bytes then hash."""
    data = bytes(b for b in data if b not in (9, 10, 13, 32))
    length = len(data)
    m = 0x5bd1e995
    h = 1 ^ length  # seed = 1
    i = 0
    remaining = length
    while remaining >= 4:
        k = int.from_bytes(data[i:i + 4], "little")
        k = (k * m) & 0xFFFFFFFF
        k ^= k >> 24
        k = (k * m) & 0xFFFFFFFF
        h = (h * m) & 0xFFFFFFFF
        h ^= k
        i += 4
        remaining -= 4
    if remaining == 3:
        h ^= data[i + 2] << 16
        h ^= data[i + 1] << 8
        h ^= data[i]
        h = (h * m) & 0xFFFFFFFF
    elif remaining == 2:
        h ^= data[i + 1] << 8
        h ^= data[i]
        h = (h * m) & 0xFFFFFFFF
    elif remaining == 1:
        h ^= data[i]
        h = (h * m) & 0xFFFFFFFF
    h ^= h >> 13
    h = (h * m) & 0xFFFFFFFF
    h ^= h >> 15
    return h


def murmur2_file(filepath: str) -> int:
    with open(filepath, "rb") as f:
        return _murmur2(f.read())


def scan_local_folder(folder_path: str) -> list:
    folder = Path(folder_path)
    if not folder.exists():
        raise FileNotFoundError(f"Mods folder not found: {folder_path}")

    mods = []
    for entry in os.listdir(folder):
        if entry.lower().endswith(".jar"):
            filepath = str(folder / entry)
            sha = hash_file(filepath)
            fp = murmur2_file(filepath)
            mods.append(LocalMod(filename=entry, filepath=filepath, sha512=sha, murmur2=fp))

    return mods
