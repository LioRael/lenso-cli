import { chmodSync, copyFileSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { platformTag } = require('../bin/lenso.js');

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const tag = process.env.LENSO_NPM_TAG || platformTag();
const supportedTags = new Set(['darwin-arm64', 'darwin-x64', 'linux-x64', 'win32-x64']);
if (!tag || !supportedTags.has(tag)) {
  throw new Error(`unsupported npm target ${tag ?? `${process.platform}/${process.arch}`}`);
}
const cargoTarget = process.env.LENSO_CARGO_TARGET;

// ponytail: clean only this crate so the npm vendor binary always matches this checkout.
const clean = spawnSync('cargo', ['clean', '--release', '-p', 'lenso-cli'], {
  cwd: root,
  stdio: 'inherit'
});
if (clean.status !== 0) {
  process.exit(clean.status ?? 1);
}

const buildArgs = ['build', '--release', '--locked'];
if (cargoTarget) {
  buildArgs.push('--target', cargoTarget);
}

const build = spawnSync('cargo', buildArgs, {
  cwd: root,
  stdio: 'inherit'
});
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

const exe = tag.startsWith('win32-') ? 'lenso.exe' : 'lenso';
const src = cargoTarget
  ? path.join(root, 'target', cargoTarget, 'release', exe)
  : path.join(root, 'target', 'release', exe);
const destDir = path.join(root, 'vendor', tag);
const dest = path.join(destDir, exe);

mkdirSync(destDir, { recursive: true });
copyFileSync(src, dest);
if (process.platform !== 'win32') {
  chmodSync(dest, 0o755);
}

console.log(`packed ${path.relative(root, dest)}`);
