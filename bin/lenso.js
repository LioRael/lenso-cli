#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');
const path = require('node:path');

function platformTag(platform = process.platform, arch = process.arch) {
  const platforms = new Set(['darwin', 'linux', 'win32']);
  const arches = new Set(['arm64', 'x64']);
  if (!platforms.has(platform) || !arches.has(arch)) {
    return null;
  }
  return `${platform}-${arch}`;
}

function binaryPath(baseDir = path.join(__dirname, '..'), platform = process.platform, arch = process.arch) {
  const tag = platformTag(platform, arch);
  if (!tag) {
    return null;
  }
  const exe = platform === 'win32' ? 'lenso.exe' : 'lenso';
  return path.join(baseDir, 'vendor', tag, exe);
}

function run() {
  const exe = binaryPath();
  if (!exe) {
    console.error(`lenso: unsupported platform ${process.platform}/${process.arch}`);
    process.exit(1);
  }

  const child = spawn(exe, process.argv.slice(2), { stdio: 'inherit' });
  child.on('error', (error) => {
    if (error.code === 'ENOENT') {
      console.error(`lenso: bundled binary is missing for ${process.platform}/${process.arch}`);
    } else {
      console.error(`lenso: ${error.message}`);
    }
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}

if (require.main === module) {
  run();
}

module.exports = { binaryPath, platformTag };
