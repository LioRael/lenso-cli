#!/usr/bin/env python3
"""Accept the optional adapter against real Acme, MCP stdio and Node/Rust Ops."""

import json
import http.server
import os
from pathlib import Path
import platform
import queue
import shutil
import signal
import socket
import subprocess
import tempfile
import threading
import time
from urllib.request import urlopen

from project_config import literal, save

root = Path(__file__).resolve().parent.parent
adapter = Path(os.environ.get("LENSO_AGENT_BIN", root / "target/debug/lenso-agent")).resolve()
acme = Path(os.environ.get("ACME_BIN", root / "target/debug/acme")).resolve()
lenso = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso")).resolve()
cargo = os.environ.get("CARGO", "cargo")
secret = "adapter-boundary-private-fixture-value"
cases = []


def passed(name):
    cases.append(name)
    print("PASS:", name, flush=True)


def command(project, allowed, action, program=acme, timeout=30000):
    args = [str(adapter), "--program", str(program), "--project", str(project), "--offline",
            "--timeout-ms", str(timeout)]
    for path in allowed:
        args += ["--allow", path]
    return args + action


def execute(args, expected=0, env=None):
    result = subprocess.run(args, capture_output=True, text=True, env=env, timeout=30)
    assert result.returncode == expected, (args, result.returncode, result.stdout, result.stderr)
    assert secret not in result.stdout + result.stderr
    return json.loads(result.stdout)


