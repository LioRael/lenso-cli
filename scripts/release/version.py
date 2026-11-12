"""Expose the workspace release version and reject tags for other versions."""
import os
from pathlib import Path
import re

text = Path("Cargo.toml").read_text()
section = text.split("[workspace.package]\n", 1)[1].split("\n[", 1)[0]
version = re.search(r'^version = "(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)"$', section, re.M)[1]
if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ["GITHUB_REF_NAME"] != "v" + version:
    raise RuntimeError("Tag does not match workspace.package.version")
if os.environ.get("GITHUB_OUTPUT"):
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        stream.write("version=" + version + "\n")
print(version)
