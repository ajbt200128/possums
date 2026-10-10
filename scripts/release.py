#!/usr/bin/env python3
"""Paid release orchestration. Only public pins leave this module's boundary.

No persisted coordinator state: a deterministic commit and immutable paid tag
are the recovery record. An incomplete release is never overwritten/replayed.
"""
import base64
import hashlib
import io
import json
import os
import re
import selectors
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

REPO = "ajbt200128/possums"
IMAGE = f"ghcr.io/{REPO}-gateway"
PREDICATE = "https://tinfoil.sh/predicate/snp-tdx-multiplatform/v1"
CONFIG = "tinfoil-config.yml"
SHA = r"[0-9a-f]{40}"
DIGEST = r"sha256:[0-9a-f]{64}"
VERSION = r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
# Closed diagnostics: never interpolate exceptions, values or upstream output.
CODES = {
    "arguments": ("input", "Check the documented public pin arguments."),
    "command": ("transport", "Inspect permissions and availability privately; do not blindly retry publication."),
    "disabled": ("activation", "Obtain separate authorization for repository setup."),
    "event": ("source", "Use successful check.yml on an exact main push or current-main manual dispatch."),
    "checks": ("source", "Complete successful checks on the exact eligible commit/ref."),
    "stale": ("eligibility", "Use the newest main candidate; no update was requested by this check."),
    "transform": ("configuration", "Review the one-line gateway digest template and release-only tree."),
    "identity": ("build", "Resolve the source/release image derivation mismatch."),
    "collision": ("publication", "Manually reconcile existing immutable tags/releases; never delete or repoint them."),
    "partial": ("publication", "Inspect the existing publication privately; never replace assets or automatically replay it."),
    "timeout": ("publication", "Inspect the exact pending invocation; do not dispatch a duplicate."),
    "manifest": ("public-provenance", "Reconcile the exact manifest, config and hash assets before proceeding."),
    "signature": ("public-provenance", "Resolve the cryptographic or tagged signer/predicate mismatch."),
    "invocation": ("public-provenance", "Require the successful actual signed publication invocation."),
    "internal": ("validation", "Review the local verifier privately; raw diagnostics were suppressed."),
}


class Stop(Exception):
    def __init__(self, code, status=None):
        self.code = code
        self.status = status if type(status) is int and 100 <= status <= 599 else None


def require(condition, code):
    if not condition:
        raise Stop(code)


def diagnostic(code, status=None):
    code = code if code in CODES else "internal"
    stage, action = CODES[code]
    result = {"code": code if code.startswith("SERVING_") else f"RELEASE_{code.upper()}",
              "passed": False, "stage": stage, "constraint": code, "action": action}
    if type(status) is int and 100 <= status <= 599:
        result["status"] = status
    return result


def capture(args, *, data=None, env=None, timeout=120, limit=32 << 20, code="command"):
    """Bound both pipes during collection; never echo a child diagnostic."""
    require(data is None or len(data) <= limit, code)
    try:
        with subprocess.Popen(args, stdin=subprocess.PIPE if data is not None else subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env) as process:
            try:
                buffers = [bytearray(), bytearray()]
                pending = memoryview(data or b"")
                deadline = time.monotonic() + timeout
                with selectors.DefaultSelector() as selector:
                    for index, stream in enumerate((process.stdout, process.stderr)):
                        if stream is None:
                            raise Stop(code)
                        selector.register(stream, selectors.EVENT_READ, index)
                    if process.stdin is not None:
                        os.set_blocking(process.stdin.fileno(), False)
                        selector.register(process.stdin, selectors.EVENT_WRITE, 2)
                    while selector.get_map():
                        require(time.monotonic() < deadline, code)
                        for key, _ in selector.select(timeout=0.2):
                            if key.data == 2:
                                if pending:
                                    pending = pending[os.write(key.fd, pending[:65536]):]
                                if not pending:
                                    selector.unregister(key.fileobj)
                                    if process.stdin is not None:
                                        process.stdin.close()
                                continue
                            chunk = os.read(key.fd, 65536)
                            if not chunk:
                                selector.unregister(key.fileobj)
                                continue
                            buffers[key.data].extend(chunk)
                            require(len(buffers[key.data]) <= limit, code)
                result = process.wait(timeout=max(0.01, deadline - time.monotonic()))
                return subprocess.CompletedProcess(args, result, bytes(buffers[0]), bytes(buffers[1]))
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
    except (OSError, subprocess.TimeoutExpired):
        raise Stop(code) from None


