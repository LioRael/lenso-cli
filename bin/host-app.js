"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.startApplication = startApplication;
const host_owner_js_1 = require("./host-owner.js");
/** Private distribution assembly API; the public generated host.js will wrap it. */
async function startApplication(options) {
    const process = await (0, host_owner_js_1.launchOwnedProcess)({ ...options, application: true });
    let id = 1;
    let revision;
    let stopping;
    const pending = new Map();
    let resolveTerminal;
    const terminal = new Promise(resolve => { resolveTerminal = resolve; });
    const unsubscribe = process.onMessage(value => {
        if (!value || typeof value !== "object" || Array.isArray(value))
            return failAll("invalid application envelope");
        const message = value;
        if (message.version !== 1 || typeof message.kind !== "string")
            return failAll("incompatible application envelope");
        if (message.kind === "ready")
            revision = Number(message.revision);
        if (message.kind === "terminal") {
            resolveTerminal(message);
            return;
        }
        if (typeof message.id === "number") {
            const request = pending.get(message.id);
            if (request) {
                pending.delete(message.id);
                message.kind === "rejected" || message.kind === "start_failed" ? request.reject(new Error(String(message.code ?? message.cause))) : request.resolve(message);
            }
        }
    });
    function failAll(reason) {
        for (const request of pending.values())
            request.reject(new Error(reason));
        pending.clear();
    }
    function request(message) {
        const requestId = ++id;
        message.id = requestId;
        return new Promise((resolve, reject) => { pending.set(requestId, { resolve, reject }); process.send(message); });
    }
    const startup = new Promise((resolve, reject) => pending.set(1, { resolve, reject }));
    process.send({ op: "start", version: 1, id: 1, distribution: options.distribution });
    const timeout = setTimeout(() => failAll("application startup timed out"), options.startupMs ?? 30_000);
    let started;
    try {
        started = await startup;
    }
    catch (error) {
        await process.stop();
        throw error;
    }
    finally {
        clearTimeout(timeout);
    }
    if (started.kind !== "started" || revision === undefined || started.revision !== revision) {
        await process.stop();
        throw new Error("application readiness handshake is incomplete");
    }
    const readyRevision = revision;
    async function stop() {
        if (!stopping)
            stopping = (async () => {
                let receipt;
                try {
                    receipt = await Promise.race([request({ op: "stop", version: 1 }), terminal]);
                }
                catch {
                    receipt = await terminal;
                }
                const ownership = await process.stop();
                unsubscribe();
                failAll("application is terminal");
                return { shutdown: receipt.shutdown === "suspended" ? "suspended" : "failed", ownership };
            })();
        return stopping;
    }
    const closed = (async () => {
        const [receipt, ownership] = await Promise.all([terminal, process.closed]);
        unsubscribe();
        failAll("application is terminal");
        return { shutdown: receipt.shutdown === "suspended" ? "suspended" : "failed", ownership };
    })();
    return { closed, stop,
        async inspect() {
            if (stopping)
                throw new Error("application is stopping");
            const instances = [];
            let offset = 0;
            let generation = "";
            let diagnostics = [];
            while (offset !== null) {
                if (pending.size >= 8)
                    throw new Error("too many outstanding inspections");
                const page = await request({ op: "inspect", version: 1, revision: readyRevision, offset, limit: 64 });
                generation = String(page.generation);
                diagnostics = page.diagnostics;
                instances.push(...page.instances);
                offset = page.next;
            }
            return { revision: readyRevision, generation, instances, diagnostics };
        } };
}
