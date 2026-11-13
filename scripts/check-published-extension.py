#!/usr/bin/env python3
"""Detect an existing immutable publication before rebuilding in another environment."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import quote


def api(path, missing=False):
    result = subprocess.run(["gh", "api", path], capture_output=True, text=True)
    if result.returncode:
        if missing and "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip())
    return json.loads(result.stdout)


def tree(repo, commit):
    value = api(f"repos/{repo}/git/trees/{commit}?recursive=1")
    if value.get("truncated"):
        raise RuntimeError("Source tree was truncated; cannot verify a previous publication")
    return {item["path"]: item["sha"] for item in value["tree"]}


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--id", required=True)
args = parser.parse_args()
if not re.fullmatch(r"[A-Za-z0-9_-]+", args.id):
    parser.error("invalid package ID")
repo, commit = os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_SHA"]
manifest = tomllib.loads(Path(f"presets/{args.id}/extension.toml").read_text())
version = manifest["version"]
descriptor = tomllib.loads(Path("lenso.registry.toml").read_text())
package = next(item for item in descriptor["extensions"] if item["id"] == args.id)
tag = package.get("tag", "v{version}").replace("{version}", version).replace("{id}", args.id)
release = api(f"repos/{repo}/releases/tags/{quote(tag, safe='')}", missing=True)
existing = release is not None and not release["draft"]
if existing:
    published = api(f"repos/{repo}/commits/{quote(tag, safe='')}")["sha"]
    original, current = tree(repo, published), tree(repo, commit)
    for path in [f"extensions/{args.id}", f"presets/{args.id}/extension.toml", "lenso.registry.toml"]:
        if path not in original or original[path] != current.get(path):
            raise RuntimeError(f"Published {tag} has different source at {path}; increment the extension version")
    output = Path("dist/lenso")
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run(["gh", "release", "download", tag, "--repo", repo, "--pattern", "lenso-release.json", "--dir", str(output), "--clobber"], check=True)
    metadata = json.loads((output / "lenso-release.json").read_text())
    if metadata["schema_version"] != 1 or metadata["id"] != args.id or metadata["version"] != version:
        raise RuntimeError("Published metadata identity/version mismatch")
with open(os.environ["GITHUB_OUTPUT"], "a") as outputs:
    for key, value in {"existing": str(existing).lower(), "version": version, "tag": tag}.items():
        if "\n" in value or "\r" in value:
            raise RuntimeError("Invalid Actions output")
        outputs.write(f"{key}={value}\n")
print(f"{'Verify existing publication' if existing else 'Build new publication'}: {tag}")
