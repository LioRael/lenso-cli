import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, existsSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { extractHost } = require("../bin/host-extract.js");

function fixture(t, source, extras = {}) {
  const root = mkdtempSync(path.join(tmpdir(), "lenso-host-source-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  writeFileSync(path.join(root, "app.ts"), source);
  for (const [name, bytes] of Object.entries(extras)) {
    mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    writeFileSync(path.join(root, name), bytes);
  }
  return realpathSync(root);
}

test("extract aliased helpers and imported const declarations without running bundle code", t => {
  const root = fixture(t, `
    import { defineHost as host } from '@lenso/cli/host';
    import store from './plugins/store';
    const id = 'company.app';
    export default host({ id, plugins: [store] });
  `, { "plugins/store.ts": `
    import { pluginBundle as packed } from '@lenso/cli/host';
    const file = './store.lenso-plugin';
    export default packed(file, { execution: 'process' });
  ` });
  const result = extractHost(path.join(root, "app.ts"));
  assert.equal(result.id, "company.app");
  assert.equal(result.plugins[0].bundle, path.join(root, "plugins/store.lenso-plugin"));
  assert.equal(result.plugins[0].execution, "process");
  assert.match(result.plugins[0].source, /store\.ts:4:/);
});

test("supports named Instances and literal configuration without importing SDK code", t => {
  const root = fixture(t, `
    import { defineHost, pluginBundle } from '@lenso/cli/host';
    export default defineHost({ id: 'company.app', plugins: [
      { plugin: pluginBundle('./store.lenso-plugin'), instance: 'source', configuration: { limit: 8 } }
    ], slots: [{ id: 'store', cardinality: 'many' }], dependencies: [{
      consumer: { plugin: 'company.copy' }, requirement: 'source',
      allow: [{ plugin: 'company.store', instance: 'source' }],
      default: { plugin: 'company.store', instance: 'source' }
    }] });
  `);
  const declaration = extractHost(path.join(root, "app.ts"));
  assert.equal(declaration.plugins[0].configuration.limit, 8);
  assert.equal(declaration.dependencies[0].requirement, "source");
  assert.equal(declaration.dependencies[0].default.instance, "source");
});

for (const [name, expression, expected] of [
  ["environment", "process.env.PLUGINS", /unsupported declaration expression/],
  ["arbitrary call", "fetch('https://example.com')", /unsupported declaration call/],
  ["spread", "[...other]", /unsupported declaration expression/],
  ["duplicate", "{ one: 1, one: 2 }", /duplicate property/],
  ["nonfinite", "1e999", /finite/],
]) {
  test(`rejects ${name} with source diagnostics`, t => {
    const root = fixture(t, `import { defineHost } from '@lenso/cli/host';\nexport default defineHost({ id: 'company.app', plugins: ${expression} });`);
    assert.throws(() => extractHost(path.join(root, "app.ts")), expected);
  });
}

test("never evaluates side-effecting source on rejection", t => {
  const root = fixture(t, "");
  const marker = path.join(root, "executed");
  writeFileSync(path.join(root, "app.ts"), `import { defineHost } from '@lenso/cli/host';
    require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'bad');
    export default defineHost({ id: 'company.app', plugins: [] });`);
  assert.throws(() => extractHost(path.join(root, "app.ts")), /never evaluated/);
  assert.equal(existsSync(marker), false);
});

test("rejects cycles and local functions masquerading as SDK helpers", t => {
  const root = fixture(t, `import { defineHost } from '@lenso/cli/host';
    const a = b; const b = a; export default defineHost(a);`);
  assert.throws(() => extractHost(path.join(root, "app.ts")), /cyclic/);
  writeFileSync(path.join(root, "app.ts"), `const defineHost = (x) => x; export default defineHost({});`);
  assert.throws(() => extractHost(path.join(root, "app.ts")), /must use defineHost/);
});
