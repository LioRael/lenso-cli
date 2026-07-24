import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { binaryPath, forwardTerminationSignals, platformTag } = require('../bin/lenso.js');

assert.equal(platformTag('darwin', 'arm64'), 'darwin-arm64');
assert.equal(platformTag('linux', 'x64'), 'linux-x64');
assert.equal(platformTag('win32', 'x64'), 'win32-x64');
assert.equal(platformTag('freebsd', 'x64'), null);
assert.match(binaryPath('/pkg', 'darwin', 'arm64'), /vendor[/\\]darwin-arm64[/\\]lenso$/);
assert.match(binaryPath('/pkg', 'win32', 'x64'), /vendor[/\\]win32-x64[/\\]lenso\.exe$/);

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
