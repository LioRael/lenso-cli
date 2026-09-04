#!/usr/bin/env node
import { EventEmitter } from "node:events";
export type Platform = "darwin" | "linux" | "win32";
export type Architecture = "arm64" | "x64";
export declare function platformTag(platform?: string, arch?: string): `${Platform}-${Architecture}` | null;
export declare function binaryPath(baseDir?: string, platform?: string, arch?: string): string | null;
export declare function ensureExecutableBinary(executable: string, platform?: string): void;
interface SignalParent {
    on(signal: NodeJS.Signals, listener: () => void): unknown;
    off(signal: NodeJS.Signals, listener: () => void): unknown;
}
interface SignalChild {
    exitCode: number | null;
    signalCode: NodeJS.Signals | null;
    kill(signal: NodeJS.Signals): unknown;
}
export declare function forwardTerminationSignals(parent: SignalParent | EventEmitter, child: SignalChild, signals?: readonly NodeJS.Signals[]): () => void;
export {};
