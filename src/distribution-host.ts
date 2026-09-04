#!/usr/bin/env node
import { createHash } from "node:crypto";
import { lstatSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { startApplication, type AppHandle } from "./host-app.js";

interface DistributionFile {
  readonly path: string;
  readonly role: string;
  readonly sha256: string;
  readonly size: number;
  readonly executable: boolean;
}

interface DistributionLock {
  readonly schema: "lenso.host-distribution.v1";
  readonly app_id: string;
  readonly target: string;
  readonly platform: "darwin" | "linux";
  readonly arch: "arm64" | "x64";
  readonly files: readonly DistributionFile[];
}

export interface StartOptions {
  readonly root: string;
  readonly registry?: string;
  readonly startupMs?: number;
  readonly stopMs?: number;
  readonly confirmationMs?: number;
}

function digest(bytes: Buffer): string {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function relativeFile(directory: string, relative: string): string {
  if (!relative || path.isAbsolute(relative) || relative.includes("\\")) {
    throw new Error(`invalid distribution path: ${relative}`);
  }
  const normalized = path.posix.normalize(relative);
  if (normalized !== relative || normalized === ".." || normalized.startsWith("../")) {
    throw new Error(`distribution path escapes its root: ${relative}`);
  }
  return path.join(directory, ...relative.split("/"));
}

/** Verifies the complete immutable distribution before any process is started. */
export function verifyDistribution(directory = __dirname): { lock: DistributionLock; identity: string } {
  const verifiesExecutableMode = process.platform !== "win32";
  const lockPath = path.join(directory, ".lenso", "distribution.lock.json");
  const metadata = lstatSync(lockPath);
  if (!metadata.isFile() || metadata.isSymbolicLink()) throw new Error("distribution lock must be a regular file");
  const bytes = readFileSync(lockPath);
  const lock = JSON.parse(bytes.toString("utf8")) as DistributionLock;
  if (lock.schema !== "lenso.host-distribution.v1" || !lock.app_id || !lock.target || !Array.isArray(lock.files)) {
    throw new Error("invalid distribution lock");
  }
  if (lock.platform !== process.platform || lock.arch !== process.arch) {
    throw new Error(`distribution target ${lock.target} does not support ${process.platform}/${process.arch}`);
  }
  const seen = new Set<string>();
  for (const file of lock.files) {
    if (!file || typeof file !== "object" || typeof file.role !== "string" ||
        typeof file.sha256 !== "string" || !Number.isSafeInteger(file.size) || file.size < 0 ||
        typeof file.executable !== "boolean" || seen.has(file.path)) {
      throw new Error("invalid distribution file inventory");
    }
    seen.add(file.path);
    relativeFile(directory, file.path);
  }
  for (const file of lock.files) {
    const absolute = relativeFile(directory, file.path);
    const fileMetadata = lstatSync(absolute);
    if (!fileMetadata.isFile() || fileMetadata.isSymbolicLink()) throw new Error(`distribution artifact is not a regular file: ${file.path}`);
    const artifact = readFileSync(absolute);
    if (artifact.length !== file.size || digest(artifact) !== file.sha256) {
      throw new Error(`distribution artifact failed integrity: ${file.path}`);
    }
    if (verifiesExecutableMode && file.executable && (fileMetadata.mode & 0o111) === 0) {
      throw new Error(`distribution artifact is not executable: ${file.path}`);
    }
  }
  for (const required of ["host_authority", "bundle_inventory", "host_runtime", "process_owner", "runtime_resolver", "entrypoint", "notices"]) {
    if (!lock.files.some(file => file.role === required)) throw new Error(`distribution is missing ${required}`);
  }
  return { lock, identity: digest(bytes) };
}

/** Starts the exact prepared Host. Business entrypoints remain Plugin-owned. */
export async function start(options: StartOptions): Promise<AppHandle> {
  const directory = __dirname;
  const { lock, identity } = verifyDistribution(directory);
  const root = path.resolve(options.root);
  const registry = path.resolve(options.registry ?? path.join(path.dirname(root), ".lenso-owners"));
  const startupMs = options.startupMs ?? 30_000;
  const stopMs = options.stopMs ?? 10_000;
  mkdirSync(registry, { recursive: true });
  return startApplication({
    owner: path.join(directory, "runtime", "lenso-process-owner"),
    executable: path.join(directory, "runtime", "lenso-host-runtime"),
    arguments: [
      "--distribution", path.join(directory, ".lenso", "distribution.lock.json"),
      "--root", root,
      "--startup-ms", String(startupMs),
      "--stop-ms", String(stopMs),
    ],
    distribution: identity,
    root,
    registry,
    startupMs,
    stopMs,
    confirmationMs: options.confirmationMs,
  });
}

function argument(name: string): string | undefined {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

async function main(): Promise<void> {
  const root = argument("--root");
  if (!root) throw new Error("usage: host.js --root /absolute/app/root [--registry /absolute/owner/registry]");
  const app = await start({ root, registry: argument("--registry") });
  let stopping = false;
  const stop = () => {
    if (!stopping) {
      stopping = true;
      void app.stop();
    }
  };
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
  const outcome = await app.closed;
  process.exitCode = outcome.shutdown === "suspended" && outcome.ownership.termination === "confirmed" ? 0 : 1;
}

if (require.main === module) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
