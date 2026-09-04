import type { OwnerOptions, OwnershipOutcome } from "./host-owner.js";
export interface AppArtifact {
    id: string;
    digest: string;
    target: string;
}
export interface AppInstance {
    instance: string;
    plugin: string;
    package_revision: string;
    execution_class: string;
    artifacts: readonly AppArtifact[];
}
export interface AppSnapshot {
    revision: number;
    generation: string;
    instances: readonly AppInstance[];
    diagnostics: readonly string[];
}
export interface AppOutcome {
    shutdown: "suspended" | "failed";
    ownership: OwnershipOutcome;
}
export interface AppHandle {
    readonly closed: Promise<AppOutcome>;
    inspect(): Promise<AppSnapshot>;
    stop(): Promise<AppOutcome>;
}
export interface ApplicationOptions extends OwnerOptions {
    startupMs?: number;
}
/** Private distribution assembly API; the public generated host.js will wrap it. */
export declare function startApplication(options: ApplicationOptions): Promise<AppHandle>;
