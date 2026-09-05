#!/usr/bin/env node
"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.verifyDistribution = verifyDistribution;
exports.start = start;
const node_crypto_1 = require("node:crypto");
const node_fs_1 = require("node:fs");
const node_path_1 = __importDefault(require("node:path"));
const host_app_js_1 = require("./host-app.js");
function digest(bytes) {
    return `sha256:${(0, node_crypto_1.createHash)("sha256").update(bytes).digest("hex")}`;
}
function relativeFile(directory, relative) {
    if (!relative || node_path_1.default.isAbsolute(relative) || relative.includes("\\")) {
        throw new Error(`invalid distribution path: ${relative}`);
    }
    const normalized = node_path_1.default.posix.normalize(relative);
    if (normalized !== relative || normalized === ".." || normalized.startsWith("../")) {
        throw new Error(`distribution path escapes its root: ${relative}`);
    }
    return node_path_1.default.join(directory, ...relative.split("/"));
}
/** Verifies the complete immutable distribution before any process is started. */
function verifyDistribution(directory = __dirname) {
    const verifiesExecutableMode = process.platform !== "win32";
    const lockPath = node_path_1.default.join(directory, ".lenso", "distribution.lock.json");
    const metadata = (0, node_fs_1.lstatSync)(lockPath);
    if (!metadata.isFile() || metadata.isSymbolicLink())
        throw new Error("distribution lock must be a regular file");
    const bytes = (0, node_fs_1.readFileSync)(lockPath);
    const lock = JSON.parse(bytes.toString("utf8"));
    if (lock.schema !== "lenso.host-distribution.v1" || !lock.app_id || !lock.target || !Array.isArray(lock.files)) {
        throw new Error("invalid distribution lock");
    }
    if (lock.platform !== process.platform || lock.arch !== process.arch) {
        throw new Error(`distribution target ${lock.target} does not support ${process.platform}/${process.arch}`);
    }
    const seen = new Set();
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
        const fileMetadata = (0, node_fs_1.lstatSync)(absolute);
        if (!fileMetadata.isFile() || fileMetadata.isSymbolicLink())
            throw new Error(`distribution artifact is not a regular file: ${file.path}`);
        const artifact = (0, node_fs_1.readFileSync)(absolute);
        if (artifact.length !== file.size || digest(artifact) !== file.sha256) {
            throw new Error(`distribution artifact failed integrity: ${file.path}`);
        }
        if (verifiesExecutableMode && file.executable && (fileMetadata.mode & 0o111) === 0) {
            throw new Error(`distribution artifact is not executable: ${file.path}`);
        }
    }
    for (const required of ["host_authority", "bundle_inventory", "host_runtime", "process_owner", "runtime_resolver", "entrypoint", "notices"]) {
        if (!lock.files.some(file => file.role === required))
            throw new Error(`distribution is missing ${required}`);
    }
    return { lock, identity: digest(bytes) };
}
/** Starts the exact prepared Host. Business entrypoints remain Plugin-owned. */
async function start(options) {
    const directory = __dirname;
    const { lock, identity } = verifyDistribution(directory);
    const root = node_path_1.default.resolve(options.root);
    const registry = node_path_1.default.resolve(options.registry ?? node_path_1.default.join(node_path_1.default.dirname(root), ".lenso-owners"));
    const startupMs = options.startupMs ?? 30_000;
    const stopMs = options.stopMs ?? 10_000;
    (0, node_fs_1.mkdirSync)(registry, { recursive: true });
    return (0, host_app_js_1.startApplication)({
        owner: node_path_1.default.join(directory, "runtime", "lenso-process-owner"),
        executable: node_path_1.default.join(directory, "runtime", "lenso-host-runtime"),
        arguments: [
            "--distribution", node_path_1.default.join(directory, ".lenso", "distribution.lock.json"),
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
function argument(name) {
    const index = process.argv.indexOf(name);
    return index < 0 ? undefined : process.argv[index + 1];
}
async function main() {
    const root = argument("--root");
    if (!root)
        throw new Error("usage: host.js --root /absolute/app/root [--registry /absolute/owner/registry]");
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
