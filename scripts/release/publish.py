#!/usr/bin/env python3
"""Assemble a native CLI draft, or publish it after all native installer checks pass."""
import argparse
import json
import os
from pathlib import Path
import re
import urllib.parse
from github import api, request
from package import TARGETS, sha256

def asset_seal(assets):
    values = [{key: asset.get(key) for key in ("name", "id", "size", "state", "digest")} for asset in assets]
    return sha256(json.dumps(sorted(values, key=lambda asset: asset["name"]), sort_keys=True).encode())

def release_for(repo, tag):
    for page in range(1, 11):
        releases = api(repo, f"releases?per_page=100&page={page}")
        found = [release for release in releases if release["tag_name"] == tag]
        if found:
            if len(found) != 1:
                raise RuntimeError("Ambiguous release tag")
            return found[0]
        if len(releases) < 100:
            return None
    raise RuntimeError("Release pagination limit exceeded")

def check_tag(repo, tag, commit):
    # The commits API resolves lightweight and annotated tags to the final commit.
    try:
        actual = api(repo, "commits/" + urllib.parse.quote(tag, safe=""))["sha"]
    except RuntimeError as error:
        if "HTTP 404" in str(error):
            return
        raise
    if actual != commit:
        raise RuntimeError("Tag points to a different commit; tags are never moved")

def verify_asset(repo, asset, body):
    if asset["size"] != len(body) or asset["state"] != "uploaded":
        raise RuntimeError("Release asset size/state mismatch: " + asset["name"])
    digest = asset.get("digest")
    if digest is None:
        digest = "sha256:" + sha256(api(repo, "releases/assets/" + str(asset["id"]), binary=True))
    if digest != "sha256:" + sha256(body):
        raise RuntimeError("Conflicting release asset; nothing was overwritten: " + asset["name"])

def put(repo, release, name, body):
    matches = [asset for asset in release["assets"] if asset["name"] == name]
    if len(matches) > 1:
        raise RuntimeError("Duplicate release asset: " + name)
    if matches:
        verify_asset(repo, matches[0], body)
        return matches[0]
    if not release["draft"]:
        raise RuntimeError("Published releases are never modified")
    upload = release["upload_url"].split("{")[0]
    expected = f"https://uploads.github.com/repos/{repo}/releases/{release['id']}/assets"
    if upload != expected:
        raise RuntimeError("Unexpected release upload URL")
    asset = request(upload + "?" + urllib.parse.urlencode({"name": name}), method="POST", data=body, content_type="application/octet-stream")
    verify_asset(repo, asset, body)
    release["assets"].append(asset)
    print("Verified " + name, flush=True)
    return asset

