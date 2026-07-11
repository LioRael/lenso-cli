#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const lock = JSON.parse(await readFile(path.join(root, ".lenso-release/hosted-console.json"), "utf8"));
assert.equal(lock.schema, "lenso.hosted-artifact-lock.v1");
assert.equal(lock.packageId, "artifact:lenso-runtime-console");
assert.match(lock.version, /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/u);
assert.match(lock.sha256, /^sha256:[0-9a-f]{64}$/u);
const url = new URL(lock.url);
assert.equal(url.origin, "https://github.com");
assert.equal(url.pathname, `/LioRael/lenso-runtime-console/releases/download/v${lock.version}/lenso-runtime-console.tar.gz`);

const response = await fetch(url, { redirect: "follow" });
assert.equal(response.ok, true, `hosted Console download failed: ${response.status}`);
const archive = Buffer.from(await response.arrayBuffer());
const digest = `sha256:${createHash("sha256").update(archive).digest("hex")}`;
assert.equal(digest, lock.sha256, "hosted Console digest does not match the reviewed lock");

const staging = path.join(root, ".lenso-release/downloads");
const archivePath = path.join(staging, "lenso-runtime-console.tar.gz");
const consoleRoot = path.join(root, "console");
await rm(staging, { recursive: true, force: true });
await rm(consoleRoot, { recursive: true, force: true });
await mkdir(staging, { recursive: true });
await mkdir(consoleRoot, { recursive: true });
await writeFile(archivePath, archive, { mode: 0o600 });
const extracted = spawnSync("tar", ["-xzf", archivePath, "-C", consoleRoot], { cwd: root, stdio: "inherit" });
if (extracted.status !== 0) process.exit(extracted.status ?? 1);
await readFile(path.join(consoleRoot, "dist/index.html"));
console.log(`installed hosted Console ${lock.version} (${lock.sha256})`);
