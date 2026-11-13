#!/usr/bin/env python3
"""Live consumer verification against the curated official Registry and published Wasm."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
args = parser.parse_args()
binary = args.binary.resolve()
with tempfile.TemporaryDirectory(prefix="lenso-official-") as directory:
    root = Path(directory)
    env = dict(os.environ, PATH="", LENSO_CACHE_DIR=str(root / "cache"))
    for key in ("GH_TOKEN", "GITHUB_TOKEN", "LENSO_PUBLISH_TOKEN"):
        env.pop(key, None)
    def run(project, *arguments, offline=False):
        environment = dict(env)
        if offline:
            environment.update(HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9", ALL_PROXY="http://127.0.0.1:9", NO_PROXY="")
        result = subprocess.run([str(binary), "--project", str(project), *arguments, "--json"], env=environment, capture_output=True, text=True, timeout=180)
        assert result.returncode == 0, (arguments, result.stdout, result.stderr)
        report = json.loads(result.stdout)
        assert report["status"] == "complete"
        return report
    settings = json.dumps({"repo": "LioRael/lenso-cli", "ref": "v0.1.0", "catalog": "fixtures/distribution/catalog.toml"})
    run(root, "new", "code-project", "--preset", "code", "--input", settings, "--yes")
    code = root / "code-project"
    run(code, "code", "search")
    run(code, "code", "preview", "hello")
    assert not (code / "src/hello.txt").exists()
    run(code, "code", "add", "hello", "--yes")
    assert (code / "src/hello.txt").read_text() == "Hello from a published Lenso Wasm extension.\n"
    run(code, "code", "diff", "hello", "--locked", "--offline", offline=True)
    run(root, "new", "vite-project", "--preset", "shadcn", "--yes")
    vite = root / "vite-project"
    run(vite, "shadcn", "preview", "button")
    assert not (vite / "src/components/ui/button.tsx").exists()
    run(vite, "shadcn", "init", "--yes", "--locked", "--offline", offline=True)
    for project in (code, vite):
        config = run(project, "config")["outputs"]["main"]["config"]
        assert config["cli"]["presets"]
        nodes = run(project, "ext", "tree", "--locked", "--offline", offline=True)["outputs"]["main"]["extensions"]
        assert "project" in {node["id"] for node in nodes}
        assert not (project / ".lenso").exists()
        assert not (project / "node_modules").exists()
print("Official onboarding passed: anonymous curated aliases, published Wasm, Code application, Shadcn Vite creation/preview, locked offline reuse, empty PATH, JSON/Agent.")
