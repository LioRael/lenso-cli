#!/usr/bin/env python3
"""Run real Wasm/file workflows and compare full versus compact result bytes."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

root = Path(__file__).resolve().parent.parent
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso")).resolve()
with tempfile.TemporaryDirectory(prefix="lenso-agent-protocol-") as directory:
    scratch = Path(directory)
    env = dict(os.environ, LENSO_CACHE_DIR=str(scratch / "cache"), LENSO_NO_UPDATE_CHECK="1")
    projects = {}
    for mode in ["full", "summary"]:
        project = scratch / mode
        project.mkdir()
        shutil.copytree(root / "examples/m0/lenso", project / "lenso")
        shutil.copyfile(root / "examples/m0/lenso.toml", project / "lenso.toml")
        projects[mode] = project

    def invoke(project, *args, expected=0):
        output = subprocess.run([str(binary), "--project", str(project), *args, "--json"],
                                capture_output=True, env=env, timeout=90)
        assert output.returncode == expected, (args, output.returncode, output.stdout, output.stderr)
        return output.stdout, json.loads(output.stdout)

    _, pending = invoke(projects["full"], "greet", "--yes", "--summary", expected=2)
    assert pending["code"] == "missing_input" and pending["question"]["kind"]["type"] == "select"
    assert not (projects["full"] / "greeting.txt").exists(), "--yes cannot choose a recipient"

    baseline, full = invoke(projects["full"], "greet", "--name", "Rust", "--yes")
    report_path = scratch / "execution.json"
    compact, summary = invoke(projects["summary"], "greet", "--name", "Rust", "--yes",
                              "--summary", "--report", str(report_path))
    saved = json.loads(report_path.read_bytes())
    assert full["status"] == summary["status"] == "complete"
    assert "outputs" in full and "outputs" not in summary and "outputs" in saved
    assert saved["execution"]["id"] == summary["execution"]["id"]
    assert summary["execution"]["verification_status"] == "passed"
    assert summary["execution"]["application_verification"] == "not_reported"
    assert summary["execution"]["calls"][-1]["receipt"]["affected"] == ["greeting.txt"]
    assert saved["outputs"]["apply"]["plan_id"] == summary["execution"]["calls"][-1]["receipt"]["identifiers"]["plan_id"]
    for project in projects.values():
        assert (project / "greeting.txt").read_text() == "Hello, Rust!\n"
    assert len(compact) < len(baseline), (len(baseline), len(compact))
    print(json.dumps({"task": "Wasm greeting -> validated file plan -> approved apply",
                      "default_json_bytes": len(baseline), "summary_json_bytes": len(compact),
                      "saved_full_report_bytes": report_path.stat().st_size,
                      "cli_calls_per_completed_task": 1, "extra_execution_to_read_report": 0,
                      "token_count": None, "status": "passed"}))
