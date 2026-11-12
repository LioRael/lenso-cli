#!/usr/bin/env python3
"""Consume a real published code extension with a new project, empty cache and empty PATH."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from project_config import save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--repo", required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--ref", required=True, help="Full source commit containing fixtures/distribution")
parser.add_argument("--token-env", default="GITHUB_TOKEN")
parser.add_argument("--out", type=Path, help="New output directory; preserved for inspection")
args = parser.parse_args()
if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
    parser.error("--repo must be owner/repo")
if not re.fullmatch(r"[a-fA-F0-9]{40}", args.ref):
    parser.error("--ref must be a full commit SHA")
token = os.environ.get(args.token_env)
if not token:
    parser.error("the named credential variable must be set")
binary = args.binary.resolve(strict=True)
root = Path(__file__).resolve().parent.parent
output = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix="lenso-live-release-"))
if args.out:
    output.mkdir(parents=True, exist_ok=False)
project = output / "consumer"
cache = output / "cache"
empty_path = output / "empty-path"
traces = output / "commands"
for directory in (project, cache, empty_path, traces):
    directory.mkdir()
env = dict(os.environ, LENSO_CACHE_DIR=str(cache), PATH=str(empty_path))
config = {"cli": {"schema_version": 1, "github": {"token_env": args.token_env}, "extensions": [{
    "id": "code", "source": "github-release:" + args.repo + "/code", "version": "=" + args.version,
    "sources": ["github:" + args.repo], "read_roots": ["generated"], "write_roots": ["generated"],
    "options": {"registries": {"e2e": {"source": "github-repo:" + args.repo, "ref": args.ref,
        "path": "fixtures/distribution/catalog.toml", "format": "lenso-code-v1", "target": "generated"}}}
}]}}
save(project / "lenso.toml", config)
commands = []

def run(*arguments, expected=0):
    completed = subprocess.run([str(binary), "--project", str(project), *arguments, "--json"],
                               capture_output=True, text=True, env=env, timeout=120)
    if token in completed.stdout + completed.stderr:
        raise RuntimeError("credential leaked into CLI output")
    if completed.returncode != expected:
        raise RuntimeError(f"{arguments}: exit {completed.returncode}\n{completed.stdout}\n{completed.stderr}")
    result = json.loads(completed.stdout)
    assert "\x1b" not in completed.stdout, "JSON contains terminal formatting"
    commands.append({"args": list(arguments), "exit_code": completed.returncode})
    (traces / f"{len(commands):02}.json").write_text(json.dumps(result, indent=2) + "\n")
    return result

search = run("code", "search", "--query", "hello")
assert [item["name"] for item in search["outputs"]["search"]["results"]] == ["hello"]
expected_content = (root / "fixtures/distribution/hello.txt").read_bytes()
destination = project / "generated/hello.txt"
preview = run("code", "preview", "hello")
assert expected_content.decode().strip() in preview["outputs"]["plan"]["diff"]
assert not destination.exists(), "preview wrote application files"
pending = run("code", "add", "hello", expected=2)
question = pending["question"]["id"]
assert question.startswith("apply:"), pending
answers = output / "answers.json"
answers.write_text(json.dumps({question: False}))
run("code", "add", "hello", "--answers", str(answers), expected=130)
assert not destination.exists(), "declining approval wrote application files"
answers.write_text(json.dumps({question: True}))
run("code", "add", "hello", "--answers", str(answers))
assert destination.read_bytes() == expected_content
lock_before = (project / "lenso.lock").read_bytes()
diff = run("code", "diff", "hello", "--locked")
assert diff["outputs"]["resolve"]["summary"]["changed"] == 0
assert (project / "lenso.lock").read_bytes() == lock_before
lock = json.loads(lock_before)
extension = lock["extensions"][0]
pin = extension["github"]
assert extension["definition"]["version"] == args.version
assert pin["tag"] == "code-v" + args.version
assert all(resource["commit"] == args.ref.lower() for resource in lock["resources"].values())
hashes = {"release_metadata": pin["metadata_sha256"], "manifest": pin["metadata"]["manifest"]["sha256"],
          "archive": pin["metadata"]["archive"]["sha256"],
          **{name: module["sha256"] for name, module in extension["modules"].items()}}

def verify_cache(names=None):
    for name, digest in hashes.items():
        if names is not None and name not in names:
            continue
        data = (cache / "v1/blobs/sha256" / digest).read_bytes()
        assert hashlib.sha256(data).hexdigest() == digest, name

verify_cache()
# Restore from immutable Release/asset IDs and resource commits, then replay without network.
shutil.rmtree(cache)
run("code", "preview", "hello", "--locked")
verify_cache({"archive", *extension["modules"]})
assert (project / "lenso.lock").read_bytes() == lock_before
env.update(HTTPS_PROXY="http://127.0.0.1:9", HTTP_PROXY="http://127.0.0.1:9", ALL_PROXY="http://127.0.0.1:9", NO_PROXY="")
run("code", "diff", "hello", "--locked", "--offline")
run("code", "add", "hello", "--locked", "--offline")
assert (project / "lenso.lock").read_bytes() == lock_before
assert destination.read_bytes() == expected_content
report = {"repository": args.repo, "version": args.version, "source_commit": args.ref,
          "release_id": pin["release_id"], "tag": pin["tag"], "archive_asset": pin["archive_asset"],
          "manifest_asset": pin["manifest_asset"], "sha256": hashes,
          "empty_path": True, "cold_cache": True, "cache_restore": True, "locked_offline": True,
          "preview_and_consent": True, "commands": commands, "project": str(project)}
(output / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
summary = os.environ.get("GITHUB_STEP_SUMMARY")
if summary:
    with open(summary, "a") as stream:
        stream.write(f"Verified `{args.repo}` / `{pin['tag']}` / Release `{pin['release_id']}`.\n\n")
        stream.write("Cold download, Wasm execution, preview, decline/approve, cache restoration and locked/offline replay passed with an empty PATH.\n\n")
        stream.write("```json\n" + json.dumps(hashes, indent=2) + "\n```\n")
print(json.dumps(report, indent=2))
