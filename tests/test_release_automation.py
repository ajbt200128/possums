"""Offline synthetic release/deployment safety contracts; no network or credentials."""
import base64
import copy
import hashlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


r = load("release")
d = load("deploy")

S, R, D = "a" * 40, "b" * 40, "sha256:" + "c" * 64
V = "v1.2.3"
CONFIG = b"containers:\n  - name: gateway\n    image: " + r.IMAGE.encode() + b"@sha256:" + b"0" * 64 + b"\n    read_only: true\n"


def run(workflow="check.yml", sha=S, branch="main", event="push") -> dict:
    return {"id": 10, "workflow_id": 20, "repository": {"full_name": r.REPO},
            "head_repository": {"full_name": r.REPO}, "path": f".github/workflows/{workflow}",
            "event": event, "head_branch": branch, "head_sha": sha, "status": "completed",
            "conclusion": "success", "run_attempt": 2}


def environment() -> dict:
    return {"id": 7, "name": "production", "can_admins_bypass": False,
            "protection_rules": [{"type": "required_reviewers", "prevent_self_review": False,
                                  "reviewers": [{"type": "User", "reviewer": {"id": 8, "login": "ajbt200128"}}]}],
            "deployment_branch_policy": {"protected_branches": False, "custom_branch_policies": True}}


def branches() -> dict:
    return {"total_count": 1, "branch_policies": [{"name": "main", "type": "branch"}]}


def view(tag="v1.2.2") -> dict:
    data: dict = dict.fromkeys(d.STABLE)
    data.update(repo=r.REPO, current_tag=tag, status="running", update_tag=None,
                debug=False, disable_cc_mode=False, held=False, mark_latest_release=False,
                variables={"private": "SYNTHETIC-SECRET"}, secrets=sorted(d.REQUIRED_REFS),
                ssh_keys=[], cpus=2, gpus=0, memory_mb=8192, domain=d.DOMAIN,
                id="11111111-1111-1111-1111-111111111111", name=d.NAME)
    return data


def plan() -> dict:
    return {"read_only": True, "current": {"tag": "v1.2.2", "cpus": 2, "gpus": 0, "memory_mb": 8192},
            "target": {"tag": V, "cpus": 2, "gpus": 0, "memory_mb": 8192},
            "update_strategy": "blue_green", "downtime_required": False, "hold": False,
            "mark_latest_release": False,
            "configuration_changes": {"variables": {}, "secrets": {}, "ssh_keys": {}, "settings": []}}


class StopTest(unittest.TestCase):
    def stop(self, code, function, *args):
        with self.assertRaises(r.Stop) as caught:
            function(*args)
        self.assertEqual(caught.exception.code, code)


