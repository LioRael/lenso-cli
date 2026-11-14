#!/usr/bin/env python3
"""Exercise optional Ops with real Node/Rust processes, health checks and cleanup."""

import copy
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time
from urllib.request import urlopen

from project_config import literal, save

root = Path(__file__).resolve().parent.parent
binary = Path(os.environ.get("LENSO_BIN", root / "target/debug/lenso")).resolve()
cargo = os.environ.get("CARGO", "cargo")
if os.name != "posix":
    raise SystemExit("Ops acceptance requires Unix process groups")
node = shutil.which("node")
assert node, "Node is required for both-language Ops acceptance"
node = subprocess.check_output([node, "-p", "process.execPath"], text=True).strip()
secret = "ops-fixture-private-value-89f3c"
cases = []

with tempfile.TemporaryDirectory(prefix="lenso-ops-") as directory:
    scratch = Path(directory)
    cache = scratch / "cache"
    empty = scratch / "empty"
    empty.mkdir()

    def invoke(project, *arguments, expected=0, env=None):
        result = subprocess.run([str(binary), "--project", str(project), *arguments, "--json"],
                                env=env or environment(project), capture_output=True, text=True, timeout=20)
        assert result.returncode == expected, (arguments, result.returncode, result.stdout, result.stderr)
        assert secret not in result.stdout + result.stderr
        value = json.loads(result.stdout)
        assert "ambient leaked" not in result.stdout
        return value

    def environment(project, mode="ready"):
        return dict(os.environ, LENSO_CACHE_DIR=str(cache), LENSO_NO_UPDATE_CHECK="1",
                    OPS_MODE=mode, OPS_PORT=str(ports.get(project, 1)), OPS_PID_FILE="pids.json",
                    OPS_SECRET=secret, UNGRANTED_SECRET="must-not-be-inherited")

    def free_port():
        with socket.socket() as connection:
            connection.bind(("127.0.0.1", 0))
            return connection.getsockname()[1]

    def write_workflows(project, workflows):
        (project / "lenso.ops.toml").write_text("schema_version = 1\nworkflows = " + literal(workflows) + "\n")

    def alive(pid):
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def assert_clean(project, evidence):
        for key in ["group_signalled", "child_reaped", "logs_drained"]:
            assert evidence["cleanup"][key] is True, evidence
        assert evidence["cleanup"]["scope"] == "owned_unix_process_group"
        pids = [evidence["pid"]]
        pidfile = project / "pids.json"
        if pidfile.exists():
            recorded = json.loads(pidfile.read_text())
            assert recorded["parent"] == evidence["pid"], recorded
            pids.extend(recorded["children"])
        deadline = time.monotonic() + 5
        while any(alive(pid) for pid in pids) and time.monotonic() < deadline:
            time.sleep(0.025)
        assert not any(alive(pid) for pid in pids), ("remaining fixture process", pids)
        with socket.socket() as connection:
            # Match ordinary server binds without mistaking TCP TIME_WAIT for a live listener.
            connection.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            connection.bind(("127.0.0.1", ports[project]))
        assert "stdout marker" in evidence["stdout"]["text"]
        assert "stderr marker" in evidence["stderr"]["text"]
        assert "stderr marker" not in evidence["stdout"]["text"]
        assert "ambient absent" in evidence["stdout"]["text"]
        assert "[redacted]" in evidence["stdout"]["text"]

    ports = {}
    catalog = invoke(empty, "help")
    assert not any(command["path"][0] == "ops" for command in catalog["commands"])
    help_text = subprocess.run([str(binary), "--project", str(empty), "--help"], capture_output=True, text=True, check=True).stdout
    assert "ops" not in [line.strip().split()[0] for line in help_text.splitlines() if line.strip()]

    for language in ["node", "rust"]:
        project = scratch / language
        shutil.copytree(root / "fixtures/ops" / language, project)
        if language == "rust":
            for action in ["check", "test", "clippy", "build"]:
                command = [cargo, action, "--locked", "--offline", "--manifest-path", str(project / "Cargo.toml")]
                if action == "clippy":
                    command += ["--all-targets", "--", "-D", "warnings"]
                subprocess.run(command, check=True)
            program = str(project / "target/debug/lenso-ops-rust-fixture")
            arguments = []
        else:
            program, arguments = node, ["server.mjs"]
        ports[project] = free_port()
        health_url = f"http://127.0.0.1:{ports[project]}/health/ready"
        shutil.copyfile(root / "presets/ops/extension.toml", project / "ops-extension.toml")
        declaration = {"cli": {"schema_version": 1, "extensions": [{
            "id": "ops", "source": "ops-extension.toml", "reads": ["lenso.ops.toml"],
            "env": ["OPS_MODE", "OPS_PORT", "OPS_PID_FILE", "OPS_SECRET"],
            "http": [f"http://127.0.0.1:{ports[project]}/health/"],
            "processes": [{"program": program, "args": arguments}],
        }]}}
        save(project / "lenso.toml", declaration)
        workflow = {"program": program, "args": arguments, "cwd": ".",
                    "env": ["OPS_MODE", "OPS_PORT", "OPS_PID_FILE", "OPS_SECRET"],
                    "timeout_ms": 15000, "readiness_timeout_ms": 3000,
                    "readiness": {"type": "http", "url": health_url, "status": 200}}
        workflows = {"dev": workflow, "check": {**workflow, "readiness_timeout_ms": 5000, "readiness": {"type": "exit", "code": 0}}}
        write_workflows(project, workflows)
        source_files = {path: path.read_bytes() for path in project.rglob("*")
                        if path.is_file() and "target" not in path.relative_to(project).parts}
        if core_binary := os.environ.get("LENSO_CORE_BIN"):
            unavailable = subprocess.run([core_binary, "--project", str(project), "help", "ops", "--json"],
                                         env=environment(project),capture_output=True,text=True,timeout=10)
            assert unavailable.returncode == 1
            failure = json.loads(unavailable.stdout)
            assert failure["code"] == "dependency_unavailable" and failure["details"]["provider"] == "ops-host", failure
        metadata = invoke(project, "help", "ops", "--offline")
        assert len(metadata["commands"]) == 3
        assert not (project / "lenso.lock").exists(), "static help must not create a lock"
        assert not (project / "pids.json").exists()
        inspected = invoke(project, "ops", "inspect", "dev")["outputs"]["inspect"]["workflows"][0]
        assert inspected["ready_to_run"] and inspected["cancellable"]
        assert "host.processes" in inspected["required_capabilities"]
        assert secret not in json.dumps(inspected)
        lock = (project / "lenso.lock").read_bytes()
        invoke(project, "ops", "inspect", "dev", "--locked", "--offline")
        assert (project / "lenso.lock").read_bytes() == lock
        pending = invoke(project, "ops", "run", "--yes", expected=2)
        assert pending["question"]["kind"]["type"] == "select"
        assert not (project / "pids.json").exists()

        for field, mutation in [
            ("program", {"program": "missing-ops-executable"}),
            ("cwd", {"cwd": "missing-directory"}),
        ]:
            write_workflows(project, {"dev": {**workflow, **mutation}})
            result = invoke(project, "ops", "inspect", "dev")["outputs"]["inspect"]["workflows"][0]
            assert not result["ready_to_run"]
            assert any(issue.get("details", {}).get("field") == field for issue in result["issues"]), result
            invoke(project, "ops", "run", "dev", "--yes", expected=1)
            assert not (project / "pids.json").exists()
        write_workflows(project, workflows)
        for capability in ["processes", "env", "http"]:
            denied = copy.deepcopy(declaration)
            denied["cli"]["extensions"][0][capability] = []
            save(project / "lenso.toml", denied)
            result = invoke(project, "ops", "run", "dev", "--yes", "--input",
                            json.dumps({"capabilities": {"host.processes": declaration["cli"]["extensions"][0]["processes"]}}), expected=1)
            assert result["code"] == "permission_denied" and result["details"]["reason"] == "preflight_failed"
            assert not (project / "pids.json").exists()
        save(project / "lenso.toml", declaration)
        literal_args = [*arguments, "literal; touch unexpected-shell-effect"]
        literal_declaration = copy.deepcopy(declaration)
        literal_declaration["cli"]["extensions"][0]["processes"] = [{"program": program, "args": literal_args}]
        write_workflows(project, {"check": {**workflows["check"], "args": literal_args}})
        invoke(project, "ops", "run", "check", "--yes", expected=1, env=environment(project, "exit0"))
        save(project / "lenso.toml", literal_declaration)
        literal_result = invoke(project, "ops", "run", "check", env=environment(project, "exit0"))
        assert literal_result["outputs"]["run"]["exit_verified"] and not (project / "unexpected-shell-effect").exists()
        save(project / "lenso.toml", declaration)
        write_workflows(project, workflows)
        missing = environment(project)
        del missing["OPS_SECRET"]
        invoke(project, "ops", "run", "dev", "--yes", expected=1, env=missing)
        assert not (project / "pids.json").exists()
        for url in ["http://example.com/health", "http://127.0.0.1:1/?token=private", "http://user:pass@127.0.0.1:1/"]:
            write_workflows(project, {"dev": {**workflow, "readiness": {"type": "http", "url": url, "status": 200}}})
            result = invoke(project, "ops", "run", "dev", "--yes", expected=1)
            assert result["code"] == "invalid_input"
        write_workflows(project, workflows)
        with socket.socket() as occupied:
            occupied.bind(("127.0.0.1", ports[project]))
            result = invoke(project, "ops", "inspect", "dev")["outputs"]["inspect"]["workflows"][0]
            assert any(issue.get("reason") == "port_unavailable" for issue in result["issues"])
            invoke(project, "ops", "run", "dev", "--yes", expected=1)
        assert not (project / "pids.json").exists()

        for mode, expected in [("exit0", 0), ("exit7", 7)]:
            result = invoke(project, "ops", "run", "check", expected=expected, env=environment(project, mode))
            evidence = result["outputs"]["run"] if expected == 0 else result["details"]
            assert evidence["exit_code"] == expected
            if expected == 0:
                assert evidence["exit_verified"]
            else:
                assert result["code"] == "process_failed"
            assert_clean(project, evidence)
            cases.append([language, mode, "passed"])
        premature = invoke(project, "ops", "run", "dev", expected=1, env=environment(project, "exit0"))
        assert premature["code"] == "verification_failed" and premature["details"]["ready_observed"] is None
        assert_clean(project, premature["details"])
        cases.append([language, "exit_before_http_ready", "passed"])

        for mode, expected_status in [("not_ready", 503), ("redirect", 302), ("silent", None)]:
            result = invoke(project, "ops", "run", "dev", expected=1, env=environment(project, mode))
            assert result["code"] == "verification_failed"
            assert result["details"]["cause"]["details"]["last_probe"]["status"] == expected_status
            assert_clean(project, result["details"])
            (project / "pids.json").unlink()
            cases.append([language, mode, "passed"])

        write_workflows(project, {"dev": {**workflow, "timeout_ms": 4500, "readiness_timeout_ms": 3000}})
        result = invoke(project, "ops", "run", "dev", expected=1)
        assert result["code"] == "process_timeout" and result["details"]["ready_observed"]["state"] == "ready"
        assert_clean(project, result["details"])
        cases.append([language, "runtime_deadline", "passed"])
        (project / "pids.json").unlink()
        write_workflows(project, {"dev": {**workflow, "readiness": {"type": "http", "url": health_url.replace("/ready", "/missing"), "status": 200}}})
        wrong_path = invoke(project, "ops", "run", "dev", expected=1)
        assert wrong_path["code"] == "verification_failed" and wrong_path["details"]["cause"]["details"]["last_probe"]["status"] == 404
        assert_clean(project, wrong_path["details"])
        (project / "pids.json").unlink()
        cases.append([language, "wrong_health_path", "passed"])
        write_workflows(project, workflows)
        for stop_signal in [signal.SIGINT, signal.SIGTERM]:
            process = subprocess.Popen([str(binary), "--project", str(project), "ops", "run", "dev", "--json", "--summary"],
                                       env=environment(project), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                deadline = time.monotonic() + 5
                while True:
                    assert process.poll() is None, "runner exited before readiness"
                    try:
                        with urlopen(health_url, timeout=0.25) as response:
                            if response.status == 200:
                                break
                    except OSError:
                        pass
                    assert time.monotonic() < deadline, "fixture never became healthy"
                    time.sleep(0.025)
                verified = invoke(project, "ops", "verify", "dev")["outputs"]["verify"]
                assert verified["state"] == "ready" and verified["health"]["status"] == 200
                assert verified["health"]["endpoint"] == health_url
                assert verified["process"] == "unknown" and verified["scope"] == "configured_endpoint_only"
                process.send_signal(stop_signal)
                stdout, stderr = process.communicate(timeout=5)
                assert process.returncode == 130, (stdout, stderr)
                assert secret not in stdout + stderr
                result = json.loads(stdout)
                assert result["code"] == "cancelled"
                observed = result["details"]["ready_observed"]
                assert observed is None or observed["state"] == "ready", "run only reports readiness it actually observed"
                assert_clean(project, result["details"])
                failed_verify = invoke(project, "ops", "verify", "dev", expected=1)
                assert failed_verify["details"]["health"]["state"] == "not_ready"
                cases.append([language, stop_signal.name, "passed"])
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.communicate(timeout=5)
                (project / "pids.json").unlink(missing_ok=True)
        for path, original in source_files.items():
            assert path.read_bytes() == original, ("Ops modified project source", path)
    print(json.dumps({"status": "passed", "cases": cases, "core_without_installation": "no_ops_commands",
                      "preflight": "grants, executable, cwd, env, port, endpoint",
                      "cleanup": "direct children reaped; recorded descendant PIDs absent; ports reusable",
                      "json": "one document, distinct bounded redacted streams", "source_files": "unchanged"}))
