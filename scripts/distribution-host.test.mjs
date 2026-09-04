import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { verifyDistribution } from "../bin/distribution-host.js";

const sha256 = bytes => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;

function fixture() {
  const directory = mkdtempSync(path.join(tmpdir(), "lenso-distribution-"));
  mkdirSync(path.join(directory, ".lenso"));
  const sources = [
    [".lenso/host-build.json", "host_authority", false],
    ["bundles.json", "bundle_inventory", false],
    ["runtime/lenso-host-runtime", "host_runtime", true],
    ["runtime/lenso-process-owner", "process_owner", true],
    ["runtime/lenso-resolver", "runtime_resolver", true],
    ["host.js", "entrypoint", true],
    ["THIRD_PARTY_NOTICES.txt", "notices", false],
  ];
  const files = sources.map(([relative, role, executable]) => {
    const absolute = path.join(directory, relative);
    mkdirSync(path.dirname(absolute), { recursive: true });
    const bytes = Buffer.from(`${role}\n`);
    writeFileSync(absolute, bytes);
    if (executable) chmodSync(absolute, 0o755);
    return { path: relative, role, sha256: sha256(bytes), size: bytes.length, executable };
  });
  const lock = {
    schema: "lenso.host-distribution.v1",
    app_id: "company.app",
    target: process.platform === "darwin" ? "aarch64-apple-darwin" : "x86_64-unknown-linux-gnu",
    platform: process.platform,
    arch: process.arch,
    files,
  };
  writeFileSync(path.join(directory, ".lenso/distribution.lock.json"), JSON.stringify(lock, null, 2));
  return { directory, lock };
}

test("prepared distribution verifies every immutable artifact", () => {
  const { directory } = fixture();
  try {
    const result = verifyDistribution(directory);
    assert.equal(result.lock.app_id, "company.app");
    assert.equal(result.identity, sha256(readFileSync(path.join(directory, ".lenso/distribution.lock.json"))));
    writeFileSync(path.join(directory, "bundles.json"), "tampered");
    assert.throws(() => verifyDistribution(directory), /failed integrity/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("prepared distribution rejects escaping and duplicate inventory paths", () => {
  const { directory, lock } = fixture();
  try {
    lock.files[0].path = "../outside";
    writeFileSync(path.join(directory, ".lenso/distribution.lock.json"), JSON.stringify(lock));
    assert.throws(() => verifyDistribution(directory), /escapes its root/);
    lock.files[0].path = lock.files[1].path;
    writeFileSync(path.join(directory, ".lenso/distribution.lock.json"), JSON.stringify(lock));
    assert.throws(() => verifyDistribution(directory), /invalid distribution file inventory/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
