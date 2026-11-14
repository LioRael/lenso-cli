import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";

if (process.argv.includes("--child")) {
  setInterval(() => {}, 1000);
} else {
  const mode = process.env.OPS_MODE;
  console.log("stdout marker", process.env.OPS_SECRET);
  console.error("stderr marker", process.env.OPS_SECRET);
  console.log(process.env.UNGRANTED_SECRET ? "ambient leaked" : "ambient absent");
  if (mode === "exit0" || mode === "exit7") {
    process.exit(mode === "exit0" ? 0 : 7);
  }
  const child = spawn(process.execPath, [fileURLToPath(import.meta.url), "--child"]);
  writeFileSync(process.env.OPS_PID_FILE, JSON.stringify({ parent: process.pid, children: [child.pid] }));
  const server = createServer((request, response) => {
    const status = request.url !== "/health/ready" ? 404 : mode === "not_ready" ? 503 : mode === "redirect" ? 302 : 200;
    response.writeHead(status, { Location: "http://127.0.0.1:1/", "Content-Type": "text/plain" });
    response.end("fixture\n");
  });
  if (mode !== "silent") server.listen(Number(process.env.OPS_PORT), "127.0.0.1");
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => {
      child.kill("SIGTERM");
      child.once("exit", () => process.exit(0));
      server.close();
    });
  }
}
