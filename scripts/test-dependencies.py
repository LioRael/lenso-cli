#!/usr/bin/env python3
"""Exercise cold remote preparation and cross-project cache reuse through the actual CLI."""
import argparse
import collections
import concurrent.futures
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--registry", action="store_true", help="Resolve the real Wasm fixture by package name through a static extension Registry")
args = parser.parse_args()

root = Path(__file__).resolve().parent.parent
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso"))
module = (root / "examples/m0/lenso/extensions/greeting/greeting.wasm").read_bytes()
pin = hashlib.sha256(module).hexdigest()
# The native/guest ABI is exercised by the real development fixture.
# Python 3.9 has no TOML parser; the fixture's authored metadata is also in its root lock.
lock = json.loads((root / "examples/m0/lenso.lock").read_text())
manifest = next(entry["definition"] for entry in lock["extensions"] if entry["id"] == "demo")
for handler in manifest["handlers"].values():
    if handler["type"] == "wasm":
        handler["sha256"] = pin
bodies = {"/extension.json": json.dumps(manifest).encode(), "/greeting.wasm": module}
bodies["/registry/demo.json"] = json.dumps({"schema_version":1,"name":"@test/demo","releases":[{"version":manifest["version"],"manifest":"../extension.json","sha256":hashlib.sha256(bodies["/extension.json"]).hexdigest()}]}).encode()
requests = collections.Counter()
mutex = threading.Lock()

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        with mutex:
            requests[self.path] += 1
        body = bodies.get(self.path)
        self.send_response(200 if body is not None else 404)
        self.send_header("Content-Length", str(len(body or b"")))
        self.end_headers()
        self.wfile.write(body or b"")

    def log_message(self, *args):
        pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
try:
    with tempfile.TemporaryDirectory(prefix="lenso-dependencies-") as directory:
        scratch = Path(directory)
        cache = scratch / "cache"
        env = dict(os.environ, LENSO_CACHE_DIR=str(cache))
        projects = [scratch / "first", scratch / "second"]
        for project in projects:
            project.mkdir()
            source = f"http://127.0.0.1:{server.server_port}/extension.json"
            registry = f'registries={{"@test"="http://127.0.0.1:{server.server_port}/registry/"}}\n' if args.registry else ""
            source = "registry:@test/demo" if args.registry else source
            (project / "lenso.toml").write_text(
                f'[cli]\nschema_version=1\n{registry}[[cli.extensions]]\nid="demo"\nsource={json.dumps(source)}\nfiles=["greeting.txt"]\n')

        def run(project, *args, expected=2):
            result = subprocess.run([str(binary), "--project", str(project), *args, "--json", "--color", "always"],
                                    capture_output=True, text=True, env=env, timeout=40)
            assert result.returncode == expected, (result.returncode, result.stdout, result.stderr)
            assert "\x1b" not in result.stdout and "Downloading" not in result.stderr
            return json.loads(result.stdout)

        result = run(projects[0], "greet", "--locked", expected=1)
        assert result["type"] == "diagnostic" and not requests
        for project in projects:
            result = run(project, "greet")
            assert result["status"] == "needs_input" and result["question"]["id"] == "name"
            assert (project / "lenso.lock").exists() and not (project / ".lenso").exists()
        assert requests["/extension.json"] == requests["/greeting.wasm"] == 1
        assert requests["/registry/demo.json"] == int(args.registry)
        lock_before = (projects[0] / "lenso.lock").read_bytes()
        result = run(projects[0], "preview", "--name", "Rust", "--locked", "--offline", expected=0)
        assert "+Hello, Rust!" in result["outputs"]["plan"]["diff"]
        assert not (projects[0] / "greeting.txt").exists()
        shutil.rmtree(cache)
        result = run(projects[0], "greet", "--offline", expected=1)
        assert result["code"] == "dependency_unavailable" and result["details"]["offline"]
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda _: run(projects[0], "greet"), range(4)))
        assert all(result["status"] == "needs_input" for result in results)
        assert requests["/greeting.wasm"] == 2, "concurrent CLI misses must download once"
        assert (projects[0] / "lenso.lock").read_bytes() == lock_before
        assert (cache / "v1/blobs/sha256" / pin).read_bytes() == module
    print(("Registry-name " if args.registry else "Direct-source ") + "dependency acceptance passed: cold remote execution, shared cache, locked/offline modes, concurrent restore, pure JSON")
finally:
    server.shutdown()
    server.server_close()
    thread.join()
