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
import urllib.request
from github import request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--installer-url", required=True)
parser.add_argument("--repo", required=True)
parser.add_argument("--commit", required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--public-latest", action="store_true", help="Download and install the public latest release without credentials")
args = parser.parse_args()
args.out.mkdir(parents=True, exist_ok=False)
output = args.out.resolve()
windows = sys.platform == "win32"
installer = output / ("install.ps1" if windows else "install.sh")
if args.public_latest:
    expected = f"https://github.com/{args.repo}/releases/latest/download/{installer.name}"
    if args.installer_url != expected:
        parser.error("--public-latest requires the canonical latest installer URL")
    with urllib.request.urlopen(urllib.request.Request(expected, headers={"User-Agent": "lenso-install-verification"}), timeout=300) as response:
        installer.write_bytes(response.read())
else:
    installer.write_bytes(request(args.installer_url, binary=True))
prefix = output / "installed bin"
env = dict(os.environ)
if args.public_latest:
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)
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
receipt_path = prefix / 'lenso-install.json'
receipt_before = receipt_path.read_bytes()
receipt = json.loads(receipt_before)
assert receipt['schema_version'] == 1 and receipt['method'] == 'native'
assert receipt['version'] == args.version and receipt['binary'] == binary.name
assert receipt['binary_sha256'] == before
assert subprocess.check_output([str(binary), "--version"], env=dict(env, PATH=""), text=True).strip() == "lenso " + args.version
install(success=False)
damaged = output / ("damaged.zip" if windows else "damaged.tar.gz")
damaged.write_bytes(b"tampered archive")
extra = ["-Force", "-Archive", str(damaged)] if windows else ["--force", "--archive", str(damaged)]
failure = install(extra, success=False)
assert "SHA-256 mismatch" in failure.stdout + failure.stderr
assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
assert receipt_path.read_bytes() == receipt_before
receipt_path.unlink()
receipt_path.mkdir()
try:
    install(["-Force"] if windows else ["--force"], success=False)
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
finally:
    receipt_path.rmdir()
    receipt_path.write_bytes(receipt_before)
install(["-Force"] if windows else ["--force"])
assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
assert json.loads(receipt_path.read_bytes()) == receipt
# These commands must work independently of project extensions and PATH tools.
subprocess.run([str(binary), 'self', 'update', '--help'], env=dict(env, PATH=''), check=True)
offline = subprocess.run([str(binary), 'self', 'update', '--offline', '--json'],
                         env=dict(env, PATH=''), capture_output=True, text=True)
assert offline.returncode != 0 and 'requires network access' in offline.stdout + offline.stderr
# Exercise the real network updater in a disposable copy, keeping consumer verification intact.
update_dir = output / 'self-update'
update_dir.mkdir()
updater = update_dir / binary.name
shutil.copy2(binary, updater)
update_version = args.version if args.public_latest else '1.0.0'
update_env = dict(env, PATH='')
check = subprocess.run([str(updater), 'self', 'update', '--check', '--version', update_version, '--json'],
                       env=update_env, capture_output=True, text=True, timeout=600)
assert check.returncode == 0, check.stdout + check.stderr
assert not (update_dir / 'lenso-install.json').exists()
assert hashlib.sha256(updater.read_bytes()).hexdigest() == before
updated = subprocess.run([str(updater), 'self', 'update', '--adopt', '--version', update_version, '--json'],
                         env=update_env, capture_output=True, text=True, timeout=600)
for name in ('GH_TOKEN', 'GITHUB_TOKEN'):
    token = os.environ.get(name)
    if token and token in updated.stdout + updated.stderr:
        raise RuntimeError('Credential in self-update output')
assert updated.returncode == 0, updated.stdout + updated.stderr
result = json.loads(updated.stdout)['outputs']['main']
assert result['state'] == 'updated' and result['version'] == update_version
updated_receipt = json.loads((update_dir / 'lenso-install.json').read_bytes())
assert updated_receipt['version'] == update_version
assert updated_receipt['binary_sha256'] == hashlib.sha256(updater.read_bytes()).hexdigest()
assert subprocess.check_output([str(updater), '--version'], env=update_env, text=True).strip() == 'lenso ' + update_version
if update_version != args.version:
    assert updated_receipt['binary_sha256'] != before
root = Path(__file__).resolve().parents[2]
subprocess.run([sys.executable, str(root / "scripts/verify-github-release.py"), "--binary", str(binary),
                "--repo", args.repo, "--version", "0.1.2", "--ref", args.commit,
                "--token-env", "GITHUB_TOKEN", "--out", str(output / "consumer-check")], check=True)
report = {"cli_version": args.version, "platform": sys.platform, "binary_sha256": before,
          "installed": True, "refused_overwrite": True, "refused_tampering": True,
          "native_receipt_verified": True, "self_update_offline_verified": True,
          "online_self_update_verified": True, "self_update_target_version": update_version,
          "wasm_consumer_verified": True, "no_node_git_rust": True,
          "public_latest_without_credentials": args.public_latest}
(output / "installation.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report), flush=True)
