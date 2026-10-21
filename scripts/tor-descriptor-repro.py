#!/usr/bin/env python3
"""One explicitly selected scratch publisher; no public work without --run."""
import argparse
import contextlib
import json
import os
import pathlib
import re
import selectors
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

ENTRY = 'tor_runtime::enabled::tests::local_e2e::operator_fresh_descriptor_generation'
LIMIT = 40 * 1024
LABEL = 'possums.tor-descriptor-repro'
# Docker mount specification, not a shared host temporary file.
TMPFS = '/tmp:rw,nosuid,nodev,noexec,size=512m,mode=1777'  # noqa: S108 (container-private tmpfs)
STATES = 'not_observed shutdown bootstrapping degraded_reachable degraded_unreachable running recovering broken status_stream_ended status_stream_interrupted unknown'.split()
PROBLEMS = 'none_observed runtime descriptor_upload introduction_point descriptor_fatal descriptor_retry_exhausted descriptor_timeout descriptor_internal unknown descriptor_timeout_circuit descriptor_timeout_stream descriptor_timeout_request descriptor_timeout_internal descriptor_timeout_mixed descriptor_timeout_no_recorded_source descriptor_timeout_unknown'.split()
OUTCOMES = ['fatal_error', 'max_retry_count_exceeded', 'timeout', 'bug', 'unknown']
COVERAGE = ['retained_sources_only', 'empty_batch', 'no_retained_sources', 'no_nested_sources',
            'depth_limit', 'node_limit', 'unsupported_variant', 'unsupported_kind', 'unsupported_io_kind']
MEMBERS = ('mixed_timeout', 'descriptor_selected', 'circuit_timeout', 'circuit_protocol',
           'circuit_collapse', 'circuit_network_timeout', 'request_directory_timeout', 'request_network_timeout')
STAGES = ['selection', 'attempt', 'image', 'network', 'publisher', 'controller', 'initialization',
          'allowance', 'diagnostic', 'drain', 'child_cleanup', 'attach', 'cleanup']
CAUSES = ['malformed', 'bound', 'deadline', 'eof', 'command_failed', 'cause_unknown', 'interrupted',
          'cleanup_unknown', 'child_exit', 'consumed', 'ownership_mismatch', 'prerequisites']
SEAM_FAILURES = ['ARTI_BOOTSTRAP', 'ARTI_LAUNCH', 'DIAGNOSTIC_REQUEST', 'DIAGNOSTIC_RESPONSE',
                 'DRAIN_UNCONFIRMED', 'GATEWAY_BIND', 'GATEWAY_PANIC', 'GATEWAY_STOPPED',
                 'IPC_BOUND', 'IPC_COMMAND', 'IPC_FORMAT', 'IPC_READ', 'IPC_WRITE', 'KEY_GENERATION',
                 'LAUNCH_DEADLINE', 'PREPARE_REFUSED', 'RUNTIME', 'SCRATCH_INVALID', 'STAGE_DEADLINE']
ACTION = 'Known failure stages/codes do not establish root cause; cause_unknown means the underlying cause is unknown. Inspect prerequisites and exact owned resources; never replay or delete consumed markers.'


class Stop(Exception):
    def __init__(self, stage, constraint):
        self.stage, self.constraint = stage, constraint


def need(condition, stage, constraint='malformed'):
    if not condition:
        raise Stop(stage, constraint)


def keys(value, names, stage='diagnostic'):
    need(type(value) is dict and set(value) == set(names.split()), stage)


def decode(raw, stage):
    need(len(raw) < LIMIT, stage, 'bound')
    def unique(pairs):
        value = {}
        for key, item in pairs:
            need(key not in value, stage)
            value[key] = item
        return value
    try:
        return json.loads(raw, object_pairs_hook=unique,
                          parse_constant=lambda _: need(False, stage))
    except (ValueError, UnicodeError, RecursionError):
        raise Stop(stage, 'malformed') from None


def closed(values, catalog):
    need(type(values) is list and all(type(v) is str and v in catalog for v in values), 'diagnostic')
    need(values == sorted(set(values), key=catalog.index), 'diagnostic')
    return values