def command(args, *, data=None, env=None, timeout=120):
    result = capture(args, data=data, env=env, timeout=timeout)
    require(result.returncode == 0, "command")
    return result.stdout


def api(path, data=None):
    args = ["gh", "api", f"repos/{REPO}/{path}"]
    if data is not None:
        args += ["--method", "POST", "--input", "-"]
    raw = command(args, data=json.dumps(data).encode() if data is not None else None)
    return parse_json(raw) if raw.strip() else {}


def parse_json(raw, code="command"):
    try:
        return json.loads(raw)
    except (ValueError, TypeError):
        raise Stop(code) from None


def output(**pins):
    # Call only with locally validated public pins, never API/control-plane objects.
    try:
        with open(os.environ["GITHUB_OUTPUT"], "a") as file:
            for key, value in pins.items():
                file.write(f"{key}={value}\n")
    except OSError:
        raise Stop("command") from None


def valid_pins(source, release=None, tag=None, image=None):
    require(re.fullmatch(SHA, source) is not None, "arguments")
    if release is not None:
        require(re.fullmatch(SHA, release) is not None, "arguments")
    if tag is not None:
        require(re.fullmatch(VERSION, tag) is not None, "arguments")
    if image is not None:
        require(re.fullmatch(DIGEST, image) is not None, "arguments")


def latest(source):
    require(api("git/ref/heads/main")["object"]["sha"] == source, "stale")


def validate_run(run, workflow, sha, branch, events):
    require(run["repository"]["full_name"] == REPO and run["head_repository"]["full_name"] == REPO,
            "event")
    require(run["path"] == f".github/workflows/{workflow}" and run["event"] in events
            and run["head_branch"] == branch and run["head_sha"] == sha, "event")
    require(run["status"] == "completed" and run["conclusion"] == "success", "checks")


def checks(sha, branch):
    runs = api(f"actions/workflows/check.yml/runs?head_sha={sha}&per_page=100")["workflow_runs"]
    for run in runs:
        try:
            validate_run(run, "check.yml", sha, branch, {"push", "workflow_dispatch"} if branch == "main" else {"workflow_dispatch"})
            return
        except Stop:
            continue
    raise Stop("checks")


def source_event(event, kind, context_sha, context_ref, workflow_id):
    require(event["repository"]["full_name"] == REPO, "event")
    if kind == "workflow_run":
        run = event["workflow_run"]
        source = run["head_sha"]
        validate_run(run, "check.yml", source, "main", {"push"})
        require(event["action"] == "completed" and run["workflow_id"] == workflow_id, "event")
    else:
        require(kind == "workflow_dispatch" and context_ref == "refs/heads/main", "event")
        source = context_sha
    valid_pins(source)
    return source


def select_source():
    require(os.environ.get("RELEASE_AUTOMATION_ENABLED") == "true", "disabled")
    require(os.environ["GITHUB_REPOSITORY"] == REPO, "event")
    event = parse_json(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes(), "event")
    workflow = api("actions/workflows/check.yml")
    source = source_event(event, os.environ["GITHUB_EVENT_NAME"], os.environ["GITHUB_SHA"],
                          os.environ["GITHUB_REF"], workflow["id"])
    if os.environ["GITHUB_EVENT_NAME"] == "workflow_run":
        run_id = event["workflow_run"].get("id")
        require(type(run_id) is int and run_id > 0, "event")
        live = api(f"actions/runs/{run_id}")
        validate_run(live, "check.yml", source, "main", {"push"})
        require(live["workflow_id"] == workflow["id"], "event")
    checks(source, "main")
    latest(source)
    output(source=source)


