#!/usr/bin/env python3
"""Real Wasm acceptance for Registry authentication, generic payloads, and monorepos."""
import argparse
import copy
import http.server
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit
from project_config import load, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--keep", action="store_true")
parser.add_argument("--pnpm", action="store_true", help="Also install and build a pnpm workspace")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
binary = root / "target/debug/lenso"
scratch = Path(tempfile.mkdtemp(prefix="lenso-registry-"))
token = "test-private-authorization-7d0c031b"
query_token = "test-private-query-secret-3496dbda"
tool_token = "test-tools-authorization-3dbf7e51"
items = {}
leaks = []
search_requests = []

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url = urlsplit(self.path)
        if url.path.startswith("/private/"):
            valid = self.headers.get("Authorization") == "Bearer " + token and parse_qs(url.query).get("token") == [query_token]
        elif url.path.startswith("/tools/"):
            valid = self.headers.get("X-API-Key") == tool_token
            if self.headers.get("Authorization") or set(parse_qs(url.query)) - {"q","type","limit","offset"}:
                leaks.append("private authentication reached another namespace")
        else:
            valid = True
            if self.headers.get("Authorization") or self.headers.get("X-API-Key") or set(parse_qs(url.query)) - {"q","type","limit","offset"}:
                leaks.append("authentication reached a public endpoint")
        item = items.get(url.path)
        if url.path == "/private/registry.json" and valid:
            params = parse_qs(url.query)
            search_requests.append(params)
            offset = int(params.get("offset", ["0"])[0])
            limit = int(params.get("limit", ["100"])[0])
            rows = [{"name":"feature","type":"registry:block","description":"Server-ranked result"},
                    {"name":"custom-button","type":"registry:ui","description":"Another server-ranked result"},
                    {"name":"last","type":"registry:lib","description":"Last result"}]
            item = {"items":rows[offset:offset+limit],"pagination":{"total":len(rows),"offset":offset,"limit":limit,"hasMore":offset+limit<len(rows)}}
        self.send_response(200 if valid and item is not None else 403 if not valid else 404)
        body = json.dumps(item if valid else {"error":"unauthorized"}).encode()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
origin = f"http://127.0.0.1:{server.server_port}"
items.update({
    "/public/registry.json": {"items":[
        {"name":"badge","type":"registry:ui","description":"Public badge","categories":["display"]},
        {"name":"targeted","type":"registry:file","description":"Explicit note"}]},
    "/private/feature.json": {"name":"feature","type":"registry:block", "registryDependencies":["@private/custom-button",origin + "/public/badge.json"],
        "files":[{"path":"registry/components/feature.tsx","type":"registry:component", "content":'import { CustomButton } from "@/components/ui/custom-button"\nimport { variant } from "@/lib/variant"\nexport function Feature() { return <div data-variant={variant}><CustomButton /></div> }\n'}],
        "cssVars":{"theme":{"color-registry-accent":"var(--registry-accent)"},"light":{"registry-accent":"red"}},
        "css":{"@keyframes registry-fade":{"from":{"opacity":"0"},"to":{"opacity":"1"}}},
        "envVars":{"REGISTRY_DEMO":"enabled"}, "docs":"Registry acceptance instructions"},
    "/private/custom-button.json": {"name":"custom-button","type":"registry:ui","registryDependencies":["@tools/variant"],"dependencies":["clsx@^2.0.0"],
        "files":[{"path":"registry/ui/custom-button.tsx","type":"registry:ui","content":'import { clsx } from "clsx"\nimport { variant } from "@/lib/variant"\nexport function CustomButton() { return <button className={clsx(variant)}>Custom</button> }\n'}]},
    "/tools/variant.json": {"name":"variant","type":"registry:lib", "files":[
            {"path":"registry/lib/variant.ts","type":"registry:lib","content":'import { suffix } from "@/registry/private/variant/internal"\nexport const variant = "registry-demo" + suffix\n'},
            {"path":"registry/private/variant/internal.ts","type":"registry:lib","content":'import { core } from "./core"\nexport const suffix = core\n'},
            {"path":"registry/private/variant/core.ts","type":"registry:lib","content":'export const core = "-from-graph"\n'}]},
    "/public/badge.json": {"name":"badge","type":"registry:ui","files":[{"path":"registry/ui/public-badge.tsx","type":"registry:ui","content":"export function PublicBadge() { return <span>Public</span> }\n"}]},
    "/public/legacy-range.json": {"name":"legacy-range","type":"registry:lib","dependencies":["clsx@^1.0.0"],"files":[{"path":"registry/lib/legacy.ts","type":"registry:lib","content":'import clsx from "clsx"\nexport { clsx }\n'}]},
    "/public/targeted.json": {"name":"targeted","type":"registry:file","files":[{"path":"registry/note.txt","type":"registry:file","target":"@components/note.txt","content":"Explicit target\n"}]},
    "/public/escape.json": {"name":"escape","type":"registry:file","files":[{"path":"registry/note.txt","type":"registry:file","target":"../../outside.txt","content":"outside"}]},
    "/public/cycle-a.json": {"name":"cycle-a","type":"registry:item","registryDependencies":[origin + "/public/cycle-b.json"]},
    "/public/cycle-b.json": {"name":"cycle-b","type":"registry:item","registryDependencies":[origin + "/public/cycle-a.json"]},
})
env = dict(os.environ, LENSO_CACHE_DIR=str(scratch / "cache"), PRIVATE_TOKEN=token, QUERY_TOKEN=query_token, TOOL_TOKEN=tool_token)
env["NO_PROXY"] = "127.0.0.1,localhost"
env["no_proxy"] = env["NO_PROXY"]

