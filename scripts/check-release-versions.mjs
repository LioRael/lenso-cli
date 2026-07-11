#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const cargo = await readFile(new URL("../Cargo.toml", import.meta.url), "utf8");
const manifest = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
const cargoVersion = /^version = "([^"]+)"$/mu.exec(cargo)?.[1];

assert.ok(cargoVersion, "Cargo package version is missing");
assert.equal(cargoVersion, manifest.version, "CLI Cargo and npm versions must match");
console.log(`CLI release versions match at ${cargoVersion}`);