def transform(config, image):
    valid_pins("0" * 40, image=image)
    pattern = rb"(?m)^    image: " + re.escape(IMAGE.encode()) + rb"@sha256:[0-9a-f]{64}$"
    replaced, count = re.subn(pattern, b"    image: " + IMAGE.encode() + b"@" + image.encode(), config)
    require(count == 1, "transform")
    return replaced


def git(*args, **kwargs):
    return command(["git", *args], **kwargs)


def config_at(commit):
    return git("show", f"{commit}:{CONFIG}")


def relation(source, release, image):
    valid_pins(source, release, image=image)
    require(git("show", "-s", "--format=%P", release).decode().strip() == source, "transform")
    require(config_at(release) == transform(config_at(source), image), "transform")
    # Compare trees with the sole allowed path removed; includes modes and blobs.
    def tree(commit):
        return [entry for entry in git("ls-tree", "-rz", commit).split(b"\0")
                if entry and entry.split(b"\t", 1)[1] != CONFIG.encode()]
    require(tree(source) == tree(release), "transform")
    entries = [git("ls-tree", commit, "--", CONFIG).split()[0] for commit in (source, release)]
    require(entries == [b"100644", b"100644"], "transform")


def image_identity(source, release):
    # Use git flakes at both exact commits, not path flakes or the dirty worktree.
    paths = [command(["nix", "eval", "--raw", f"git+file://{Path.cwd()}?rev={rev}#packages.x86_64-linux.gateway-image.drvPath"],
                     timeout=600) for rev in (source, release)]
    require(paths[0] == paths[1] and paths[0].startswith(b"/nix/store/"), "identity")


def release_commit(source, image):
    valid_pins(source, image=image)
    with tempfile.TemporaryDirectory() as directory:
        env = dict(os.environ, GIT_INDEX_FILE=f"{directory}/index")
        git("read-tree", source, env=env)
        blob = git("hash-object", "-w", "--stdin", data=transform(config_at(source), image)).decode().strip()
        git("update-index", "--cacheinfo", f"100644,{blob},{CONFIG}", env=env)
        tree = git("write-tree", env=env).decode().strip()
        date = git("show", "-s", "--format=%cI", source).decode().strip()
        env.update(GIT_AUTHOR_NAME="possums-release", GIT_COMMITTER_NAME="possums-release",
                   GIT_AUTHOR_EMAIL="release@possums.invalid", GIT_COMMITTER_EMAIL="release@possums.invalid",
                   GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date)
        message = f"release: bind reproduced image to source\n\nSource: {source}\nImage: {image}\n"
        release = git("commit-tree", tree, "-p", source, data=message.encode(), env=env).decode().strip()
    relation(source, release, image)
    return release


def allocate(tags, release):
    paid = {tag: commit for tag, commit in tags.items() if re.fullmatch(VERSION, tag)}
    matching = [tag for tag, commit in paid.items() if commit == release]
    require(len(matching) <= 1, "collision")
    if matching:
        return matching[0], False
    versions = [tuple(map(int, tag[1:].split('.'))) for tag in paid]
    major, minor, patch = max(versions, default=(0, 0, 0))
    return f"v{major}.{minor}.{patch + 1}", True


def fetch_tags():
    git("fetch", "origin", "+refs/heads/main:refs/remotes/origin/main", "refs/tags/*:refs/tags/*")
    return {tag: git("rev-parse", f"refs/tags/{tag}^{{commit}}").decode().strip()
            for tag in git("tag", "--list").decode().splitlines()}


def release_record(tag):
    # Listing distinguishes absence from an unreadable API. Never treat errors as absence.
    pages = parse_json(command(["gh", "api", f"repos/{REPO}/releases?per_page=100", "--paginate", "--slurp"]))
    matches = [item for page in pages for item in page if item["tag_name"] == tag]
    require(len(matches) <= 1, "collision")
    return matches[0] if matches else None


