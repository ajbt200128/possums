import type { Socket } from 'node:net';

export interface TorSocksConfiguration {
  host: string;
  port: number;
  username?: string;
  password?: string;
  timeoutMs?: number;
}
export interface TorSocksDestination { host: string; port: number }
export interface TorSocksDiagnostic {
  readonly code: string;
  readonly stage: string;
  readonly constraint: string;
  readonly action: string;
  readonly outcome: string;
}
export class TorSocksTransportFailure extends Error {
  constructor(kind: string);
  readonly diagnostic: TorSocksDiagnostic;
}
export function torSocksTransportDiagnostic(error: unknown): TorSocksDiagnostic;
/** Unqualified, unwired research candidate. Caller owns TLS/pin checks before HTTP. */
export function createTorSocksTransport(
  input: TorSocksConfiguration,
  qualification?: { sdk: { SocksClient: {
    createConnection(options: {
      command: 'connect';
      proxy: { host: string; port: number; type: 5; userId?: string; password?: string };
      destination: TorSocksDestination;
      existing_socket: Socket;
      timeout: number;
    }): Promise<{ socket: Socket }>;
  } } },
): {
  connectSocket(destination: TorSocksDestination, signal?: AbortSignal): Promise<Socket>;
  close(): Promise<void>;
};