def render_installers(directory, version, repo, assets, metadata):
    template = Path(__file__).parent
    shell = (template / "install.sh.in").read_text()
    rows = []
    for target in TARGETS:
        if "windows" in target:
            continue
        item = metadata[target]
        url = f"https://api.github.com/repos/{repo}/releases/assets/{assets[item['name']]['id']}"
        rows.append(f"  {target}) url='{url}'; sha256='{item['sha256']}' ;;")
    shell = shell.replace("@VERSION@", version).replace("@ASSETS@", "\n".join(rows))
    (directory / "install.sh").write_text(shell)
    item = metadata["x86_64-pc-windows-msvc"]
    url = f"https://api.github.com/repos/{repo}/releases/assets/{assets[item['name']]['id']}"
    powershell = (template / "install.ps1.in").read_text().replace("@VERSION@", version)
    powershell = powershell.replace("@ASSET@", f"$Url = '{url}'\n$Sha256 = '{item['sha256']}'")
    (directory / "install.ps1").write_text(powershell)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"), required=not os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--commit", default=os.environ.get("GITHUB_SHA"), required=not os.environ.get("GITHUB_SHA"))
    parser.add_argument("--version", required=True)
    parser.add_argument("--directory", type=Path, default=Path("dist/cli"))
    parser.add_argument("--finalize", action="store_true")
    parser.add_argument("--expected-seal", help="Asset identity/checksum seal approved by native install tests")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo) or not re.fullmatch(r"[a-f0-9]{40}", args.commit):
        parser.error("invalid repository/commit")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.version):
        parser.error("invalid version")
    tag = "v" + args.version
    check_tag(args.repo, tag, args.commit)
    release = release_for(args.repo, tag)
    if args.finalize:
        if release is None:
            raise RuntimeError("Draft is missing")
        if not args.expected_seal or asset_seal(release["assets"]) != args.expected_seal:
            raise RuntimeError("Release assets changed after installer verification")
        # Install tests have completed; verify the entire sealed asset set once more.
        checksums_asset = next(asset for asset in release["assets"] if asset["name"] == "SHA256SUMS")
        checksums = api(args.repo, "releases/assets/" + str(checksums_asset["id"]), binary=True)
        verify_asset(args.repo, checksums_asset, checksums)
        pins = {}
        for line in checksums.decode().splitlines():
            digest, name = line.split("  ", 1)
            if name in pins or not re.fullmatch(r"[a-f0-9]{64}", digest):
                raise RuntimeError("Invalid checksum manifest")
            pins[name] = digest
        expected_names = {f"lenso-{tag}-{target}.{'zip' if 'windows' in target else 'tar.gz'}" for target in TARGETS} | {"install.sh", "install.ps1"}
        if set(pins) != expected_names or {a["name"] for a in release["assets"]} != expected_names | {"SHA256SUMS"} or len(release["assets"]) != 8:
            raise RuntimeError("Unexpected/incomplete release asset set")
        for asset in release["assets"]:
            if asset["name"] in pins and asset.get("digest") != "sha256:" + pins[asset["name"]]:
                body = api(args.repo, "releases/assets/" + str(asset["id"]), binary=True)
                if sha256(body) != pins[asset["name"]]:
                    raise RuntimeError("Asset changed after installer verification")
        if release["target_commitish"] != args.commit:
            raise RuntimeError("Draft target changed")
        if release["draft"]:
            release = api(args.repo, "releases/" + str(release["id"]), method="PATCH", data={"draft": False, "make_latest": "false" if "-" in args.version else "true"})
        if release["draft"]:
            raise RuntimeError("Release was not published")
        print("Published " + release["html_url"])
        return
    metadata = {}
    bodies = {}
    for target in TARGETS:
        item = json.loads((args.directory / (target + ".json")).read_text())
        expected_name = f"lenso-{tag}-{target}.{'zip' if 'windows' in target else 'tar.gz'}"
        if item["target"] != target or item["version"] != args.version or item["name"] != expected_name:
            raise RuntimeError("Package identity mismatch")
        body = (args.directory / expected_name).read_bytes()
        if sha256(body) != item["sha256"] or len(body) != item["size"]:
            raise RuntimeError("Package checksum mismatch")
        metadata[target] = item
        bodies[expected_name] = body
    if release is None:
        release = api(args.repo, "releases", method="POST", data={"tag_name": tag, "target_commitish": args.commit, "name": "Lenso CLI " + tag, "draft": True, "prerelease": "-" in args.version})
    if release["target_commitish"] != args.commit or release["prerelease"] != ("-" in args.version):
        raise RuntimeError("Existing release target/prerelease differs")
    print(f"Preparing Release {release['id']}; failures preserve drafts and assets", flush=True)
    assets = {name: put(args.repo, release, name, body) for name, body in bodies.items()}
    render_installers(args.directory, args.version, args.repo, assets, metadata)
    for name in ("install.sh", "install.ps1"):
        bodies[name] = (args.directory / name).read_bytes()
        assets[name] = put(args.repo, release, name, bodies[name])
    checksums = "".join(f"{sha256(body)}  {name}\n" for name, body in sorted(bodies.items())).encode()
    assets["SHA256SUMS"] = put(args.repo, release, "SHA256SUMS", checksums)
    if {asset["name"] for asset in release["assets"]} != set(assets) or len(release["assets"]) != 8:
        raise RuntimeError("Unexpected release assets; nothing was removed")
    script_sh = f"https://api.github.com/repos/{args.repo}/releases/assets/{assets['install.sh']['id']}"
    script_ps = f"https://api.github.com/repos/{args.repo}/releases/assets/{assets['install.ps1']['id']}"
    body = f"Native CLI for macOS (Intel/Apple Silicon), Linux (x64/ARM64, glibc 2.35+), Windows x64.\n\nNo Node.js, Git or Rust is required. Archives and installers are pinned by SHA256SUMS.\n\nPublic install:\n```sh\ncurl -fsSL https://github.com/{args.repo}/releases/download/{tag}/install.sh -o install.sh\nsh install.sh\n```\n\nPrivate repository (set GH_TOKEN):\n```sh\ncurl -fsSL -H \"Authorization: Bearer $GH_TOKEN\" -H 'Accept: application/octet-stream' '{script_sh}' -o install.sh\nsh install.sh\n```\n\nWindows: download install.ps1, then run `& ./install.ps1`. Authenticated installer API: {script_ps}\n\nInstallers install this exact version, verify SHA-256 and refuse replacement without --force / -Force. Add the printed install directory to PATH."
    if release["draft"]:
        api(args.repo, "releases/" + str(release["id"]), method="PATCH", data={"body": body})
    current = api(args.repo, "releases/" + str(release["id"]))
    outputs = {"release_id": release["id"], "install_sh_url": script_sh, "install_ps1_url": script_ps,
               "asset_seal": asset_seal(current["assets"])}
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            for key, value in outputs.items():
                stream.write(f"{key}={value}\n")
    print(json.dumps(outputs))

if __name__ == "__main__":
    main()
