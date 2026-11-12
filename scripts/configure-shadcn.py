#!/usr/bin/env python3
"""Build local shadcn sources and declare scoped Registry/workspace permissions."""
import argparse
import fnmatch
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
from urllib.parse import urlsplit
from project_config import load, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("project", type=Path)
parser.add_argument("--allow-install", action="store_true", help="Grant the root package manager's exact reviewed install invocation")
parser.add_argument("--read-root", action="append", default=[], help="Additional project-relative readable directory")
parser.add_argument("--write-root", action="append", default=[], help="Additional project-relative writable directory")
parser.add_argument("--http", action="append", default=[], help="Additional public Registry URL prefix")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
project = args.project.resolve(strict=True)
if not (project / "package.json").is_file():
    parser.error("project must contain package.json")

def safe(relative):
    path = project / relative
    if Path(relative).is_absolute() or any(part in ("..", ".git", ".lenso", "node_modules") for part in Path(relative).parts):
        parser.error(f"unsafe project path: {relative}")
    for parent in [path, *path.parents]:
        if parent == project:
            break
        if parent.is_symlink():
            parser.error(f"refusing symlink: {relative}")
    return path

package = json.loads((project / "package.json").read_text())
patterns = package.get("workspaces", [])
if isinstance(patterns, dict):
    patterns = patterns.get("packages", [])
patterns = list(patterns)
workspace_yaml = project / "pnpm-workspace.yaml"
if workspace_yaml.exists():
    in_packages = False
    for line in workspace_yaml.read_text().splitlines():
        if line.startswith("packages:"):
            in_packages = True
            continue
        if line and not line[0].isspace() and not line.startswith("#"):
            in_packages = False
        if in_packages and line.strip().startswith("-"):
            patterns.append(line.strip()[1:].strip().split(" #", 1)[0].strip("\"'"))
workspace_dirs = set()
for pattern in [value for value in patterns if not value.startswith("!")]:
    if Path(pattern).is_absolute() or ".." in Path(pattern).parts:
        parser.error("workspace patterns must stay inside the project")
    for candidate in project.glob(pattern):
        relative = candidate.relative_to(project).as_posix()
        if any(fnmatch.fnmatchcase(relative, value[1:]) for value in patterns if value.startswith("!")):
            continue
        if safe(relative).is_dir() and (candidate / "package.json").is_file():
            workspace_dirs.add(relative)
        if len(workspace_dirs) > 128:
            parser.error("workspace package limit exceeded")
packages = ["", *sorted(workspace_dirs)]
files = set()
reads = {"package.json", "package-lock.json", "pnpm-lock.yaml", "bun.lock", "pnpm-workspace.yaml"}
write_roots = set(args.write_root)
read_roots = set(args.read_root)
http = {"https://ui.shadcn.com/r/", "https://registry.npmjs.org/", *args.http}
env = set()
credentials = {}
for directory in packages:
    base = project / directory
    relative = lambda value: f"{directory}/{value}" if directory else value
    reads.update(relative(value) for value in ["package.json", "components.json", "tsconfig.json", "tsconfig.app.json"])
    reads.update(path.relative_to(project).as_posix() for path in base.glob("tsconfig*.json"))
    files.update([relative("package.json"), relative("components.json")])
    if directory and not (base / "components.json").exists():
        metadata = json.loads((base / "package.json").read_text())
        if "config" in metadata.get("name", ""):
            read_roots.add(directory)
        continue
    for folder in ["src", "app", "components", "hooks", "lib"]:
        write_roots.add(relative(folder))
        read_roots.add(relative(folder))
    config_path = base / "components.json"
    config = json.loads(config_path.read_text()) if config_path.exists() else {}
    css = config.get("tailwind", {}).get("css", "src/index.css")
    css_path = (base / css).resolve()
    try:
        css_relative = css_path.relative_to(project).as_posix()
    except ValueError:
        parser.error("CSS entry escapes project")
    safe(css_relative)
    files.add(css_relative)
    reads.add(css_relative)
    for namespace, registry in config.get("registries", {}).items():
        template = registry if isinstance(registry, str) else registry["url"]
        url = urlsplit(template.replace("{name}", "resource"))
        try:
            loopback = ipaddress.ip_address(url.hostname or "").is_loopback
        except ValueError:
            loopback = False
        if "{name}" not in template or url.username or url.password or url.fragment or (url.scheme != "https" and not (url.scheme == "http" and loopback)):
            parser.error("Registry URL requires HTTPS (or loopback HTTP) and a {name} template")
        prefix = template.split("{name}", 1)[0]
        prefix = prefix.rsplit("/", 1)[0] + "/" if not prefix.endswith("/") else prefix
        http.add(prefix)
        if isinstance(registry, dict):
            if registry.get("headers") or registry.get("params"):
                credentials[relative("components.json") + "#" + namespace] = {"prefix":prefix,"headers":registry.get("headers", {}),"params":registry.get("params", {})}
            for section in ["headers", "params"]:
                for value in registry.get(section, {}).values():
                    env.update(re.findall(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-.*?)?\}", value))
for path in files | reads | write_roots | read_roots:
    safe(path)
crate = root / "extensions/shadcn"
subprocess.run([os.environ.get("CARGO", "cargo"), "build", "--manifest-path", str(crate / "Cargo.toml"),
                "--target", "wasm32-unknown-unknown", "--release", "--target-dir", str(crate / "target")], check=True, cwd=root)
wasm = crate / "target/wasm32-unknown-unknown/release/lenso_shadcn.wasm"
digest = hashlib.sha256(wasm.read_bytes()).hexdigest()
source_id = hashlib.sha256(wasm.read_bytes() + (root / "presets/shadcn/extension.toml").read_bytes()).hexdigest()
destination = safe(f"lenso/extensions/shadcn/{source_id}")
destination.mkdir(parents=True, exist_ok=True)
shutil.copyfile(wasm, safe(f"lenso/extensions/shadcn/{source_id}/shadcn.wasm"))
shutil.copyfile(root / "presets/shadcn/extension.toml", safe(f"lenso/extensions/shadcn/{source_id}/extension.toml"))
manager = package.get("packageManager", "npm").split("@")[0]
commands = {"npm":["install","--ignore-scripts","--no-audit","--no-fund"], "pnpm":["install","--ignore-scripts"], "bun":["install","--ignore-scripts"]}
if manager not in commands:
    parser.error("supported package managers are npm, pnpm, and bun")
installation = {"id":"shadcn", "source":f"lenso/extensions/shadcn/{source_id}/extension.toml", "files":sorted(files), "reads":sorted(reads),
                "write_roots":sorted(write_roots), "read_roots":sorted(read_roots), "http":sorted(http), "env":sorted(env), "discover":bool(patterns), "credentials":credentials,
                "processes":[{"program":manager,"args":commands[manager]}] if args.allow_install else []}
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso"))
if not (project / "lenso.toml").exists() and (project / ".lenso/project.json").exists():
    subprocess.run([str(binary), "--project", str(project), "migrate"], check=True)
project_path = safe("lenso.toml")
installed = load(project_path) if project_path.exists() else {"cli":{"schema_version":1,"extensions":[]}}
installed["cli"]["extensions"] = [entry for entry in installed["cli"]["extensions"] if entry["id"] != "shadcn"] + [installation]
save(project_path, installed)
print(f"Declared shadcn sources ({digest}) in {project}/lenso.toml; first command prepares dependencies")
print(f"Workspace discovery: {bool(patterns)}; root install grant: {manager if args.allow_install else 'disabled'}")
