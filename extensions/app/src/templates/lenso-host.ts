export interface Principal { readonly subject: string; readonly scopes: readonly string[] }
export interface Identity { resolve(token: string | undefined): Promise<Principal | undefined> }
export interface Services { identity?: Identity }
export interface Request {
  readonly method: string;
  readonly path: string;
  readonly url: URL;
  readonly headers: Readonly<Record<string, string | string[] | undefined>>;
  readonly authorization: string | undefined;
  readonly source: AsyncIterable<Uint8Array>;
  readonly signal: AbortSignal;
  json(): Promise<unknown>;
}
export interface Response {
  status: number;
  body?: unknown;
  stream?: AsyncIterable<Uint8Array>;
  headers?: Record<string, string>;
}
export interface Module {
  identity?: Identity;
  handleStream?(request: Request): Promise<Response | undefined>;
  handle?(request: {method:string;path:string;authorization:string|undefined;body:unknown}): Promise<Response | undefined>;
  close(): Promise<void>;
}
