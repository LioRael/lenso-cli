import type { OwnerOptions, OwnedProcess, OwnershipOutcome } from "./host-owner.js";
import { launchOwnedProcess } from "./host-owner.js";

export interface AppArtifact { id: string; digest: string; target: string }
export interface AppInstance { instance: string; plugin: string; package_revision: string; execution_class: string; artifacts: readonly AppArtifact[] }
export interface AppSnapshot { revision: number; generation: string; instances: readonly AppInstance[]; diagnostics: readonly string[] }
export interface AppOutcome {
  shutdown: "suspended" | "failed";
  ownership: OwnershipOutcome;
}
export interface AppHandle {
  readonly closed: Promise<AppOutcome>;
  inspect(): Promise<AppSnapshot>;
  stop(): Promise<AppOutcome>;
}
export interface ApplicationOptions extends OwnerOptions { startupMs?: number }

interface Pending { resolve(value: Record<string, unknown>): void; reject(error: Error): void }

/** Private distribution assembly API; the public generated host.js will wrap it. */
export async function startApplication(options: ApplicationOptions): Promise<AppHandle> {
  const process = await launchOwnedProcess({ ...options, application: true });
  let id = 1;
  let revision: number | undefined;
  let stopping: Promise<AppOutcome> | undefined;
  const pending = new Map<number, Pending>();
  let resolveTerminal!: (value: Record<string, unknown>) => void;
  const terminal = new Promise<Record<string, unknown>>(resolve => { resolveTerminal = resolve; });
  const unsubscribe = process.onMessage(value => {
    if (!value || typeof value !== "object" || Array.isArray(value)) return failAll("invalid application envelope");
    const message = value as Record<string, unknown>;
    if (message.version !== 1 || typeof message.kind !== "string") return failAll("incompatible application envelope");
    if (message.kind === "ready") revision = Number(message.revision);
    if (message.kind === "terminal") { resolveTerminal(message); return; }
    if (typeof message.id === "number") {
      const request = pending.get(message.id);
      if (request) { pending.delete(message.id); message.kind === "rejected" || message.kind === "start_failed" ? request.reject(new Error(String(message.code ?? message.cause))) : request.resolve(message); }
    }
  });
  function failAll(reason: string): void {
    for (const request of pending.values()) request.reject(new Error(reason));
    pending.clear();
  }
  function request(message: Record<string, unknown>): Promise<Record<string, unknown>> {
    const requestId = ++id;
    message.id = requestId;
    return new Promise((resolve, reject) => { pending.set(requestId, { resolve, reject }); process.send(message); });
  }
  const startup = new Promise<Record<string, unknown>>((resolve, reject) => pending.set(1, { resolve, reject }));
  process.send({ op: "start", version: 1, id: 1, distribution: options.distribution });
  const timeout = setTimeout(() => failAll("application startup timed out"), options.startupMs ?? 30_000);
  let started: Record<string, unknown>;
  try { started = await startup; } catch (error) { await process.stop(); throw error; } finally { clearTimeout(timeout); }
  if (started.kind !== "started" || revision === undefined || started.revision !== revision) { await process.stop(); throw new Error("application readiness handshake is incomplete"); }
  const readyRevision = revision;
  async function stop(): Promise<AppOutcome> {
    if (!stopping) stopping = (async () => {
      let receipt: Record<string, unknown>;
      try { receipt = await Promise.race([request({ op: "stop", version: 1 }), terminal]); }
      catch { receipt = await terminal; }
      const ownership = await process.stop();
      unsubscribe(); failAll("application is terminal");
      return { shutdown: receipt.shutdown === "suspended" ? "suspended" : "failed", ownership };
    })();
    return stopping;
  }
  const closed = (async () => {
    const [receipt, ownership] = await Promise.all([terminal, process.closed]);
    unsubscribe(); failAll("application is terminal");
    return { shutdown: receipt.shutdown === "suspended" ? "suspended" : "failed", ownership } as AppOutcome;
  })();
  return { closed, stop,
    async inspect() {
      if (stopping) throw new Error("application is stopping");
      const instances: AppInstance[] = []; let offset: number | null = 0; let generation = ""; let diagnostics: string[] = [];
      while (offset !== null) {
        if (pending.size >= 8) throw new Error("too many outstanding inspections");
        const page = await request({ op: "inspect", version: 1, revision: readyRevision, offset, limit: 64 });
        generation = String(page.generation); diagnostics = page.diagnostics as string[];
        instances.push(...(page.instances as AppInstance[])); offset = page.next as number | null;
      }
      return { revision: readyRevision, generation, instances, diagnostics };
    } };
}