class SafetyTest(StopTest):
    def test_event_uses_origin_not_workflow_run_default_sha(self):
        event = {"action": "completed", "repository": {"full_name": r.REPO}, "workflow_run": run()}
        self.assertEqual(r.source_event(event, "workflow_run", R, "refs/heads/main", 20), S)
        mutations = [("event", "pull_request"), ("head_branch", "feature"), ("path", ".github/workflows/other.yml"),
                     ("conclusion", "failure"), ("workflow_id", 99), ("head_sha", "x" * 40),
                     ("head_repository", {"full_name": "hostile/fork"})]
        for key, value in mutations:
            changed = copy.deepcopy(event)
            changed["workflow_run"][key] = value
            with self.subTest(key=key), self.assertRaises(r.Stop):
                r.source_event(changed, "workflow_run", R, "refs/heads/main", 20)
        self.stop("event", r.source_event, event, "workflow_dispatch", S, "refs/tags/" + V, 20)
        self.stop("event", r.source_event, event, "pull_request", S, "refs/heads/main", 20)

    def test_latest_and_exact_checks(self):
        with patch.object(r, "api", return_value={"object": {"sha": R}}):
            self.stop("stale", r.latest, S)
        with patch.object(r, "api", return_value={"workflow_runs": [run(sha=R)]}):
            self.stop("checks", r.checks, S, "main")
        with patch.object(r, "api", return_value={"workflow_runs": [run()]}):
            r.checks(S, "main")

    def test_activation_absent_and_manual_branch_rejected_before_lookup(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(r, "api") as api:
            self.stop("disabled", r.select_source)
            api.assert_not_called()

    def test_transform_preserves_every_other_byte(self):
        expected = CONFIG.replace(b"0" * 64, b"c" * 64)
        self.assertEqual(r.transform(CONFIG, D), expected)
        self.stop("transform", r.transform, CONFIG + CONFIG, D)
        self.stop("transform", r.transform, CONFIG.replace(b"gateway@", b"free@"), D)
        self.stop("arguments", r.transform, CONFIG, D + "\nhostile")

    def test_semantic_versions_rerun_collisions_and_free_exclusion(self):
        tags = {"v0.0.9": S, "v0.0.10": S, "free-v900.0.0": S, "v01.2.3": S}
        self.assertEqual(r.allocate(tags, R), ("v0.0.11", True))
        tags[V] = R
        self.assertEqual(r.allocate(tags, R), (V, False))
        tags["v1.2.4"] = R
        self.stop("collision", r.allocate, tags, R)

    def test_publication_attempt_retry_is_never_replayed(self):
        env = {"RELEASE_AUTOMATION_ENABLED": "true", "GITHUB_REPOSITORY": r.REPO,
               "GITHUB_REF": "refs/tags/" + V, "GITHUB_RUN_ATTEMPT": "2"}
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api") as api:
            self.stop("partial", r.publication_guard)
            api.assert_not_called()

    def test_existing_failed_invocation_is_not_dispatched_again(self):
        failed = run(sha=R, branch=V, event="workflow_dispatch")
        failed.update(display_title="check.yml:" + R, conclusion="failure")
        with patch.object(r, "api", return_value={"workflow_runs": [failed]}) as api:
            self.stop("checks", r.dispatch_wait, "check.yml", V, R, R)
            self.assertTrue(all(len(call.args) == 1 for call in api.call_args_list))

    def test_image_identity_mismatch(self):
        with patch.object(r, "command", side_effect=[b"/nix/store/a.drv", b"/nix/store/b.drv"]):
            self.stop("identity", r.image_identity, S, R)

    def test_exact_build_artifact_attempt_and_digest(self):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("digest", D + "\n")
        artifacts = {"total_count": 2, "artifacts": [
            {"id": i, "name": f"image-digest-10-2-{S}-{builder}", "expired": False}
            for i, builder in enumerate(("a", "b"), 1)]}
        with patch.object(r, "api", return_value=artifacts), patch.object(r, "command", return_value=buffer.getvalue()):
            r.build_digests("10", "2", S, D)
            self.stop("invocation", r.build_digests, "10", "1", S, D)
            self.stop("identity", r.build_digests, "10", "2", S, "sha256:" + "d" * 64)

    def test_build_evidence_source_and_attempt(self):
        origin = run("publish-image.yml", event="workflow_run")
        origin.update(head_sha=R, display_title=f"Image source {S}")
        jobs = {"jobs": [{"name": name, "conclusion": "success"} for name in ("source-checks", "build (a)", "build (b)")]}
        # A later default-branch implementation SHA is valid for workflow_run.
        with patch.object(r, "api", side_effect=[origin, jobs]):
            r.build_evidence("10", "2", S)
        for key, value in (("display_title", f"Image source {R}"),
                           ("run_attempt", 3), ("event", "workflow_dispatch")):
            changed = {**origin, key: value}
            with self.subTest(key=key), patch.object(r, "api", return_value=changed):
                self.stop("invocation", r.build_evidence, "10", "2", S)
        origin.update(head_sha=S, event="workflow_dispatch")
        with patch.object(r, "api", side_effect=[origin, jobs]):
            r.build_evidence("10", "2", S)

    def test_policy_requires_reviewer_and_closed_branch_restrictions(self):
        self.assertEqual(d.policy(environment(), branches(), 8), "7")
        self.stop("environment_identity", d.policy, environment(), branches(), 8, "99")
        self.stop("reviewer", d.policy, environment(), branches(), 99)
        # The documented REST response has no bypass field; actual operator
        # approval is independently checked inside the protected job.
        changed = environment()
        changed.pop("can_admins_bypass")
        self.assertEqual(d.policy(changed, branches(), 8), "7")
        changed = environment()
        changed["protection_rules"][0]["prevent_self_review"] = True
        self.stop("reviewer", d.policy, changed, branches(), 8)
        changed["protection_rules"] = []
        self.stop("reviewer", d.policy, changed, branches(), 8)
        for field, value in (("type", "tag"), ("name", "*")):
            changed = branches()
            changed["branch_policies"][0][field] = value
            self.stop("branch_policy", d.policy, environment(), changed, 8)
        with patch.object(r, "api", side_effect=r.Stop("command")):
            self.stop("environment_unreadable", d.live_policy)

    def test_policy_needs_no_secret_or_variable_metadata(self):
        with patch.object(r, "api", side_effect=[environment(), branches()]) as api, \
                patch.object(r, "command", return_value=b'{"id":8}'):
            self.assertEqual(d.live_policy(), "7")
            self.assertEqual([call.args[0] for call in api.call_args_list], [
                "environments/production", "environments/production/deployment-branch-policies?per_page=100"])

    def test_eligibility_before_and_after_approval(self):
        env = {"GITHUB_REPOSITORY": r.REPO, "GITHUB_REF": "refs/heads/main",
               "GITHUB_EVENT_NAME": "workflow_dispatch", "PRODUCTION_DEPLOYMENT_ENABLED": "true",
               "RELEASE_AUTOMATION_ENABLED": "true"}
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api") as api, \
                patch.object(d, "live_policy", return_value="7"), patch.object(r, "latest", side_effect=[None, r.Stop("stale")]):
            self.assertEqual(d.eligibility(S), "7")
            self.stop("stale", d.eligibility, S, "7")
            api.assert_not_called()
        for name in ("PRODUCTION_DEPLOYMENT_ENABLED", "RELEASE_AUTOMATION_ENABLED"):
            for value in (None, "false", "TRUE"):
                changed = dict(env)
                if value is None:
                    del changed[name]
                else:
                    changed[name] = value
                with patch.dict(os.environ, changed, clear=True), patch.object(d, "live_policy") as policy:
                    self.stop("disabled", d.eligibility, S)
                    policy.assert_not_called()

    def test_serving_tool_required_before_approval_without_serving_traffic(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "GITHUB_RUN_ATTEMPT": "1", "RUNNER_TEMP": directory, "SERVING_VERIFIED": "true"}), \
                patch.object(d, "eligibility", return_value="7"), patch.object(r, "verify_public"), \
                patch.object(r, "output") as output, patch.object(r, "capture") as capture:
            self.stop("serving_tool", d.preflight, S, R, V, D, "d" * 64)
            output.assert_not_called()
            capture.assert_not_called()
            tool = Path(directory) / "possums-verify-serving"
            tool.write_text("synthetic executable")
            tool.chmod(0o700)
            d.preflight(S, R, V, D, "d" * 64)
            output.assert_called_once_with(environment_id="7")
            capture.assert_not_called()

    def test_authenticated_review_not_gate_bypass_authorizes_run(self):
        receipt = {"state": "approved", "environments": [{"id": 7, "name": "production"}],
                   "user": {"id": 8, "login": "ajbt200128"}}
        with patch.dict(os.environ, {"GITHUB_RUN_ID": "123"}), \
                patch.object(r, "api", return_value=[receipt]), patch.object(r, "command", return_value=b'{"id":8}'):
            d.approved("7")
        rejected = {**receipt, "state": "rejected"}
        wrong_user = {**receipt, "user": {"id": 9, "login": "other"}}
        wrong_environment = {**receipt, "environments": [{"id": 99, "name": "production"}]}
        for reviews in ([], [rejected], [wrong_user], [wrong_environment], [receipt, rejected], [receipt, receipt]):
            with self.subTest(reviews=reviews), patch.dict(os.environ, {"GITHUB_RUN_ID": "123"}), \
                    patch.object(r, "api", return_value=reviews), patch.object(r, "command", return_value=b'{"id":8}'):
                self.stop("approval", d.approved, "7")

    def test_serving_capture_is_credential_free_and_preserves_closed_failure(self):
        good = subprocess.CompletedProcess([], 0, b'{"stage":"serving","passed":true}', b"")
        failure = subprocess.CompletedProcess([], 1, b"", b'{"code":"SERVING_HTTP_STATUS","status":503}')
        hostile = subprocess.CompletedProcess([], 1, b"", b'{"code":"SYNTHETIC-SECRET","status":503}')
        with patch.dict(os.environ, {"RUNNER_TEMP": "/synthetic", "TINFOIL_ADMIN_KEY": "must-not-leak", "GH_TOKEN": "must-not-leak"}), \
                patch.object(d, "serving_tool", return_value="synthetic-tool"), patch.object(r, "capture", return_value=good) as capture:
            d.verify_serving(V, "d" * 64)
            self.assertNotIn("TINFOIL_ADMIN_KEY", capture.call_args.kwargs["env"])
            self.assertNotIn("GH_TOKEN", capture.call_args.kwargs["env"])
            capture.return_value = failure
            with self.assertRaises(r.Stop) as caught:
                d.verify_serving(V, "d" * 64)
            self.assertEqual(caught.exception.code, "SERVING_HTTP_STATUS")
            self.assertEqual(r.diagnostic(caught.exception.code, caught.exception.status)["status"], 503)
            capture.return_value = hostile
            self.stop("SERVING_UNKNOWN", d.verify_serving, V, "d" * 64)

    def test_hostile_parser_and_subprocess_errors_are_closed(self):
        for script in ("release.py", "deploy.py"):
            result = subprocess.run([sys.executable, str(ROOT / "scripts" / script), "SYNTHETIC-SECRET"],
                                    capture_output=True, check=False)
            self.assertEqual(result.returncode, 1)
            self.assertNotIn(b"SYNTHETIC-SECRET", result.stdout + result.stderr)
            self.assertNotIn(b"Traceback", result.stdout + result.stderr)
            self.assertEqual(json.loads(result.stdout)["constraint"], "arguments")
        self.stop("plan_missing", d.inspect_plan, b'{"private":"SYNTHETIC-SECRET"', "v1.2.2", V)
        with patch.object(subprocess, "Popen", side_effect=subprocess.TimeoutExpired("SYNTHETIC-SECRET", 1)):
            self.stop("command", r.command, ["synthetic"])

    def test_public_commands_bound_both_pipes_and_deliver_input(self):
        result = r.capture([sys.executable, "-c", "import sys; sys.stdout.buffer.write(sys.stdin.buffer.read())"],
                           data=b"synthetic input")
        self.assertEqual(result.stdout, b"synthetic input")
        for stream in ("stdout", "stderr"):
            self.stop("command", lambda stream=stream: r.capture(
                [sys.executable, "-c", f"import sys; sys.{stream}.write('x'*10000)"], limit=32))
        self.stop("command", lambda: r.capture(
            [sys.executable, "-c", "import time; time.sleep(5)"], timeout=0.01))


