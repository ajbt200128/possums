import type { Duplex } from 'node:stream';
export interface TorNativeDiagnostic {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  readonly outcome: string;
}
export class TorNativeTransportFailure extends Error {
  constructor(kind?: string);
  readonly diagnostic: TorNativeDiagnostic;
}
export function torNativeTransportDiagnostic(error: unknown): TorNativeDiagnostic;
export interface TorNativeProxy {
  /** Absolute path to a separately built, matching-platform helper. */
  helperPath: string;
  /** Numeric loopback only. */
  host: string;
  port: number;
  /** Optional SOCKS-only credentials, NEVER Possums credentials. */
  username?: string;
  password?: string;
  /** Connection/readiness deadline only; default 30000, maximum 300000 ms. */
  timeoutMs?: number;
}
export interface TorNativeScope {
  /** Proxy-resolved hostname. Raw Duplex implies NO channel authorization. */
  connectSocket(destination: { host: string; port: number }, signal?: AbortSignal): Promise<Duplex>;
  /** Idempotent; retains pending/established children until confirmed exit. */
  close(): Promise<void>;
}
/** Unwired offline candidate; official SDK accepts nonzero RSV metadata. */
export function createTorNativeTransport(proxy: TorNativeProxy): TorNativeScope;
