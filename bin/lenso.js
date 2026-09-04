#!/usr/bin/env node
"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.platformTag = platformTag;
exports.binaryPath = binaryPath;
exports.ensureExecutableBinary = ensureExecutableBinary;
exports.forwardTerminationSignals = forwardTerminationSignals;
const node_child_process_1 = require("node:child_process");
const node_fs_1 = require("node:fs");
const node_path_1 = __importDefault(require("node:path"));
const SUPPORTED_PLATFORMS = new Set(["darwin", "linux", "win32"]);
const SUPPORTED_ARCHES = new Set(["arm64", "x64"]);
function platformTag(platform = process.platform, arch = process.arch) {
    if (!SUPPORTED_PLATFORMS.has(platform) || !SUPPORTED_ARCHES.has(arch)) {
        return null;
    }
    return `${platform}-${arch}`;
}
function binaryPath(baseDir = node_path_1.default.join(__dirname, ".."), platform = process.platform, arch = process.arch) {
    const tag = platformTag(platform, arch);
    if (!tag) {
        return null;
    }
    const exe = platform === "win32" ? "lenso.exe" : "lenso";
    return node_path_1.default.join(baseDir, "vendor", tag, exe);
}
function ensureExecutableBinary(executable, platform = process.platform) {
    if (platform === "win32") {
        return;
    }
    const mode = (0, node_fs_1.statSync)(executable).mode;
    if ((mode & 0o111) === 0) {
        (0, node_fs_1.chmodSync)(executable, mode | 0o111);
    }
}
function forwardTerminationSignals(parent, child, signals = ["SIGINT", "SIGTERM"]) {
    const handlers = new Map(signals.map((signal) => [
        signal,
        () => {
            if (child.exitCode === null && child.signalCode === null) {
                child.kill(signal);
            }
        },
    ]));
    for (const [signal, handler] of handlers) {
        parent.on(signal, handler);
    }
    return () => {
        for (const [signal, handler] of handlers) {
            parent.off(signal, handler);
        }
    };
}
function run() {
    const exe = binaryPath();
    if (!exe) {
        console.error(`lenso: unsupported platform ${process.platform}/${process.arch}`);
        process.exit(1);
    }
    try {
        ensureExecutableBinary(exe);
    }
    catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        console.error(`lenso: bundled binary is not executable: ${message}`);
        process.exit(1);
    }
    const child = (0, node_child_process_1.spawn)(exe, process.argv.slice(2), {
        stdio: "inherit",
        env: {
            ...process.env,
            LENSO_HOST_EXTRACTOR: node_path_1.default.join(__dirname, "host-extract.js"),
            LENSO_HOST_JS_RUNTIME: process.execPath,
            LENSO_HOST_DISTRIBUTION_LIB: __dirname,
        },
    });
    const stopForwardingSignals = forwardTerminationSignals(process, child);
    child.on("error", (error) => {
        if (error.code === "ENOENT") {
            console.error(`lenso: bundled binary is missing for ${process.platform}/${process.arch}`);
        }
        else {
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
