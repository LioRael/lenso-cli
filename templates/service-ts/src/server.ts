import { serveService } from "@lenso/service-kit";

import { moduleRelease, service } from "./service.ts";

if (process.argv.includes("--check")) {
  console.log(JSON.stringify(service, null, 2));
  process.exit(0);
}

if (process.argv.includes("--check-release")) {
  console.log(JSON.stringify(moduleRelease, null, 2));
  process.exit(0);
}

const port = Number(process.env.PORT ?? "{{service_port}}");
const server = await serveService(service, { port });

console.log(`Lenso service ready: ${server.manifestUrl}`);
