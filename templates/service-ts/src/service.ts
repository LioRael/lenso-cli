import { createHash } from "node:crypto";

import { defineModule, defineService, getRoute } from "@lenso/service-kit";

const canonicalize = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => (left === right ? 0 : left < right ? -1 : 1))
      .map(([key, entry]) => [key, canonicalize(entry)])
  );
};

const digest = (value: unknown) =>
  `sha256:${createHash("sha256")
    .update(JSON.stringify(canonicalize(value)))
    .digest("hex")}`;

const serviceId = "{{service_id}}";
const serviceVersion = "0.1.0";
const moduleId = "{{module_name}}";
const moduleVersion = "0.1.0";
const operationContractDigest = digest({
  operations: ["GET /status"],
  protocol: "lenso.provider-http.v1",
});
const serviceReleaseDigest = digest({
  modules: [moduleId],
  protocol: "lenso.provider-service-release.v1",
  serviceId,
  version: serviceVersion,
});

export const providedModule = defineModule({
  capabilities: ["{{module_name}}.read"],
  httpRoutes: [getRoute("/status", { capability: "{{module_name}}.read" })],
  name: moduleId,
  version: moduleVersion,
});

export const providerManifest = {
  capabilities: ["{{module_name}}.read"],
  console: [],
  console_contributions: [],
  console_slots: [],
  http_routes: [
    {
      capability: "{{module_name}}.read",
      display_name: "Read service status",
      method: "GET",
      path: "/status",
      story_title: "Service status read",
    },
  ],
  module_id: moduleId,
  protocol: "lenso.module-manifest.v1",
  story_display: [],
};
const manifestDigest = digest(providerManifest);

const releaseWithoutDigest = {
  compatibility: {},
  delivery: {
    kind: "service",
    service_id: serviceId,
    service_release_version: serviceVersion,
    service_release_digest: serviceReleaseDigest,
    export: moduleId,
    responsibility_profile: "provider",
    contract_digests: [operationContractDigest],
  },
  manifest: providerManifest,
  manifest_digest: manifestDigest,
  module_id: moduleId,
  protocol: "lenso.module-release.v1",
  version: moduleVersion,
};

export const moduleRelease = releaseWithoutDigest;
export const moduleReleaseDigest = digest(moduleRelease);

export const providerV1 = {
  exports: [
    {
      contractDigests: { http: operationContractDigest },
      exportKey: moduleId,
      manifest: providerManifest,
      manifestDigest,
      moduleId,
      moduleReleaseDigest,
      moduleVersion,
    },
  ],
  protocolContractDigest: digest({ protocol: "lenso.provider.v1" }),
  runtimeInstanceId: `${serviceId}-local`,
  serviceId,
  serviceReleaseDigest,
  serviceReleaseVersion: serviceVersion,
};

export const service = defineService({
  name: serviceId,
  version: serviceVersion,
  compatibility: {
    provider_protocol_version: "lenso.provider.v1",
    required_host_features: ["service.status"],
  },
  install: {
    services: [
      {
        name: "{{service_name}}",
        command: "pnpm start",
        cwd: {{service_cwd}},
        readyUrl: "{{service_status_url}}",
        autoStart: true,
        readyTimeoutMs: 10000,
      },
    ],
  },
  modules: [providedModule],
});
