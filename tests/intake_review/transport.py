#!/usr/bin/env python3
"""Build the pinned gateway test binary offline; Go owns its disposable TLS service."""
import json
import re
import subprocess
import sys
from pathlib import Path

from run import (
    GO,
    INTAKE,
    ROOT,
    SOURCE_COMMIT,
    WORK,
    passed_tests,
    prepare,
    readonly_command,
)

RUST = Path('/Users/mb5/projects/possums/.devenv/profile/bin')


def main():
    overlay, env = prepare()
    print('intake_source_pin: ' + SOURCE_COMMIT, flush=True)
    rust_env = {
        'PATH': str(RUST) + ':/usr/bin:/bin', 'HOME': str(WORK / 'home'),
        'CARGO_HOME': str(Path.home() / '.cargo'), 'CARGO_NET_OFFLINE': 'true',
        'CARGO_TARGET_DIR': str(Path.home() / '.cache/possums/target/aarch64-apple-darwin/1.92.0'),
        'OTEL_SDK_DISABLED': 'true',
    }
    build = subprocess.run(
        readonly_command([str(RUST / 'cargo'), 'test', '--locked', '--offline', '--lib', '--no-run',
                          '--message-format=json']), cwd=ROOT, env=rust_env,
        capture_output=True, timeout=600,
    )
    if build.returncode:
        print('INTAKE_TRANSPORT_BUILD_UNAVAILABLE: pinned offline build did not complete')
        return 1
    binaries = []
    for line in build.stdout.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if (item.get('reason') == 'compiler-artifact' and
                item.get('target', {}).get('name') == 'possums' and
                item.get('target', {}).get('kind') == ['lib'] and item.get('executable')):
            binaries.append(item['executable'])
    if len(binaries) != 1:
        print('INTAKE_TRANSPORT_BINARY_UNAVAILABLE: no unique test artifact established')
        return 1
    env['GATEWAY_TRANSPORT_TEST_BINARY'] = binaries[0]
    result = subprocess.run(
        readonly_command([str(GO), 'test', '-json', '-race', '-tags', 'elpscheck', '-mod=readonly',
                          '-overlay=' + str(overlay), '-p', '1', './internal/siteauth',
                          '-run', '^TestGatewayTransport(ExistingHTTP|RenewalHTTP)$', '-count=1', '-timeout=90s']),
        cwd=INTAKE, env=env, capture_output=True, timeout=180,
    )
    expected = ['TestGatewayTransportExistingHTTP', 'TestGatewayTransportRenewalHTTP']
    if result.returncode or not passed_tests(result.stdout, expected):
        print('INTAKE_TRANSPORT_ACCEPTANCE_UNAVAILABLE: disposable HTTP fixture did not pass')
        if result.returncode == 0:
            print('INTAKE_TRANSPORT_TEST_SELECTION_INCOMPLETE')
        source = (ROOT / 'tests/intake_review/transport_test.go').read_text()
        markers = {part.split('"', 1)[0] for part in source.split('t.Fatal("')[1:]}
        for line in result.stdout.splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            location = re.search(r'TRANSPORT_ASSERTION_(?:tests|gateway_tests|renewal_gateway_tests)_LINE_[0-9]+|TRANSPORT_CONSTRAINT_(?:Configuration|Input|EncodingLimit|EncodingUnknown|ClientUnknown|RequestUnknown|ConnectUnknown|TransportUnknown|Deadline|ResponseEncoding|ResponseLimit|ResponseReadUnknown|ResponseJson)', event.get('Output', ''))
            if location:
                print(location.group())
            for marker in sorted(markers):
                if marker in event.get('Output', ''):
                    print(marker)
        return 1
    print('actual_go_pocketbase_rust_https_transport: pass')
    print('actual_go_pocketbase_rust_https_renewal: pass')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, subprocess.SubprocessError):
        print('INTAKE_TRANSPORT_RUNNER_UNAVAILABLE: cause unknown; no acceptance established')
        sys.exit(1)
