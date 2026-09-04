// Private distribution plumbing. An owned process is NOT a ready Lenso App.
import { spawn } from "node:child_process";
import { isAbsolute } from "node:path";

export const FRAME_LIMIT = 256 * 1024;

function parseJson(bytes: Buffer): unknown {
  const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  const objects: Set<string>[] = [];
  for (let offset = 0; offset < text.length; offset++) {
    if (text[offset] === "{") {
      if (objects.length >= 64) throw new Error("control JSON nesting exceeded");
      objects.push(new Set());
    } else if (text[offset] === "}") objects.pop();
    else if (text[offset] === '"') {
      const begin = offset++;
      while (offset < text.length && text[offset] !== '"') {
        if (text[offset] === "\\") offset++;
        offset++;
      }
      let next = offset + 1;
      while (next < text.length && /\s/.test(text[next])) next++;
      if (text[next] === ":") {
        const key: string = JSON.parse(text.slice(begin, offset + 1));
        const keys = objects.at(-1);
        if (!keys || keys.has(key)) throw new Error("duplicate or invalid control JSON key");
        keys.add(key);
      }
    }
  }
  return JSON.parse(text);
}

export function frame(value: unknown): Buffer {
  const payload = Buffer.from(JSON.stringify(value), "utf8");
  if (!payload.length || payload.length > FRAME_LIMIT) throw new Error("invalid control frame length");
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payload.length);
  return Buffer.concat([header, payload]);
}

