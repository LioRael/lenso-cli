#!/usr/bin/env python3
"""Install a real Release asset without development tools, then execute its Wasm workflow."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from github import request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--installer-url", required=True)
parser.add_argument("--repo", required=True)
parser.add_argument("--commit", required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--out", type=Path, required=True)
args = parser.parse_args()
args.out.mkdir(parents=True, exist_ok=False)
output = args.out.resolve()
windows = sys.platform == "win32"
installer = output / ("install.ps1" if windows else "install.sh")
installer.write_bytes(request(args.installer_url, binary=True))
prefix = output / "installed bin"
env = dict(os.environ)
if windows:
    shell = shutil.which("pwsh")
    if not shell:
        raise RuntimeError("CI test driver requires PowerShell")
    command = [shell, "-NoLogo", "-NoProfile", "-NonInteractive", "-File", str(installer), "-InstallDir", str(prefix)]
    env["PATH"] = ""
else:
    tools = output / "system-tools"
    tools.mkdir()
    for tool in ("uname", "curl", "tar", "gzip", "mktemp", "rm", "chmod", "mkdir", "cat", "mv", "sha256sum", "shasum"):
        path = shutil.which(tool)
        if path:
            (tools / tool).symlink_to(path)
    env["PATH"] = str(tools)
    command = ["/bin/sh", str(installer), "--prefix", str(prefix)]

def install(extra=(), success=True):
    result = subprocess.run(command + list(extra), env=env, capture_output=True, text=True, timeout=600)
    for name in ("GITHUB_TOKEN", "GH_TOKEN"):
        token = os.environ.get(name)
        if token and token in result.stdout + result.stderr:
            raise RuntimeError("Credential in installer output")
    if (result.returncode == 0) != success:
        raise RuntimeError(f"Installer exit {result.returncode}\n{result.stdout}\n{result.stderr}")
    print(result.stdout, end="", flush=True)
    return result

install()
binary = prefix / ("lenso.exe" if windows else "lenso")
before = hashlib.sha256(binary.read_bytes()).hexdigest()
assert subprocess.check_output([str(binary), "--version"], env=dict(env, PATH=""), text=True).strip() == "lenso " + args.version
install(success=False)
damaged = output / ("damaged.zip" if windows else "damaged.tar.gz")
damaged.write_bytes(b"tampered archive")
extra = ["-Force", "-Archive", str(damaged)] if windows else ["--force", "--archive", str(damaged)]
failure = install(extra, success=False)
assert "SHA-256 mismatch" in failure.stdout + failure.stderr
assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
root = Path(__file__).resolve().parents[2]
subprocess.run([sys.executable, str(root / "scripts/verify-github-release.py"), "--binary", str(binary),
                "--repo", args.repo, "--version", "0.1.1", "--ref", args.commit,
                "--token-env", "GITHUB_TOKEN", "--out", str(output / "consumer-check")], check=True)
report = {"cli_version": args.version, "platform": sys.platform, "binary_sha256": before,
          "installed": True, "refused_overwrite": True, "refused_tampering": True,
          "wasm_consumer_verified": True, "no_node_git_rust": True}
(output / "installation.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report), flush=True)
