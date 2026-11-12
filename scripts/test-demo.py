#!/usr/bin/env python3
"""Exercise the actual compiled Wasm fixture against an isolated project copy."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from project_config import load, save

root = Path(__file__).resolve().parent.parent
binary = root / "target/debug/lenso"

with tempfile.TemporaryDirectory(prefix="lenso-m0-") as directory:
    project = Path(directory)
    shutil.copytree(root / "examples/m0/lenso", project / "lenso")
    shutil.copyfile(root / "examples/m0/lenso.toml", project / "lenso.toml")
    cache = project / "cache"
    env = dict(os.environ, LENSO_CACHE_DIR=str(cache))

    def run(*args, expected=0):
        result = subprocess.run([str(binary), "--project", str(project), *args, "--json"],
                                capture_output=True, text=True, check=False, env=env)
        assert result.returncode == expected, (result.returncode, result.stdout, result.stderr)
        return json.loads(result.stdout)

    result = run("greet", expected=2)
    assert result["status"] == "needs_input" and result["question"]["kind"]["type"] == "select"
    assert (project / "lenso.lock").exists() and not (project / ".lenso").exists()
    result = run("preview", "--name", "Rust")
    assert "+Hello, Rust!" in result["outputs"]["plan"]["diff"]
    assert not (project / "greeting.txt").exists()
    result = run("greet", "--name", "Rust", expected=2)
    assert result["question"]["id"].startswith("apply:")
    result = run("greet", "--name", "Rust", "--yes")
    assert result["status"] == "complete"
    assert (project / "greeting.txt").read_text() == "Hello, Rust!\n"
    answers = project / "answers.json"
    answers.write_text(json.dumps({"name": "Wasm"}))
    result = run("greet", "--answers", str(answers), "--yes")
    assert result["status"] == "complete"
    assert (project / "greeting.txt").read_text() == "Hello, Wasm!\n"
    result = run("toolchain", "version")
    assert result["outputs"]["version"]["exit_code"] == 0
    config_path = project / "lenso.toml"
    config = load(config_path)
    config["cli"]["extensions"][0]["files"] = []
    save(config_path, config)
    result = run("preview", "--name", "Rust", expected=1)
    assert "permission denied: file destination" in result["message"]
    config["cli"]["extensions"][0]["files"] = ["greeting.txt"]
    save(config_path, config)
    shutil.rmtree(cache)
    result = run("preview", "--name", "Rust")
    assert result["status"] == "complete", "missing cache must restore from locked sources"
    wasm = project / "lenso/extensions/greeting/greeting.wasm"
    wasm.write_bytes(wasm.read_bytes() + b"tampered")
    shutil.rmtree(cache)
    result = run("preview", "--name", "Rust", expected=1)
    assert "locked source module changed" in result["message"]

print("M0 demo passed: real Wasm, hook, questions, preview, approval, answers, process, file grants, tamper rejection")
