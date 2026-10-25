#!/usr/bin/env python3
"""Offline source-review fixture; intake source and module cache are read-only inputs."""
import hashlib
import io
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
ORIGINAL = Path('/Users/mb5/projects/possums-intake')
CANDIDATE = Path('/Users/mb5/.t3/worktrees/possums-intake/t3-a8787a0b')
SOURCE_COMMIT = '1946aa43f21f3ccd6ceaee7819aa53a5f8221f6f'
WORK = ROOT / 'target/intake-review'
INTAKE = WORK / ('source-' + SOURCE_COMMIT[:7])
GO = ORIGINAL / '.devenv/profile/bin/go'
MODULE_CACHE = ORIGINAL / '.devenv/state/go/pkg/mod'


def materialize_source():
    # Exact reviewed committed source, never worktree files or stale root main.
    # Capture git output privately; only the fixed pin is reported.
    subprocess.run(readonly_command(['git', 'merge-base', '--is-ancestor', SOURCE_COMMIT, 'HEAD']),
                   cwd=CANDIDATE, check=True, capture_output=True)
    archive = subprocess.run(readonly_command(['git', 'archive', '--format=tar', SOURCE_COMMIT]),
                             cwd=CANDIDATE, check=True, capture_output=True).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        members = tar.getmembers()
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or '..' in path.parts or not (member.isdir() or member.isfile()):
                raise OSError('fixture source archive constraint')
        contents = {}
        for member in members:
            if member.isfile():
                stream = tar.extractfile(member)
                if stream is None:
                    raise OSError('fixture source file unavailable')
                with stream:
                    contents[member.name] = stream.read()
        expected = {name: hashlib.sha256(data).digest() for name, data in contents.items()}
        if not INTAKE.exists():
            # Only remove our own newly allocated staging directory on failure.
            # Never clear target/intake-review or an existing source directory.
            staging = Path(tempfile.mkdtemp(prefix='source-staging-', dir=WORK))
            try:
                for member in members:
                    destination = staging / member.name
                    if member.isdir():
                        destination.mkdir(parents=True, exist_ok=True)
                    else:
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        destination.write_bytes(contents[member.name])
                        destination.chmod(member.mode & 0o777)
                staging.rename(INTAKE)
            finally:
                if staging.exists():
                    shutil.rmtree(staging)
    if INTAKE.is_symlink():
        raise OSError('fixture source root symlink constraint')
    actual = {}
    for path in INTAKE.rglob('*'):
        if path.is_symlink():
            raise OSError('fixture source symlink constraint')
        if path.is_file():
            actual[path.relative_to(INTAKE).as_posix()] = hashlib.sha256(path.read_bytes()).digest()
    if actual != expected:
        raise OSError('fixture source content mismatch; existing scratch preserved')


def prepare():
    if shutil.disk_usage(ROOT).free < 6 * 1024**3:
        raise OSError('fixture disk headroom unavailable')
    for name in ('cache', 'tmp', 'home', 'gopath'):
        (WORK / name).mkdir(parents=True, exist_ok=True)
    materialize_source()
    overlay = WORK / 'overlay.json'
    overlay.write_text(json.dumps({'Replace': {
        str(INTAKE / 'internal/siteauth/gateway_review_test.go'):
        str(ROOT / 'tests/intake_review/native_auth_test.go'),
        str(INTAKE / 'internal/siteauth/gateway_transport_test.go'):
        str(ROOT / 'tests/intake_review/transport_test.go'),
    }}))
    env = {
        'PATH': str(GO.parent) + ':/usr/bin:/bin',
        'HOME': str(WORK / 'home'), 'TMPDIR': str(WORK / 'tmp'),
        'GOCACHE': str(WORK / 'cache'), 'GOPATH': str(WORK / 'gopath'),
        'GOMODCACHE': str(MODULE_CACHE),
        'GOTOOLCHAIN': 'local', 'GOPROXY': 'off', 'GOSUMDB': 'off',
        'GOTELEMETRY': 'off', 'OTEL_SDK_DISABLED': 'true',
        'CC': '/usr/bin/clang', 'CGO_ENABLED': '1',
    }
    return overlay, env


def readonly_command(command):
    # -mod=readonly does not protect GOMODCACHE. Enforce the sibling boundary,
    # including cache locks/extraction and any accidental fixture-relative writes.
    protected = [ORIGINAL, CANDIDATE, Path('/Users/mb5/projects/possums'),
                 MODULE_CACHE.resolve(), INTAKE]
    policy = '(version 1)(allow default)' + ''.join(
        '(deny file-write* (subpath ' + json.dumps(str(path)) + '))'
        for path in protected
    )
    return ['/usr/bin/sandbox-exec', '-p', policy, *command]


