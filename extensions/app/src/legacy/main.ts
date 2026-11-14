import { createServer, type IncomingMessage } from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { resolve, dirname, join, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
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
export interface Request { method: string; path: string; authorization: string | undefined; body: unknown }
export interface Response { status: number; body: unknown }
export interface Module { handle(request: Request): Promise<Response | undefined>; close(): Promise<void> }
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
  for (const entry of entries.sort((a,b) => a.name.localeCompare(b.name))) {
    if (!entry.isDirectory() || !/^[a-zA-Z0-9_-]+$/.test(entry.name)) continue;
    const file = join(modules, entry.name, "assembly", "host.js");
    try { await readFile(file); } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") continue;
      throw error;
    }
    const plugin = await import(pathToFileURL(file).href);
    loaded.push(await plugin.mount((process.argv.includes("--migrate") || process.argv.includes("--migrate-only"))) as Module);
  }
  if (process.argv.includes("--migrate-only")) { await close(); process.exit(0); }
  let active = 0;
  server = createServer(async (request, response) => {
    const send = (status: number, result: unknown) => {
      if (!response.destroyed) { response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store" }); response.end(JSON.stringify(result)); }
    };
    if (active >= 32) { request.resume(); send(429, { code: "capacity" }); return; }
    active++;
    try {
      const path = new URL(request.url ?? "/", "http://localhost").pathname;
      if (request.method === "GET" && path === "/health") { request.resume(); send(200, { status: "ok", modules: loaded.length }); return; }
      let input: unknown;
      try { input = await body(request); } catch (error) {
        request.resume();
        const message = error instanceof Error ? error.message : "invalid_input";
        send(message === "body_limit" ? 413 : message === "content_type" ? 415 : 400, { code: "invalid_input" }); return;
      }
      for (const module of loaded) {
        const result = await module.handle({ method: request.method ?? "GET", path, authorization: request.headers.authorization, body: input });
        if (result) { send(result.status, result.body); return; }
      }
      send(404, { code: "not_found" });
    } catch { send(500, { code: "internal", message: "Request failed" }); }
    finally { active--; }
  });
  server.requestTimeout = 10_000; server.headersTimeout = 10_000;
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
} catch {
  await close();
  console.error("Application startup failed; check module configuration and explicit migrations.");
  process.exitCode = 1;
}
