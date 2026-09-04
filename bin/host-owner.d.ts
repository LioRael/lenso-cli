export declare const FRAME_LIMIT: number;
export declare function frame(value: unknown): Buffer;
/** Incremental and bounded even when the input chunk contains many frames. */
export declare class Frames {
    private readonly receive;
    private header;
    private headerBytes;
    private payload;
    private payloadBytes;
    constructor(receive: (value: unknown) => void);
    push(chunk: Buffer): void;
    end(): void;
}
export interface OwnerOptions {
    owner: string;
    distribution: string;
    root: string;
    registry: string;
    executable: string;
    arguments?: readonly string[];
    application?: boolean;
    startupMs?: number;
    stopMs?: number;
    confirmationMs?: number;
}
export interface OwnershipOutcome {
    termination: "confirmed" | "unconfirmed";
    cause: string;
    forced: boolean;
}
export interface OwnedProcess {
    readonly pid: number;
    readonly closed: Promise<OwnershipOutcome>;
    stop(): Promise<OwnershipOutcome>;
    send(message: unknown): void;
    onMessage(listener: (message: unknown) => void): () => void;
}
/** Internal OS ownership handshake only. No public Host start/ready promise. */
export declare function launchOwnedProcess(options: OwnerOptions): Promise<OwnedProcess>;
