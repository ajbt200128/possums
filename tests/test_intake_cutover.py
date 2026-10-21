"""Offline synthetic contracts for the reviewed one-time intake reference cutover.

Privacy: PRIVACY.md mandatory typed failures and forbidden diagnostic data apply.
No secret values, live control plane, CLI mutation, or release tooling is used.
Operator custody is an assertion, not technical concurrency exclusion. The fresh
snapshot rejects already-stale plans; it is not a lock/CAS and cannot exclude a
writer racing after admission. Private provisioning remains operator-reported.
"""
import copy
import io
import json
import os
import subprocess
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import Mock, patch
from unittest.mock import call as mock_call

import test_release_automation as fixtures

r, d = fixtures.r, fixtures.d
V = fixtures.V
INTAKE = {"POSSUMS_INTAKE_ORIGIN", "POSSUMS_INTAKE_LEDGER_KEY"}
LEGACY = "POSSUMS_ACCOUNTS_JSON"
TOR = {"POSSUMS_TOR_IDENTITY_NATIVE_KEY", "POSSUMS_TOR_EXPECTED_ONION"}
SENTINEL = "SYNTHETIC-PRIVATE-CUTOVER"


def before():
    data = fixtures.view()
    data["secrets"] = sorted((d.REQUIRED_REFS - INTAKE) | TOR | {LEGACY, "OTHER_APPROVED_REFERENCE"})
    return data


def target(tag=V):
    data = before()
    data["secrets"] = sorted((set(data["secrets"]) - {LEGACY}) | INTAKE)
    data["current_tag"] = tag
    return data


def plan():
    data = fixtures.plan()
    data.update(instance_id=before()["id"], name=d.NAME)
    data["configuration_changes"]["secrets"] = {
        "added": sorted(INTAKE), "removed": [LEGACY], "changed": []}
    return data


def raw(data):
    return json.dumps(data).encode()


def cli_result(data=None):
    return subprocess.CompletedProcess([], 0, b"opaque", raw(plan() if data is None else data))


