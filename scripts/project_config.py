"""Development helpers for root TOML declarations; old Python delegates parsing to Lenso."""
import json
import os
import subprocess
from pathlib import Path
try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

def load(path):
    path = Path(path)
    if tomllib is not None:
        return tomllib.loads(path.read_text())
    binary = Path(os.environ.get("LENSO_BIN", Path(__file__).resolve().parent.parent / "target/debug/lenso"))
    result = subprocess.run([str(binary), "--project", str(path.parent), "config", "--json"], capture_output=True, text=True, check=True)
    return json.loads(result.stdout)["outputs"]["main"]["config"]

def literal(value):
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(literal(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{literal(key)} = {literal(item)}" for key, item in value.items()) + " }"
    raise ValueError(f"unsupported TOML value: {type(value).__name__}")

def save(path, config):
    if set(config) != {"cli"} or set(config["cli"]) - {"schema_version", "extensions", "registries", "github", "presets"}:
        raise ValueError("development installer cannot rewrite unrelated TOML sections; edit lenso.toml manually")
    lines = ["[cli]", f"schema_version = {config['cli']['schema_version']}"]
    if config["cli"].get("registries"):
        lines.append("registries = " + literal(config["cli"]["registries"]))
    if config["cli"].get("github"):
        lines.append("github = " + literal(config["cli"]["github"]))
    if config["cli"].get("presets"):
        lines.append("presets = " + literal(config["cli"]["presets"]))
    for entry in config["cli"].get("extensions", []):
        lines += ["", "[[cli.extensions]]"]
        lines += [f"{key} = {literal(value)}" for key, value in entry.items()]
    Path(path).write_text("\n".join(lines) + "\n")
