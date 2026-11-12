#!/usr/bin/env python3
"""Package an already-built native CLI; runs on each target's native runner."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import struct
import tarfile
import zipfile

TARGETS = (
    "aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc",
)

def sha256(data):
    return hashlib.sha256(data).hexdigest()

def check_architecture(body, target):
    if "linux" in target:
        valid = body[:6] == b"\x7fELF\x02\x01" and struct.unpack_from("<H", body, 18)[0] == (183 if target.startswith("aarch64") else 62)
    elif "darwin" in target:
        valid = body[:4] == b"\xcf\xfa\xed\xfe" and struct.unpack_from("<I", body, 4)[0] == (0x100000C if target.startswith("aarch64") else 0x1000007)
    else:
        offset = struct.unpack_from("<I", body, 60)[0]
        valid = body[:2] == b"MZ" and body[offset:offset+4] == b"PE\0\0" and struct.unpack_from("<H", body, offset+4)[0] == 0x8664
    if not valid:
        raise RuntimeError("Executable format/architecture does not match " + target)

def dependencies(body, target, binary):
    if "darwin" in target:
        lines = subprocess.check_output(["otool", "-L", str(binary)], text=True).splitlines()[1:]
        names = [line.strip().split(" (", 1)[0] for line in lines]
        if any(not name.startswith(("/usr/lib/", "/System/Library/")) for name in names):
            raise RuntimeError("Executable depends on a non-system dylib")
        return names
    if "linux" in target:
        output = subprocess.check_output(["ldd", str(binary)], text=True)
        if "not found" in output or "/home/runner/" in output:
            raise RuntimeError("Executable has unavailable/build-directory shared libraries")
        return [line.strip().split(" => ", 1)[0].split(" (", 1)[0] for line in output.splitlines()]
    pe = struct.unpack_from("<I", body, 60)[0]
    sections = struct.unpack_from("<H", body, pe + 6)[0]
    optional = pe + 24
    section_table = optional + struct.unpack_from("<H", body, pe + 20)[0]
    def offset(rva):
        for index in range(sections):
            base = section_table + 40 * index
            virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from("<IIII", body, base + 8)
            if virtual_address <= rva < virtual_address + max(virtual_size, raw_size):
                return raw_offset + rva - virtual_address
        raise RuntimeError("Invalid PE import address")
    table = offset(struct.unpack_from("<I", body, optional + 120)[0])
    names = []
    while any(body[table:table + 20]):
        name = offset(struct.unpack_from("<I", body, table + 12)[0])
        names.append(body[name:body.index(b"\0", name)].decode("ascii"))
        table += 20
    if any(name.lower().startswith(("vcruntime", "msvcp", "msvcr")) for name in names):
        raise RuntimeError("Executable requires a Visual C++ Redistributable; build with crt-static")
    return names

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", args.version):
        parser.error("invalid version")
    binary = args.binary.resolve(strict=True)
    actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if actual != "lenso " + args.version:
        raise RuntimeError(f"binary version mismatch: {actual}")
    subprocess.run([str(binary), "--help"], check=True, stdout=subprocess.DEVNULL)
    name = "lenso.exe" if "windows" in args.target else "lenso"
    body = binary.read_bytes()
    check_architecture(body, args.target)
    metadata = {"version": args.version, "target": args.target, "binary": name,
                "binary_sha256": sha256(body), "system_dependencies": dependencies(body, args.target, binary)}
    files = {name: body, "release.json": (json.dumps(metadata, indent=2) + "\n").encode(),
             "README.txt": (f"Lenso CLI {args.version}\nTarget: {args.target}\n\n"
                            "Copy the executable to a directory on PATH. No Node.js, Git or Rust is required.\n"
                            "https://github.com/LioRael/lenso-cli\n").encode()}
    args.out.mkdir(parents=True, exist_ok=True)
    suffix = "zip" if "windows" in args.target else "tar.gz"
    archive_name = f"lenso-v{args.version}-{args.target}.{suffix}"
    archive = args.out / archive_name
    if suffix == "zip":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for path, data in files.items():
                info = zipfile.ZipInfo(path, (1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                output.writestr(info, data)
    else:
        stream = io.BytesIO()
        with gzip.GzipFile(fileobj=stream, mode="wb", mtime=0, filename="") as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as output:
                for path, data in files.items():
                    info = tarfile.TarInfo(path)
                    info.size = len(data)
                    info.mode = 0o755 if path == name else 0o644
                    output.addfile(info, io.BytesIO(data))
        archive.write_bytes(stream.getvalue())
    metadata.update({"name": archive_name, "sha256": sha256(archive.read_bytes()), "size": archive.stat().st_size})
    (args.out / (args.target + ".json")).write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata))

if __name__ == "__main__":
    main()