def dispatch_wait(workflow, tag, release, request):
    inputs = {"release_request": request}
    title = f"{workflow}:{request}"
    if workflow == "tinfoil-release-publish.yml":
        inputs.update(build_run=os.environ["GITHUB_RUN_ID"], build_attempt=os.environ["GITHUB_RUN_ATTEMPT"])
        title += f":{inputs['build_run']}:{inputs['build_attempt']}"
    # A rerun may inspect a known invocation, never dispatch over a pending/failed one.
    def candidates():
        return [run for run in api(f"actions/workflows/{workflow}/runs?head_sha={release}&per_page=100")["workflow_runs"]
                if run["display_title"] == title and run["head_branch"] == tag]
    runs = candidates()
    require(len(runs) <= 1, "partial")
    if not runs:
        api(f"actions/workflows/{workflow}/dispatches", {"ref": tag, "inputs": inputs})
    deadline = time.monotonic() + 5400
    while time.monotonic() < deadline:
        runs = candidates()
        require(len(runs) <= 1, "partial")
        if runs and runs[0]["status"] == "completed":
            validate_run(runs[0], workflow, release, tag, {"workflow_dispatch"})
            return runs[0]
        time.sleep(15)
    raise Stop("timeout")


def prepare(source, image):
    require(os.environ.get("RELEASE_AUTOMATION_ENABLED") == "true", "disabled")
    valid_pins(source, image=image)
    latest(source)
    checks(source, "main")
    tags = fetch_tags()
    release = release_commit(source, image)
    image_identity(source, release)
    tag, new = allocate(tags, release)
    # Prevent the same source from acquiring different images on a later run.
    for old_tag, commit in tags.items():
        if (re.fullmatch(VERSION, old_tag)
                and git("show", "-s", "--format=%P", commit).decode().strip() == source):
            require(commit == release, "collision")
    latest(source)
    if new:
        require(release_record(tag) is None, "collision")
        git("tag", tag, release)
        git("push", "origin", f"refs/tags/{tag}")
    output(source=source, release=release, tag=tag, image=image)
    record = release_record(tag)
    if record is None:
        # Stable request identity permits inspection after an orchestration interruption.
        request = release
        dispatch_wait("check.yml", tag, release, request)
        # Never repeat a partially executed official publisher, even from another parent run.
        prior = api(f"actions/workflows/tinfoil-release-publish.yml/runs?head_sha={release}&per_page=100")["workflow_runs"]
        require(not prior, "partial")
        dispatch_wait("tinfoil-release-publish.yml", tag, release, request)
    verify_public(source, release, tag, image)


def publication_guard():
    require(os.environ.get("RELEASE_AUTOMATION_ENABLED") == "true", "disabled")
    require(os.environ["GITHUB_REPOSITORY"] == REPO and os.environ["GITHUB_REF"].startswith("refs/tags/"), "event")
    require(os.environ["GITHUB_RUN_ATTEMPT"] == "1", "partial")
    release, tag = os.environ["GITHUB_SHA"], os.environ["GITHUB_REF_NAME"]
    source = git("show", "-s", "--format=%P", release).decode().strip()
    image = image_at(release)
    valid_pins(source, release, tag, image)
    relation(source, release, image)
    require(git("rev-parse", f"refs/tags/{tag}").decode().strip() == release, "collision")
    require(os.environ.get("RELEASE_REQUEST") == release, "event")
    build_evidence(os.environ["BUILD_RUN"], os.environ["BUILD_ATTEMPT"], source)
    build_digests(os.environ["BUILD_RUN"], os.environ["BUILD_ATTEMPT"], source, image)
    prior = api(f"actions/workflows/tinfoil-release-publish.yml/runs?head_sha={release}&per_page=100")["workflow_runs"]
    require(all(str(run["id"]) == os.environ["GITHUB_RUN_ID"] for run in prior), "partial")
    latest(source)
    checks(release, tag)
    require(release_record(tag) is None, "partial")


def image_at(release):
    found = re.findall(rb"(?m)^    image: " + re.escape(IMAGE.encode()) + rb"@(sha256:[0-9a-f]{64})$", config_at(release))
    require(len(found) == 1, "transform")
    return found[0].decode()


