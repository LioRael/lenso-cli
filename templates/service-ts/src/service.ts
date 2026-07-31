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
    serviceManifest: "{{local_service_base_url}}/manifest",
  },
  capabilities: ["{{module_name}}.read"],
});

export const service = defineService({
  name: "{{service_name}}",
  version: "0.1.0",
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
