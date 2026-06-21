import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { platformTag } = require('../bin/lenso.js');

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const tag = platformTag();
if (!tag) {
  throw new Error(`unsupported platform ${process.platform}/${process.arch}`);
}

const consoleIndex = path.join(root, 'console', 'dist', 'index.html');
if (!existsSync(consoleIndex)) {
  console.error('console/dist/index.html is missing; build and copy Runtime Console dist before npm packing');
  process.exit(1);
}

// ponytail: include_dir embeds console files at compile time; clean only this crate before packaging.
const clean = spawnSync('cargo', ['clean', '--release', '-p', 'lenso-cli'], {
  cwd: root,
  stdio: 'inherit'
});
if (clean.status !== 0) {
  process.exit(clean.status ?? 1);
}

const build = spawnSync('cargo', ['build', '--release', '--locked'], {
  cwd: root,
  stdio: 'inherit'
});
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

const exe = process.platform === 'win32' ? 'lenso.exe' : 'lenso';
const src = path.join(root, 'target', 'release', exe);
const destDir = path.join(root, 'vendor', tag);
const dest = path.join(destDir, exe);

mkdirSync(destDir, { recursive: true });
copyFileSync(src, dest);
if (process.platform !== 'win32') {
  chmodSync(dest, 0o755);
}

console.log(`packed ${path.relative(root, dest)}`);