def project(value):
    keys(value, 'kind status body')
    need(value['kind'] == 'diagnostic' and type(value['status']) is int, 'diagnostic')
    body = value['body']
    keys(body, 'code stage constraint action lifecycle_serving ready tor_state tor_problem descriptor_evidence')
    # The Rust seam validates the full message/enum catalogs before IPC egress.
    # Do not duplicate its hundreds of labels or forward unprojected strings.
    need(all(type(body[k]) is str for k in ('code', 'stage', 'constraint', 'action')), 'diagnostic')
    need(type(body['ready']) is bool and type(body['lifecycle_serving']) is bool, 'diagnostic')
    need(not body['ready'] or body['lifecycle_serving'], 'diagnostic')
    need(value['status'] == (200 if body['ready'] else 503), 'diagnostic')
    need(body['tor_state'] in STATES and body['tor_problem'] in PROBLEMS, 'diagnostic')
    evidence = body['descriptor_evidence']
    keys(evidence, 'selection outcomes coverage circuit stream request unphased')
    need(evidence['selection'] in ('no_snapshot', 'no_problem_selected', 'other_problem_selected', 'descriptor_upload_selected'), 'diagnostic')
    for name in ('circuit', 'stream', 'request', 'unphased'):
        bucket = evidence[name]
        keys(bucket, 'variants kinds io_kinds http_statuses')
        for field in ('variants', 'kinds', 'io_kinds'):
            labels = bucket[field]
            need(type(labels) is list and len(labels) <= 128, 'diagnostic')
            need(all(type(v) is str and re.fullmatch('[a-z_]{1,64}', v) for v in labels), 'diagnostic')
            need(len(labels) == len(set(labels)), 'diagnostic')
        statuses = bucket['http_statuses']
        need(type(statuses) is list and all(type(v) is int and 100 <= v <= 599 for v in statuses), 'diagnostic')
        need(statuses == sorted(set(statuses)), 'diagnostic')
    cv, ck = evidence['circuit']['variants'], evidence['circuit']['kinds']
    rv, rk = evidence['request']['variants'], evidence['request']['kinds']
    members = dict(zip(MEMBERS, (body['tor_problem'] == 'descriptor_timeout_mixed',
        evidence['selection'] == 'descriptor_upload_selected', 'circuit_timeout' in cv,
        'protocol' in cv, 'circuit_collapse' in ck, 'tor_network_timeout' in ck,
        'directory_timeout' in rv, 'tor_network_timeout' in rk), strict=True))
    return {**{k: body[k] for k in ('tor_state', 'tor_problem', 'lifecycle_serving', 'ready')},
            'members': members, 'outcomes': closed(evidence['outcomes'], OUTCOMES),
            'coverage': closed(evidence['coverage'], COVERAGE)}


def validate_projection(value):
    keys(value, 'tor_state tor_problem lifecycle_serving ready members outcomes coverage')
    need(value['tor_state'] in STATES and value['tor_problem'] in PROBLEMS, 'diagnostic')
    need(type(value['ready']) is bool and type(value['lifecycle_serving']) is bool, 'diagnostic')
    need(not value['ready'] or value['lifecycle_serving'], 'diagnostic')
    keys(value['members'], ' '.join(MEMBERS))
    need(all(type(v) is bool for v in value['members'].values()), 'diagnostic')
    need(value['members']['mixed_timeout'] == (value['tor_problem'] == 'descriptor_timeout_mixed'), 'diagnostic')
    closed(value['outcomes'], OUTCOMES)
    closed(value['coverage'], COVERAGE)


def emit(value):
    print(json.dumps(value, sort_keys=True), flush=True)


def line(stream, deadline, stage):
    raw = bytearray()
    with selectors.DefaultSelector() as poll:
        poll.register(stream, selectors.EVENT_READ)
        while True:
            remaining = deadline - time.monotonic()
            need(remaining > 0 and bool(poll.select(max(0, remaining))), stage, 'deadline')
            byte = os.read(stream.fileno(), 1)
            need(bool(byte), stage, 'eof')
            if byte == b'\n':
                return decode(raw, stage)
            raw += byte
            need(len(raw) < LIMIT, stage, 'bound')


