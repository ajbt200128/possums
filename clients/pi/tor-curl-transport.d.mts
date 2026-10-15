export interface TorCurlProxySelection {
  /** Numeric loopback only; destinations are SOCKS5 proxy-resolved. */
  host: string;
  port: number;
  /** Optional pair, independent from gateway credentials. No isolation guarantee. */
  username?: string;
  password?: string;
  /** Connect/response-start deadline; body cancellation remains caller-controlled. */
  timeoutMs?: number;
  /** Parent must authenticate fresh evidence and exact reserved TLS-SPKI first. */
  gateway?: {
    origin: string;
    /** Selected v3 B onion. CONNECT_TO always uses port 80 without changing SNI. */
    onion: string;
    expectedLeafPem: string;
    /** Base64 SHA256 of exact appraised DER TLS SubjectPublicKeyInfo. */
    expectedSpkiSha256: string;
  };
}
export interface TorCurlDiagnostic {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  readonly outcome: string;
}
export class TorCurlTransportFailure extends Error {
  constructor(kind: string);
  readonly diagnostic: Readonly<TorCurlDiagnostic>;
}
export function torCurlTransportDiagnostic(error: unknown): Readonly<TorCurlDiagnostic>;
export interface TorCurlTransport {
  /** HTTPS Fetch-shaped subset: streaming body, signal, manual redirects;
   * follows public GET/HEAD HTTPS acquisitions only. Never replays an upload.
   * Native standard roots for public endpoints; exact leaf anchor + pin gateway.
   */
  fetch(resource: string | URL | Request, init?: RequestInit): Promise<Response>;
  /** Idempotent local disposal, not proof of remote cancellation or billing. */
  close(): Promise<void>;
}
/** Uninstalled offline candidate. Native packaging requires parent approval.
 * Qualification injection uses the unmodified released package, never a stub.
 */
export function createTorCurlTransport(selection: TorCurlProxySelection, qualification?: {
  sdk: typeof import('node-libcurl');
}): TorCurlTransport;