class TreeTest(StopTest):
    def test_real_git_determinism_and_tree_relation(self):
        with tempfile.TemporaryDirectory() as directory:
            before = Path.cwd()
            os.chdir(directory)
            try:
                r.git("init", "-q")
                r.git("config", "user.name", "synthetic")
                r.git("config", "user.email", "synthetic@example.invalid")
                Path(r.CONFIG).write_bytes(CONFIG)
                Path("opaque").write_bytes(b"unchanged\x00bytes")
                r.git("add", ".")
                r.git("commit", "-qm", "synthetic source")
                source = r.git("rev-parse", "HEAD").decode().strip()
                release = r.release_commit(source, D)
                self.assertEqual(release, r.release_commit(source, D))
                r.relation(source, release, D)
                self.assertEqual(r.git("status", "--porcelain"), b"")
                self.stop("transform", r.relation, source, source, D)
                # A changed extra blob must not pass even with the right config.
                r.git("read-tree", release)
                Path("opaque").write_text("changed")
                r.git("add", "opaque")
                tree = r.git("write-tree").decode().strip()
                hostile = r.git("commit-tree", tree, "-p", source, data=b"hostile\n").decode().strip()
                self.stop("transform", r.relation, source, hostile, D)
                r.git("update-index", "--chmod=+x", r.CONFIG)
                tree = r.git("write-tree").decode().strip()
                hostile = r.git("commit-tree", tree, "-p", source, data=b"mode\n").decode().strip()
                self.stop("transform", r.relation, source, hostile, D)
            finally:
                os.chdir(before)


class ProvenanceTest(StopTest):
    def fixture(self):
        config = r.transform(CONFIG, D)
        manifest = {"config": base64.b64encode(config).decode(), "hashes": {"version": "v0.14.12"},
                    "cmdline": "tinfoil-config-hash=" + hashlib.sha256(config).hexdigest(),
                    "vm_shape": {"cpus": 2, "memory_mb": 8192, "gpus": 0, "disks": 3}}
        raw = json.dumps(manifest).encode()
        digest = hashlib.sha256(raw).hexdigest()
        identity = f"https://github.com/{r.REPO}/.github/workflows/tinfoil-release-publish.yml@refs/tags/{V}"
        checked = [{"verificationResult": {"statement": {"predicateType": r.PREDICATE, "predicate": manifest,
                    "subject": [{"digest": {"sha256": digest}}]}, "signature": {"certificate": {
                    "subjectAlternativeName": identity, "buildSignerDigest": R, "sourceRepositoryDigest": R,
                    "sourceRepositoryRef": "refs/tags/" + V, "runnerEnvironment": "github-hosted",
                    "runInvocationURI": f"https://github.com/{r.REPO}/actions/runs/123/attempts/2"}}}}]
        return config, manifest, raw, digest, checked

    def test_manifest_exact_config_hash_image_and_substitution(self):
        config, manifest, raw, digest, _ = self.fixture()
        self.assertEqual(r.verify_manifest(raw, digest.encode(), config, D), (manifest, digest))
        self.stop("manifest", r.verify_manifest, raw, b"0" * 64, config, D)
        self.stop("manifest", r.verify_manifest, raw, digest.encode(), config + b"\n", D)
        self.stop("manifest", r.verify_manifest, raw, digest.encode(), config, "sha256:" + "e" * 64)
        self.stop("manifest", r.verify_manifest, b"SYNTHETIC-SECRET", hashlib.sha256(b"SYNTHETIC-SECRET").hexdigest().encode(), config, D)

    def test_manifest_canonical_shape_rejects_missing_changed_and_untyped_dimensions(self):
        config, manifest, _, _, _ = self.fixture()
        for key in ("cpus", "memory_mb", "gpus", "disks"):
            for value in (None, False, "3", manifest["vm_shape"][key] + 1):
                changed = copy.deepcopy(manifest)
                if value is None:
                    del changed["vm_shape"][key]
                else:
                    changed["vm_shape"][key] = value
                raw = json.dumps(changed).encode()
                self.stop("manifest", r.verify_manifest, raw,
                          hashlib.sha256(raw).hexdigest().encode(), config, D)

    def test_signed_actual_attempt_not_hardcoded_one(self):
        _, manifest, _, digest, checked = self.fixture()
        self.assertEqual(r.verify_statement(checked, manifest, digest, R, V), ("123", "2"))
        for key, value in (("buildSignerDigest", S), ("sourceRepositoryRef", "refs/heads/main"),
                           ("runnerEnvironment", "self-hosted"), ("subjectAlternativeName", "hostile")):
            changed = copy.deepcopy(checked)
            changed[0]["verificationResult"]["signature"]["certificate"][key] = value
            self.stop("signature", r.verify_statement, changed, manifest, digest, R, V)
        for key, value in (("predicateType", "hostile"), ("predicate", {}), ("subject", [])):
            changed = copy.deepcopy(checked)
            changed[0]["verificationResult"]["statement"][key] = value
            self.stop("signature", r.verify_statement, changed, manifest, digest, R, V)