def gateway_line(conn, deadline, stage, terminal=False):
    value = line(conn, deadline, stage)
    # The seam may deliver drain evidence before its primary failure terminal.
    if type(value) is dict and value.get('kind') == 'finished' and stage != 'drain':
        keys(value, 'kind cleanup zeroUpstream bridgeUntouched', stage)
        need(all(type(value[k]) is bool for k in ('cleanup', 'zeroUpstream', 'bridgeUntouched')), stage)
        value = line(conn, deadline, stage)
        need(type(value) is dict and value.get('kind') == 'terminal', stage)
    if type(value) is dict and value.get('kind') == 'terminal':
        keys(value, 'kind code', stage)
        need(type(value['code']) is str, stage)
        if value['code'] in SEAM_FAILURES:
            raise Stop(stage, value['code'])
        need(terminal and value['code'] == 'GATEWAY_CLOSED', stage)
    return value


def command(args, deadline, stage, capture=True, timeout=2.0, accepted=(0,)):
    remaining = min(timeout, deadline - time.monotonic())
    need(remaining > 0, stage, 'deadline')
    try:
        result = subprocess.run(args, stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, timeout=remaining)
    except subprocess.TimeoutExpired:
        raise Stop(stage, 'deadline') from None
    except OSError:
        raise Stop(stage, 'cause_unknown') from None
    need(result.returncode in accepted, stage, 'command_failed')
    need(not capture or len(result.stdout) < LIMIT, stage, 'bound')
    return result


def tc_commands():
    base = ['tc']
    return [base + ['qdisc', 'add', 'dev', 'eth0', 'root', 'handle', '1:', 'prio', 'bands', '3', 'priomap'] + ['0'] * 16,
        base + ['qdisc', 'add', 'dev', 'eth0', 'parent', '1:3', 'handle', '30:', 'netem', 'delay', '12000ms'],
        base + ['filter', 'add', 'dev', 'eth0', 'parent', '1:', 'protocol', 'ip', 'priority', '1', 'u32',
                'match', 'ip', 'protocol', '6', '0xff', 'match', 'u16', '0x0000', '0xfc00', 'at', '2', 'flowid', '1:1'],
        base + ['filter', 'add', 'dev', 'eth0', 'parent', '1:', 'protocol', 'ip', 'priority', '2', 'u32',
                'match', 'ip', 'protocol', '6', '0xff', 'flowid', '1:3']]


def reap(proc, root):
    clean = True
    if proc is not None:
        try:
            proc.wait(timeout=12)
            clean = proc.returncode == 0
        except subprocess.TimeoutExpired:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait(timeout=3)
            clean = False
        try:
            os.killpg(proc.pid, 0)
            os.killpg(proc.pid, signal.SIGKILL)
            clean = False
        except ProcessLookupError:
            pass
    try:
        shutil.rmtree(root)
    except OSError:
        return False
    return clean and not root.exists()


