import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { chmodSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';

const require = createRequire(import.meta.url);
const { binaryPath, ensureExecutableBinary, forwardTerminationSignals, platformTag } = require('../bin/lenso.js');

assert.equal(platformTag('darwin', 'arm64'), 'darwin-arm64');
assert.equal(platformTag('linux', 'x64'), 'linux-x64');
assert.equal(platformTag('win32', 'x64'), 'win32-x64');
assert.equal(platformTag('freebsd', 'x64'), null);
assert.match(binaryPath('/pkg', 'darwin', 'arm64'), /vendor[/\\]darwin-arm64[/\\]lenso$/);
assert.match(binaryPath('/pkg', 'win32', 'x64'), /vendor[/\\]win32-x64[/\\]lenso\.exe$/);

if (process.platform !== 'win32') {
  const tempDir = mkdtempSync(path.join(os.tmpdir(), 'lenso-cli-shim-'));
  const executable = path.join(tempDir, 'lenso');
  writeFileSync(executable, 'binary');
  chmodSync(executable, 0o644);
  ensureExecutableBinary(executable);
  assert.notEqual(statSync(executable).mode & 0o111, 0);
  rmSync(tempDir, { recursive: true });
}

const parent = new EventEmitter();
const forwarded = [];
const child = {
  exitCode: null,
  signalCode: null,
  kill(signal) {
    forwarded.push(signal);
  },
};
const stopForwarding = forwardTerminationSignals(parent, child);
parent.emit('SIGINT');
parent.emit('SIGTERM');
assert.deepEqual(forwarded, ['SIGINT', 'SIGTERM']);
stopForwarding();
parent.emit('SIGINT');
assert.deepEqual(forwarded, ['SIGINT', 'SIGTERM']);

console.log('npm shim check passed');