class CutoverTest(unittest.TestCase):
    def setUp(self):
        env = patch.dict(os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic",
                                      "INTAKE_CONFIGURATION_CUSTODY": "true"}, clear=True)
        env.start()
        self.addCleanup(env.stop)
        # Any accidentally unmocked I/O fails locally instead of reaching a service.
        for owner, name in ((d.urllib.request, "build_opener"), (r, "command"), (r, "capture"), (r, "api")):
            guard = patch.object(owner, name, side_effect=AssertionError("unmocked external I/O"))
            guard.start()
            self.addCleanup(guard.stop)

    def rejected(self, function, *args, **kwargs):
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr), self.assertRaises(r.Stop) as caught:
            function(*args, **kwargs)
        diagnostic = json.dumps(r.diagnostic(caught.exception.code, caught.exception.status))
        self.assertNotIn(SENTINEL, diagnostic + stdout.getvalue() + stderr.getvalue())
        self.assertNotIn("Traceback", diagnostic + stdout.getvalue() + stderr.getvalue())
        return caught.exception

    def inspect(self, data, snapshot=None):
        return d.inspect_plan(raw(data), "v1.2.2", V, d.DOMAIN,
                              current_resources=before() if snapshot is None else snapshot,
                              intake_cutover=True)

    def test_constants_and_sorted_reference_projection_without_mutation(self):
        self.assertEqual(d.INTAKE_REFS, INTAKE)
        self.assertEqual(d.LEGACY_REF, LEGACY)
        self.assertEqual(d.TOR_REFS, TOR)
        old = before()
        old["secrets"].reverse()
        saved = copy.deepcopy(old)
        self.assertEqual(d.cutover_references(old), target()["secrets"])
        self.assertEqual(old, saved)
        self.assertIn("OTHER_APPROVED_REFERENCE", d.cutover_references(old))
        self.assertFalse(d.stable_running(old, old["current_tag"]))
        self.assertTrue(d.stable_running(target(), V))

    def test_legacy_only_once_and_both_intake_refs_must_be_absent(self):
        cases = [target("v1.2.2")]
        for selected in ([], [LEGACY, *INTAKE], [LEGACY, *sorted(INTAKE)[:1]],
                         [LEGACY, *sorted(INTAKE)[1:]]):
            data = before()
            data["secrets"] = sorted((set(data["secrets"]) - {LEGACY}) | set(selected))
            cases.append(data)
        for data in cases:
            with self.subTest(refs=data["secrets"]):
                self.rejected(d.cutover_references, data)

    def test_missing_duplicate_and_malformed_reference_lists_fail_closed(self):
        for ref in sorted((d.REQUIRED_REFS - INTAKE) | TOR | {LEGACY}):
            data = before()
            data["secrets"].remove(ref)
            with self.subTest(missing=ref):
                self.rejected(d.cutover_references, data)
        for refs in (None, {}, "not-a-list", [LEGACY, None], [LEGACY, ""],
                     [*before()["secrets"], LEGACY], [*before()["secrets"], 1]):
            with self.subTest(refs=refs):
                self.rejected(d.cutover_references, {**before(), "secrets": refs})

    def test_canonical_domain_stable_confidential_running_state_required(self):
        mutations = {"domain": [d.OLD_DOMAIN, None, "hostile.invalid"],
                     "repo": ["hostile/repository"], "status": ["updating", None],
                     "update_tag": [V], "debug": [True, 0], "disable_cc_mode": [True, 0],
                     "held": [True], "mark_latest_release": [None, 0]}
        for field, values in mutations.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    self.rejected(d.cutover_references, {**before(), field: value})
        for field in d.STABLE:
            data = before()
            del data[field]
            with self.subTest(missing=field):
                self.rejected(d.cutover_references, data)

    def test_transition_input_is_exact_and_defaults_to_none(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertIs(d.reference_transition(), False)
        for value in ("none", "intake"):
            with patch.dict(os.environ, {"REFERENCE_TRANSITION": value}):
                self.assertIs(d.reference_transition(), value == "intake")
        for value in ("", "INTAKE", " intake", "intake\n", "None", SENTINEL):
            with patch.dict(os.environ, {"REFERENCE_TRANSITION": value}):
                error = self.rejected(d.reference_transition)
                self.assertEqual(error.code, "intake_cutover_input")

    def test_workflow_choice_is_explicit_default_and_automatic_dispatch_stays_ordinary(self):
        workflow = (fixtures.ROOT / ".github/workflows/deploy-production.yml").read_text()
        choice = workflow.split("      reference_transition:\n", 1)[1].split("\npermissions:", 1)[0]
        for expected in ("type: choice", "default: none", "- none", "- intake"):
            self.assertIn(expected, choice)
        custody = workflow.split("      exclusive_configuration_custody:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(custody.splitlines()[-3:],
                         ["        required: false", "        type: boolean", "        default: false"])
        self.assertIn("INTAKE_CONFIGURATION_CUSTODY: ${{ inputs.exclusive_configuration_custody && 'true' || 'false' }}", workflow)
        self.assertIn("REFERENCE_TRANSITION: ${{ inputs.reference_transition || 'none' }}", workflow)
        self.assertIn("inputs.reference_transition == 'intake' && ' [intake cutover]' || ''", workflow)
        dispatch = (fixtures.ROOT / "scripts/release.py").read_text().split("def dispatch_deploy(", 1)[1].split("def main(", 1)[0]
        self.assertNotIn("reference_transition", dispatch)
        self.assertNotIn("REFERENCE_TRANSITION", dispatch)
        self.assertNotIn("exclusive_configuration_custody", dispatch)
        self.assertNotIn("INTAKE_CONFIGURATION_CUSTODY", dispatch)
        ci = (fixtures.ROOT / ".github/workflows/check.yml").read_text()
        self.assertIn("python3 -m unittest discover -s tests -p 'test_intake_cutover.py'", ci)

    def test_custody_requires_exact_true_before_any_entrypoint_io(self):
        args = (fixtures.S, fixtures.R, V, fixtures.D, "d" * 64)
        for value in (None, "false", "", "True", "TRUE", "1", " true", "true\n", SENTINEL):
            for operation in ("reference_transition", "preflight", "protected", "update_once"):
                env = dict(os.environ, REFERENCE_TRANSITION="intake", GITHUB_SHA=fixtures.S)
                env.pop("INTAKE_CONFIGURATION_CUSTODY", None)
                if value is not None:
                    env["INTAKE_CONFIGURATION_CUSTODY"] = value
                with self.subTest(value=value, operation=operation), patch.dict(os.environ, env, clear=True), \
                        patch.object(d, "get") as get, patch.object(d, "plan_update") as planning, \
                        patch.object(d, "call") as call, patch.object(d, "eligibility") as eligibility, \
                        patch.object(d, "approved") as approved, patch.object(r, "verify_public") as verify, \
                        patch.object(d, "install_cli") as install:
                    gate = Mock()
                    if operation == "update_once":
                        error = self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
                    else:
                        supplied = () if operation == "reference_transition" else args
                        error = self.rejected(getattr(d, operation), *supplied,
                                              *(("7",) if operation == "protected" else ()))
                    self.assertEqual(error.code, "intake_cutover_custody")
                    for guarded in (get, planning, call, eligibility, approved, verify, install, gate):
                        guarded.assert_not_called()

    def test_ordinary_update_needs_no_custody_and_keeps_name_target(self):
        env = dict(os.environ, REFERENCE_TRANSITION="none")
        env.pop("INTAKE_CONFIGURATION_CUSTODY", None)
        with patch.dict(os.environ, env, clear=True), \
                patch.object(d, "get", side_effect=[target("v1.2.2"), target()]) as get, \
                patch.object(d, "plan_update") as planning, \
                patch.object(d, "call", return_value=cli_result(fixtures.plan())) as call:
            self.assertFalse(d.reference_transition())
            gate = Mock()
            result = d.update_once("unused", V, gate)
            gate.assert_called_once_with()
            planning.assert_called_once_with(target("v1.2.2"), V)
            self.assertEqual(get.call_args_list, [mock_call("unused"), mock_call("unused")])
            self.assertEqual(call.call_args.args[1], ["container", "update", d.NAME, "--tag", V,
                             "--custom-domain", d.DOMAIN, "--output", "json", "--mark-latest=false"])
            self.assertTrue(result["opaqueConfigurationPreserved"])

    def test_custody_does_not_bypass_protected_approval_or_final_gate(self):
        args = (fixtures.S, fixtures.R, V, fixtures.D, "d" * 64, "7")
        with patch.dict(os.environ, {"REFERENCE_TRANSITION": "intake", "GITHUB_SHA": fixtures.S}), \
                patch.object(d, "eligibility"), patch.object(d, "approved", side_effect=r.Stop("approval")) as approved, \
                patch.object(r, "verify_public") as verify, patch.object(d, "install_cli") as install, \
                patch.object(d, "update_once") as update:
            self.assertEqual(self.rejected(d.protected, *args).code, "approval")
            approved.assert_called_once_with("7")
            for guarded in (verify, install, update):
                guarded.assert_not_called()
        with patch.object(d, "get", side_effect=[before(), before()]), \
                patch.object(d, "plan_update"), patch.object(d, "call") as call:
            gate = Mock(side_effect=r.Stop("approval"))
            self.assertEqual(self.rejected(d.update_once, "unused", V, gate,
                                          intake_cutover=True).code, "approval")
            gate.assert_called_once_with()
            call.assert_not_called()

    def test_malformed_uuid_is_rejected_before_planning_or_admission(self):
        for identifier in (None, 1, "", d.NAME, "../gateway", SENTINEL,
                           "11111111-1111-1111-1111-11111111111", "a" * 32,
                           "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA"):
            snapshot = {**before(), "id": identifier}
            with self.subTest(identifier=identifier), patch.object(d, "get", return_value=snapshot) as get, \
                    patch.object(d, "plan_update") as planning, patch.object(d, "admit_cutover_snapshot") as admit, \
                    patch.object(d, "call") as call:
                gate = Mock()
                self.assertEqual(self.rejected(d.cutover_references, snapshot).code, "intake_cutover_state")
                self.assertEqual(self.rejected(d.update_once, "unused", V, gate,
                                              intake_cutover=True).code, "intake_cutover_state")
                get.assert_called_once_with("unused")
                for guarded in (planning, admit, call, gate):
                    guarded.assert_not_called()

    def test_fresh_snapshot_blocks_added_selection_before_gate_and_cli_planning(self):
        events = []
        stale = before()
        stale["secrets"].append("CONCURRENT_APPROVED_REFERENCE")

        def fetch(cli, identifier=d.NAME):
            events.append("initial" if identifier == d.NAME else "fresh")
            self.assertEqual(identifier, d.NAME if len(events) == 1 else before()["id"])
            return before() if len(events) == 1 else stale

        with patch.object(d, "get", side_effect=fetch) as get, \
                patch.object(d, "plan_update", side_effect=lambda *a, **k: events.append("wrapper-plan")), \
                patch.object(d, "call") as call:
            gate = Mock()
            error = self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
            self.assertEqual(error.code, "intake_cutover_state")
            self.assertEqual(events, ["initial", "wrapper-plan", "fresh"])
            self.assertEqual(get.call_count, 2)
            gate.assert_not_called()
            call.assert_not_called()  # CLI planning/execution has not started.

    def test_fresh_snapshot_rejects_all_other_captured_field_drift(self):
        for field in (*d.STABLE, "current_tag", "mark_latest_release", "status", "update_tag"):
            if field == "secrets":
                continue
            snapshot = before()
            snapshot[field] = (True if field == "mark_latest_release" else
                               "v1.2.1" if field == "current_tag" else SENTINEL)
            with self.subTest(field=field), patch.object(d, "get", side_effect=[before(), snapshot]), \
                    patch.object(d, "plan_update") as planning, patch.object(d, "call") as call:
                gate = Mock()
                self.assertEqual(self.rejected(d.update_once, "unused", V, gate,
                                              intake_cutover=True).code, "intake_cutover_state")
                planning.assert_called_once()
                gate.assert_not_called()
                call.assert_not_called()

    def test_actual_cli_uses_captured_id_after_initial_name_lookup(self):
        responses = [subprocess.CompletedProcess([], 0, raw(snapshot), b"")
                     for snapshot in (before(), before())]
        responses += [cli_result(), subprocess.CompletedProcess([], 0, raw(target()), b"")]
        with patch.object(d, "plan_update"), patch.object(d, "call", side_effect=responses) as call:
            d.update_once("unused", V, Mock(), intake_cutover=True)
            commands = [item.args[1] for item in call.call_args_list]
            self.assertEqual(commands[0], ["container", "get", d.NAME, "--output", "json"])
            self.assertEqual([command[1] for command in commands], ["get", "get", "update", "get"])
            self.assertTrue(all(command[2] == before()["id"] for command in commands[1:]))
            self.assertTrue(all(d.NAME not in command for command in commands[1:]))

    def test_actual_decoder_rejects_wrong_id_at_admission_and_readback(self):
        wrong = "22222222-2222-2222-2222-222222222222"
        for admission in (True, False):
            bad = {**(before() if admission else target()), "id": wrong}
            responses = [subprocess.CompletedProcess([], 0, raw(before()), b"")]
            if not admission:
                responses += [subprocess.CompletedProcess([], 0, raw(before()), b""), cli_result()]
            responses.append(subprocess.CompletedProcess([], 0, raw(bad), b""))
            with self.subTest(admission=admission), patch.object(d, "plan_update"), \
                    patch.object(d, "call", side_effect=responses) as call:
                gate = Mock()
                self.assertEqual(self.rejected(d.update_once, "unused", V, gate,
                                              intake_cutover=True).code, "control_shape")
                self.assertEqual(sum(item.args[1][1] == "update" for item in call.call_args_list),
                                 0 if admission else 1)
                if admission:
                    gate.assert_not_called()
                else:
                    gate.assert_called_once_with()

    def test_measured_config_binds_release_and_exact_single_secret_block(self):
        config = (fixtures.ROOT / "tinfoil-config.yml").read_bytes()
        with patch.object(r, "git", return_value=config) as git:
            d.cutover_config(fixtures.R)
            self.assertTrue(any(fixtures.R in str(arg) for call in git.call_args_list for arg in call.args))
        for ref in sorted(d.REQUIRED_REFS | TOR):
            changed = config.replace(("      - " + ref + "\n").encode(), b"")
            self.assertNotEqual(changed, config, ref)
            with self.subTest(missing=ref), patch.object(r, "git", return_value=changed):
                self.rejected(d.cutover_config, fixtures.R)
        for changed in (config + b"\n    secrets:\n      - OTHER_REFERENCE\n",
                        config.replace(b"    secrets:\n", b"    secrets:\n      - POSSUMS_ACCOUNTS_JSON\n"),
                        config.replace(b"    secrets:\n", b"    secrets:\n      - OTHER_REFERENCE\n"),
                        config.replace(b"    secrets:\n", b"    secrets:\n      - POSSUMS_INTAKE_ORIGIN\n")):
            with patch.object(r, "git", return_value=changed):
                self.rejected(d.cutover_config, fixtures.R)

    def test_exact_cutover_plan_and_order_independence(self):
        self.inspect(plan())
        data = plan()
        data["configuration_changes"]["secrets"]["added"].reverse()
        snapshot = before()
        snapshot["secrets"].reverse()
        self.inspect(data, snapshot)
        data["configuration_changes"]["secrets"]["changed"] = None
        self.inspect(data)
        self.rejected(d.inspect_plan, raw(plan()), "v1.2.2", V, intake_cutover=True)
        # The same intentional reference replacement is never ordinary no-drift.
        self.rejected(d.inspect_plan, raw(plan()), "v1.2.2", V, current_resources=before())
        d.inspect_plan(raw(fixtures.plan()), "v1.2.2", V, current_resources=target("v1.2.2"))

    def test_reference_plan_allowlist_rejects_mixed_missing_extra_and_duplicates(self):
        mutations = {"added": [[], None, sorted(INTAKE)[:1], [*sorted(INTAKE), SENTINEL],
                               [*sorted(INTAKE), sorted(INTAKE)[0]], [LEGACY], SENTINEL],
                     "removed": [[], None, [LEGACY, SENTINEL], [LEGACY, LEGACY], sorted(TOR)],
                     "changed": [[LEGACY], sorted(INTAKE), [SENTINEL], False, {}]}
        for kind, values in mutations.items():
            for value in values:
                data = plan()
                data["configuration_changes"]["secrets"][kind] = value
                with self.subTest(kind=kind, value=value):
                    self.rejected(self.inspect, data)
        data = plan()
        data["configuration_changes"]["secrets"]["unexpected"] = []
        self.rejected(self.inspect, data)

    def test_no_other_change_family_setting_or_resource_is_authorized(self):
        for family in ("variables", "ssh_keys"):
            for kind in ("added", "changed", "removed"):
                data = plan()
                data["configuration_changes"][family] = {kind: [SENTINEL]}
                self.rejected(self.inspect, data)
            data = plan()
            del data["configuration_changes"][family]
            self.rejected(self.inspect, data)
        for family, value in (("settings", ["custom_domain"]), ("settings", ["debug"]),
                              ("volumes", {}), ("unknown", {"changed": [SENTINEL]})):
            data = plan()
            data["configuration_changes"][family] = value
            self.rejected(self.inspect, data)
        for field in ("cpus", "gpus", "memory_mb"):
            data = plan()
            data["target"][field] += 1
            self.rejected(self.inspect, data)
            data["current"][field] += 1
            self.rejected(self.inspect, data)
        for field, value in (("hold", True), ("downtime_required", True), ("read_only", False),
                             ("mark_latest_release", True), ("update_strategy", "replace")):
            self.rejected(self.inspect, {**plan(), field: value})

    def test_refresh_metadata_is_only_unique_target_subset_or_null(self):
        for refs in (None, [], sorted(INTAKE), target()["secrets"]):
            data = plan()
            data["configuration_changes"]["secrets_refreshed"] = refs
            self.inspect(data)
        for refs in ([LEGACY], [SENTINEL], [*sorted(INTAKE), sorted(INTAKE)[0]], "not-a-list", {}):
            data = plan()
            data["configuration_changes"]["secrets_refreshed"] = refs
            self.rejected(self.inspect, data)

    def test_read_only_plan_sends_full_explicit_target_names_only_for_opt_in(self):
        with patch.object(d.urllib.request, "build_opener") as build:
            build.return_value.open.return_value = io.BytesIO(raw(plan()))
            d.plan_update(before(), V, intake_cutover=True)
            request = build.return_value.open.call_args.args[0]
            self.assertEqual(json.loads(request.data), {
                "tag": V, "custom_domain": d.DOMAIN, "mark_latest_release": False,
                "secrets": target()["secrets"]})
            self.assertEqual(request.get_method(), "POST")
            self.assertEqual(build.return_value.open.call_args.kwargs, {"timeout": 30})
            self.assertIsInstance(build.call_args.args[0], d.urllib.request.ProxyHandler)
            self.assertIsInstance(build.call_args.args[1], d.NoPlanRedirect)
            self.assertIsNone(d.NoPlanRedirect().redirect_request(None, None, None, None, None, None))
        with patch.object(d.urllib.request, "build_opener") as build:
            data = fixtures.plan()
            data.update(instance_id=before()["id"], name=d.NAME)
            build.return_value.open.return_value = io.BytesIO(raw(data))
            d.plan_update(target("v1.2.2"), V)
            self.assertNotIn("secrets", json.loads(build.return_value.open.call_args.args[0].data))

    def test_preplan_failures_block_gate_and_mutation_with_safe_cleanup(self):
        for failure in (OSError(SENTINEL), RuntimeError(SENTINEL)):
            with patch.object(d, "get", return_value=before()), patch.object(d, "call") as call, \
                    patch.object(d.urllib.request, "build_opener") as build:
                build.return_value.open.side_effect = failure
                gate = Mock()
                error = self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
                self.assertEqual(error.code, "plan_fetch")
                gate.assert_not_called()
                call.assert_not_called()
        for status in (302, 403, 500):
            failure = d.urllib.error.HTTPError("synthetic.invalid", status, SENTINEL, {}, None)
            with patch.object(d, "get", return_value=before()), patch.object(d, "call") as call, \
                    patch.object(d.urllib.request, "build_opener") as build, \
                    patch.object(failure, "close", side_effect=OSError(SENTINEL)):
                build.return_value.open.side_effect = failure
                gate = Mock()
                error = self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
                self.assertEqual((error.code, error.status), ("plan_http", status))
                gate.assert_not_called()
                call.assert_not_called()
            failure.close()

    def test_plan_before_final_gate_before_one_named_secret_mutation(self):
        events = []

        def planning(*args, **kwargs):
            events.append("plan")
            self.assertEqual(args, (before(), V))
            self.assertTrue(kwargs["intake_cutover"])

        def mutate(cli, args, **kwargs):
            events.append("mutation")
            expected = ["container", "update", before()["id"], "--tag", V,
                        "--custom-domain", d.DOMAIN, "--output", "json", "--mark-latest=false"]
            for ref in target()["secrets"]:
                expected.extend(["--secret", ref])
            self.assertEqual(args, expected)
            self.assertNotIn("--yes", args)
            self.assertNotIn(LEGACY, args)
            self.assertEqual(kwargs, {"timeout": 1800})
            return cli_result()

        with patch.object(d, "get", side_effect=[before(), before(), target()]) as get, \
                patch.object(d, "plan_update", side_effect=planning), patch.object(d, "call", side_effect=mutate) as call:
            result = d.update_once("unused", V, lambda: events.append("gate"), intake_cutover=True)
        self.assertEqual(events, ["plan", "gate", "mutation"])
        self.assertEqual(get.call_args_list, [mock_call("unused"),
                         mock_call("unused", before()["id"]),
                         mock_call("unused", before()["id"])])
        call.assert_called_once()
        self.assertTrue(result["exclusiveConfigurationCustodyAsserted"])
        self.assertTrue(result["approvedIntakeReferencesConfirmed"])
        self.assertTrue(result["targetRunningNoPending"])
        self.assertEqual(result["mutationCommandCount"], 1)
        self.assertIsNot(result.get("opaqueConfigurationPreserved"), True)
        self.assertFalse(result["servingVerified"])

    def test_second_cutover_and_bad_before_stop_before_plan_or_call(self):
        for snapshot in (target("v1.2.2"), {**before(), "domain": d.OLD_DOMAIN}):
            with patch.object(d, "get", return_value=snapshot), patch.object(d, "plan_update") as planning, \
                    patch.object(d, "call") as call:
                gate = Mock()
                self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
                planning.assert_not_called()
                gate.assert_not_called()
                call.assert_not_called()

    def test_readback_can_show_either_full_set_but_final_requires_target(self):
        old_pending = before()
        old_pending.update(update_tag=V, status="updating")
        new_pending = target("v1.2.2")
        new_pending.update(update_tag=V, status="updating")
        new_pending["secrets"].reverse()
        with patch.object(d, "get", side_effect=[before(), before(), old_pending, new_pending, target()]), \
                patch.object(d, "plan_update"), patch.object(d, "call", return_value=cli_result()) as call, \
                patch.object(d.time, "sleep") as sleep:
            result = d.update_once("unused", V, Mock(), intake_cutover=True)
            self.assertTrue(result["approvedIntakeReferencesConfirmed"])
            self.assertEqual(sleep.call_count, 2)
            call.assert_called_once()
        old_running = before()
        old_running["current_tag"] = V
        with patch.object(d, "get", side_effect=[before(), before(), old_running]), patch.object(d, "plan_update"), \
                patch.object(d, "call", return_value=cli_result()) as call, \
                patch.object(d.time, "monotonic", side_effect=[0, 0, d.COMPLETION_TIMEOUT_SECONDS + 1]), \
                patch.object(d.time, "sleep"):
            self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
            call.assert_called_once()

    def test_readback_never_tolerates_mixed_refs_or_other_stable_drift(self):
        cases = []
        for refs in ([*target()["secrets"], LEGACY], sorted(INTAKE), sorted(set(target()["secrets"]) - TOR),
                     [*target()["secrets"], target()["secrets"][0]]):
            cases.append({**target(), "secrets": refs})
        for field in d.STABLE:
            if field in ("secrets", "domain"):
                continue
            data = target()
            data[field] = {"private": SENTINEL} if field == "variables" else SENTINEL
            cases.append(data)
        cases.append({**target(), "domain": d.OLD_DOMAIN})
        for snapshot in cases:
            with patch.object(d, "get", side_effect=[before(), before(), snapshot]), patch.object(d, "plan_update"), \
                    patch.object(d, "call", return_value=cli_result()) as call:
                self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
                call.assert_called_once()

    def test_missing_readback_fields_are_rejected_by_actual_control_decoder(self):
        for field in d.STABLE:
            # These two omissions are normalized by the documented CLI decoder.
            if field in ("volume_slots", "volumes"):
                continue
            missing = target()
            del missing[field]
            responses = [subprocess.CompletedProcess([], 0, raw(before()), b""),
                         subprocess.CompletedProcess([], 0, raw(before()), b""), cli_result(),
                         subprocess.CompletedProcess([], 0, raw(missing), b"")]
            with self.subTest(field=field), patch.object(d, "plan_update"), \
                    patch.object(d, "call", side_effect=responses) as call:
                self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
                self.assertEqual(sum(c.args[1][1] == "update" for c in call.call_args_list), 1)

    def test_cutover_plan_identity_is_checked_before_and_after_mutation(self):
        for field, values in (("instance_id", (None, "22222222-2222-2222-2222-222222222222")),
                              ("name", (None, SENTINEL))):
            for value in values:
                bad = plan()
                if value is None:
                    del bad[field]
                else:
                    bad[field] = value
                with self.subTest(field=field, value=value):
                    self.rejected(self.inspect, bad)
                    with patch.object(d.urllib.request, "build_opener") as build, \
                            patch.object(d, "get", return_value=before()), patch.object(d, "call") as call:
                        build.return_value.open.return_value = io.BytesIO(raw(bad))
                        gate = Mock()
                        self.rejected(d.update_once, "unused", V, gate, intake_cutover=True)
                        gate.assert_not_called()
                        call.assert_not_called()
                    with patch.object(d, "get", return_value=before()) as get, \
                            patch.object(d, "plan_update"), patch.object(d, "call", return_value=cli_result(bad)) as call:
                        self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
                        self.assertEqual(get.call_count, 2)
                        call.assert_called_once()

    def test_uncertain_cli_and_each_postcall_plan_are_never_replayed(self):
        for failure in (TimeoutError(SENTINEL), RuntimeError(SENTINEL)):
            with patch.object(d, "get", return_value=before()), patch.object(d, "plan_update"), \
                    patch.object(d, "call", side_effect=failure) as call:
                error = self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
                self.assertEqual(error.code, "cli_uncertain")
                call.assert_called_once()
        bad = plan()
        bad["configuration_changes"]["variables"] = {"changed": [SENTINEL]}
        for payload in (b"not-json", raw(bad), raw(plan()) + b"\n" + raw(bad)):
            response = subprocess.CompletedProcess([], 0, b"opaque", payload)
            with patch.object(d, "get", return_value=before()) as get, patch.object(d, "plan_update"), \
                    patch.object(d, "call", return_value=response) as call:
                self.rejected(d.update_once, "unused", V, Mock(), intake_cutover=True)
                self.assertEqual(get.call_count, 2)
                call.assert_called_once()

    def test_entrypoints_bind_opt_in_to_verified_release_without_weakening_gates(self):
        args = (fixtures.S, fixtures.R, V, fixtures.D, "d" * 64)
        for operation in ("preflight", "protected"):
            for transition in ("none", "intake"):
                with self.subTest(operation=operation, transition=transition), \
                        patch.dict(os.environ, {"REFERENCE_TRANSITION": transition, "GITHUB_SHA": fixtures.S}), \
                        patch.object(d, "eligibility", return_value="7") as eligibility, \
                        patch.object(d, "approved") as approved, patch.object(r, "verify_public") as verify, \
                        patch.object(d, "cutover_config") as config, patch.object(d, "serving_tool"), \
                        patch.object(d, "install_cli", return_value="unused"), patch.object(d, "update_once", return_value={}) as update, \
                        patch.object(d, "verify_serving"), patch.object(r, "output"), patch("builtins.print"):
                    getattr(d, operation)(*args, *(("7",) if operation == "protected" else ()))
                    verify.assert_called_once_with(*args)
                    self.assertTrue(eligibility.called)
                    if transition == "intake":
                        config.assert_called_once_with(fixtures.R)
                    else:
                        config.assert_not_called()
                    if operation == "protected":
                        approved.assert_called_once_with("7")
                        self.assertEqual(update.call_args.kwargs.get("intake_cutover", False), transition == "intake")
                    else:
                        update.assert_not_called()

    def test_opt_in_requires_workflow_source_sha_before_any_mutation(self):
        args = (fixtures.S, fixtures.R, V, fixtures.D, "d" * 64)
        for operation in ("preflight", "protected"):
            for sha in (None, fixtures.R, SENTINEL):
                env = {"REFERENCE_TRANSITION": "intake", "GITHUB_RUN_ATTEMPT": "1",
                       "INTAKE_CONFIGURATION_CUSTODY": "true"}
                if sha is not None:
                    env["GITHUB_SHA"] = sha
                with self.subTest(operation=operation, sha=sha), patch.dict(os.environ, env, clear=True), \
                        patch.object(d, "eligibility", return_value="7"), patch.object(d, "approved"), \
                        patch.object(r, "verify_public"), patch.object(d, "cutover_config"), \
                        patch.object(d, "serving_tool"), patch.object(d, "install_cli") as install, \
                        patch.object(d, "update_once") as update, patch.object(r, "output") as output:
                    self.rejected(getattr(d, operation), *args,
                                  *(("7",) if operation == "protected" else ()))
                    install.assert_not_called()
                    update.assert_not_called()
                    output.assert_not_called()


if __name__ == "__main__":
    unittest.main()
