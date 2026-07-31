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
  "build, publish, and partial recovery jobs must use the exact reviewed release commit",
);
assert.match(
  workflow,
  /^  build:\n(?:.*\n)*?    if: startsWith\(github\.ref_name, 'release-execution\/'\)$/mu,
  "build must be restricted to a protected release execution ref",
);
assert.match(
  workflow,
  /^  publish:\n    if: startsWith\(github\.ref_name, 'release-execution\/'\)$/mu,
  "publish must be restricted to a protected release execution ref",
);
assert.match(
  workflow,
  /^  recover-partial:\n    if: github\.ref_name == github\.event\.repository\.default_branch && \(contains\(inputs\.packages_json, 'npm:'\) \|\| contains\(inputs\.packages_json, 'oci:'\)\)$/mu,
  "partial recovery must be restricted to the default branch and a non-Cargo package set",
);
assert.match(
  workflow,
  /LENSO_WORKFLOW_PATH: \.github\/workflows\/publish\.yml/u,
  "partial recovery must bind the trusted publisher workflow",
);
assert.match(
  workflow,
  /chmod \+x recovery-candidate\/vendor\/darwin-arm64\/lenso recovery-candidate\/vendor\/darwin-x64\/lenso recovery-candidate\/vendor\/linux-x64\/lenso/u,
  "partial recovery must restore executable modes before npm packing",
);

console.log("reviewed publisher workflow check passed");
