#!/usr/bin/env python3
"""Prepare a Registry release record and optionally open a GitHub review PR (author tool)."""
import argparse
import base64
import json
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import quote


def api(path, method="GET", body=None, missing=False):
    command = ["gh", "api", path, "--method", method]
    with tempfile.TemporaryDirectory(prefix="lenso-registry-request-") as directory:
        if body is not None:
            payload = Path(directory) / "request.json"
            payload.write_text(json.dumps(body))
            command += ["--input", str(payload)]
        result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode:
        if missing and "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(f"GitHub {method} {path} failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--repo", required=True, help="Published package owner/repo")
    parser.add_argument("--name", required=True, help="Registry @namespace/package")
    parser.add_argument("--registry-repo", default="LioRael/lenso-registry")
    parser.add_argument("--head-repo", help="Your already-created fork when you cannot push to the Registry")
    parser.add_argument("--out", type=Path, help="Write the reviewable release record")
    parser.add_argument("--submit", action="store_true", help="Create a review branch and pull request using gh authentication")
    args = parser.parse_args()
    for repo in [args.repo, args.registry_repo, args.head_repo or args.registry_repo]:
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
            parser.error("repositories must be owner/repo")
    if not re.fullmatch(r"@[A-Za-z0-9_-]+/[A-Za-z0-9_-]+", args.name):
        parser.error("name must be @namespace/package")
    metadata = json.loads(args.metadata.read_text())
    version, package = metadata["version"], metadata["id"]
    if metadata["schema_version"] != 1 or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?", version) or not re.fullmatch(r"[A-Za-z0-9_-]+", package):
        parser.error("invalid release metadata")
    digest = metadata["manifest"]["sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        parser.error("invalid manifest digest")
    record = {"version": version, "source": f"github-release:{args.repo}/{package}", "sha256": digest}
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(record, indent=2) + "\n")
    if not args.submit:
        print(json.dumps(record, indent=2))
        return
    repository = api(f"repos/{args.registry_repo}")
    branch = repository["default_branch"]
    base = api(f"repos/{args.registry_repo}/git/ref/heads/{branch}")["object"]["sha"]
    path = "extensions/" + args.name.split("/")[1] + ".json"
    current = api(f"repos/{args.registry_repo}/contents/{path}?ref={base}", missing=True)
    index = json.loads(base64.b64decode(current["content"])) if current else {"schema_version": 1, "name": args.name, "releases": []}
    if index["schema_version"] != 1 or index["name"] != args.name:
        raise RuntimeError("Registry identity/schema mismatch")
    existing = next((release for release in index["releases"] if release["version"] == version), None)
    if existing:
        if existing != record:
            raise RuntimeError("Registry version is immutable and already has different metadata")
        print(json.dumps({"status": "already_registered", "name": args.name, "version": version}))
        return
    index["releases"].append(record)
    content = (json.dumps(index, indent=2) + "\n").encode()
    head_repo = args.head_repo or args.registry_repo
    head = f"publish/{package}-{version}-{digest[:12]}"
    reference = api(f"repos/{head_repo}/git/ref/heads/{head}", missing=True)
    if reference is None:
        api(f"repos/{head_repo}/git/refs", "POST", {"ref": "refs/heads/" + head, "sha": base})
    previous = api(f"repos/{head_repo}/contents/{path}?ref={quote(head, safe='')}", missing=True)
    if previous is None or base64.b64decode(previous["content"]) != content:
        if reference is not None:
            raise RuntimeError("Existing review branch differs; inspect it before retrying")
        request = {"message": f"Register {args.name} {version}", "branch": head, "content": base64.b64encode(content).decode()}
        if previous:
            request["sha"] = previous["sha"]
        api(f"repos/{head_repo}/contents/{path}", "PUT", request)
    owner = head_repo.split("/")[0]
    pulls = api(f"repos/{args.registry_repo}/pulls?state=open&head={quote(owner + ':' + head, safe='')}")
    if pulls:
        pull = pulls[0]
    else:
        pull = api(f"repos/{args.registry_repo}/pulls", "POST", {"title": f"Register {args.name} {version}", "base": branch, "head": f"{owner}:{head}", "body": f"Registers the published `{record['source']}` package at `{version}`.\n\nManifest SHA-256: `{digest}`. Registry validation must pass before merge."})
    print(json.dumps({"status": "submitted", "name": args.name, "version": version, "pull_request": pull["html_url"]}))


if __name__ == "__main__":
    main()