class ReadOnlyPlanTest(StopTest):
    def fixture(self):
        data = plan()
        data.update(instance_id=view()["id"], name=d.NAME)
        return json.dumps(data).encode()

    def check(self, raw):
        with patch.dict(os.environ, {"TINFOIL_ADMIN_KEY": "SYNTHETIC-SECRET"}), \
                patch.object(d.urllib.request, "build_opener") as build:
            build.return_value.open.return_value = io.BytesIO(raw)
            d.plan_update(view(), V)
            request = build.return_value.open.call_args.args[0]
            self.assertEqual(request.full_url, "https://api.tinfoil.sh/api/containers/" + view()["id"] + "/update/plan")
            self.assertEqual(json.loads(request.data), {"tag": V, "custom_domain": d.DOMAIN,
                                                       "mark_latest_release": False})
            self.assertEqual(request.get_method(), "POST")
            self.assertEqual(request.get_header("Authorization"), "Bearer SYNTHETIC-SECRET")
            self.assertEqual(request.get_header("Content-type"), "application/json")
            self.assertEqual(request.get_header("Accept"), "application/json")
            self.assertEqual(request.get_header("User-agent"), "possums-release-verifier/1")
            self.assertEqual(build.return_value.open.call_args.kwargs, {"timeout": 30})
            self.assertIsInstance(build.call_args.args[0], d.urllib.request.ProxyHandler)
            self.assertIsInstance(build.call_args.args[1], d.NoPlanRedirect)

    def test_supported_read_only_route_has_only_fixed_update_fields(self):
        self.check(self.fixture())
        self.assertIsNone(d.NoPlanRedirect().redirect_request(None, None, None, None, None, None))

    def test_unknown_change_family_stops_before_cli_mutation(self):
        data = json.loads(self.fixture())
        data["configuration_changes"]["volumes"] = {"changed": ["SYNTHETIC-SECRET"]}
        with patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"}), \
                patch.object(d.urllib.request, "build_opener") as build, \
                patch.object(d, "get", return_value=view()), patch.object(d, "call") as call:
            build.return_value.open.return_value = io.BytesIO(json.dumps(data).encode())
            self.stop("plan_rejected", d.update_once, "unused", V, lambda: None)
            call.assert_not_called()
        self.stop("plan_configuration", d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)

    def test_plan_http_refusal_never_reaches_update_or_final_admission(self):
        with patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"}), \
                patch.object(d.urllib.request, "build_opener") as build, \
                patch.object(d, "get", return_value=view()), patch.object(d, "call") as call:
            build.return_value.open.side_effect = d.urllib.error.HTTPError("private-url", 403, "SYNTHETIC-SECRET", {}, None)
            gate = Mock()
            with self.assertRaises(r.Stop) as caught:
                d.update_once("unused", V, gate)
            self.assertEqual((caught.exception.code, caught.exception.status), ("plan_http", 403))
            call.assert_not_called()
            gate.assert_not_called()

    def test_documented_secret_refresh_metadata_remains_supported(self):
        data = json.loads(self.fixture())
        data["configuration_changes"]["secrets_refreshed"] = sorted(d.REQUIRED_REFS)
        self.check(json.dumps(data).encode())

    def test_wrong_instance_resources_or_hostile_plan_are_rejected(self):
        for field, value in (("instance_id", "SYNTHETIC-SECRET"), ("name", "other"),
                             ("current", {"tag": "v1.2.2", "cpus": 99, "gpus": 0, "memory_mb": 8192}),
                             ("read_only", False)):
            data = json.loads(self.fixture())
            data[field] = value
            self.stop("plan_rejected", self.check, json.dumps(data).encode())
        self.stop("plan_rejected", self.check, b"SYNTHETIC-SECRET")
        with patch.object(d, "LIMIT", 32):
            self.stop("plan_rejected", self.check, self.fixture())

    def test_http_and_transport_failures_keep_safe_categories(self):
        for status in (302, 403, 500):
            with patch.dict(os.environ, {"TINFOIL_ADMIN_KEY": "synthetic"}), \
                    patch.object(d.urllib.request, "build_opener") as build:
                build.return_value.open.side_effect = d.urllib.error.HTTPError("private-url", status, "SYNTHETIC-SECRET", {}, None)
                with self.assertRaises(r.Stop) as caught:
                    d.plan_update(view(), V)
                self.assertEqual(caught.exception.code, "plan_http")
                self.assertEqual(caught.exception.status, status)
                self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic(caught.exception.code, caught.exception.status)))
        with patch.dict(os.environ, {"TINFOIL_ADMIN_KEY": "synthetic"}), \
                patch.object(d.urllib.request, "build_opener") as build:
            build.return_value.open.side_effect = OSError("SYNTHETIC-SECRET")
            self.stop("plan_fetch", d.plan_update, view(), V)
        error = d.urllib.error.HTTPError("private-url", 403, "SYNTHETIC-SECRET", {}, None)
        with patch.dict(os.environ, {"TINFOIL_ADMIN_KEY": "synthetic"}), \
                patch.object(d.urllib.request, "build_opener") as build, \
                patch.object(error, "close", side_effect=OSError("SYNTHETIC-SECRET")):
            build.return_value.open.side_effect = error
            with self.assertRaises(r.Stop) as caught:
                d.plan_update(view(), V)
            self.assertEqual((caught.exception.code, caught.exception.status), ("plan_http", 403))
        error.close()


