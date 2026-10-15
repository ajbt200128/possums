import { TorFailure } from './tor-client.mjs';

/**
 * Real external-listener adapter: no control credential/file access, process
 * launch, configuration change or stop. Readiness is established by the normal
 * strict-auth SOCKS connection AND backend public verification, not by a TCP
 * port check or a fabricated bootstrap snapshot. The user's Tor assumption is
 * explicit; a SOCKS peer cannot prove it runs Tor or honors IsolateSOCKSAuth.
 */
export function createExistingSocksRuntime() {
  return Object.freeze({
    async waitReadiness(config, signal) {
      if (signal.aborted) throw new TorFailure('cancelled', 'readiness');
      if (config.mode !== 'existing' || config.readiness !== 'connection') throw new TorFailure('config');
      return Object.freeze({ readiness: 'connection' });
    },
  });
}
