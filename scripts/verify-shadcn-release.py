#!/usr/bin/env python3
"""Consume a published Shadcn package through native onboarding with an empty PATH."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from project_config import literal, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--repo", required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--token-env")
args = parser.parse_args()
binary = args.binary.resolve()
root = Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix="lenso-shadcn-release-") as directory:
    project = Path(directory) / "project"
    shutil.copytree(root / "extensions/shadcn/templates/vite", project)
    config = {"cli": {"schema_version": 1, "extensions": []}}
    if args.token_env:
        config["cli"]["github"] = {"token_env": args.token_env}
    save(project / "lenso.toml", config)
    preset = {"schema_version": 1, "id": "release-check", "version": "1.0.0", "adopt": "shadcn/init", "extensions": [{"id": "shadcn", "source": f"github-release:{args.repo}/shadcn", "version": "=" + args.version}]}
    (project / "release-preset.toml").write_text("\n".join(f"{key} = {literal(value)}" for key, value in preset.items()) + "\n")
    env = dict(os.environ, PATH="", LENSO_CACHE_DIR=str(Path(directory) / "cache"))
    for arguments in [("init", "--preset", "release-preset.toml", "--yes"), ("shadcn", "init", "--locked", "--offline", "--yes"), ("ext", "tree", "--locked", "--offline")]:
        result = subprocess.run([str(binary), "--project", str(project), *arguments, "--json"], env=env, capture_output=True, text=True, timeout=180)
        assert result.returncode == 0, (arguments, result.stdout, result.stderr)
        assert json.loads(result.stdout)["status"] == "complete"
    assert (project / "components.json").is_file()
    assert not (project / "node_modules").exists()
print("Published Shadcn onboarding passed with empty PATH and offline reuse.")
