import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const workflow = readFileSync(
  fileURLToPath(new URL("../.github/workflows/publish.yml", import.meta.url)),
  "utf8",
);

for (const target of ["darwin-arm64", "darwin-x64", "linux-x64", "win32-x64"]) {
  assert.match(workflow, new RegExp(`tag: ${target}`), `missing ${target} publisher build`);
}
assert.match(workflow, /^  build:/mu, "reviewed publisher must build universal CLI payloads");
assert.match(workflow, /^    needs: build$/mu, "publishing must wait for every target build");
assert.match(workflow, /actions\/upload-artifact@[0-9a-f]{40}/u, "build artifacts must use a pinned action");
assert.match(workflow, /actions\/download-artifact@[0-9a-f]{40}/u, "publisher artifacts must use a pinned action");
assert.match(workflow, /npm run check:npm-publish/u, "publisher must reject an incomplete npm payload");
assert.ok(
  workflow.indexOf("actions/download-artifact@") < workflow.indexOf("Complete fail-closed preflight"),
  "universal CLI bytes must be assembled before preflight seals registry artifacts",
);
assert.equal(
  workflow.match(/ref: \$\{\{ inputs\.release_commit \}\}/gu)?.length,
  3,
  "build, publish, and recovery jobs must use the exact reviewed release commit",
);

console.log("reviewed publisher workflow check passed");