def verify_manifest(raw, hash_bytes, config, image):
    digest = hashlib.sha256(raw).hexdigest()
    require(hash_bytes.decode().strip() == digest, "manifest")
    try:
        manifest = json.loads(raw)
        decoded_config = base64.b64decode(manifest["config"], validate=True)
    except (ValueError, KeyError, TypeError):
        raise Stop("manifest") from None
    require(decoded_config == config and transform(config, image) == config, "manifest")
    config_hash = hashlib.sha256(config).hexdigest()
    require(manifest["hashes"]["version"] == "v0.14.12"
            and f"tinfoil-config-hash={config_hash}" in manifest["cmdline"].split(), "manifest")
    # The canonical SDK shape includes root, config and external-config disks.
    shape = manifest["vm_shape"]
    require(isinstance(shape, dict)
            and shape == {"cpus": 2, "memory_mb": 8192, "gpus": 0, "disks": 3}
            and all(type(value) is int for value in shape.values()), "manifest")
    return manifest, digest


def verify_statement(checked, manifest, digest, release, tag):
    identity = f"https://github.com/{REPO}/.github/workflows/tinfoil-release-publish.yml@refs/tags/{tag}"
    require(len(checked) == 1, "signature")
    verified = checked[0]["verificationResult"]
    statement, cert = verified["statement"], verified["signature"]["certificate"]
    require(statement["predicateType"] == PREDICATE and statement["predicate"] == manifest
            and len(statement["subject"]) == 1 and statement["subject"][0]["digest"] == {"sha256": digest}, "signature")
    require(cert["subjectAlternativeName"] == identity
            and cert["buildSignerDigest"] == cert["sourceRepositoryDigest"] == release
            and cert["sourceRepositoryRef"] == f"refs/tags/{tag}"
            and cert["runnerEnvironment"] == "github-hosted", "signature")
    invocation = re.fullmatch(r"https://github\.com/" + re.escape(REPO) + r"/actions/runs/([1-9][0-9]*)/attempts/([1-9][0-9]*)",
                              cert["runInvocationURI"])
    if invocation is None:
        raise Stop("invocation")
    return invocation.groups()


def build_digests(run, attempt, source, image):
    artifacts = api(f"actions/runs/{run}/artifacts?per_page=100")
    require(artifacts["total_count"] <= 100, "invocation")
    for builder in ("a", "b"):
        name = f"image-digest-{run}-{attempt}-{source}-{builder}"
        matches = [artifact for artifact in artifacts["artifacts"] if artifact["name"] == name]
        require(len(matches) == 1 and not matches[0]["expired"], "invocation")
        artifact_id = matches[0].get("id")
        require(type(artifact_id) is int and artifact_id > 0, "invocation")
        archive = command(["gh", "api", f"repos/{REPO}/actions/artifacts/{artifact_id}/zip"])
        with zipfile.ZipFile(io.BytesIO(archive)) as file:
            require(file.namelist() == ["digest"] and file.getinfo("digest").file_size <= 128, "invocation")
            require(file.read("digest") == (image + "\n").encode(), "identity")


def build_evidence(run, attempt, source):
    require(re.fullmatch(r"[1-9][0-9]*", run) is not None
            and re.fullmatch(r"[1-9][0-9]*", attempt) is not None, "invocation")
    origin = api(f"actions/runs/{run}/attempts/{attempt}")
    require(origin["repository"]["full_name"] == origin["head_repository"]["full_name"] == REPO
            and origin["path"] == ".github/workflows/publish-image.yml"
            and origin["event"] in {"workflow_run", "workflow_dispatch"}
            and origin["head_branch"] == "main"
            and origin["display_title"] == f"Image source {source}"
            and str(origin["run_attempt"]) == attempt, "invocation")
    # workflow_run records the workflow implementation's default-branch SHA,
    # not necessarily the admitted source. The source-checks job authenticates
    # the triggering event; the immutable run title binds that source here.
    valid_pins(origin["head_sha"])
    if origin["event"] == "workflow_dispatch":
        require(origin["head_sha"] == source, "invocation")
    jobs = api(f"actions/runs/{run}/attempts/{attempt}/jobs?per_page=100")
    for name in ("source-checks", "build (a)", "build (b)"):
        matching = [job for job in jobs["jobs"] if job["name"] == name]
        require(len(matching) == 1 and matching[0]["conclusion"] == "success", "invocation")


