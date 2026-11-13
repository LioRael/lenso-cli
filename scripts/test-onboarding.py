#!/usr/bin/env python3
"""Exercise native onboarding with real Code/Shadcn Wasm and no tools on PATH."""
import functools
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading

root = Path(__file__).resolve().parent.parent
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso")).resolve()
with tempfile.TemporaryDirectory(prefix="lenso-onboarding-") as directory:
    scratch = Path(directory)
    served = scratch / "served"
    served.mkdir()
    for package in ("code", "shadcn"):
        shutil.copytree(root / "dist/mainline" / package, served / package)
        module = root / "dist/author" / package / (package + ".wasm")
        shutil.copyfile(module, served / package / module.name)
    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=str(served)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f"http://127.0.0.1:{server.server_port}"
    env = dict(os.environ, PATH="", LENSO_CACHE_DIR=str(scratch / "cache"))
    def invoke(project, *args, expected=0):
        output = subprocess.run([str(binary), "--project", str(project), *args, "--json"], env=env, capture_output=True, text=True, timeout=90)
        assert output.returncode == expected, (args, output.returncode, output.stdout, output.stderr)
        return json.loads(output.stdout)
    def composition(name, package, create, adopt):
        data = {"schema_version": 1, "id": "@test/" + name, "version": "1.0.0", "create": create, "adopt": adopt,
                "extensions": [{"id": package, "source": f"{base}/{package}/extension.toml"}]}
        (served / (name + ".json")).write_text(json.dumps(data))
        return f"{base}/{name}.json"
    code = composition("code", "code", "code/ready", "code/ready")
    shared = composition("shared", "code", "code/ready", "code/ready")
    shadcn = composition("shadcn", "shadcn", "shadcn/create", "shadcn/init")
    project = scratch / "existing"
    project.mkdir()
    pending = invoke(project, "ext", "add", f"{base}/code/extension.toml", "--id", "code", expected=2)
    assert not (project / "lenso.toml").exists()
    assert not (project / "lenso.lock").exists()
    # Confirmation alone cannot answer extension-owned text questions.
    pending = invoke(project, "ext", "add", f"{base}/code/extension.toml", "--id", "code", "--yes", expected=2)
    assert pending["question"]["id"] == "code:repository"
    assert not (project / "lenso.toml").exists()
    settings = json.dumps({"repo": "LioRael/lenso-cli", "ref": "v0.1.0", "catalog": "fixtures/distribution/catalog.toml"})
    invoke(project, "ext", "add", f"{base}/code/extension.toml", "--id", "code", "--input", settings, "--yes")
    config = (project / "lenso.toml").read_text()
    assert "github:LioRael/lenso-cli" in config
    tree = invoke(project, "ext", "tree", "--locked", "--offline")["outputs"]["main"]["extensions"]
    assert {item["id"] for item in tree} == {"code", "project"}
    invoke(project, "preset", "apply", code, "--yes")
    invoke(project, "preset", "apply", shared, "--yes")
    invoke(project, "ext", "remove", "code", "--yes")
    invoke(project, "preset", "remove", "@test/code", "--yes")
    assert "code" in {item["id"] for item in invoke(project, "ext", "tree")["outputs"]["main"]["extensions"]}
    invoke(project, "preset", "remove", "@test/shared", "--yes")
    assert invoke(project, "ext", "tree")["outputs"]["main"]["extensions"] == []
    invoke(scratch, "new", "vite", "--preset", shadcn, expected=2)
    assert not (scratch / "vite").exists()
    invoke(scratch, "new", "vite", "--preset", shadcn, "--yes")
    vite = scratch / "vite"
    for name in ("lenso.toml", "lenso.lock", "package.json", "components.json", "src/main.tsx", "src/index.css"):
        assert (vite / name).is_file(), name
    assert not (vite / "node_modules").exists()
    assert not (vite / ".lenso").exists()
    invoke(vite, "shadcn", "init", "--yes", "--locked", "--offline")
    assert {item["id"] for item in invoke(vite, "ext", "tree")["outputs"]["main"]["extensions"]} == {"shadcn", "project"}
    mono = scratch / "monorepo"
    web = mono / "modules/web"
    shutil.copytree(vite, web, ignore=shutil.ignore_patterns("lenso.toml", "lenso.lock"))
    (mono / "package.json").write_text(json.dumps({"name": "workspace-root", "private": True, "workspaces": ["modules/*"], "packageManager": "npm@11.20.0"}))
    component_config = json.loads((web / "components.json").read_text())
    component_config["registries"] = {"@private": {"url": base + "/private/{name}.json", "headers": {"Authorization": "Bearer ${ONBOARDING_TOKEN}"}}}
    (web / "components.json").write_text(json.dumps(component_config))
    before = {str(path.relative_to(mono)): path.read_bytes() for path in mono.rglob("*") if path.is_file()}
    invoke(mono, "init", "--preset", shadcn, "--input", '{"cwd":"modules/web"}', "--yes")
    policy = invoke(mono, "config")["outputs"]["main"]["config"]["cli"]["extensions"][0]
    assert "modules" in policy["read_roots"]
    assert "modules/web/src" in policy["write_roots"]
    assert "ONBOARDING_TOKEN" in policy["env"]
    assert policy["credentials"]["modules/web/components.json#@private"]["prefix"] == base + "/private/"
    assert all((mono / path).read_bytes() == content for path, content in before.items())
    server.shutdown()
    thread.join()
print("Onboarding passed: reviewed configuration, real Wasm setup, shared presets, optional project, Vite creation, custom monorepo roots/auth, empty PATH, JSON/Agent.")