def passed_tests(output, expected):
    # go test exits zero even when a regex selects nothing. Require an actual
    # terminal pass event for every named top-level test, never a skip/no-match.
    passed = set()
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except (ValueError, TypeError):
            continue
        if event.get('Action') == 'pass' and event.get('Test') in expected:
            passed.add(event['Test'])
    return passed == set(expected)


def main():
    overlay, env = prepare()
    print('intake_source_pin: ' + SOURCE_COMMIT, flush=True)
    cases = [
        ('native_auth_contract', './internal/siteauth', [
            'GatewayReviewNativeAccountAuthority', 'GatewayReviewRenewalHTTP',
            'NativeCustomerKeyPreFormQualification', 'NativeCustomerKeyUpdateAndSoleRecordIdentity',
            'CustomerTokenLifetimeNativeUpdate', 'CustomerInitializerPreservesStoredDuration',
            'CustomerStoredDurationRestart',
        ]),
        ('private_auth_authority', './internal/siteauth', [
            'CustomerAdapterCurrentBindingAndSDKAuthority',
        ]),
        ('ledger_contract', './internal/ledger', [
            'MoneyAndKeyBoundaries', 'FullUint64OverflowAndRollback',
            'IdempotencyOriginalResultsAndInputConflict', 'LastCreditAndDuplicateRaces',
            'TerminalRacesDisabledAndExactExpiry', 'SnapshotCompleteSortedScopedAndExpiry',
            'KeyPruningRetentionRollbackClockAndRestart', 'SnapshotRacingSettlementNeverReturnsMixedState',
            'RenewalCompareExtendAndRefusals', 'RenewalDisabledIncludingDuplicateCommitsOtherExpiry',
            'RenewalConcurrentDuplicateAndTerminalRaces', 'RenewalLostCompletionAndLostResponse',
            'RenewalSettlementAfterInitialDeadlineAndClassification',
            'RenewalBeyondKeyWindowRestartExpiryAndPruning',
        ]),
        ('http_contract', './internal/httpapi', [
            'ProvisionCreditBalanceAndAccountScoping', 'ReserveLookupSettleRefundAndPendingRows',
            'ExpiredSettlementPreservesAuthoritativeRow', 'MoneyAndTimeoutWireValidation',
            'DeliveryLossAndUnknownCommitNeverInventReceipt', 'SingletonHeadersOnActualHTTP',
            'RenewalHTTPConditionalCurrentSnapshotAndTerminals',
            'RenewalHTTPExactExpiryCommitsAndCannotCollectLate', 'RenewalHTTPStrictTimestampAndPrivacy',
            'RenewalHTTPUnknownDeliveryNeverRetriesOrExports', 'RenewalHTTPNoProgressCarriesCurrentRow',
            'RenewalHTTPExtraFieldsIgnored',
        ]),
    ]
    # Never persist or forward arbitrary compiler/SDK/test output. Named cases
    # contain fixed synthetic assertions; this runner reports only stage/result.
    for label, package, names in cases:
        expected = ['Test' + name for name in names]
        pattern = '^(' + '|'.join(expected) + ')$'
        result = subprocess.run(
            readonly_command([str(GO), 'test', '-json', '-race', '-tags', 'elpscheck', '-mod=readonly',
                              '-overlay=' + str(overlay), '-p', '1', package, '-run', pattern,
                              '-count=1', '-timeout=180s']), cwd=INTAKE, env=env,
            capture_output=True, timeout=600,
        )
        accepted = result.returncode == 0 and passed_tests(result.stdout, expected)
        print(label + ': ' + ('pass' if accepted else 'unavailable'), flush=True)
        if not accepted:
            if result.returncode == 0:
                print('INTAKE_REVIEW_TEST_SELECTION_INCOMPLETE')
            # Only locally authored test markers may leave this process.
            source = '\n'.join((ROOT / 'tests/intake_review' / name).read_text()
                               for name in ('native_auth_test.go', 'transport_test.go'))
            markers = {part.split('"', 1)[0] for part in source.split('t.Fatal("')[1:]}
            for line in result.stdout.splitlines():
                try:
                    event = json.loads(line)
                except (ValueError, TypeError):
                    continue
                output = event.get('Output', '')
                for status in range(100, 600):
                    if f'REVIEW_OBSERVED_HTTP_STATUS_{status}\n' in output:
                        print(f'observed_http_status: {status}')
                for marker in sorted(markers):
                    if marker in output:
                        print(marker)
                if event.get('Action') == 'build-fail':
                    print('INTAKE_REVIEW_BUILD_UNAVAILABLE')
            return 1
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, subprocess.SubprocessError):
        print('INTAKE_REVIEW_RUNNER_UNAVAILABLE: cause unknown; no acceptance established')
        sys.exit(1)
