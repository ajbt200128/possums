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
from unittest.mock import patch

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
                debug=False, disable_cc_mode=False, held=False,
                variables={"private": "SYNTHETIC-SECRET"}, secrets=sorted(d.REQUIRED_REFS),
                ssh_keys=[], cpus=2, gpus=0, memory_mb=8192)
    return data


def plan() -> dict:
    return {"read_only": True, "current": {"tag": "v1.2.2", "cpus": 2, "gpus": 0, "memory_mb": 8192},
            "target": {"tag": V, "cpus": 2, "gpus": 0, "memory_mb": 8192},
            "update_strategy": "blue_green", "downtime_required": False, "hold": False,
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

    def test_no_environment_secret_fallback(self):
        with patch.object(r, "api", side_effect=[environment(), branches(), {"total_count": 0, "secrets": []}]), \
                patch.object(r, "command", return_value=b'{"id":8}'):
            self.stop("credential", d.live_policy)

    def test_eligibility_before_and_after_approval(self):
        env = {"GITHUB_REPOSITORY": r.REPO, "GITHUB_REF": "refs/heads/main",
               "GITHUB_EVENT_NAME": "workflow_dispatch", "PRODUCTION_DEPLOYMENT_ENABLED": "true"}
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api", return_value={"value": "true"}), \
                patch.object(d, "live_policy", return_value="7"), patch.object(r, "latest", side_effect=[None, r.Stop("stale")]):
            self.assertEqual(d.eligibility(S), "7")
            self.stop("stale", d.eligibility, S, "7")
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api", return_value={"value": "false"}):
            self.stop("disabled", d.eligibility, S)

    def test_supplemental_reader_is_scoped_to_metadata_and_not_ambient_native_auth(self):
        env = {"GITHUB_REPOSITORY": r.REPO, "GITHUB_REF": "refs/heads/main",
               "GITHUB_EVENT_NAME": "workflow_dispatch", "PRODUCTION_DEPLOYMENT_ENABLED": "true",
               "GH_TOKEN": "synthetic-native", "GH_PRODUCTION_GATE_READ_TOKEN": "synthetic-reader"}
        def api(path, *, env=None):
            metadata = path.startswith("actions/variables/") or path.endswith("/secrets?per_page=100")
            if metadata:
                assert env is not None
                self.assertEqual(env["GH_TOKEN"], "synthetic-reader")
            else:
                self.assertIsNone(env)
            if path.startswith("actions/variables/"):
                return {"value": "true"}
            if path.endswith("/secrets?per_page=100"):
                return {"total_count": 1, "secrets": [{"name": "TINFOIL_PRODUCTION_ADMIN_KEY"}]}
            return branches() if "deployment-branch-policies" in path else environment()
        with patch.dict(os.environ, env, clear=True), patch.object(r, "api", side_effect=api), \
                patch.object(r, "command", return_value=b'{"id":8}'), patch.object(r, "latest"):
            self.assertEqual(d.eligibility(S), "7")
            self.assertEqual(os.environ["GH_TOKEN"], "synthetic-native")

    def test_metadata_failure_is_closed_and_preserves_only_known_status(self):
        for error in (r.Stop("command", 403), ValueError("SYNTHETIC-SECRET")):
            with patch.dict(os.environ, {"GH_PRODUCTION_GATE_READ_TOKEN": "synthetic-reader"}), \
                    patch.object(r, "api", side_effect=error):
                with self.assertRaises(r.Stop) as caught:
                    d.metadata_api("actions/variables/PRODUCTION_DEPLOYMENT_ENABLED")
                self.assertEqual(caught.exception.code, "metadata_unreadable")
                self.assertEqual(caught.exception.status, 403 if isinstance(error, r.Stop) else None)
                self.assertNotIn("SYNTHETIC-SECRET", json.dumps(r.diagnostic(caught.exception.code)))
        with patch.object(r, "api", side_effect=r.Stop("disabled")):
            self.stop("disabled", d.metadata_api, "synthetic")

    def test_api_reader_env_is_passed_only_to_the_supported_client(self):
        env = {"GH_TOKEN": "synthetic-reader"}
        with patch.object(r, "command", return_value=b'{"value":"true"}') as command:
            self.assertEqual(r.api("actions/variables/PRODUCTION_DEPLOYMENT_ENABLED", env=env), {"value": "true"})
            self.assertEqual(command.call_args.kwargs["env"], env)
            self.assertNotIn("synthetic-reader", json.dumps(command.call_args.args))

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
        with patch.dict(os.environ, {"RUNNER_TEMP": "/synthetic", "TINFOIL_ADMIN_KEY": "must-not-leak", "GH_TOKEN": "must-not-leak",
                                     "GH_PRODUCTION_GATE_READ_TOKEN": "must-not-leak"}), \
                patch.object(d, "serving_tool", return_value="synthetic-tool"), patch.object(r, "capture", return_value=good) as capture:
            d.verify_serving(V, "d" * 64)
            self.assertNotIn("TINFOIL_ADMIN_KEY", capture.call_args.kwargs["env"])
            self.assertNotIn("GH_TOKEN", capture.call_args.kwargs["env"])
            self.assertNotIn("GH_PRODUCTION_GATE_READ_TOKEN", capture.call_args.kwargs["env"])
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


class MutationTest(StopTest):
    def setUp(self):
        self.environment = patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"})
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def result(self):
        return subprocess.CompletedProcess([], 0, b"opaque", json.dumps(plan()).encode())

    def test_one_mutation_preserves_config_and_does_not_cancel_on_new_main(self):
        with patch.object(d, "get", side_effect=[view(), view(V)]), patch.object(d, "call", return_value=self.result()) as call:
            gates = []
            result = d.update_once("unused", V, lambda: gates.append("latest at admission"))
            self.assertEqual(len(gates), 1)  # No cancellation/recheck loop after admission.
            self.assertEqual(result["mutationCommandCount"], 1)
            self.assertFalse(result["servingVerified"])
            self.assertEqual(call.call_args.args[1], ["container", "update", d.NAME, "--tag", V, "--output", "json"])
            call.assert_called_once()

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

    def test_cli_closed_stdin_output_bound_and_admin_env_only(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"RUNNER_TEMP": directory,
                "TINFOIL_API_KEY": "must-not-inherit", "TINFOIL_CONTROLPLANE_URL": "must-not-inherit",
                "GH_PRODUCTION_GATE_READ_TOKEN": "must-not-inherit"}):
            program = "import os,sys; assert sys.stdin.read()==''; assert 'TINFOIL_API_KEY' not in os.environ; assert 'TINFOIL_CONTROLPLANE_URL' not in os.environ; assert 'GH_PRODUCTION_GATE_READ_TOKEN' not in os.environ; print('ok')"
            result = d.call(sys.executable, ["-c", program])
            self.assertEqual(result.returncode, 0)
            with patch.object(d, "LIMIT", 32):
                self.stop("cli_uncertain", d.call, sys.executable, ["-c", "print('x'*100)"])


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
        for job in (preflight, production):
            self.assertIn("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}", job)
            self.assertIn("GH_PRODUCTION_GATE_READ_TOKEN: ${{ secrets.PRODUCTION_GATE_READ_TOKEN }}", job)
        self.assertIn("cancel-in-progress: false", production)
        self.assertNotIn("cancel-in-progress: true", image + deployment)
        self.assertNotIn("pending_deployments", image + deployment)
        self.assertIn("go" , preflight)
        self.assertIn("../scripts/verify-serving/main.go", production)


if __name__ == "__main__":
    unittest.main()
