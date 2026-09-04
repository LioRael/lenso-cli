import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const tempDir = mkdtempSync(path.join(os.tmpdir(), "lenso-cli-shim-"));
const typescriptCli = path.join(root, "node_modules", "typescript", "bin", "tsc");

try {
  const compile = spawnSync(
    process.execPath,
    [typescriptCli, "-p", "tsconfig.json", "--outDir", tempDir],
    { cwd: root, stdio: "inherit" },
  );
  if (compile.status !== 0) {
    process.exit(compile.status ?? 1);
  }

  for (const file of ["lenso.js", "lenso.d.ts", "host.js", "host.d.ts", "host-extract.js", "host-extract.d.ts", "host-owner.js", "host-owner.d.ts", "host-app.js", "host-app.d.ts", "distribution-host.js", "distribution-host.d.ts"]) {
    assert.deepEqual(
      readFileSync(path.join(root, "bin", file)),
      readFileSync(path.join(tempDir, file)),
      `bin/${file} is stale; run pnpm build:shim and commit the generated output`,
    );
  }
  console.log("generated npm shim is up to date");
} finally {
  rmSync(tempDir, { force: true, recursive: true });
}
