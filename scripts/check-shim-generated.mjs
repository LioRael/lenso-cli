import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const tempDir = mkdtempSync(path.join(os.tmpdir(), "lenso-cli-shim-"));

try {
  const compile = spawnSync(
    "pnpm",
    ["exec", "tsc", "-p", "tsconfig.json", "--outDir", tempDir],
    { cwd: root, stdio: "inherit" },
  );
  if (compile.status !== 0) {
    process.exit(compile.status ?? 1);
  }

  assert.deepEqual(
    readFileSync(path.join(root, "bin", "lenso.js")),
    readFileSync(path.join(tempDir, "lenso.js")),
    "bin/lenso.js is stale; run pnpm build:shim and commit the generated output",
  );
  console.log("generated npm shim is up to date");
} finally {
  rmSync(tempDir, { force: true, recursive: true });
}