class MutationTest(StopTest):
    def setUp(self):
        self.environment = patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        planning = patch.object(d, "plan_update")
        self.planning = planning.start()
        self.addCleanup(planning.stop)

    def result(self):
        return subprocess.CompletedProcess([], 0, b"opaque", json.dumps(plan()).encode())

    def test_one_mutation_preserves_config_and_does_not_cancel_on_new_main(self):
        with patch.object(d, "get", side_effect=[view(), view(V)]), patch.object(d, "call", return_value=self.result()) as call:
            gates = []
            result = d.update_once("unused", V, lambda: gates.append("latest at admission"))
            self.assertEqual(len(gates), 1)  # No cancellation/recheck loop after admission.
            self.assertEqual(result["mutationCommandCount"], 1)
            self.assertFalse(result["servingVerified"])
            self.assertEqual(call.call_args.args[1], ["container", "update", d.NAME, "--tag", V,
                                                     "--custom-domain", d.DOMAIN, "--output", "json",
                                                     "--mark-latest=false"])
            self.assertTrue(result["approvedDomainConfirmed"])
            call.assert_called_once()

    def test_only_approved_old_to_new_hostname_transition_is_admitted(self):
        before = view()
        before["domain"] = d.OLD_DOMAIN
        migration = plan()
        migration["configuration_changes"]["settings"] = ["custom_domain"]
        result = subprocess.CompletedProcess([], 0, b"opaque", json.dumps(migration).encode())
        with patch.object(d, "get", side_effect=[before, view(V)]), patch.object(d, "call", return_value=result) as call:
            self.assertTrue(d.update_once("unused", V, lambda: None)["approvedDomainConfirmed"])
            call.assert_called_once()
        for domain in (None, "hostile.example", "https://gateway.possums.dev"):
            bad = view()
            bad["domain"] = domain
            with patch.object(d, "get", return_value=bad), patch.object(d, "call") as call:
                self.stop("domain_source", d.update_once, "unused", V, lambda: None)
                call.assert_not_called()

    def test_hostname_migration_plan_cannot_hide_other_settings(self):
        for settings in ([], ["domain"], ["custom_domain", "debug"], ["custom_domain", "custom_domain"]):
            changed = plan()
            changed["configuration_changes"]["settings"] = settings
            self.stop("plan_configuration", d.inspect_plan, json.dumps(changed).encode(),
                      "v1.2.2", V, d.OLD_DOMAIN)
        changed = plan()
        changed["configuration_changes"]["settings"] = ["custom_domain"]
        self.stop("plan_configuration", d.inspect_plan, json.dumps(changed).encode(),
                  "v1.2.2", V, d.DOMAIN)

    def test_old_hostname_is_not_accepted_as_successful_completion(self):
        before, after = view(), view(V)
        before["domain"] = after["domain"] = d.OLD_DOMAIN
        migration = plan()
        migration["configuration_changes"]["settings"] = ["custom_domain"]
        result = subprocess.CompletedProcess([], 0, b"opaque", json.dumps(migration).encode())
        with patch.object(d, "get", side_effect=[before, after]), patch.object(d, "call", return_value=result) as call:
            self.stop("domain_completion", d.update_once, "unused", V, lambda: None)
            call.assert_called_once()
        after["domain"] = "hostile.example"
        with patch.object(d, "get", side_effect=[view(), after]), patch.object(d, "call", return_value=self.result()):
            self.stop("configuration_changed", d.update_once, "unused", V, lambda: None)

    def test_deployment_and_serving_hostnames_match(self):
        source = (ROOT / "scripts/verify-serving/main.go").read_text()
        self.assertIn('host       = "' + d.DOMAIN + '"', source)
        self.assertNotIn(d.OLD_DOMAIN, source)

    def test_official_cli_omitted_empty_volume_fields_allow_update(self):
        def response(tag):
            data = view(tag)
            data.pop("volume_slots")
            data.pop("volumes")
            return subprocess.CompletedProcess([], 0, json.dumps(data).encode(), b"")

        with patch.object(d, "call", side_effect=[response("v1.2.2"), self.result(), response(V)]) as call:
            result = d.update_once("unused", V, lambda: None)
            self.assertTrue(result["opaqueConfigurationPreserved"])
            self.assertEqual(sum(item.args[1][1] == "update" for item in call.call_args_list), 1)

    def test_nonempty_volume_change_is_not_normalized_away(self):
        before = view()
        before.pop("volume_slots")
        before.pop("volumes")
        after = view(V)
        after.update(volume_slots=[{"name": "synthetic"}], volumes={"synthetic": "changed"})
        responses = [subprocess.CompletedProcess([], 0, json.dumps(before).encode(), b""), self.result(),
                     subprocess.CompletedProcess([], 0, json.dumps(after).encode(), b"")]
        with patch.object(d, "call", side_effect=responses):
            self.stop("configuration_changed", d.update_once, "unused", V, lambda: None)

    def test_rejected_read_only_plan_sends_zero_updates(self):
        self.planning.side_effect = r.Stop("plan_rejected")
        with patch.object(d, "get", return_value=view()), patch.object(d, "call") as call:
            self.stop("plan_rejected", d.update_once, "unused", V, lambda: None)
            call.assert_not_called()

    def test_stale_after_approval_sends_zero_updates(self):
        with patch.object(d, "get", return_value=view()), patch.object(d, "call") as call:
            self.stop("stale", d.update_once, "unused", V, lambda: r.require(False, "stale"))
            call.assert_not_called()

    def test_missing_secret_and_rerun_never_mutate(self):
        with patch.object(d, "get") as get, patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "2"}):
            self.stop("rerun", d.update_once, "unused", V, lambda: None)
            get.assert_not_called()
        with patch.object(d, "get") as get, patch.dict(os.environ, {"TINFOIL_ADMIN_KEY": ""}):
            self.stop("credential", d.update_once, "unused", V, lambda: None)
            get.assert_not_called()

    def test_uncertain_update_is_not_retried(self):
        with patch.object(d, "get", return_value=view()), patch.object(d, "call", side_effect=TimeoutError("SYNTHETIC-SECRET")) as call:
            self.stop("cli_uncertain", d.update_once, "unused", V, lambda: None)
            call.assert_called_once()

    def test_bounded_completion_and_opaque_preservation(self):
        changed = view(V)
        changed["variables"] = {"private": "other"}
        with patch.object(d, "get", side_effect=[view(), changed]), patch.object(d, "call", return_value=self.result()):
            self.stop("configuration_changed", d.update_once, "unused", V, lambda: None)
        with patch.object(d, "get", return_value=view()), patch.object(d, "call", return_value=self.result()) as call, \
                patch.object(d.time, "monotonic", side_effect=[0, 301]):
            self.stop("completion_timeout", d.update_once, "unused", V, lambda: None)
            call.assert_called_once()

    def test_plan_closed_constraints(self):
        mutations = [("hold", True, "plan_strategy"), ("update_strategy", "replace", "plan_strategy"),
                     ("downtime_required", True, "plan_strategy"), ("read_only", False, "plan_target")]
        for key, value, code in mutations:
            data = plan()
            data[key] = value
            self.stop(code, d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)
        data = plan()
        data["configuration_changes"]["secrets"] = {"removed": ["SYNTHETIC-SECRET"]}
        self.stop("plan_configuration", d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)

    def test_native_cli_null_change_slices_preserve_zero_changes(self):
        for additions in ((), sorted(d.TOR_REFS)):
            data = plan()
            for family in ("variables", "secrets", "ssh_keys"):
                data["configuration_changes"][family] = dict.fromkeys(("added", "changed", "removed"))
            if additions:
                data["configuration_changes"]["secrets"]["added"] = additions
            d.inspect_plan(json.dumps(data).encode(), "v1.2.2", V, approved_additions=additions)
            if additions:
                data["configuration_changes"]["secrets"]["added"] = None
                self.stop("plan_configuration", lambda data=data, additions=additions: d.inspect_plan(
                    json.dumps(data).encode(), "v1.2.2", V, approved_additions=additions))
        for invalid in (False, "", {}):
            data = plan()
            data["configuration_changes"]["secrets"]["changed"] = invalid
            self.stop("plan_configuration", d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)

    def test_cli_closed_stdin_output_bound_and_admin_env_only(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"RUNNER_TEMP": directory,
                "TINFOIL_API_KEY": "must-not-inherit", "TINFOIL_CONTROLPLANE_URL": "must-not-inherit"}):
            program = "import os,sys; assert sys.stdin.read()==''; assert 'TINFOIL_API_KEY' not in os.environ; assert 'TINFOIL_CONTROLPLANE_URL' not in os.environ; print('ok')"
            result = d.call(sys.executable, ["-c", program])
            self.assertEqual(result.returncode, 0)
            with patch.object(d, "LIMIT", 32):
                self.stop("cli_uncertain", d.call, sys.executable, ["-c", "print('x'*100)"])


