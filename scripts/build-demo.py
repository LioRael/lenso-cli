#!/usr/bin/env python3
"""Build the local Extism fixture; commands prepare their own lock and shared cache."""

import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
crate = root / "examples/wasm-greeting"
cargo = os.environ.get("CARGO", "cargo")
subprocess.run(
    [cargo, "build", "--manifest-path", str(crate / "Cargo.toml"),
     "--target", "wasm32-unknown-unknown", "--release", "--target-dir", str(crate / "target")],
    check=True,
    cwd=root,
)
wasm = crate / "target/wasm32-unknown-unknown/release/greeting_extension.wasm"
destination = root / "examples/m0/lenso/extensions/greeting"
shutil.copyfile(wasm, destination / "greeting.wasm")
print("Built M0 source artifact; the first command prepares its lock and shared cache")
