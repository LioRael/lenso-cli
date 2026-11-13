#!/usr/bin/env python3
"""Live M1 acceptance: official Registry -> reviewed files -> npm -> Vite build."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from project_config import load, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--keep", action="store_true", help="Keep the installed Vite project for inspection")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso"))
scratch = Path(tempfile.mkdtemp(prefix="lenso-shadcn-"))
os.environ["LENSO_CACHE_DIR"] = str(scratch / "cache")
project = scratch / "vite"
shutil.copytree(root / "examples/vite-shadcn", project,
                ignore=shutil.ignore_patterns("node_modules", "dist", ".lenso", "lenso", "lenso.toml", "lenso.lock"))

def run(*arguments, expected=0, env=None):
    result = subprocess.run([str(binary), "--project", str(project), "shadcn", *arguments, "--json"],
                            capture_output=True, text=True, env=env, timeout=300)
    assert result.returncode == expected, (arguments, result.returncode, result.stdout, result.stderr)
    return json.loads(result.stdout)

def snapshot():
    return {str(path.relative_to(project)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in project.rglob("*") if path.is_file()
            and not any(part in ("node_modules", "dist", ".lenso") for part in path.relative_to(project).parts)}

try:
    subprocess.run(["python3", str(root / "scripts/configure-shadcn.py"), str(project), "--allow-install"], check=True)
    assert not (project / "lenso.lock").exists(), "declaration must not eagerly prepare dependencies"
    component_config = (project / "components.json").read_text()
    (project / "components.json").unlink()
    result = run("init", "--yes")
    assert json.loads((project / "components.json").read_text())["style"] == "new-york"
    assert result["outputs"]["apply"]["commands"] == []
    (project / "components.json").write_text(component_config)
    assert (project / "lenso.lock").exists() and not (project / ".lenso").exists()
    before = snapshot()
    search = run("search", "--input", '{"query":"button","limit":2}')
    assert any(row["add"] == "button" for row in search["outputs"]["search"]["results"])
    assert search["outputs"]["search"]["pagination"]["total"] >= 1
    run("search", "--input", '{"query":true}', expected=1)
    result = run("add", expected=2)
    assert result["status"] == "needs_input" and result["question"]["id"] == "item"
    assert "button" in result["question"]["kind"]["options"] and "input" in result["question"]["kind"]["options"]
    result = run("preview", "button")
    plan = result["outputs"]["plan"]["plan"]
    assert any(file["path"] == "src/components/ui/button.tsx" for file in plan["files"])
    assert "https://ui.shadcn.com/r/styles/new-york-v4/button.json" in plan["evidence"]["sources"]
    assert "package.json" in plan["evidence"]["reads"]
    assert plan["commands"][0]["args"][0] == "install"
    assert before == snapshot(), "selection and preview changed project files"
    result = run("add", "button", expected=2)
    assert result["question"]["id"].startswith("apply:")
    assert before == snapshot(), "pending approval changed project files"
    config_path = project / "lenso.toml"
    original_config_text = config_path.read_text()
    original_config = load(config_path)
    for capability, expected_message in [("processes", "permission denied: process"), ("reads", "permission denied: file read"), ("http", "permission denied: HTTP GET")]:
        config = json.loads(json.dumps(original_config))
        config["cli"]["extensions"][0][capability] = []
        save(config_path, config)
        denied_snapshot = snapshot()
        result = run("add", "button", "--yes", expected=1)
        assert expected_message in result["message"], result
        assert denied_snapshot == snapshot(), f"denied {capability} changed project files"
    config_path.write_text(original_config_text)
    offline = os.environ.copy()
    for key in list(offline):
        if key.lower() in ("http_proxy", "https_proxy", "all_proxy", "no_proxy"):
            del offline[key]
    offline.update({"https_proxy":"http://127.0.0.1:9", "no_proxy":""})
    result = run("preview", "button", expected=1, env=offline)
    assert "HTTP GET" in result["message"]
    assert before == snapshot(), "network failure changed project files"
    target = project / "src/components/ui/button.tsx"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("// user-maintained component\n")
    result = run("preview", "button", expected=2)
    conflict_id = result["question"]["id"]
    assert conflict_id == "conflict:src/components/ui/button.tsx"
    assert target.read_text() == "// user-maintained component\n"
    answers = scratch / "answers.json"
    answers.write_text(json.dumps({conflict_id:"keep"}))
    result = run("preview", "button", "--answers", str(answers))
    assert all(file["path"] != "src/components/ui/button.tsx" for file in result["outputs"]["plan"]["plan"]["files"])
    target.unlink()
    answers.write_text(json.dumps({"item":"button"}))
    result = run("add", "--answers", str(answers), "--yes")
    assert result["status"] == "complete" and result["outputs"]["verify"]["verified"] == "button"
    for file in result["outputs"]["resolve"]["files"]:
        assert (project / file["path"]).read_text() == file["content"]
    assert (project / "package-lock.json").exists()
    assert (project / "node_modules/class-variance-authority/package.json").exists()
    multiple = run("add", "input", "badge", "dialog", "--yes")
    assert multiple["outputs"]["verify"]["verified"] == "input, badge, dialog"
    for name in ["input", "badge", "dialog"]:
        assert (project / f"src/components/ui/{name}.tsx").exists()
    assert (project / "node_modules/lucide-react/package.json").exists()
    target.write_text(target.read_text() + "// local button customization\n")
    modified = snapshot()
    difference = run("diff", "button")
    assert difference["outputs"]["resolve"]["summary"]["changed"] == 1
    assert "local button customization" in difference["outputs"]["plan"]["diff"]
    assert modified == snapshot()
    conflict = run("components", "update", "button", "--yes", expected=2)
    assert conflict["question"]["id"] == conflict_id
    answers.write_text(json.dumps({conflict_id:"overwrite"}))
    updated = run("components", "update", "button", "--answers", str(answers), "--yes")
    assert updated["outputs"]["verify"]["verified"] == "button"
    assert "local button customization" not in target.read_text()
    assert run("diff", "button")["outputs"]["resolve"]["summary"]["changed"] == 0
    subprocess.run(["npm", "run", "build"], cwd=project, check=True, timeout=180)
    print("M1 passed: init, live Registry, search, guarded update, read-only diff/preview, dynamic selection, missing input, grants, network failure, conflicts, answers, npm install, Vite build", flush=True)
    print(f"Project: {project}", flush=True)
except BaseException:
    print(f"Failed project retained for inspection: {project}", flush=True)
    raise
else:
    if not args.keep:
        shutil.rmtree(scratch)
