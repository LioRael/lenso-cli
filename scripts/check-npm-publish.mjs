import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const required = [
  ['darwin-arm64', 'lenso'],
  ['darwin-x64', 'lenso'],
  ['linux-x64', 'lenso'],
  ['win32-x64', 'lenso.exe']
];

for (const [tag, exe] of required) {
  assert.ok(
    existsSync(path.join(root, 'vendor', tag, exe)),
    `missing vendor/${tag}/${exe}`
  );
}

console.log('npm publish check passed');
