import { serveService } from "@lenso/service-kit";

import { moduleRelease, providerV1, service } from "./service.ts";

if (process.argv.includes("--check")) {
  console.log(JSON.stringify(service, null, 2));
  process.exit(0);
}

if (process.argv.includes("--check-release")) {
  console.log(JSON.stringify(moduleRelease, null, 2));
  process.exit(0);
}

const port = Number(process.env.PORT ?? "{{service_port}}");
const enrollmentToken = process.env.LENSO_LOCAL_ENROLLMENT_TOKEN;
const server = await serveService(service, {
  modules: {
    "{{module_name}}": {
      http: {
        "GET /status": () => ({ service: "{{service_name}}", status: "ready" }),
      },
    },
  },
  port,
  ...(enrollmentToken
    ? {
        providerCore: {
          bearerToken: enrollmentToken,
          serviceId: providerV1.serviceId,
          servicePrincipal: `service:${providerV1.serviceId}`,
          serviceRevision: "1",
        },
      }
    : {}),
  providerV1,
});

console.log(`Lenso service ready: ${new URL("/lenso/provider/v1", server.baseUrl).href}`);
