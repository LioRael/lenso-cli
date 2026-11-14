import { createServer, type IncomingMessage } from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { resolve, dirname, join, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import type { Module, Services, Response } from "./lenso-host.js";
import { parse } from "smol-toml";

try { process.loadEnvFile(".env"); } catch (error) {
  if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw new Error("Invalid project .env");
}
const config = parse(await readFile("lenso.code.toml", "utf8"));
const paths = config.paths;
const source = typeof paths === "object" && paths !== null && "modules" in paths ? paths.modules : undefined;
if (typeof source !== "string" || !source.startsWith("src/") || source.split("/").some(p => !p || p === "." || p === "..")) throw new Error("Invalid project modules path");
const output = dirname(fileURLToPath(import.meta.url));
const modules = resolve(output, source.slice(4));
if (!modules.startsWith(output + sep)) throw new Error("Modules must remain below src");
const loaded: Module[] = [];
let server: ReturnType<typeof createServer> | undefined;
async function body(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const value of request.iterator({ destroyOnReturn: false })) {
    const chunk = Buffer.from(value); size += chunk.length;
    if (size > 4096) throw new Error("body_limit");
    chunks.push(chunk);
  }
  if (!size) return null;
  if (request.headers["content-type"]?.split(";")[0] !== "application/json") throw new Error("content_type");
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}
async function close(): Promise<void> {
  await new Promise<void>(done => { if (server) server.close(() => done()); else done(); });
  await Promise.all(loaded.map(module => module.close()));
}
try {
  const entries = await readdir(modules, { withFileTypes: true }).catch((error: unknown) => {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return [];
    throw error;
  });
  const plugins: Array<{ name: string; requiresIdentity?: boolean; mount(migrate:boolean, services?: Services): Promise<Module> }> = [];
  for (const entry of entries.sort((a,b) => a.name.localeCompare(b.name))) {
    if (!entry.isDirectory() || !/^[a-zA-Z0-9_-]+$/.test(entry.name)) continue;
    const file = join(modules, entry.name, "assembly", "host.js");
    try { await readFile(file); } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") continue;
      throw error;
    }
    const plugin = await import(pathToFileURL(file).href);
    let requiresIdentity = false;
    try {
      const descriptor = parse(await readFile(join(modules, entry.name, "assembly", "host.toml"), "utf8"));
      if (descriptor.protocol !== 2) throw new Error("Unsupported module host protocol");
      requiresIdentity = descriptor.requires_identity === true;
    } catch (error) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
    plugins.push({ ...plugin, requiresIdentity, name: entry.name });
  }
  const services: Services = {};
  for (const plugin of plugins.sort((a,b) => Number(Boolean(a.requiresIdentity))-Number(Boolean(b.requiresIdentity)))) {
    if (plugin.requiresIdentity && !services.identity) throw new Error(`Module ${plugin.name} requires an identity provider`);
    const module = await plugin.mount(process.argv.includes("--migrate") || process.argv.includes("--migrate-only"), services);
    loaded.push(module);
    if (module.identity) {
      if (services.identity) throw new Error("Multiple identity providers; select one application identity explicitly");
      services.identity = module.identity;
    }
  }
  if (process.argv.includes("--migrate-only")) { await close(); process.exit(0); }
  let active = 0;
  server = createServer(async (request, response) => {
    const controller = new AbortController();
    const deadline=setTimeout(() => { controller.abort(); request.destroy(); response.destroy(); },30_000);
    deadline.unref();
    request.once("aborted", () => controller.abort());
    response.once("close", () => { if (!response.writableFinished) controller.abort(); });
    const send = (status: number, result: unknown) => {
      if (!response.destroyed) { response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store" }); response.end(JSON.stringify(result)); }
    };
    const reply = async (result: Response) => {
      if (result.stream) {
        response.writeHead(result.status, { "Content-Type": "application/octet-stream", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", ...result.headers });
        await pipeline(Readable.from(result.stream), response, { signal: controller.signal });
      } else { send(result.status, result.body ?? null); }
    };
    if (active >= 32) { request.resume(); send(429, { code: "capacity" }); return; }
    active++;
    try {
      const path = new URL(request.url ?? "/", "http://localhost").pathname;
      if (request.method === "GET" && path === "/health") { request.resume(); send(200, { status: "ok", modules: loaded.length }); return; }
      let parsed: Promise<unknown> | undefined;
      const url = new URL(request.url ?? "/", "http://localhost");
      for (const module of loaded) {
        const result = module.handleStream
          ? await module.handleStream({ method: request.method ?? "GET", path, url, headers: request.headers, authorization: request.headers.authorization, source: request.iterator({ destroyOnReturn: false }), signal: controller.signal, json: () => parsed ??= body(request) })
          : await module.handle?.({ method: request.method ?? "GET", path, authorization: request.headers.authorization, body: await (parsed ??= body(request)) });
        if (result) { request.resume(); await reply(result); return; }
      }
      request.resume();
      send(404, { code: "not_found" });
    } catch (error) {
      request.resume();
      if (response.headersSent) response.destroy();
      else {
        const message = error instanceof Error ? error.message : "internal";
        send(message === "body_limit" ? 413 : message === "content_type" ? 415 : error instanceof SyntaxError ? 400 : 500, { code: "invalid_request" });
      }
    }
    finally { clearTimeout(deadline); active--; }
  });
  server.requestTimeout = 30_000; server.headersTimeout = 10_000;
  const port = Number(process.env.PORT ?? "__APP_PORT__");
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error("Invalid PORT");
  server.listen(port, "127.0.0.1", () => {
    const address = server?.address();
    if (address && typeof address !== "string") console.log(`LISTENING ${address.port}`);
  });
  for (const signal of ["SIGINT", "SIGTERM"] as const) process.once(signal, () => {
    const timeout = setTimeout(() => process.exit(1), 10_000); timeout.unref();
    void close().then(() => { clearTimeout(timeout); }, () => { process.exitCode = 1; });
  });
} catch (error) {
  await close();
  if (error instanceof Error && (error.message.startsWith("Module ") || error.message.startsWith("Multiple identity"))) console.error(error.message);
  console.error("Application startup failed; check module configuration and explicit migrations.");
  process.exitCode = 1;
}