def fixture(project, mono=False, manager="npm"):
    if mono:
        project.mkdir()
        (project / "package.json").write_text(json.dumps({"name":"registry-workspace","private":True,"workspaces":["apps/*","packages/*"],"packageManager":"pnpm@12.9.1" if manager == "pnpm" else "npm@11.20.0"}))
        (project / "pnpm-workspace.yaml").write_text("packages:\n  - apps/*\n  - packages/*\n")
        if manager == "pnpm":
            proxy = os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
            if proxy:
                path = project / "pnpm-workspace.yaml"
                path.write_text(path.read_text() + "\nhttpsProxy: " + json.dumps(proxy) + "\nfetchRetries: 0\nfetchTimeout: 15000\nnetworkConcurrency: 8\n")
        active = project / "apps/web"
    else:
        active = project
    shutil.copytree(root / "examples/vite-shadcn", active, ignore=shutil.ignore_patterns("node_modules","dist",".lenso","lenso","lenso.toml","lenso.lock"))
    metadata = json.loads((active / "package.json").read_text())
    metadata["name"] = "registry-web"
    config = json.loads((active / "components.json").read_text())
    config["registries"] = {
        "@private":{"url":origin+"/private/{name}.json","headers":{"Authorization":"Bearer ${PRIVATE_TOKEN}"},"params":{"token":"${QUERY_TOKEN}"}},
        "@tools":{"url":origin+"/tools/{name}.json","headers":{"X-API-Key":"${TOOL_TOKEN}"}},
        "@public":origin+"/public/{name}.json"
    }
    if mono:
        metadata["dependencies"]["@workspace/ui"] = "workspace:*" if manager == "pnpm" else "*"
        (active / "package-lock.json").unlink(missing_ok=True)
        ui = project / "packages/ui"
        ui.mkdir(parents=True)
        (ui / "package.json").write_text(json.dumps({"name":"@workspace/ui","version":"1.0.0","private":True,"type":"module",
            "exports":{"./components/*":"./src/components/*.tsx","./lib/*":"./src/lib/*.ts","./hooks/*":"./src/hooks/*.ts"},
            "imports":{"#components/*":"./src/components/*.tsx","#lib/*":"./src/lib/*.ts","#hooks/*":"./src/hooks/*.ts"},
            "peerDependencies":{"react":metadata["dependencies"]["react"]},
            "devDependencies":{"@types/react":metadata["devDependencies"]["@types/react"],"tailwindcss":metadata["devDependencies"]["tailwindcss"]}}))
        ui_config = copy.deepcopy(config)
        ui_config["aliases"] = {"components":"#components","ui":"#components","utils":"#lib/utils","lib":"#lib","hooks":"#hooks"}
        ui_config["tailwind"]["css"] = "src/styles/globals.css"
        (ui / "components.json").write_text(json.dumps(ui_config))
        (ui / "src/styles").mkdir(parents=True)
        shutil.copyfile(active / "src/index.css", ui / "src/styles/globals.css")
        (ui / "tsconfig.json").write_text(json.dumps({"extends":"@workspace/typescript-config/react-library.json"}))
        types = project / "packages/typescript-config"
        types.mkdir()
        (types / "package.json").write_text('{"name":"@workspace/typescript-config","version":"1.0.0"}')
        (types / "react-library.json").write_text('{"compilerOptions":{"jsx":"react-jsx","moduleResolution":"bundler"}}')
        (project / "tsconfig.base.json").write_text('{"compilerOptions":{"paths":{"@/*":["./apps/web/src/*"]}}}')
        typescript_path = active / "tsconfig.json"
        typescript = json.loads(typescript_path.read_text())
        typescript["extends"] = "../../tsconfig.base.json"
        typescript["compilerOptions"].pop("baseUrl", None)
        typescript["compilerOptions"].pop("paths", None)
        typescript_path.write_text(json.dumps(typescript))
        config["aliases"]["ui"] = "@workspace/ui/components"
        config["aliases"]["utils"] = "@workspace/ui/lib/utils"
        config["tailwind"]["css"] = "../../packages/ui/src/styles/globals.css"
        main = (active / "src/main.tsx").read_text().replace('"./index.css"','"../../../packages/ui/src/styles/globals.css"')
        (active / "src/main.tsx").write_text(main)
    (active / "package.json").write_text(json.dumps(metadata))
    (active / "components.json").write_text(json.dumps(config))
    subprocess.run(["python3",str(root / "scripts/configure-shadcn.py"),str(project),"--allow-install","--http",origin+"/public/"],env=env,check=True)
    declaration = project / "lenso.toml"
    installed = load(declaration)
    env_file = "apps/web/.env" if mono else ".env"
    installed["cli"]["extensions"][0]["files"].append(env_file)
    installed["cli"]["extensions"][0]["reads"].append(env_file)
    local = project / "registry"
    local.mkdir()
    (local / "local.json").write_text(json.dumps({"name":"local","type":"registry:hook","files":[{"path":"registry/hooks/use-local.ts","type":"registry:hook","content":"export function useLocal() { return 1 }\n"}]}))
    installed["cli"]["extensions"][0]["reads"].append("registry/local.json")
    save(declaration, installed)
    return active

