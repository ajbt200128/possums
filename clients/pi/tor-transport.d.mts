export interface TorProxySelection {
  /** Numeric loopback only: 127.0.0.0/8 or ::1. No proxy DNS lookup. */
  host: string;
  port: number;
  /** Optional pair; no-auth selection remains permitted. No isolation guarantee. */
  username?: string;
  password?: string;
  /** Connection/headers deadline, not a response-body lifetime cap. */
  timeoutMs?: number;
}
export interface TorTransportDiagnostic {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  readonly outcome: string;
}
export class TorTransportFailure extends Error {
  constructor(kind: string);
  readonly diagnostic: Readonly<TorTransportDiagnostic>;
}
export function torTransportDiagnostic(error: unknown): Readonly<TorTransportDiagnostic>;
export interface TorTransport {
  /** Manual redirects; HTTP only for onion endpoints or reserved offline names. */
  fetch(resource: string | URL | Request, init?: RequestInit): Promise<Response>;
  /** Idempotent local disposal; does not establish remote cancellation or billing. */
  close(): Promise<void>;
}
/**
 * Requires unmodified Undici 8.12.1. SDK injection is offline-qualification only.
 * No arbitrary TLS override or backend trust callback is exposed. The maintained
 * requestTls.signal supports TLS cancellation; parent owns attested-key TLS design.
 */
export function createTorTransport(selection: TorProxySelection, qualification?: {
  sdk: typeof import('undici');
}): TorTransport;
