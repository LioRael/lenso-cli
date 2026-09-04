/** Build-time Host authoring. These declarations do not start an application. */
export interface PluginBundleReference {
  readonly bundle: string;
  readonly execution: "bun" | "process";
}

export interface HostPluginInstance {
  readonly plugin: PluginBundleReference;
  readonly instance: string;
  readonly configuration?: Readonly<Record<string, unknown>>;
}

export interface HostDeclaration {
  /** Stable product identity. Do not change it for each build. */
  readonly id: string;
  readonly plugins: readonly (PluginBundleReference | HostPluginInstance)[];
  /** Shared Slots require explicit cardinality. Many uses canonical Instance order. */
  readonly slots?: readonly HostSlot[];
  /** Exact Host-permitted provider sets for App-selectable named requirements. */
  readonly dependencies?: readonly HostDependency[];
}

export interface HostPluginIdentity {
  readonly plugin: string;
  readonly instance?: string;
}

export interface HostDependency {
  readonly consumer: HostPluginIdentity;
  readonly requirement: string;
  readonly allow: readonly HostPluginIdentity[];
  readonly default?: HostPluginIdentity;
}

export interface HostSlot {
  readonly id: string;
  readonly cardinality: "one" | "optional" | "many";
  readonly replaceable?: boolean;
  /** Required for extensions; applies to the final active Instance set. */
  readonly maxInstances?: number;
  /** Exact releases and execution implementations; never an open contract wildcard. */
  readonly allow?: readonly PluginBundleReference[];
  /** Additional JSON Schema constraint on effective configuration, not an OS sandbox. */
  readonly configurationSchema?: boolean | Readonly<Record<string, unknown>>;
}

export function pluginBundle(
  bundle: string,
  options: { readonly execution?: "bun" | "process" } = {},
): PluginBundleReference {
  return { bundle, execution: options.execution ?? "bun" };
}

export function defineHost(declaration: HostDeclaration): HostDeclaration {
  return declaration;
}