/** Incremental and bounded even when the input chunk contains many frames. */
export class Frames {
  private header = Buffer.alloc(4);
  private headerBytes = 0;
  private payload: Buffer | undefined;
  private payloadBytes = 0;
  constructor(private readonly receive: (value: unknown) => void) {}
  push(chunk: Buffer): void {
    let offset = 0;
    while (offset < chunk.length) {
      if (!this.payload) {
        const count = Math.min(4 - this.headerBytes, chunk.length - offset);
        chunk.copy(this.header, this.headerBytes, offset, offset + count);
        offset += count;
        this.headerBytes += count;
        if (this.headerBytes !== 4) continue;
        const length = this.header.readUInt32BE();
        if (!length || length > FRAME_LIMIT) throw new Error("invalid control frame length");
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
  end(): void {
    if (this.headerBytes || this.payload) throw new Error("truncated control frame");
  }
}

export interface OwnerOptions {
  owner: string;
  distribution: string;
  root: string;
  registry: string;
  executable: string;
  arguments?: readonly string[];
  application?: boolean;
  startupMs?: number;
  stopMs?: number;
  confirmationMs?: number;
}

export interface OwnershipOutcome {
  termination: "confirmed" | "unconfirmed";
  cause: string;
  forced: boolean;
}

export interface OwnedProcess {
  readonly pid: number;
  readonly closed: Promise<OwnershipOutcome>;
  stop(): Promise<OwnershipOutcome>;
  send(message: unknown): void;
  onMessage(listener: (message: unknown) => void): () => void;
}

function budget(value: number | undefined, fallback: number): number {
  const result = value ?? fallback;
  if (!Number.isInteger(result) || result <= 0 || result > 60_000) throw new Error("invalid owner time budget");
  return result;
}

function object(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid owner envelope");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== keys.length || keys.some(key => !(key in record))) throw new Error("invalid owner envelope fields");
  return record;
}

/** Internal OS ownership handshake only. No public Host start/ready promise. */
export async function launchOwnedProcess(options: OwnerOptions): Promise<OwnedProcess> {
  for (const path of [options.owner, options.root, options.registry, options.executable]) {
    if (!isAbsolute(path)) throw new Error("native owner paths must be absolute");
  }
  if (!options.distribution || Buffer.byteLength(options.distribution) > 256) throw new Error("invalid distribution identity");
  const startupMs = budget(options.startupMs, 30_000);
  const stopMs = budget(options.stopMs, 10_000);
  const confirmationMs = budget(options.confirmationMs, 5_000);
  const start = frame({ version: 1, distribution: options.distribution, request_id: 1,
    root: options.root, registry: options.registry, executable: options.executable,
    arguments: options.arguments ?? [], stop_ms: stopMs, confirmation_ms: confirmationMs,
    application: options.application ?? false });
  const child = spawn(options.owner, [options.distribution], { stdio: ["pipe", "pipe", "ignore"] });
  let resolveClosed!: (outcome: OwnershipOutcome) => void;
  const closed = new Promise<OwnershipOutcome>(resolve => { resolveClosed = resolve; });
  let resolveOwned!: (process: OwnedProcess) => void;
  let rejectOwned!: (error: Error) => void;
  const owned = new Promise<OwnedProcess>((resolve, reject) => { resolveOwned = resolve; rejectOwned = reject; });
  let seenOwned = false;
  let terminal = false;
  let stopping = false;
  let protocolFailed = false;
  let cleanupTimer: ReturnType<typeof setTimeout> | undefined;
  const listeners = new Set<(message: unknown) => void>();
  closed.then(outcome => {
    for (const listener of listeners) listener({ kind: "terminal", version: 1, id: 0, shutdown: "failed", ownership: outcome });
  });
  const startupTimer = setTimeout(() => fail("owner_startup_timeout"), startupMs);
  function finish(outcome: OwnershipOutcome): void {
    if (terminal) return;
    terminal = true;
    clearTimeout(startupTimer);
    clearTimeout(cleanupTimer);
    resolveClosed(outcome);
    if (!seenOwned) rejectOwned(Object.assign(new Error(`native ownership startup failed: ${outcome.cause}`), { outcome }));
    child.stdin.destroy();
    child.stdout.destroy();
    // Never kill the execution owner. It retains cleanup authority after timeout.
    child.unref();
  }
  function fail(cause: string): void {
    if (terminal || stopping) return;
    stopping = true;
    clearTimeout(startupTimer);
    child.stdin.destroy(); // EOF independently starts native cleanup.
    cleanupTimer = setTimeout(() => finish({ termination: "unconfirmed", cause, forced: true }), stopMs + confirmationMs + 250);
  }
  function stop(): Promise<OwnershipOutcome> {
    if (!stopping && !terminal) {
      stopping = true;
      clearTimeout(startupTimer);
      child.stdin.end(frame({ version: 1, request_id: 2, op: "stop" }));
      cleanupTimer = setTimeout(() => finish({ termination: "unconfirmed", cause: "owner_confirmation_timeout", forced: true }), stopMs + confirmationMs + 250);
    }
    return closed;
  }
  const frames = new Frames(value => {
    if (terminal) throw new Error("event after terminal");
    const kind = (value as { kind?: unknown } | null)?.kind;
    if (kind === "owned") {
      const message = object(value, ["kind", "version", "distribution", "request_id", "pid"]);
      if (seenOwned || stopping || message.version !== 1 || message.distribution !== options.distribution || message.request_id !== 1 || !Number.isSafeInteger(message.pid) || Number(message.pid) <= 0) throw new Error("invalid owner handshake");
      seenOwned = true;
      clearTimeout(startupTimer);
      const send = (message: unknown) => {
        if (terminal || stopping) throw new Error("owned process is stopping");
        child.stdin.write(frame({ version: 1, request_id: nextRequest++, op: "application", message }));
      };
      let nextRequest = 2;
      resolveOwned({ pid: Number(message.pid), closed, stop, send,
        onMessage(listener) { listeners.add(listener); return () => listeners.delete(listener); } });
    } else if (kind === "application") {
      const message = object(value, ["kind", "message"]);
      for (const listener of listeners) listener(message.message);
    } else if (kind === "terminal") {
      const message = object(value, ["kind", "version", "termination", "cause", "forced"]);
      if (!seenOwned || message.version !== 1 || !["confirmed", "unconfirmed"].includes(String(message.termination)) || typeof message.cause !== "string" || message.forced !== true) throw new Error("invalid ownership outcome");
      finish({ termination: message.termination as OwnershipOutcome["termination"], cause: message.cause, forced: true });
    } else throw new Error("unexpected owner event");
  });
  child.stdout.on("data", chunk => {
    if (protocolFailed) return;
    try { frames.push(chunk); } catch { protocolFailed = true; fail("invalid_owner_control"); }
  });
  child.stdout.on("error", () => fail("owner_transport_failed"));
  child.stdin.on("error", () => fail("owner_transport_failed"));
  child.on("error", () => finish({ termination: "unconfirmed", cause: "owner_spawn_failed", forced: true }));
  child.on("close", () => {
    try { frames.end(); } catch { /* EOF is never termination proof. */ }
    finish({ termination: "unconfirmed", cause: "owner_exited_without_confirmation", forced: true });
  });
  child.stdin.write(start);
  return owned;
}