def verify_public(source, release, tag, image, expected_manifest=None):
    valid_pins(source, release, tag, image)
    fetch_tags()
    require(git("rev-parse", f"refs/tags/{tag}").decode().strip() == release, "collision")
    relation(source, release, image)
    record = release_record(tag)
    if record is None:
        raise Stop("partial")
    require(not record["draft"] and not record["prerelease"], "partial")
    for name in ("tinfoil-deployment.json", "tinfoil.hash"):
        require(sum(asset["name"] == name for asset in record["assets"]) == 1, "partial")
    with tempfile.TemporaryDirectory() as directory:
        command(["gh", "release", "download", tag, "--repo", REPO, "--dir", directory,
                 "--pattern", "tinfoil-deployment.json", "--pattern", "tinfoil.hash"])
        path = Path(directory)
        manifest, digest = verify_manifest((path / "tinfoil-deployment.json").read_bytes(),
                                           (path / "tinfoil.hash").read_bytes(), config_at(release), image)
        if expected_manifest is not None:
            require(digest == expected_manifest, "manifest")
        identity = f"https://github.com/{REPO}/.github/workflows/tinfoil-release-publish.yml@refs/tags/{tag}"
        raw = command(["gh", "attestation", "verify", str(path / "tinfoil-deployment.json"), "--repo", REPO,
                       "--cert-identity", identity, "--source-ref", f"refs/tags/{tag}", "--source-digest", release,
                       "--signer-digest", release, "--deny-self-hosted-runners", "--predicate-type", PREDICATE,
                       "--format", "json"])
        try:
            checked = json.loads(raw)
        except (ValueError, TypeError):
            raise Stop("signature") from None
        run, attempt = verify_statement(checked, manifest, digest, release, tag)
    # Bind the signed attempt, not the most recent attempt or a hardcoded attempt=1.
    live = api(f"actions/runs/{run}/attempts/{attempt}")
    validate_run(live, "tinfoil-release-publish.yml", release, tag, {"workflow_dispatch"})
    require(str(live["run_attempt"]) == attempt and str(live["id"]) == run, "invocation")
    origin = re.fullmatch(r"tinfoil-release-publish\.yml:" + release + r":([1-9][0-9]*):([1-9][0-9]*)", live["display_title"])
    if origin is None:
        raise Stop("invocation")
    build_run, build_attempt = origin.groups()
    build_evidence(build_run, build_attempt, source)
    jobs = api(f"actions/runs/{run}/attempts/{attempt}/jobs?per_page=100")
    require(jobs["total_count"] == 1 and len(jobs["jobs"]) == 1
            and jobs["jobs"][0]["conclusion"] == "success", "invocation")
    checks(release, tag)
    return digest


def dispatch_deploy(source, release, tag, image):
    # Separate switch: publication is useful even when deployment is disabled.
    if os.environ.get("PRODUCTION_DEPLOYMENT_ENABLED") != "true":
        return
    require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "partial")
    digest = verify_public(source, release, tag, image)
    latest(source)
    api("actions/workflows/deploy-production.yml/dispatches", {"ref": "main", "inputs": {
        "source": source, "release": release, "tag": tag, "image": image, "manifest": digest}})


def main():
    operation, *args = sys.argv[1:]
    if operation == "source" and not args:
        select_source()
    elif operation == "prepare" and len(args) == 2:
        prepare(*args)
    elif operation == "publication-guard" and not args:
        publication_guard()
    elif operation == "verify" and len(args) == 4:
        verify_public(*args)
    elif operation == "dispatch-deploy" and len(args) == 4:
        dispatch_deploy(*args)
    else:
        raise Stop("arguments")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps(diagnostic(error.code, error.status) if isinstance(error, Stop) else diagnostic("internal")))
        sys.exit(1)