class FirstTorActivationTest(StopTest):
    def setUp(self):
        environment = patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"})
        environment.start()
        self.addCleanup(environment.stop)
        self.before = view()
        self.before["secrets"].append("EXISTING_OPAQUE_REF")
        self.selection = self.before["secrets"] + sorted(d.TOR_REFS)
        self.after = view(V)
        self.after["secrets"] = list(reversed(self.selection))

    def activation_plan(self):
        data = plan()
        data.update(instance_id=self.before["id"], name=d.NAME)
        data["configuration_changes"]["secrets"] = {"added": sorted(d.TOR_REFS)}
        if self.before["domain"] == d.OLD_DOMAIN:
            data["configuration_changes"]["settings"] = ["custom_domain"]
        return data

    def cli_result(self, data=None):
        return subprocess.CompletedProcess([], 0, b"opaque", json.dumps(
            self.activation_plan() if data is None else data).encode())

    def test_first_binding_preserves_all_names_in_plan_and_single_cli_call(self):
        for domain in (d.OLD_DOMAIN, d.DOMAIN):
            self.before["domain"] = domain
            with self.subTest(domain=domain), patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(d, "get", side_effect=[self.before, self.after]), \
                    patch.object(d, "call", return_value=self.cli_result()) as call:
                build.return_value.open.return_value = io.BytesIO(json.dumps(self.activation_plan()).encode())
                gate = Mock()
                result = d.update_once("unused", V, gate, first_tor_activation=True)
                request = build.return_value.open.call_args.args[0]
                self.assertEqual(json.loads(request.data), {
                    "tag": V, "custom_domain": d.DOMAIN, "secrets": self.selection,
                    "mark_latest_release": False})
                self.assertEqual(call.call_args.args[1], ["container", "update", d.NAME, "--tag", V,
                    "--custom-domain", d.DOMAIN, "--output", "json", "--mark-latest=false"] + [
                        item for name in self.selection for item in ("--secret", name)])
                self.assertEqual(call.call_args.kwargs, {"timeout": 1800})
                self.assertNotIn("--yes", call.call_args.args[1])
                self.assertNotIn("SYNTHETIC-SECRET", json.dumps(result))
                self.assertTrue(result["approvedTorBindingsConfirmed"])
                self.assertTrue(result["opaqueConfigurationPreserved"])
                self.assertTrue(result["targetRunningNoPending"])
                self.assertFalse(result["servingVerified"])
                gate.assert_called_once_with()
                call.assert_called_once()
        self.assertFalse(d.stable_running(view(V), V, d.REQUIRED_REFS | d.TOR_REFS))
        self.assertTrue(d.stable_running(self.after, V, d.REQUIRED_REFS | d.TOR_REFS))

    def test_pending_old_current_references_wait_for_exact_completed_bindings(self):
        pending = {**self.before, "update_tag": V, "update_status": "deploying"}
        with patch.object(d, "plan_update"), \
                patch.object(d, "get", side_effect=[self.before, pending, self.after]) as get, \
                patch.object(d, "call", return_value=self.cli_result()) as call, \
                patch.object(d.time, "sleep") as sleep:
            gate = Mock()
            result = d.update_once("unused", V, gate, first_tor_activation=True)
            self.assertTrue(result["approvedTorBindingsConfirmed"])
            self.assertTrue(result["targetRunningNoPending"])
            self.assertFalse(result["servingVerified"])
            self.assertEqual(get.call_count, 3)
            sleep.assert_called_once_with(10)
            gate.assert_called_once_with()
            call.assert_called_once()

    def test_pending_old_references_do_not_extend_deadline_or_hide_config_drift(self):
        pending = {**self.before, "update_tag": V, "update_status": "deploying"}
        with patch.object(d, "plan_update"), \
                patch.object(d, "get", side_effect=[self.before, pending]), \
                patch.object(d, "call", return_value=self.cli_result()) as call, \
                patch.object(d.time, "monotonic", side_effect=[0, 1, 301]), \
                patch.object(d.time, "sleep") as sleep:
            self.stop("completion_timeout", d.update_once, "unused", V, Mock(), True)
            call.assert_called_once()
            sleep.assert_called_once_with(10)
        pending = {**pending, "cpus": self.before["cpus"] + 1, "update_status": "failed"}
        with patch.object(d, "plan_update"), \
                patch.object(d, "get", side_effect=[self.before, pending]), \
                patch.object(d, "call", return_value=self.cli_result()) as call, \
                patch.object(d.time, "sleep") as sleep:
            self.stop("configuration_changed", d.update_once, "unused", V, Mock(), True)
            call.assert_called_once()
            sleep.assert_not_called()

    def test_observed_failed_target_candidate_is_not_a_binding_failure(self):
        for names in (self.before["secrets"], self.selection):
            pending = {**self.before, "secrets": names, "update_tag": V, "update_status": "failed"}
            with self.subTest(names=names), patch.object(d, "plan_update"), \
                    patch.object(d, "get", side_effect=[self.before, pending]) as get, \
                    patch.object(d, "call", return_value=self.cli_result()) as call, \
                    patch.object(d.time, "sleep") as sleep:
                self.stop("candidate_failed", d.update_once, "unused", V, Mock(), True)
                self.assertEqual(get.call_count, 2)
                call.assert_called_once()
                sleep.assert_not_called()
        diagnostic = r.diagnostic("candidate_failed")
        self.assertIn("initiating cause", diagnostic["action"])
        self.assertIn("unknown", diagnostic["action"])
        self.assertIn("no replay", diagnostic["action"])

    def test_old_current_references_require_exact_pending_target(self):
        variants = [
            {**self.before, "update_tag": "v9.9.9", "update_status": "deploying"},
            {**self.before, "update_tag": "", "update_status": ""},
            {**self.before, "current_tag": V, "update_tag": V, "update_status": "deploying"},
            {**self.before, "status": "failed", "update_tag": V, "update_status": "failed"},
            {**self.before, "secrets": self.before["secrets"] + ["UNAPPROVED_REF"],
             "update_tag": V, "update_status": "deploying"},
            {**self.before, "secrets": self.before["secrets"] + [sorted(d.TOR_REFS)[0]],
             "update_tag": V, "update_status": "deploying"},
        ]
        for pending in variants:
            with self.subTest(pending=pending), patch.object(d, "plan_update"), \
                    patch.object(d, "get", side_effect=[self.before, pending]), \
                    patch.object(d, "call", return_value=self.cli_result()) as call, \
                    patch.object(d.time, "sleep") as sleep:
                self.stop("tor_binding_completion", d.update_once, "unused", V, Mock(), True)
                call.assert_called_once()
                sleep.assert_not_called()

    def test_either_existing_tor_reference_blocks_before_planning_even_without_opt_in(self):
        for refs in ([name] for name in sorted(d.TOR_REFS)):
            self.check_existing_references(refs)
        self.check_existing_references(sorted(d.TOR_REFS))

    def check_existing_references(self, refs):
        before = copy.deepcopy(self.before)
        before["secrets"] += refs
        for opt_in in (False, True):
            with self.subTest(refs=refs, opt_in=opt_in), patch.object(d, "get", return_value=before), \
                    patch.object(d, "plan_update") as planning, patch.object(d, "call") as call:
                gate = Mock()
                self.stop("tor_exclusive_publication", d.update_once, "unused", V, gate, opt_in)
                planning.assert_not_called()
                gate.assert_not_called()
                call.assert_not_called()
        diagnostic = r.diagnostic("tor_exclusive_publication")
        self.assertIn("unknown", diagnostic["action"])
        self.assertIn("qualify", diagnostic["action"])

    def test_invalid_before_selection_is_closed_and_never_admitted(self):
        for names in (None, "SYNTHETIC-SECRET", {}, [None], [{"private": "SYNTHETIC-SECRET"}],
                      [""], self.selection[:1] * 2):
            before = {**self.before, "secrets": names}
            with self.subTest(names=names), patch.object(d, "get", return_value=before), \
                    patch.object(d, "plan_update") as planning, patch.object(d, "call") as call:
                gate = Mock()
                self.stop("tor_binding", d.update_once, "unused", V, gate, True)
                planning.assert_not_called()
                gate.assert_not_called()
                call.assert_not_called()
                self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic("tor_binding")))

    def invalid_plans(self):
        mutations = []
        for added in ([], sorted(d.TOR_REFS)[:1], self.selection, sorted(d.TOR_REFS) + ["UNKNOWN"],
                      sorted(d.TOR_REFS) * 2, "SYNTHETIC-SECRET", None, [False], [{"private": "SYNTHETIC-SECRET"}]):
            mutations.append(("secrets", {"added": added}, "plan_configuration"))
        for kind in ("changed", "removed", "unknown"):
            mutations.append(("secrets", {"added": sorted(d.TOR_REFS), kind: ["EXISTING_OPAQUE_REF"]}, "plan_configuration"))
        for family in ("variables", "ssh_keys", "volumes"):
            mutations.append((family, {"added": ["SYNTHETIC-SECRET"]}, "plan_configuration"))
        mutations += [("settings", ["debug"], "plan_configuration")]
        for family, value, code in mutations:
            data = self.activation_plan()
            data["configuration_changes"][family] = value
            yield data, code
        for key, value, code in (("hold", True, "plan_strategy"), ("downtime_required", True, "plan_strategy"),
                                 ("update_strategy", "replace", "plan_strategy"), ("read_only", False, "plan_target")):
            data = self.activation_plan()
            data[key] = value
            yield data, code
        for field in ("cpus", "gpus", "memory_mb"):
            for both in (False, True):
                data = self.activation_plan()
                data["target"][field] += 1
                if both:
                    data["current"][field] += 1
                yield data, "plan_configuration"
        data = self.activation_plan()
        data["target"]["gpus"] = False
        yield data, "plan_configuration"

    def test_invalid_preplans_never_reach_final_gate_or_cli(self):
        for data, _ in self.invalid_plans():
            with self.subTest(data=data), patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(d, "get", return_value=self.before), patch.object(d, "call") as call:
                build.return_value.open.return_value = io.BytesIO(json.dumps(data).encode())
                gate = Mock()
                self.stop("plan_rejected", d.update_once, "unused", V, gate, True)
                gate.assert_not_called()
                call.assert_not_called()

    def test_cli_plans_are_independently_checked_after_one_mutation(self):
        for data, code in self.invalid_plans():
            with self.subTest(data=data), patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(d, "get", return_value=self.before) as get, \
                    patch.object(d, "call", return_value=self.cli_result(data)) as call:
                build.return_value.open.return_value = io.BytesIO(json.dumps(self.activation_plan()).encode())
                gate = Mock()
                self.stop(code, d.update_once, "unused", V, gate, True)
                gate.assert_called_once_with()
                call.assert_called_once()
                get.assert_called_once()
        # A CLI re-review cannot hide a changed second plan behind an accepted first plan.
        changed = self.activation_plan()
        changed["hold"] = True
        raw = json.dumps(self.activation_plan()).encode() + b"\n" + json.dumps(changed).encode()
        self.stop("plan_strategy", d.inspect_plan, raw, "v1.2.2", V, d.DOMAIN, d.TOR_REFS)

    def test_only_exact_fixed_approved_additions_are_allowed(self):
        for additions in (["UNKNOWN"], sorted(d.TOR_REFS)[:1], sorted(d.TOR_REFS) * 2):
            self.stop("plan_configuration", d.inspect_plan, json.dumps(self.activation_plan()).encode(),
                      "v1.2.2", V, d.DOMAIN, additions)
        self.stop("plan_configuration", d.inspect_plan, json.dumps(self.activation_plan()).encode(), "v1.2.2", V)

    def test_after_selection_and_other_opaque_changes_remain_uncertain_without_replay(self):
        variants = []
        for names in (None, {}, "SYNTHETIC-SECRET", [None], [{"private": "SYNTHETIC-SECRET"}],
                      self.selection[:-1], self.selection[1:], self.selection + ["UNKNOWN"],
                      self.selection + self.selection[:1]):
            variants.append(({**self.after, "secrets": names}, "tor_binding_completion"))
        for field in d.STABLE:
            if field not in ("secrets", "domain"):
                variants.append(({**self.after, field: "SYNTHETIC-SECRET"}, "configuration_changed"))
        for after, code in variants:
            with self.subTest(after=after), patch.object(d, "plan_update"), \
                    patch.object(d, "get", side_effect=[self.before, after]), \
                    patch.object(d, "call", return_value=self.cli_result()) as call:
                self.stop(code, d.update_once, "unused", V, lambda: None, True)
                call.assert_called_once()
                diagnostic = json.dumps(r.diagnostic(code))
                self.assertIn("unknown", diagnostic)
                self.assertIn("no replay", diagnostic)
                self.assertNotIn("SYNTHETIC-SECRET", diagnostic)

    def test_hostile_planning_and_cli_errors_remain_closed_without_replay(self):
        for raw in (b'{"error":"SYNTHETIC-SECRET"}', b"SYNTHETIC-SECRET"):
            with patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(d, "get", return_value=self.before), patch.object(d, "call") as call:
                build.return_value.open.return_value = io.BytesIO(raw)
                gate = Mock()
                self.stop("plan_rejected", d.update_once, "unused", V, gate, True)
                gate.assert_not_called()
                call.assert_not_called()
        for result in (TimeoutError("SYNTHETIC-SECRET"), subprocess.CompletedProcess(
                [], 1, b"SYNTHETIC-SECRET", b"SYNTHETIC-SECRET")):
            with patch.object(d, "plan_update"), patch.object(d, "get", return_value=self.before), \
                    patch.object(d, "call") as call:
                if isinstance(result, Exception):
                    call.side_effect = result
                else:
                    call.return_value = result
                self.stop("cli_uncertain", d.update_once, "unused", V, lambda: None, True)
                call.assert_called_once()
                self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic("cli_uncertain")))

    def test_first_binding_still_requires_stable_source_and_final_admission(self):
        for field, value in (("status", "updating"), ("update_tag", V), ("held", True),
                             ("debug", True), ("disable_cc_mode", True),
                             ("secrets", ["EXISTING_OPAQUE_REF"])):
            before = {**self.before, field: value}
            with self.subTest(field=field), patch.object(d, "get", return_value=before), \
                    patch.object(d, "plan_update") as planning, patch.object(d, "call") as call:
                gate = Mock()
                self.stop("not_stable", d.update_once, "unused", V, gate, True)
                planning.assert_not_called()
                gate.assert_not_called()
                call.assert_not_called()
        with patch.object(d, "get", return_value=self.before), patch.object(d, "plan_update") as planning, \
                patch.object(d, "call") as call:
            gate = Mock(side_effect=r.Stop("stale"))
            self.stop("stale", d.update_once, "unused", V, gate, True)
            planning.assert_called_once_with(self.before, V, first_tor_activation=True)
            gate.assert_called_once_with()
            call.assert_not_called()

    def test_protected_shipping_entry_explicitly_opts_in_after_existing_gates(self):
        events = []
        with patch.object(r, "valid_pins"), patch.object(d, "eligibility", side_effect=lambda *args: events.append("eligibility")), \
                patch.object(d, "approved", side_effect=lambda *args: events.append("approved")), \
                patch.object(r, "verify_public", side_effect=lambda *args: events.append("provenance")), \
                patch.object(d, "serving_tool"), patch.object(d, "install_cli", return_value="pinned-cli"), \
                patch.object(d, "update_once", return_value={}) as update, \
                patch.object(d, "verify_serving"), patch("builtins.print"):
            d.protected(S, R, V, D, "d" * 64, "7")
            self.assertEqual(events, ["eligibility", "approved", "provenance"])
            self.assertEqual(update.call_args.args[:2], ("pinned-cli", V))
            self.assertEqual(update.call_args.kwargs, {"first_tor_activation": True})
            update.call_args.args[2]()
            self.assertEqual(events[-2:], ["eligibility", "approved"])


