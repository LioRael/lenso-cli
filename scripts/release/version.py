"""Expose the workspace release version and reject tags for other versions."""
import os
from pathlib import Path
import re
import base64
from github import api
from package import TARGETS

source_commit = os.environ.get("GITHUB_SHA", "")
artifact_run_id = os.environ.get("GITHUB_RUN_ID", "")
text = Path("Cargo.toml").read_text()
reuse = os.environ.get("REUSE_RUN_ID", "")
if reuse:
    if not reuse.isdecimal():
        raise RuntimeError("reuse_run_id must be numeric")
    repo = os.environ["GITHUB_REPOSITORY"]
    run = api(repo, "actions/runs/" + reuse)
    if run["path"] != ".github/workflows/release-cli.yml" or run["head_repository"]["id"] != run["repository"]["id"]:
        raise RuntimeError("Only this repository's native release workflow builds can be reused")
    jobs = []
    for page in range(1, 11):
        batch = api(repo, f"actions/runs/{reuse}/jobs?filter=all&per_page=100&page={page}")["jobs"]
        jobs.extend(batch)
        if len(batch) < 100:
            break
    for target in TARGETS:
        if not any(job["name"].startswith("build (") and target in job["name"] and job["conclusion"] == "success" for job in jobs):
            raise RuntimeError("Reusable native build is missing for " + target)
    source_commit = run["head_sha"]
    artifact_run_id = reuse
    contents = api(repo, "contents/Cargo.toml?ref=" + source_commit)
    text = base64.b64decode(contents["content"]).decode()
section = text.split("[workspace.package]\n", 1)[1].split("\n[", 1)[0]
version = re.search(r'^version = "(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)"$', section, re.M)[1]
if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ["GITHUB_REF_NAME"] != "v" + version:
    raise RuntimeError("Tag does not match workspace.package.version")
if os.environ.get("GITHUB_OUTPUT"):
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        for name, value in {"version": version, "source_commit": source_commit, "artifact_run_id": artifact_run_id}.items():
            stream.write(name + "=" + value + "\n")
print(f"version={version} source_commit={source_commit} artifact_run_id={artifact_run_id}")