def run(project, *arguments, expected=0, overrides=None, cwd=None):
    child = subprocess.Popen([str(binary),"--project",str(project),"shadcn",*arguments,"--json"],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,
                             env=env if overrides is None else overrides,cwd=cwd)
    try:
        stdout, stderr = child.communicate(timeout=180)
    except subprocess.TimeoutExpired:
        child.send_signal(signal.SIGINT)
        stdout, stderr = child.communicate(timeout=10)
        raise AssertionError("CLI acceptance timed out; supervised processes were cancelled")
    result = subprocess.CompletedProcess(child.args, child.returncode, stdout, stderr)
    for secret in [token, query_token, tool_token]:
        assert secret not in result.stdout + result.stderr, "authentication value escaped into CLI output"
    assert result.returncode == expected,(arguments,result.returncode,result.stdout[-3000:],result.stderr[-2000:])
    return json.loads(result.stdout)

def snapshot(project):
    return {str(path.relative_to(project)):path.read_bytes() for path in project.rglob("*") if path.is_file()
            and not any(part in ("node_modules","dist","lenso",".lenso") for part in path.relative_to(project).parts)}

try:
    for mono, manager in [(False,"npm"),(True,"npm"), *([(True,"pnpm")] if args.pnpm else [])]:
        project = scratch / ("mono-" + manager if mono else "single")
        active = fixture(project, mono, manager)
        options = ["--cwd","apps/web"] if mono else []
        if mono:
            pending = run(project,"preview","@private/feature",expected=2)
            assert pending["question"]["id"] == "workspace"
        result = run(project,"preview","@private/feature",*options)
        assert result["status"] == "complete"
        before = snapshot(project)
        searched = run(project,"search","@private","--query","semantic match","--limit","1","--offset","1",*options)
        search = searched["outputs"]["search"]
        assert [row["add"] for row in search["results"]] == ["@private/custom-button"]
        assert search["pagination"] == {"total":3,"offset":1,"limit":1,"hasMore":True}
        assert search_requests[-1]["q"] == ["semantic match"]
        listed = run(project,"list","@public","--query","display","--type","registry:ui",*options)
        assert [row["name"] for row in listed["outputs"]["search"]["results"]] == ["badge"]
        direct = run(project,"search",origin+"/public/registry.json","--query","note",*options)
        assert direct["outputs"]["search"]["results"][0]["add"] == origin+"/public/targeted.json"
        combined = run(project,"search","@private","@public","--limit","2","--offset","2",*options)
        assert [row["name"] for row in combined["outputs"]["search"]["results"]] == ["last","badge"]
        assert combined["outputs"]["search"]["pagination"]["total"] == 5
        assert search_requests[-1]["limit"] == ["4"] and search_requests[-1]["offset"] == ["0"]
        empty = run(project,"search","@public","--query","no match",*options)
        assert empty["outputs"]["search"]["results"] == []
        run(project,"search","@public","--limit","0",*options,expected=1)
        assert before == snapshot(project), "search changed project files"
        human_search = subprocess.run([str(binary),"--project",str(project),"shadcn","search","@public","--query","badge",*options,"--color","always"],capture_output=True,text=True,env=env,timeout=30)
        assert human_search.returncode == 0 and "@public/badge" in human_search.stdout and "1 results" in human_search.stdout and "\x1b[" in human_search.stdout
        assert all(secret not in human_search.stdout + human_search.stderr for secret in [token,query_token,tool_token])
        paths = [entry["path"] for entry in result["outputs"]["resolve"]["files"]]
        prefix = "packages/ui/" if mono else ""
        assert prefix + "src/components/ui/custom-button.tsx" in paths if not mono else prefix + "src/components/custom-button.tsx" in paths
        if mono:
            legacy = run(project,"preview",origin+"/public/legacy-range.json",*options)
            metadata = next(entry["content"] for entry in legacy["outputs"]["resolve"]["files"] if entry["path"] == "packages/ui/package.json")
            assert json.loads(metadata)["dependencies"]["clsx"].startswith("1.")
            run(project,"preview","@private/custom-button",origin+"/public/legacy-range.json",*options,expected=1)
            assert before == snapshot(project)
            assert "packages/ui/package.json" in paths and "apps/web/package.json" not in paths
            button = next(entry["content"] for entry in result["outputs"]["resolve"]["files"] if entry["path"].endswith("custom-button.tsx"))
            assert '"#lib/variant"' in button
            feature = next(entry["content"] for entry in result["outputs"]["resolve"]["files"] if entry["path"].endswith("feature.tsx"))
            assert '"@workspace/ui/components/custom-button"' in feature and '"@workspace/ui/lib/variant"' in feature
        if not mono:
            human = subprocess.run([str(binary),"--project",str(project),"shadcn","preview","@private/feature","--color","always"],capture_output=True,text=True,env=env,timeout=40)
            assert human.returncode == 0 and "Registry acceptance instructions" in human.stdout and "\x1b[" in human.stdout
            assert all(secret not in human.stdout + human.stderr for secret in [token,query_token,tool_token])
        local = run(project,"preview","./registry/local.json",*options)
        assert any(entry["path"].endswith("hooks/use-local.ts") for entry in local["outputs"]["resolve"]["files"])
        public = run(project,"preview","@public/targeted",*options)
        assert any(entry["path"].endswith("components/note.txt") for entry in public["outputs"]["resolve"]["files"])
        for item in [origin+"/public/escape.json",origin+"/public/cycle-a.json"]:
            run(project,"preview",item,*options,expected=1)
            assert before == snapshot(project)
        missing = dict(env)
        del missing["PRIVATE_TOKEN"]
        failed = run(project,"preview","@private/feature",*options,expected=1,overrides=missing)
        assert failed["code"] == "invalid_input" and failed["details"]["environment_variable"] == "PRIVATE_TOKEN"
        run(project,"search","@private",*options,expected=1,overrides=missing)
        assert before == snapshot(project)
        original_declaration = (project / "lenso.toml").read_text()
        policy = load(project / "lenso.toml")
        policy["cli"]["extensions"][0]["env"] = []
        save(project / "lenso.toml", policy)
        denied = run(project,"preview","@private/feature",*options,expected=1)
        assert denied["code"] == "permission_denied" and denied["details"]["capability"] == "host.env"
        (project / "lenso.toml").write_text(original_declaration)
        assert before == snapshot(project)
        targeted = run(project,"preview",origin+"/public/targeted.json",*options)
        assert any(entry["path"].endswith("components/note.txt") for entry in targeted["outputs"]["resolve"]["files"])
        pending = run(project,"add","@private/feature",*options,expected=2)
        answers = scratch / "answers.json"
        answers.write_text(json.dumps({pending["question"]["id"]:True}))
        pending_commands = run(project,"add","@private/feature",*options,"--answers",str(answers),expected=2)
        assert pending_commands["question"]["id"].startswith("commands:")
        denied = json.loads(answers.read_text())
        denied[pending_commands["question"]["id"]] = False
        answers.write_text(json.dumps(denied))
        run(project,"add","@private/feature",*options,"--answers",str(answers),expected=130)
        assert before == snapshot(project), "command denial changed application files"
        if not mono and os.name != "nt":
            tools = scratch / "failing-tools"
            tools.mkdir()
            stub = tools / "npm"
            stub.write_text("#!/bin/sh\nexit 7\n")
            stub.chmod(0o755)
            broken = dict(env, PATH=str(tools) + os.pathsep + env["PATH"])
            failed = run(project,"add","@private/feature",*options,"--yes",expected=1,overrides=broken)
            assert failed["code"] == "partial_failure" and failed["details"]["completed_files"] and failed["details"]["cause"]["details"]["exit_code"] == 7
            retry = run(project,"preview","@private/feature",*options)
            assert retry["outputs"]["resolve"]["files"] == [] and retry["outputs"]["resolve"]["commands"], "matching source files must retain dependency recovery"
        installed = run(project,"add","@private/feature",*options,"--yes")
        assert installed["outputs"]["verify"]["verified"] == "@private/feature"
        again = run(project,"preview","@private/feature",*options)
        assert again["outputs"]["resolve"]["files"] == [], "Registry CSS and file application must be idempotent"
        feature_path = active / "src/components/feature.tsx"
        original_feature = items["/private/feature.json"]["files"][0]["content"]
        feature_path.write_text(feature_path.read_text() + "// local customization\n")
        items["/private/feature.json"]["files"][0]["content"] += "// upstream revision\n"
        local_before = snapshot(project)
        difference = run(project,"diff","@private/feature",*options)
        assert difference["outputs"]["resolve"]["summary"]["changed"] == 1
        assert "local customization" in difference["outputs"]["plan"]["diff"] and "upstream revision" in difference["outputs"]["plan"]["diff"]
        assert local_before == snapshot(project), "diff modified the project"
        # Diff needs read grants only, even when the proposed resource would replace a file.
        policy = load(project / "lenso.toml")
        original_policy = (project / "lenso.toml").read_text()
        policy["cli"]["extensions"][0]["files"] = []
        policy["cli"]["extensions"][0]["write_roots"] = []
        save(project / "lenso.toml", policy)
        run(project,"diff","@private/feature",*options)
        (project / "lenso.toml").write_text(original_policy)
        # Refresh the extension lock after restoring its declaration before the no-write snapshot.
        run(project,"diff","@private/feature",*options)
        conflict = run(project,"components","update","@private/feature",*options,"--yes",expected=2)
        conflict_id = conflict["question"]["id"]
        assert conflict_id == "conflict:" + str(feature_path.relative_to(project))
        answers.write_text(json.dumps({conflict_id:"keep"}))
        kept = run(project,"components","update","@private/feature",*options,"--answers",str(answers),"--yes")
        assert kept["outputs"]["resolve"]["summary"]["kept"] == 1 and "local customization" in feature_path.read_text()
        answers.write_text(json.dumps({conflict_id:"overwrite"}))
        review = run(project,"components","update","@private/feature",*options,"--answers",str(answers),expected=2)
        decisions = {conflict_id:"overwrite",review["question"]["id"]:True}
        answers.write_text(json.dumps(decisions))
        command = run(project,"components","update","@private/feature",*options,"--answers",str(answers),expected=2)
        assert command["question"]["id"].startswith("commands:")
        decisions[command["question"]["id"]] = False
        answers.write_text(json.dumps(decisions))
        before_denial = snapshot(project)
        run(project,"components","update","@private/feature",*options,"--answers",str(answers),expected=130)
        assert before_denial == snapshot(project), "update command denial wrote files"
        answers.write_text(json.dumps({conflict_id:"overwrite"}))
        updated = run(project,"components","update","@private/feature",*options,"--answers",str(answers),"--yes")
        assert updated["outputs"]["verify"]["verified"] == "@private/feature"
        assert "upstream revision" in feature_path.read_text() and "local customization" not in feature_path.read_text()
        clean = run(project,"diff","@private/feature",*options)
        assert clean["outputs"]["resolve"]["files"] == [] and clean["outputs"]["resolve"]["summary"]["unchanged"] > 0
        run(project,"components","update","@public/targeted",*options,expected=1)
        pending_item = run(project,"components","update",*options,expected=2)
        assert pending_item["question"]["id"] == "item"
        items["/private/feature.json"]["files"][0]["content"] = original_feature
        (active / "src/App.tsx").write_text('import { Feature } from "@/components/feature"\nexport default function App() { return <Feature /> }\n')
        subprocess.run(["pnpm","--filter","registry-web","build"] if manager == "pnpm" else ["npm","run","build"],cwd=project if manager == "pnpm" else active,env=env,check=True,timeout=180)
        for path in [project / "lenso.lock", *list((scratch / "cache").rglob("*"))]:
            if path.is_file():
                content = path.read_bytes()
                assert all(secret.encode() not in content for secret in [token,query_token,tool_token]), "authentication value persisted in lock/cache"
        assert not leaks, leaks
        print(f"Registry acceptance passed: {'monorepo' if mono else 'single project'}, {manager}, namespaces/auth, static/dynamic search, pagination, read-only diff, guarded update, CSS/env, denial, Vite build",flush=True)
    print(f"Projects: {scratch}",flush=True)
except BaseException:
    print(f"Failed projects retained: {scratch}",flush=True)
    raise
else:
    if not args.keep:
        shutil.rmtree(scratch)
finally:
    server.shutdown()
    server.server_close()
    thread.join()