class LatestReleasePolicyTest(StopTest):
    def setUp(self):
        environment = patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"})
        environment.start()
        self.addCleanup(environment.stop)

    def activation(self, flag=False):
        before, after, data = view(), view(V), plan()
        before["mark_latest_release"] = after["mark_latest_release"] = data["mark_latest_release"] = flag
        after["secrets"] += sorted(d.TOR_REFS)
        data.update(instance_id=before["id"], name=d.NAME)
        data["configuration_changes"]["secrets"] = {"added": sorted(d.TOR_REFS)}
        return before, after, data

    def invalid_flags(self, data, flipped=True):
        missing = copy.deepcopy(data)
        missing.pop("mark_latest_release")
        yield missing
        for value in (None, 0, 1, 0.0, 1.0, "false", "SYNTHETIC-SECRET", [], {}):
            yield {**data, "mark_latest_release": value}
        if flipped:
            yield {**data, "mark_latest_release": not data["mark_latest_release"]}

    def test_both_boolean_snapshots_are_carried_without_defaulting(self):
        for flag in (False, True):
            before, after, data = self.activation(flag)
            response = subprocess.CompletedProcess([], 0, b"opaque", json.dumps(data).encode())
            with self.subTest(flag=flag), patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(d, "get", side_effect=[before, after]), \
                    patch.object(d, "call", return_value=response) as call:
                build.return_value.open.return_value = io.BytesIO(json.dumps(data).encode())
                gate = Mock()
                result = d.update_once("unused", V, gate, True)
                sent = json.loads(build.return_value.open.call_args.args[0].data)
                self.assertIs(sent["mark_latest_release"], flag)
                self.assertIn("--mark-latest=" + str(flag).lower(), call.call_args.args[1])
                self.assertTrue(result["opaqueConfigurationPreserved"])
                self.assertFalse(result["servingVerified"])
                gate.assert_called_once_with()
                call.assert_called_once()

    def test_invalid_current_flags_never_plan_or_admit_an_update(self):
        before, _, _ = self.activation()
        for bad in self.invalid_flags(before, flipped=False):
            self.assertFalse(d.stable_running(bad, bad["current_tag"]))
            with patch.object(d, "get", return_value=bad), patch.object(d, "plan_update") as planning, \
                    patch.object(d, "call") as call:
                gate = Mock()
                self.stop("control_shape", d.update_once, "unused", V, gate, True)
                planning.assert_not_called()
                gate.assert_not_called()
                call.assert_not_called()
            with patch.object(d.urllib.request, "build_opener") as build:
                self.stop("plan_rejected", d.plan_update, bad, V, True)
                build.assert_not_called()

    def test_missing_untyped_or_lying_preplan_flags_never_admit(self):
        for flag in (False, True):
            before, _, data = self.activation(flag)
            for bad in self.invalid_flags(data):
                with patch.object(d.urllib.request, "build_opener") as build, \
                        patch.object(d, "get", return_value=before), patch.object(d, "call") as call:
                    build.return_value.open.return_value = io.BytesIO(json.dumps(bad).encode())
                    gate = Mock()
                    self.stop("plan_rejected", d.update_once, "unused", V, gate, True)
                    gate.assert_not_called()
                    call.assert_not_called()

    def test_each_postcall_plan_flag_is_independently_checked_without_replay(self):
        for flag in (False, True):
            before, _, data = self.activation(flag)
            for bad in self.invalid_flags(data):
                # A valid first CLI review must not hide an invalid second review.
                raw = json.dumps(data).encode() + b"\n" + json.dumps(bad).encode()
                response = subprocess.CompletedProcess([], 0, b"opaque", raw)
                with patch.object(d, "plan_update"), patch.object(d, "get", return_value=before) as get, \
                        patch.object(d, "call", return_value=response) as call:
                    gate = Mock()
                    self.stop("plan_configuration", d.update_once, "unused", V, gate, True)
                    gate.assert_called_once_with()
                    call.assert_called_once()
                    get.assert_called_once()
        diagnostic = json.dumps(r.diagnostic("plan_configuration"))
        self.assertIn("unknown", diagnostic)
        self.assertIn("no replay", diagnostic)
        self.assertNotIn("SYNTHETIC-SECRET", diagnostic)

    def test_actual_control_decoder_preserves_uncertain_completion_for_invalid_flags(self):
        for flag in (False, True):
            before, after, data = self.activation(flag)
            for bad in self.invalid_flags(after):
                responses = [subprocess.CompletedProcess([], 0, json.dumps(before).encode(), b""),
                             subprocess.CompletedProcess([], 0, b"opaque", json.dumps(data).encode()),
                             subprocess.CompletedProcess([], 0, json.dumps(bad).encode(), b"")]
                with patch.object(d, "plan_update"), patch.object(d, "call", side_effect=responses) as call:
                    self.stop("configuration_changed", d.update_once, "unused", V, Mock(), True)
                    self.assertEqual(call.call_count, 3)
                    self.assertEqual(sum(item.args[1][1] == "update" for item in call.call_args_list), 1)
        diagnostic = json.dumps(r.diagnostic("configuration_changed"))
        self.assertIn("unknown", diagnostic)
        self.assertIn("no replay", diagnostic)
        self.assertNotIn("SYNTHETIC-SECRET", diagnostic)

    def test_latest_policy_is_never_an_allowed_setting_change(self):
        before, _, data = self.activation()
        data["configuration_changes"]["settings"] = ["mark_latest_release"]
        with patch.object(d.urllib.request, "build_opener") as build, \
                patch.object(d, "get", return_value=before), patch.object(d, "call") as call:
            build.return_value.open.return_value = io.BytesIO(json.dumps(data).encode())
            gate = Mock()
            self.stop("plan_rejected", d.update_once, "unused", V, gate, True)
            gate.assert_not_called()
            call.assert_not_called()


class WorkflowTest(unittest.TestCase):
    def test_wiring_is_non_canceling_and_default_off(self):
        image = (ROOT / ".github/workflows/publish-image.yml").read_text()
        self.assertIn("needs: source-checks", image)
        self.assertIn("ref: ${{ needs.source-checks.outputs.source }}", image)
        self.assertIn("vars.RELEASE_AUTOMATION_ENABLED == 'true'", image)
        self.assertIn("${{ github.run_attempt }}-${{ needs.source-checks.outputs.source }}", image)
        self.assertNotIn("gh pr create", image)
        deployment = (ROOT / ".github/workflows/deploy-production.yml").read_text()
        preflight, production = deployment.split("  production:\n")
        self.assertNotIn("environment: production", preflight)
        self.assertNotIn("secrets.TINFOIL", preflight)
        self.assertIn("environment: production", production)
        self.assertIn("cancel-in-progress: false", production)
        self.assertNotIn("cancel-in-progress: true", image + deployment)
        self.assertNotIn("pending_deployments", image + deployment)
        self.assertIn("go" , preflight)
        self.assertIn("../scripts/verify-serving/main.go", production)


if __name__ == "__main__":
    unittest.main()
