import { isIP } from 'node:net';
import { parseTorOnionAddress } from './tor-onion.mjs';
import { ConnectionFailure } from './diagnostics.js';

export type RouteInputs = Readonly<{ onion: unknown; proxy: unknown }>;
export type RouteSelection = Readonly<{ route: 'clearnet' }> |
  Readonly<{ route: 'tor'; onion: string; host: string; port: number }>;

// Pure local selection: no environment, helper, sockets or public acquisition.
export function parseTorSelection(input: RouteInputs): RouteSelection {
  if (input.onion === undefined) {
    if (input.proxy !== undefined) throw new ConnectionFailure('tor_selection_invalid');
    return Object.freeze({ route: 'clearnet' });
  }
  try {
    const onion = parseTorOnionAddress(input.onion).host;
    const authority = input.proxy === undefined ? '127.0.0.1:19050' : input.proxy;
    if (typeof authority !== 'string' || /[\s/@?#\\]/.test(authority)) throw new Error();
    const url = new URL('socks5h://' + authority);
    if (url.username || url.password || url.search || url.hash || url.pathname || !url.port) throw new Error();
    const host = url.hostname.startsWith('[') ? url.hostname.slice(1, -1) : url.hostname;
    const port = Number(url.port);
    if (!isIP(host) || !(host === '::1' || host.startsWith('127.')) || !Number.isInteger(port) || port < 1 || port > 65535) throw new Error();
    return Object.freeze({ route: 'tor', onion, host, port });
  } catch { throw new ConnectionFailure('tor_selection_invalid'); }
}

// Pi applies flags after the factory. Snapshot ALL authority inputs at first
// trust, including rejected selections; a later change never chooses a route.
export function createTorSelection(read: () => RouteInputs): () => RouteSelection {
  let locked: RouteInputs | undefined, selection: RouteSelection | undefined, failure: ConnectionFailure | undefined;
  return () => {
    const current = read();
    if (locked && (current.onion !== locked.onion || current.proxy !== locked.proxy)) {
      throw new ConnectionFailure('route_selection_changed');
    }
    if (!locked) {
      locked = Object.freeze({ ...current });
      try { selection = parseTorSelection(locked); }
      catch { failure = new ConnectionFailure('tor_selection_invalid'); }
    }
    if (failure) throw failure;
    return selection!;
  };
}