def publisher():
    deadline = time.monotonic() + 815  # 15 seconds reserved for owned child cleanup.
    root = pathlib.Path(tempfile.mkdtemp(prefix='publisher-', dir='/tmp'))
    proc = None
    snapshot = failure = None
    stage = 'publisher'
    try:
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(root / 'gateway.sock'))
            os.chmod(root / 'gateway.sock', 0o600)
            listener.listen(1)
            listener.settimeout(5)
            env = {'HOME': str(root), 'TMPDIR': str(root), 'OTEL_SDK_DISABLED': 'true',
                   'POSSUMS_LOCAL_TOR_E2E': 'ONE_ISOLATED_LOCAL_ONION_RUN'}
            proc = subprocess.Popen([os.environ['POSSUMS_REPRO_BINARY'], ENTRY, '--ignored', '--exact', '--test-threads=1'],
                cwd=root, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            conn, _ = listener.accept()
        with conn:
            stage = 'initialization'
            for milestone in ('prepared', 'bootstrapped', 'ready'):
                need(gateway_line(conn, min(deadline, time.monotonic() + 195), stage) == {'kind': milestone}, stage)
                emit({'event': milestone})
            ready = time.monotonic()
            stage = 'allowance'
            need(ready + 680 <= deadline, stage, 'deadline')
            need(line(sys.stdin, min(deadline, ready + 12), 'controller') == {'control': True}, 'controller')
            # No Running latch, status polling, extra D, reset or delayed second phase.
            time.sleep(max(0, ready + 660 - time.monotonic()))
            stage = 'diagnostic'
            conn.sendall(b'D')
            snapshot = project(gateway_line(conn, min(deadline, time.monotonic() + 5), stage))
            stage = 'drain'
            conn.sendall(b'Q')
            finished = gateway_line(conn, min(deadline, time.monotonic() + 15), stage)
            keys(finished, 'kind cleanup zeroUpstream bridgeUntouched', stage)
            need(finished['kind'] == 'finished' and all(type(finished[k]) is bool
                 for k in ('cleanup', 'zeroUpstream', 'bridgeUntouched')), stage)
            need(gateway_line(conn, min(deadline, time.monotonic() + 2), stage, terminal=True)
                 == {'kind': 'terminal', 'code': 'GATEWAY_CLOSED'}, stage)
            need(finished['cleanup'] and finished['zeroUpstream'] and finished['bridgeUntouched'], stage)
    except Stop as error:
        stage, failure = error.stage, error.constraint
    except KeyboardInterrupt:
        failure = 'interrupted'
    except Exception:
        failure = 'cause_unknown'
    try:
        clean = reap(proc, root)
    except (Exception, KeyboardInterrupt):
        clean = False
    if not clean and failure is None:
        stage, failure = 'child_cleanup', 'cleanup_unknown'
    emit({'event': 'result', 'stage': stage, 'failure': failure, 'snapshot': snapshot, 'child_cleanup': clean})
    return 2 if failure else 0


def resource_args(name, token, image, network, controller=False):
    return ['create', '--name', name, '--label', LABEL + '=' + token, '--network', network,
        '--user', '0:0' if controller else '65532:65532', '--cpus', '2', '--read-only',
        '--tmpfs', TMPFS, '--pids-limit', '256',
        '--security-opt', 'no-new-privileges', '--cap-drop', 'ALL', *(['--cap-add', 'NET_ADMIN'] if controller else []),
        '--ulimit', 'core=0', '--log-driver', 'none', '--interactive', '--pull=never', image,
        '--internal-controller' if controller else '--internal-publisher']


class Docker:
    def __init__(self, token, started):
        self.token = token
        self.work = started + 850
        self.end = self.work + 45
        self.resources = []

    def call(self, args, stage, timeout=2, cleanup=False):
        return command(['docker', *args], self.end if cleanup else self.work, stage, timeout=timeout).stdout

    def listing(self, kind, reference, name=False):
        field = 'Names' if kind == 'container' else 'Name'
        template = '[{{json .ID}},{{json .' + field + '}},{{json (.Label "' + LABEL + '")}}]'
        selector = 'name=^' + ('/' if kind == 'container' else '') + reference + '$' if name else 'id=' + reference
        args = [kind, 'ls', *(['--all'] if kind == 'container' else []), '--no-trunc', '--filter', selector, '--format', template]
        rows = self.call(args, 'cleanup', cleanup=True).splitlines()
        need(len(rows) <= 1, 'cleanup')
        if not rows:
            return None
        row = decode(rows[0], 'cleanup')
        need(type(row) is list and len(row) == 3 and type(row[0]) is str
             and re.fullmatch('[a-f0-9]{64}', row[0]) and row[2] == self.token, 'cleanup', 'ownership_mismatch')
        need(row[1] == reference if name else row[0] == reference, 'cleanup', 'ownership_mismatch')
        return row[0]

    def create(self, kind, name, args):
        need(self.listing(kind, name, True) is None, kind if kind == 'network' else 'publisher', 'ownership_mismatch')
        resource = [kind, name, None]
        self.resources.append(resource)  # Reconcile lost create replies, never repeat creation.
        raw = self.call(args, 'network' if kind == 'network' else 'publisher', timeout=8)
        need(bool(re.fullmatch(rb'[a-f0-9]{64}\n', raw)), 'publisher')
        resource[2] = raw.decode().strip()
        need(self.listing(kind, name, True) == resource[2], 'publisher', 'ownership_mismatch')
        return resource[2]

    def cleanup(self):
        clean = True
        for kind, name, identifier in reversed(self.resources):
            try:
                identifier = identifier or self.listing(kind, name, True)
                if identifier is not None and self.listing(kind, identifier) is not None:
                    need(self.listing(kind, name, True) == identifier, 'cleanup', 'ownership_mismatch')
                    # Only a successful validated listing establishes absence.
                    with contextlib.suppress(Stop):
                        self.call(['rm', '--force', identifier] if kind == 'container' else ['network', 'rm', identifier], 'cleanup', cleanup=True)
                need(self.listing(kind, name, True) is None, 'cleanup', 'cleanup_unknown')
                if identifier is not None:
                    need(self.listing(kind, identifier) is None, 'cleanup', 'cleanup_unknown')
            except (Exception, KeyboardInterrupt):
                clean = False
        return clean


def owner_result(value):
    keys(value, 'event stage failure snapshot child_cleanup', 'attach')
    need(value['event'] == 'result' and value['stage'] in STAGES
         and (value['failure'] is None or value['failure'] in CAUSES + SEAM_FAILURES)
         and type(value['child_cleanup']) is bool, 'attach')
    if value['snapshot'] is not None:
        validate_projection(value['snapshot'])
    need(value['failure'] is not None or (value['snapshot'] is not None
         and value['child_cleanup'] and value['stage'] == 'drain'), 'attach')
    return value


def attached(docker, gateway, controller):
    proc = subprocess.Popen(['docker', 'start', '--attach', '--interactive', gateway], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, start_new_session=True)
    if proc.stdin is None or proc.stdout is None:
        raise Stop('attach', 'prerequisites')
    validated = None
    try:
        for milestone in ('prepared', 'bootstrapped', 'ready'):
            value = line(proc.stdout, docker.work, 'attach')
            if type(value) is dict and value.get('event') == 'result':
                validated = owner_result(value)
                need(validated['failure'] is not None, 'attach')
                return validated
            need(value == {'event': milestone}, 'attach')
            emit({'milestone': milestone})
        control = command(['docker', 'start', '--attach', controller], docker.work,
                          'controller', timeout=9, accepted=(0, 2))
        reply = decode(control.stdout, 'controller')
        if type(reply) is dict and reply.get('code') == 'TOR_REPRO_STOPPED':
            keys(reply, 'code stage constraint action', 'controller')
            need(reply['stage'] in ('controller', 'selection') and reply['constraint'] in CAUSES
                 and reply['action'] == ACTION, 'controller')
            raise Stop(reply['stage'], reply['constraint'])
        need(control.returncode == 0 and reply == {'control': True}, 'controller')
        proc.stdin.write(b'{"control":true}\n')
        proc.stdin.flush()
        validated = owner_result(line(proc.stdout, docker.work, 'attach'))
        if validated['failure'] is not None:
            return validated
        with selectors.DefaultSelector() as poll:
            poll.register(proc.stdout, selectors.EVENT_READ)
            need(bool(poll.select(max(0, docker.work - time.monotonic()))), 'attach', 'deadline')
            need(os.read(proc.stdout.fileno(), 1) == b'', 'attach')
        proc.wait(timeout=max(0.01, docker.work - time.monotonic()))
        need(proc.returncode == 0, 'attach', 'child_exit')
        return validated
    except (Stop, OSError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        if validated is None:
            raise
        return {**validated, 'stage': 'attach', 'failure': error.constraint if isinstance(error, Stop) else 'cause_unknown'}
    finally:
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=2)
            except (OSError, subprocess.SubprocessError):
                pass  # Docker-owned resources are independently reconciled below.
        proc.stdin.close()
        proc.stdout.close()


def host(args):
    started = time.monotonic()
    need(args.run and args.attempt_dir and re.fullmatch('sha256:[a-f0-9]{64}', args.image or ''), 'selection', 'prerequisites')
    attempt = os.open(args.attempt_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    docker = result = None
    failure = None
    stage = 'attempt'
    active = False
    try:
        need(not os.listdir(attempt), stage, 'consumed')
        # Permanent exclusive marker precedes ALL Docker work, including image inspection.
        import secrets
        token = secrets.token_hex(8)
        for marker in ('CONSUMED', 'ACTIVE'):
            fd = os.open(marker, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=attempt)
            with os.fdopen(fd, 'w') as stream:
                stream.write(token + '\n')
                stream.flush()
                os.fsync(stream.fileno())
            active = marker == 'ACTIVE'
        os.fsync(attempt)
        docker = Docker(token, started)
        stage = 'image'
        need(docker.call(['image', 'inspect', args.image, '--format', '{{.Id}} {{.Os}} {{.Architecture}} {{not .Config.Volumes}} {{not .Config.ExposedPorts}}'], stage).strip()
             == (args.image + ' linux arm64 true true').encode(), stage, 'prerequisites')
        name = 'possums-repro-' + token
        network = docker.create('network', name, ['network', 'create', '--driver', 'bridge', '--label', LABEL + '=' + token, name])
        gateway = docker.create('container', name, resource_args(name, token, args.image, network))
        controller = docker.create('container', name + '-control', resource_args(name + '-control', token, args.image, 'container:' + gateway, True))
        stage = 'attach'
        result = attached(docker, gateway, controller)
        stage, failure = result['stage'], result['failure']
    except Stop as error:
        stage, failure = error.stage, error.constraint
    except FileExistsError:
        failure = 'consumed'
    except KeyboardInterrupt:
        failure = 'interrupted'
    except Exception:
        failure = 'cause_unknown'
    finally:
        clean = docker.cleanup() if docker else True
        if active and clean:
            try:
                os.unlink('ACTIVE', dir_fd=attempt)
                os.fsync(attempt)
            except (OSError, KeyboardInterrupt):
                clean = False
        os.close(attempt)
    if not clean and failure is None:
        stage, failure = 'cleanup', 'cleanup_unknown'
    snapshot = result['snapshot'] if result else None
    verdict = 'stopped'
    if not failure:
        verdict = 'match' if snapshot is not None and all(snapshot['members'].values()) else 'nonmatch'
    emit({'code': 'TOR_REPRO_STOPPED' if failure else 'TOR_REPRO_COMPLETE', 'stage': stage,
          'constraint': failure or 'none', 'action': ACTION, 'cleanup_confirmed': clean,
          'snapshot': snapshot, 'verdict': verdict})
    return 2 if failure else 0


def main():
    def interrupt(_signal, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupt)
    if sys.argv[1:] in (['--internal-publisher'], ['--internal-controller']):
        import pwd
        import resource
        control = sys.argv[1:] == ['--internal-controller']
        need(sys.platform == 'linux' and os.getuid() == (0 if control else 65532)
             and os.getgid() == (0 if control else 65532) and pwd.getpwuid(65532).pw_dir == '/tmp'  # noqa: S108 (NSS home)
             and os.statvfs('/').f_flag & os.ST_RDONLY
             and resource.getrlimit(resource.RLIMIT_CORE) == (0, 0), 'selection', 'prerequisites')
        if not control:
            return publisher()
        for args in tc_commands():
            command(args, time.monotonic() + 1.5, 'controller', capture=False, timeout=1.5)
        emit({'control': True})
        return 0
    class Parser(argparse.ArgumentParser):
        def error(self, message):
            raise Stop('selection', 'prerequisites')
    parser = Parser(description=__doc__)
    parser.add_argument('--run', action='store_true')
    parser.add_argument('--attempt-dir')
    parser.add_argument('--image')
    return host(parser.parse_args())


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (Exception, KeyboardInterrupt) as error:
        stage = {'--internal-controller': 'controller', '--internal-publisher': 'publisher'}.get(
            sys.argv[1] if len(sys.argv) == 2 else '', 'selection')
        emit({'code': 'TOR_REPRO_STOPPED', 'stage': error.stage if isinstance(error, Stop) else stage,
              'constraint': error.constraint if isinstance(error, Stop) else
              ('interrupted' if isinstance(error, KeyboardInterrupt) else 'cause_unknown'), 'action': ACTION})
        raise SystemExit(2) from None
