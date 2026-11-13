#!/usr/bin/env python3
"""Build the optional code extension and declare one explicit GitHub resource source."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
from project_config import load, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("project", type=Path)
source = parser.add_mutually_exclusive_group(required=True)
source.add_argument("--repo", help="GitHub owner/repo")
source.add_argument("--author-root", help="Project-relative local author catalog directory")
parser.add_argument("--ref", default="HEAD")
parser.add_argument("--catalog", default="catalog.toml")
parser.add_argument("--format", choices=["lenso-code-v1", "lenso-compose-v1"], default="lenso-code-v1")
parser.add_argument("--registry", default="main")
parser.add_argument("--target", default="src", help="Project-relative destination/read/write root")
parser.add_argument("--file", action="append", help="Use explicit repository files instead of a remote catalog")
parser.add_argument("--item", default="files", help="Item name for --file")
parser.add_argument("--github-api", help="Explicit API base, for GitHub Enterprise or loopback testing")
parser.add_argument("--token-env", help="Explicit environment variable name; no token values are saved")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
project = args.project.resolve(strict=True)

def safe(value):
    if not value or Path(value).is_absolute() or any(part in (".", "..", ".git", ".lenso", "node_modules") for part in value.split("/")) or "\\" in value or ":" in value:
        parser.error("paths must stay in the project and outside reserved directories")
    path = project / value
    for parent in [path, *path.parents]:
        if parent == project:
            break
        if parent.is_symlink():
            parser.error("symlinks are not allowed")
    return path

safe(args.target)
if args.author_root:
    safe(args.author_root)
if args.repo and len(args.repo.split("/")) != 2:
    parser.error("--repo must be owner/repo")
crate = root / "extensions/code"
subprocess.run([os.environ.get("CARGO", "cargo"), "build", "--manifest-path", str(crate / "Cargo.toml"), "--target", "wasm32-unknown-unknown", "--release"], check=True)
module = crate / "target/wasm32-unknown-unknown/release/lenso_code.wasm"
manifest = root / "presets/code/extension.toml"
identity = hashlib.sha256(module.read_bytes() + manifest.read_bytes()).hexdigest()
relative = f"lenso/extensions/code/{identity}"
destination = safe(relative)
destination.mkdir(parents=True, exist_ok=True)
shutil.copyfile(module, destination / "code.wasm")
shutil.copyfile(manifest, destination / "extension.toml")
profile = {"source":("github-repo:" + args.repo if args.repo else "project:" + args.author_root), "ref":args.ref, "path":args.catalog, "format":args.format, "target":args.target}
if args.author_root:
    profile["format"] = "lenso-compose-v1"
if args.file:
    if args.author_root:
        parser.error("--file is only available with --repo")
    profile.update({"format":"inline", "items":[{"name":args.item,"files":args.file}]})
policy = {"id":"code", "source":relative + "/extension.toml", "sources":(["github:" + args.repo] if args.repo else []), "read_roots":([args.target] + ([args.author_root] if args.author_root else [])), "write_roots":[args.target],
          "options":{"registries":{args.registry:profile}}}
path = project / "lenso.toml"
config = load(path) if path.exists() else {"cli":{"schema_version":1,"extensions":[]}}
config["cli"]["extensions"] = [entry for entry in config["cli"].get("extensions", []) if entry["id"] != "code"] + [policy]
if args.github_api or args.token_env:
    github = config["cli"].setdefault("github", {})
    if args.github_api:
        github["api"] = args.github_api
    if args.token_env:
        github["token_env"] = args.token_env
save(path, config)
print(f"Declared code extension and {profile['source']}; destination grant: {args.target}")
