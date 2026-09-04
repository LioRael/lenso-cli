#!/usr/bin/env node
"use strict";

import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { chmodSync, statSync } from "node:fs";
import path from "node:path";

const SUPPORTED_PLATFORMS = new Set(["darwin", "linux", "win32"]);
const SUPPORTED_ARCHES = new Set(["arm64", "x64"]);

export type Platform = "darwin" | "linux" | "win32";
export type Architecture = "arm64" | "x64";

export function platformTag(
  platform: string = process.platform,
  arch: string = process.arch,
): `${Platform}-${Architecture}` | null {
  if (!SUPPORTED_PLATFORMS.has(platform) || !SUPPORTED_ARCHES.has(arch)) {
    return null;
  }
  return `${platform as Platform}-${arch as Architecture}`;
}

export function binaryPath(
  baseDir = path.join(__dirname, ".."),
  platform: string = process.platform,
  arch: string = process.arch,
): string | null {
  const tag = platformTag(platform, arch);
  if (!tag) {
    return null;
  }
  const exe = platform === "win32" ? "lenso.exe" : "lenso";
  return path.join(baseDir, "vendor", tag, exe);
}

export function ensureExecutableBinary(
  executable: string,
  platform: string = process.platform,
): void {
  if (platform === "win32") {
    return;
  }
  const mode = statSync(executable).mode;
  if ((mode & 0o111) === 0) {
    chmodSync(executable, mode | 0o111);
  }
}

interface SignalParent {
  on(signal: NodeJS.Signals, listener: () => void): unknown;
  off(signal: NodeJS.Signals, listener: () => void): unknown;
}

interface SignalChild {
  exitCode: number | null;
  signalCode: NodeJS.Signals | null;
  kill(signal: NodeJS.Signals): unknown;
}

export function forwardTerminationSignals(
  parent: SignalParent | EventEmitter,
  child: SignalChild,
  signals: readonly NodeJS.Signals[] = ["SIGINT", "SIGTERM"],
): () => void {
  const handlers = new Map(
    signals.map((signal) => [
      signal,
      () => {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill(signal);
        }
      },
    ]),
  );
  for (const [signal, handler] of handlers) {
    parent.on(signal, handler);
  }
  return () => {
    for (const [signal, handler] of handlers) {
      parent.off(signal, handler);
    }
  };
}

function run(): void {
  const exe = binaryPath();
  if (!exe) {
    console.error(`lenso: unsupported platform ${process.platform}/${process.arch}`);
    process.exit(1);
  }

  try {
    ensureExecutableBinary(exe);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`lenso: bundled binary is not executable: ${message}`);
    process.exit(1);
  }

  const child = spawn(exe, process.argv.slice(2), {
    stdio: "inherit",
    env: {
      ...process.env,
      LENSO_HOST_EXTRACTOR: path.join(__dirname, "host-extract.js"),
      LENSO_HOST_JS_RUNTIME: process.execPath,
      LENSO_HOST_DISTRIBUTION_LIB: __dirname,
    },
  });
  const stopForwardingSignals = forwardTerminationSignals(process, child);
  child.on("error", (error: NodeJS.ErrnoException) => {
    if (error.code === "ENOENT") {
      console.error(
        `lenso: bundled binary is missing for ${process.platform}/${process.arch}`,
      );
    } else {
      console.error(`lenso: ${error.message}`);
    }
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    stopForwardingSignals();
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
