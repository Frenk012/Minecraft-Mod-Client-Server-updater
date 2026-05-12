"""
Quick diagnostic: compute Murmur2 fingerprint for each mod jar and query CF API.
Usage: python debug_cf.py <cf_api_key>
"""
import sys
import requests
from local_mods import scan_local_folder
import tomllib

with open("config.toml", "rb") as f:
    config = tomllib.load(f)

mods_folder = config["local"]["mods_folder"]
api_key = sys.argv[1] if len(sys.argv) > 1 else config.get("curseforge", {}).get("api_key", "")

if not api_key:
    print("Usage: python debug_cf.py <cf_api_key>")
    sys.exit(1)

mods = scan_local_folder(mods_folder)
fingerprints = [m.murmur2 for m in mods if m.murmur2 is not None]
fp_to_name = {m.murmur2: m.filename for m in mods if m.murmur2 is not None}

print(f"Querying CF with {len(fingerprints)} fingerprints...\n")

resp = requests.post(
    "https://api.curseforge.com/v1/fingerprints",
    headers={"x-api-key": api_key, "Accept": "application/json"},
    json={"fingerprints": fingerprints},
)
resp.raise_for_status()
data = resp.json()["data"]

matched = {m["id"] for m in data.get("exactMatches", [])}
unmatched = data.get("unmatchedFingerprints", [])

print(f"{'FILE':<60} {'FINGERPRINT':>12} {'STATUS'}")
print("-" * 85)
for fp in fingerprints:
    name = fp_to_name[fp]
    status = "FOUND" if fp in matched else "NOT FOUND"
    print(f"{name:<60} {fp:>12} {status}")

print(f"\nMatched: {len(matched)} / {len(fingerprints)}")
print(f"Unmatched fingerprints returned by CF: {unmatched[:5]}{'...' if len(unmatched) > 5 else ''}")
