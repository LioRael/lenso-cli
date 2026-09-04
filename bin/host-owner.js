"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.Frames = exports.FRAME_LIMIT = void 0;
exports.frame = frame;
exports.launchOwnedProcess = launchOwnedProcess;
// Private distribution plumbing. An owned process is NOT a ready Lenso App.
const node_child_process_1 = require("node:child_process");
const node_path_1 = require("node:path");
exports.FRAME_LIMIT = 256 * 1024;
function parseJson(bytes) {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    const objects = [];
    for (let offset = 0; offset < text.length; offset++) {
        if (text[offset] === "{") {
            if (objects.length >= 64)
                throw new Error("control JSON nesting exceeded");
            objects.push(new Set());
        }
        else if (text[offset] === "}")
            objects.pop();
        else if (text[offset] === '"') {
            const begin = offset++;
            while (offset < text.length && text[offset] !== '"') {
                if (text[offset] === "\\")
                    offset++;
                offset++;
            }
            let next = offset + 1;
            while (next < text.length && /\s/.test(text[next]))
                next++;
            if (text[next] === ":") {
                const key = JSON.parse(text.slice(begin, offset + 1));
                const keys = objects.at(-1);
                if (!keys || keys.has(key))
                    throw new Error("duplicate or invalid control JSON key");
                keys.add(key);
            }
        }
    }
    return JSON.parse(text);
}
function frame(value) {
    const payload = Buffer.from(JSON.stringify(value), "utf8");
    if (!payload.length || payload.length > exports.FRAME_LIMIT)
        throw new Error("invalid control frame length");
    const header = Buffer.alloc(4);
    header.writeUInt32BE(payload.length);
    return Buffer.concat([header, payload]);
}
/** Incremental and bounded even when the input chunk contains many frames. */
class Frames {
    receive;
    header = Buffer.alloc(4);
    headerBytes = 0;
    payload;
    payloadBytes = 0;
    constructor(receive) {
        this.receive = receive;
    }
    push(chunk) {
        let offset = 0;
        while (offset < chunk.length) {
            if (!this.payload) {
                const count = Math.min(4 - this.headerBytes, chunk.length - offset);
                chunk.copy(this.header, this.headerBytes, offset, offset + count);
                offset += count;
                this.headerBytes += count;
                if (this.headerBytes !== 4)
                    continue;
                const length = this.header.readUInt32BE();
                if (!length || length > exports.FRAME_LIMIT)
                    throw new Error("invalid control frame length");
                this.payload = Buffer.alloc(length);
            }
            const count = Math.min(this.payload.length - this.payloadBytes, chunk.length - offset);
            chunk.copy(this.payload, this.payloadBytes, offset, offset + count);
            this.payloadBytes += count;
            offset += count;
            if (this.payloadBytes === this.payload.length) {
                const value = parseJson(this.payload);
                this.payload = undefined;
                this.payloadBytes = 0;
                this.headerBytes = 0;
                this.receive(value);
            }
        }
    }
    end() {
        if (this.headerBytes || this.payload)
            throw new Error("truncated control frame");
    }
}
exports.Frames = Frames;
function budget(value, fallback) {
    const result = value ?? fallback;
    if (!Number.isInteger(result) || result <= 0 || result > 60_000)
        throw new Error("invalid owner time budget");
    return result;
}
function object(value, keys) {
    if (!value || typeof value !== "object" || Array.isArray(value))
        throw new Error("invalid owner envelope");
    const record = value;
    if (Object.keys(record).length !== keys.length || keys.some(key => !(key in record)))
        throw new Error("invalid owner envelope fields");
    return record;
}
/** Internal OS ownership handshake only. No public Host start/ready promise. */
async function launchOwnedProcess(options) {
    for (const path of [options.owner, options.root, options.registry, options.executable]) {
        if (!(0, node_path_1.isAbsolute)(path))
            throw new Error("native owner paths must be absolute");
    }
    if (!options.distribution || Buffer.byteLength(options.distribution) > 256)
        throw new Error("invalid distribution identity");
    const startupMs = budget(options.startupMs, 30_000);
    const stopMs = budget(options.stopMs, 10_000);
    const confirmationMs = budget(options.confirmationMs, 5_000);
    const start = frame({ version: 1, distribution: options.distribution, request_id: 1,
        root: options.root, registry: options.registry, executable: options.executable,
        arguments: options.arguments ?? [], stop_ms: stopMs, confirmation_ms: confirmationMs,
        application: options.application ?? false });
    const child = (0, node_child_process_1.spawn)(options.owner, [options.distribution], { stdio: ["pipe", "pipe", "ignore"] });
    let resolveClosed;
    const closed = new Promise(resolve => { resolveClosed = resolve; });
    let resolveOwned;
    let rejectOwned;
    const owned = new Promise((resolve, reject) => { resolveOwned = resolve; rejectOwned = reject; });
    let seenOwned = false;
    let terminal = false;
    let stopping = false;
    let protocolFailed = false;
    let cleanupTimer;
    const listeners = new Set();
    closed.then(outcome => {
        for (const listener of listeners)
            listener({ kind: "terminal", version: 1, id: 0, shutdown: "failed", ownership: outcome });
    });
    const startupTimer = setTimeout(() => fail("owner_startup_timeout"), startupMs);
    function finish(outcome) {
        if (terminal)
            return;
        terminal = true;
        clearTimeout(startupTimer);
        clearTimeout(cleanupTimer);
        resolveClosed(outcome);
        if (!seenOwned)
            rejectOwned(Object.assign(new Error(`native ownership startup failed: ${outcome.cause}`), { outcome }));
        child.stdin.destroy();
        child.stdout.destroy();
        // Never kill the execution owner. It retains cleanup authority after timeout.
        child.unref();
    }
    function fail(cause) {
        if (terminal || stopping)
            return;
        stopping = true;
        clearTimeout(startupTimer);
        child.stdin.destroy(); // EOF independently starts native cleanup.
        cleanupTimer = setTimeout(() => finish({ termination: "unconfirmed", cause, forced: true }), stopMs + confirmationMs + 250);
    }
    function stop() {
        if (!stopping && !terminal) {
            stopping = true;
            clearTimeout(startupTimer);
            child.stdin.end(frame({ version: 1, request_id: 2, op: "stop" }));
            cleanupTimer = setTimeout(() => finish({ termination: "unconfirmed", cause: "owner_confirmation_timeout", forced: true }), stopMs + confirmationMs + 250);
        }
        return closed;
    }
    const frames = new Frames(value => {
        if (terminal)
            throw new Error("event after terminal");
        const kind = value?.kind;
        if (kind === "owned") {
            const message = object(value, ["kind", "version", "distribution", "request_id", "pid"]);
            if (seenOwned || stopping || message.version !== 1 || message.distribution !== options.distribution || message.request_id !== 1 || !Number.isSafeInteger(message.pid) || Number(message.pid) <= 0)
                throw new Error("invalid owner handshake");
            seenOwned = true;
            clearTimeout(startupTimer);
            const send = (message) => {
                if (terminal || stopping)
                    throw new Error("owned process is stopping");
                child.stdin.write(frame({ version: 1, request_id: nextRequest++, op: "application", message }));
            };
            let nextRequest = 2;
            resolveOwned({ pid: Number(message.pid), closed, stop, send,
                onMessage(listener) { listeners.add(listener); return () => listeners.delete(listener); } });
        }
        else if (kind === "application") {
            const message = object(value, ["kind", "message"]);
            for (const listener of listeners)
                listener(message.message);
        }
        else if (kind === "terminal") {
            const message = object(value, ["kind", "version", "termination", "cause", "forced"]);
            if (!seenOwned || message.version !== 1 || !["confirmed", "unconfirmed"].includes(String(message.termination)) || typeof message.cause !== "string" || message.forced !== true)
                throw new Error("invalid ownership outcome");
            finish({ termination: message.termination, cause: message.cause, forced: true });
        }
        else
            throw new Error("unexpected owner event");
    });
    child.stdout.on("data", chunk => {
        if (protocolFailed)
            return;
        try {
            frames.push(chunk);
        }
        catch {
            protocolFailed = true;
            fail("invalid_owner_control");
        }
    });
    child.stdout.on("error", () => fail("owner_transport_failed"));
    child.stdin.on("error", () => fail("owner_transport_failed"));
    child.on("error", () => finish({ termination: "unconfirmed", cause: "owner_spawn_failed", forced: true }));
    child.on("close", () => {
        try {
            frames.end();
        }
        catch { /* EOF is never termination proof. */ }
        finish({ termination: "unconfirmed", cause: "owner_exited_without_confirmation", forced: true });
    });
    child.stdin.write(start);
    return owned;
}