class Mcp:
    def __init__(self, project, allowed, program=acme, timeout=30000, env=None):
        self.stderr = tempfile.TemporaryFile()
        self.process = subprocess.Popen(command(project, allowed, ["mcp"], program, timeout),
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, env=env)
        self.messages = queue.Queue()
        self.pending = {}
        self.next_id = 0

        def read():
            try:
                for line in self.process.stdout:
                    self.messages.put(json.loads(line))
            except Exception as error:
                self.messages.put(error)
            finally:
                self.messages.put(EOFError())

        threading.Thread(target=read, daemon=True).start()
        initialized = self.request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                                  "clientInfo": {"name": "lenso-acceptance", "version": "1"}})
        assert initialized["result"]["protocolVersion"] == "2025-11-25", initialized
        assert initialized["result"]["capabilities"]["tools"] == {}
        self.notify("notifications/initialized", {})

    def send(self, method, params):
        self.next_id += 1
        self.write({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        return self.next_id

    def write(self, value):
        self.process.stdin.write((json.dumps(value) + "\n").encode())
        self.process.stdin.flush()

    def notify(self, method, params):
        self.write({"jsonrpc": "2.0", "method": method, "params": params})

    def receive(self, request_id):
        deadline = time.monotonic() + 15
        while request_id not in self.pending:
            value = self.messages.get(timeout=max(0.01, deadline - time.monotonic()))
            assert not isinstance(value, Exception), value
            assert secret not in json.dumps(value)
            if "id" in value:
                self.pending[value["id"]] = value
        return self.pending.pop(request_id)

    def request(self, method, params=None):
        return self.receive(self.send(method, params or {}))

    def call(self, name, arguments=None):
        value = self.request("tools/call", {"name": name, "arguments": arguments or {}})
        assert "result" in value, value
        result = value["result"]
        assert json.loads(result["content"][0]["text"]) == result["structuredContent"]
        return result

    def report(self, uri):
        result = self.request("resources/read", {"uri": uri})
        assert "result" in result, result
        contents = result["result"]["contents"][0]
        assert contents["uri"] == uri and contents["mimeType"] == "application/json"
        return json.loads(contents["text"])

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=8)
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait()
        assert self.process.returncode == 0, self.process.returncode
        self.stderr.seek(0)
        assert secret.encode() not in self.stderr.read()
        self.stderr.close()


with tempfile.TemporaryDirectory(prefix="lenso-agent-acceptance-") as directory:
    scratch = Path(directory)
    project = scratch / "acme-project"
    project.mkdir()
    environment = dict(os.environ, PATH="", LENSO_CACHE_DIR=str(scratch / "cache"), LENSO_NO_UPDATE_CHECK="1")
    catalog = execute([str(acme), "--project", str(project), "help", "--json", "--offline"], env=environment)
    assert {tuple(c["path"]) for c in catalog["commands"]} >= {("hello",), ("status",), ("note",)}
    assert not (project / "lenso.lock").exists()
    assert execute(command(project, [], ["tools"]), env=environment)["tools"] == []
    tools = execute(command(project, ["hello", "status", "note"], ["tools"]), env=environment)["tools"]
    assert {tool["name"] for tool in tools} == {"hello", "status", "note"}
    for tool in tools:
        native = next(c for c in catalog["commands"] if c["path"] == [tool["name"]])
        assert tool["_meta"]["lenso/command"] == native
        assert tool["inputSchema"]["properties"]["input"] == native["contract"]["input_schema"]
    assert next(t for t in tools if t["name"] == "note")["inputSchema"]["required"] == ["input"]
    assert "required" not in next(t for t in tools if t["name"] == "status")["inputSchema"]
    assert not (project / "acme-note.txt").exists() and not (project / "lenso.lock").exists()
    passed("explicit selection and shared static contracts; no standalone Lenso or PATH required")

    native = execute([str(acme), "--project", str(project), "status", "--json"], env=environment)
    adapted = execute(command(project, ["status"], ["call", "--name", "status"]), env=environment)
    assert adapted["result"]["outputs"] == native["outputs"]
    human = subprocess.run([str(acme), "--project", str(project), "status", "--no-interactive"],
                           env=environment, capture_output=True, text=True, check=True)
    assert "Completed" in human.stdout
    denied = execute(command(project, ["status"], ["call", "--name", "note"]), expected=1, env=environment)
    assert denied["code"] == "permission_denied"
    passed("Shell, native JSON and selected Agent call agree; unselected operation denied")

    client = Mcp(project, ["hello", "status", "note"], env=environment)
    try:
        listed = client.request("tools/list")["result"]["tools"]
        assert listed == tools
        assert next(t for t in listed if t["name"] == "status")["annotations"]["readOnlyHint"]
        assert "annotations" not in next(t for t in listed if t["name"] == "note")
        assert "error" in client.request("tools/call", {"name": "config", "arguments": {}})
        passed("MCP initialization, selected descriptions and conservative effect hints")

        for arguments in [{"input": {"text": 7}}, {"input": {"text": "x", "project": "/"}},
                          {"yes": True}, {"capabilities": {"project.files": ["*"]}},
                          {"input": {"text": secret, "unknown": secret}}, {}]:
            result = client.call("note", arguments)
            assert result["isError"] and result["structuredContent"]["result"]["code"] == "invalid_input", result
        assert not (project / "acme-note.txt").exists()
        passed("Schema types/required/unknown fields checked without echoing rejected values or accepting grants")

        denied = client.call("note", {"input": {"text": "first"}})
        assert denied["isError"] and denied["structuredContent"]["result"]["code"] == "permission_denied"
        assert denied["structuredContent"]["result"]["details"]["capability"] == "project.files"
        unicode_note = client.call("note", {"input": {"text": "🙂" * 2050}})
        assert unicode_note["structuredContent"]["result"]["code"] == "permission_denied", unicode_note
        (project / "acme-policy.json").write_text('{"write_note":true}\n')
        pending = client.call("note", {"input": {"text": "first"}})["structuredContent"]
        assert pending["exit_code"] == 2 and pending["result"]["status"] == "needs_input"
        question = pending["result"]["question"]
        assert question["kind"]["type"] == "confirm" and not (project / "acme-note.txt").exists()
        declined = client.call("note", {"input": {"text": "first"}, "answers": {question["id"]: False}})
        assert declined["isError"] and declined["structuredContent"]["exit_code"] == 130
        assert not (project / "acme-note.txt").exists()
        approved = client.call("note", {"input": {"text": "first"}, "answers": {question["id"]: True}})
        assert not approved["isError"]
        assert approved["structuredContent"]["report_complete"] is True
        result = approved["structuredContent"]["result"]
        assert result["status"] == "complete" and result["mode"] == "summary" and "outputs" not in result
        assert (project / "acme-note.txt").read_text() == "first"
        full = client.report(result["report"])
        assert full["execution"]["id"] == result["execution"]["id"]
        assert full["outputs"]["apply"]["applied"] == ["acme-note.txt"]
        assert full["execution"]["calls"][-1]["receipt"]["affected"] == ["acme-note.txt"]
        assert full["execution"]["application_verification"] == "not_reported"
        assert (project / "acme-note.txt").read_text() == "first"
        passed("Host grant plus digest-bound approval; compact receipt and same-invocation full report")

        pending = client.call("note", {"input": {"text": "second"}})["structuredContent"]["result"]
        (project / "acme-note.txt").write_text("concurrent edit")
        stale = client.call("note", {"input": {"text": "second"}, "answers": {pending["question"]["id"]: True}})
        assert stale["structuredContent"]["exit_code"] == 2
        assert stale["structuredContent"]["result"]["question"]["id"] != pending["question"]["id"]
        assert (project / "acme-note.txt").read_text() == "concurrent edit"
        (project / "acme-policy.json").write_text('{"write_note":false}\n')
        revoked = client.call("note", {"input": {"text": "second"}, "answers": {pending["question"]["id"]: True}})
        assert revoked["structuredContent"]["result"]["code"] == "permission_denied"
        passed("Changed preimages invalidate approval; host grant revocation applies to an existing MCP connection")

        missing = client.call("hello")["structuredContent"]
        assert missing["exit_code"] == 2 and missing["result"]["question"]["id"] == "name"
        answered = client.call("hello", {"answers": {"name": "Agent"}, "mode": "full"})
        assert answered["structuredContent"]["result"]["outputs"]["main"]["greeting"] == "Hello, Agent!"
        assert "error" in client.request("resources/read", {"uri": "file:///etc/passwd"})
        old_uri = result["report"]
        for _ in range(17):
            client.call("status")
        assert "error" in client.request("resources/read", {"uri": old_uri})
        assert len(client.request("resources/list")["result"]["resources"]) == 16
        passed("Text questions stay explicit; report lookup cannot read arbitrary paths and retention is bounded")
    finally:
        client.close()

    saved = scratch / "full-report.json"
    compact = execute(command(project, ["status"], ["call", "--name", "status", "--arguments",
                       '{"mode":"summary"}', "--report", str(saved)]), env=environment)
    assert compact["result"]["report"] == str(saved)
    assert json.loads(saved.read_text())["execution"]["id"] == compact["result"]["execution"]["id"]
    assert saved.stat().st_mode & 0o777 == 0o600
    failed = execute(command(project, ["note"], ["call", "--name", "note", "--report", str(saved),
                     "--arguments", '{"input":{"text":"must not run"}}']), expected=1, env=environment)
    assert failed["code"] == "io_error" and (project / "acme-note.txt").read_text() == "concurrent edit"
    passed("One-shot summary saves a private full report; overwrite is rejected before execution")

    extension = {"schema_version": 1, "id": "probe", "version": "0.1.0",
                 "handlers": {"probe/work": {"type": "process", "program": "/bin/echo", "args": ["probe"]}},
                 "workflows": [{"id": "probe/work", "steps": [{"id": "work", "handler": "probe/work"}]}],
                 "commands": [{"path": ["probe"], "workflow": "probe/work", "provider": "probe", "about": "Original contract"}],
                 "contracts": {"probe/work": {"input_schema": {"type": "object"}}}}
    manifest = project / "probe.json"
    manifest.write_text(json.dumps(extension))
    save(project / "lenso.toml", {"cli": {"schema_version": 1, "extensions": [{"id": "probe", "source": "probe.json"}]}})
    client = Mcp(project, ["probe"], env=environment)
    try:
        extension["commands"][0]["about"] = "Changed contract"
        manifest.write_text(json.dumps(extension))
        changed = client.call("probe")
        assert changed["isError"] and changed["structuredContent"]["result"]["code"] == "stale_plan", changed
    finally:
        client.close()
    passed("Contract drift blocks execution before the newly described operation can run")

    requests = []

    class SchemaServer(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"type":"object"}')

        def log_message(self, *args):
            pass

    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), SchemaServer) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        reference = project / "external-schema.json"
        reference.write_text('{"type":"object"}')
        for uri in [reference.as_uri(), f"http://127.0.0.1:{server.server_port}/schema"]:
            extension["contracts"]["probe/work"]["input_schema"] = {"$ref": uri}
            manifest.write_text(json.dumps(extension))
            rejected = execute(command(project, ["probe"], ["tools"]), expected=1, env=environment)
            assert rejected["code"] == "invalid_input" and rejected["details"]["reason"] == "invalid_or_external_schema"
            assert execute(command(project, ["status"], ["tools"]), env=environment)["tools"][0]["name"] == "status"
        server.shutdown()
        thread.join()
    assert not requests
    passed("Selected schemas cannot fetch HTTP/file references; unrelated schemas are not compiled")

    client = Mcp(project, ["status"], env=environment)
    try:
        (project / "acme-policy.json").write_text(secret)
        failure = client.call("status")
        assert failure["isError"] and failure["structuredContent"]["result"]["code"] == "invalid_input", failure
    finally:
        (project / "acme-policy.json").write_text('{"write_note":false}\n')
        client.close()
    passed("Discovery failures retain the native diagnostic code without leaking internal parse input")

    if no_mcp := os.environ.get("LENSO_AGENT_NO_MCP_BIN"):
        help_text = subprocess.check_output([no_mcp, "--help"], text=True)
        assert "  mcp" not in help_text
        disabled = execute([no_mcp, "--program", str(acme), "--project", str(project), "mcp"], expected=2)
        assert disabled["code"] == "invalid_input"
        passed("Default Agent binary has no MCP command")

    for language in ["node", "rust"]:
        project = scratch / language
        shutil.copytree(root / "fixtures/ops" / language, project)
        if language == "node":
            node = shutil.which("node")
            assert node
            program = subprocess.check_output([node, "-p", "process.execPath"], text=True).strip()
            arguments = ["server.mjs"]
        else:
            subprocess.run([cargo, "build", "--locked", "--offline", "--manifest-path", str(project / "Cargo.toml")], check=True)
            program, arguments = str(project / "target/debug/lenso-ops-rust-fixture"), []
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        url = f"http://127.0.0.1:{port}/health/ready"
        shutil.copyfile(root / "presets/ops/extension.toml", project / "ops-extension.toml")
        save(project / "lenso.toml", {"cli": {"schema_version": 1, "extensions": [{
            "id": "ops", "source": "ops-extension.toml", "reads": ["lenso.ops.toml"],
            "env": ["OPS_MODE", "OPS_PORT", "OPS_PID_FILE", "OPS_SECRET"],
            "http": [f"http://127.0.0.1:{port}/health/"], "processes": [{"program": program, "args": arguments}],
        }]}})
        definition = {"program": program, "args": arguments, "cwd": ".",
                      "env": ["OPS_MODE", "OPS_PORT", "OPS_PID_FILE", "OPS_SECRET"],
                      "timeout_ms": 15000, "readiness_timeout_ms": 3000,
                      "readiness": {"type": "http", "url": url, "status": 200}}
        workflows = {"dev": definition, "check": {**definition, "readiness": {"type": "exit", "code": 0}}}
        (project / "lenso.ops.toml").write_text("schema_version = 1\nworkflows = " + literal(workflows) + "\n")
        source_files = {path: path.read_bytes() for path in project.rglob("*")
                        if path.is_file() and "target" not in path.relative_to(project).parts}
        environment = dict(os.environ, LENSO_CACHE_DIR=str(scratch / "cache"), LENSO_NO_UPDATE_CHECK="1",
                           OPS_MODE="ready", OPS_PORT=str(port), OPS_PID_FILE="pids.json", OPS_SECRET=secret)

        def healthy():
            deadline = time.monotonic() + 6
            while time.monotonic() < deadline:
                try:
                    with urlopen(url, timeout=0.25) as response:
                        if response.status == 200:
                            return
                except OSError:
                    pass
                time.sleep(0.025)
            raise AssertionError("fixture did not become healthy")

        def clean():
            pids = json.loads((project / "pids.json").read_text())
            for pid in [pids["parent"], *pids["children"]]:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    try:
                        os.kill(pid, 0)
                    except ProcessLookupError:
                        break
                    time.sleep(0.025)
                else:
                    raise AssertionError(("process remains", pid))
            with socket.socket() as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", port))
            (project / "pids.json").unlink()

        client = Mcp(project, ["ops run", "ops verify"], lenso, env=environment)
        try:
            request_id = client.send("tools/call", {"name": "ops__run", "arguments": {"input": {"workflow": "dev"}}})
            healthy()
            verified = client.call("ops__verify", {"input": {"workflow": "dev"}, "mode": "full"})
            assert verified["structuredContent"]["result"]["outputs"]["verify"]["health"]["status"] == 200
            client.notify("notifications/cancelled", {"requestId": request_id, "reason": secret})
            deadline = time.monotonic() + 6
            while True:
                resources = client.request("resources/list")["result"]["resources"]
                reports = [client.report(resource["uri"]) for resource in resources]
                cancelled = next((r for r in reports if r.get("code") == "cancelled"), None)
                if cancelled:
                    break
                assert time.monotonic() < deadline, reports
                time.sleep(0.05)
            assert cancelled["details"]["cleanup"]["child_reaped"]
            assert cancelled["details"]["cleanup"]["logs_drained"]
            assert "[redacted]" in cancelled["details"]["stdout"]["text"]
            clean()
            passed(language + ": real HTTP readiness and MCP request cancellation preserve Ops cleanup evidence")
        finally:
            client.close()

        environment["OPS_MODE"] = "exit7"
        client = Mcp(project, ["ops run"], lenso, env=environment)
        try:
            failure = client.call("ops__run", {"input": {"workflow": "check"}})
            assert failure["isError"] and failure["structuredContent"]["exit_code"] == 7
            assert failure["structuredContent"]["result"]["code"] == "process_failed"
            assert failure["structuredContent"]["result"]["details"]["exit_code"] == 7
            passed(language + ": actual nonzero exit and structured failure survive MCP transport")
        finally:
            client.close()

        environment["OPS_MODE"] = "ready"
        client = Mcp(project, ["ops run"], lenso, env=environment)
        client.send("tools/call", {"name": "ops__run", "arguments": {"input": {"workflow": "dev"}}})
        healthy()
        client.close()
        clean()
        passed(language + ": MCP EOF cancels active execution and leaves no fixture process or listener")

        client = Mcp(project, ["ops run"], lenso, timeout=1800, env=environment)
        try:
            request_id = client.send("tools/call", {"name": "ops__run", "arguments": {"input": {"workflow": "dev"}}})
            healthy()
            timeout = client.receive(request_id)["result"]
            assert timeout["isError"] and timeout["structuredContent"]["result"]["code"] == "process_timeout", timeout
            assert timeout["structuredContent"]["exit_code"] == 1
            full = client.report(timeout["structuredContent"]["result"]["report"])
            assert full["details"]["cleanup"]["child_reaped"]
            assert full["adapter_diagnostic"]["code"] == "process_timeout"
            clean()
            passed(language + ": adapter deadline forwards cancellation and retains target cleanup plus timeout evidence")
        finally:
            client.close()

        client = Mcp(project, ["ops run"], lenso, env=environment)
        client.send("tools/call", {"name": "ops__run", "arguments": {"input": {"workflow": "dev"}}})
        healthy()
        client.process.send_signal(signal.SIGTERM)
        client.process.wait(timeout=8)
        client.close()
        clean()
        passed(language + ": SIGTERM to the adapter cleans up its active runner and application")
        for path, original in source_files.items():
            assert path.read_bytes() == original, ("adapter changed fixture source", path)

    report = {"status": "passed", "cases": cases,
              "platform": {"system": platform.system(), "machine": platform.machine()},
              "versions": {"agent": subprocess.check_output([str(adapter), "--version"], text=True).strip(),
                           "acme": subprocess.check_output([str(acme), "--version"], text=True).strip(),
                           "cargo": subprocess.check_output([cargo, "--version"], text=True).strip(),
                           "node": subprocess.check_output([shutil.which("node"), "--version"], text=True).strip()},
              "scope": "real native binaries, local stdio, fake secrets, isolated projects"}
    if path := os.environ.get("LENSO_AGENT_REPORT"):
        Path(path).write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
