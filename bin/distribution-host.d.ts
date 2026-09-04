#!/usr/bin/env node
import { type AppHandle } from "./host-app.js";
interface DistributionFile {
    readonly path: string;
    readonly role: string;
    readonly sha256: string;
    readonly size: number;
    readonly executable: boolean;
}
interface DistributionLock {
    readonly schema: "lenso.host-distribution.v1";
    readonly app_id: string;
    readonly target: string;
    readonly platform: "darwin" | "linux";
    readonly arch: "arm64" | "x64";
    readonly files: readonly DistributionFile[];
}
export interface StartOptions {
    readonly root: string;
    readonly registry?: string;
    readonly startupMs?: number;
    readonly stopMs?: number;
    readonly confirmationMs?: number;
}
/** Verifies the complete immutable distribution before any process is started. */
export declare function verifyDistribution(directory?: string): {
    lock: DistributionLock;
    identity: string;
};
/** Starts the exact prepared Host. Business entrypoints remain Plugin-owned. */
export declare function start(options: StartOptions): Promise<AppHandle>;
export {};
