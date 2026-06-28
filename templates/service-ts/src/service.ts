import {
  defineModule,
  defineModuleRelease,
  defineService,
} from "@lenso/service-kit";

export const providedModule = defineModule({
  name: "{{module_name}}",
  version: "0.1.0",
  capabilities: ["{{module_name}}.read"],
});

export const moduleRelease = defineModuleRelease({
  name: "{{module_name}}",
  version: "0.1.0",
  provider: {
    name: "{{service_name}}",
    serviceManifest: "http://127.0.0.1:4100/lenso/service/v1/manifest",
  },
  capabilities: ["{{module_name}}.read"],
});

export const service = defineService({
  name: "{{service_name}}",
  version: "0.1.0",
  compatibility: {
    remote_protocol_version: "1",
    required_host_features: ["service.status"],
  },
  install: {
    services: [
      {
        name: "{{service_name}}",
        command: "pnpm start",
        cwd: {{service_cwd}},
        readyUrl: "http://127.0.0.1:4100/lenso/service/v1/status",
        autoStart: true,
        readyTimeoutMs: 10000,
      },
    ],
  },
  modules: [providedModule],
});
