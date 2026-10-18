"""Offline release/activation isolation guards; no secrets, network or deployment."""
import copy
import json
import subprocess
import unittest
from unittest.mock import Mock, patch

from test_release_automation import ROOT, S, R, D, V, StopTest, d, plan, r, view


class TorRestorationReleaseTest(StopTest):
    def setUp(self):
        environment = patch.dict(d.os.environ, {"GITHUB_RUN_ATTEMPT": "1", "TINFOIL_ADMIN_KEY": "synthetic"})
        environment.start()
        self.addCleanup(environment.stop)

    def result(self, data=None):
        return subprocess.CompletedProcess([], 0, b"opaque", json.dumps(plan() if data is None else data).encode())

    def test_deployment_lookup_targets_recreated_gateway(self):
        self.assertEqual(d.NAME, "gateway")
        response = subprocess.CompletedProcess([], 0, json.dumps(view()).encode(), b"")
        with patch.object(d, "call", return_value=response) as call:
            self.assertEqual(d.get("unused")["name"], "gateway")
        call.assert_called_once_with("unused", ["container", "get", "gateway", "--output", "json"])

    def test_opaque_secret_references_are_preserved_never_selected_or_deleted(self):
        before, after = view(), view(V)
        before["secrets"].append("EXISTING_OPAQUE_REF")
        after["secrets"] = copy.deepcopy(before["secrets"])
        with patch.object(d, "plan_update"), patch.object(d, "get", side_effect=[before, after]), \
                patch.object(d, "call", return_value=self.result()) as call:
            result = d.update_once("unused", V, Mock())
            self.assertTrue(result["opaqueConfigurationPreserved"])
            self.assertNotIn("--secret", call.call_args.args[1])
            call.assert_called_once()

    def test_tor_bound_update_requires_separate_exclusive_publication_qualification(self):
        for references in (["POSSUMS_TOR_IDENTITY_NATIVE_KEY"], ["POSSUMS_TOR_EXPECTED_ONION"], sorted(d.TOR_REFS)):
            before = view()
            before["secrets"].extend(references)
            with patch.object(d, "get", return_value=before), patch.object(d, "plan_update") as planning, \
                    patch.object(d, "call") as call:
                self.stop("tor_exclusive_publication", d.update_once, "unused", V, Mock())
                planning.assert_not_called()
                call.assert_not_called()

    def test_pending_current_waits_without_extending_deadline(self):
        pending = {**view(), "update_tag": V, "update_status": "deploying"}
        with patch.object(d, "plan_update"), patch.object(d, "get", side_effect=[view(), pending]), \
                patch.object(d, "call", return_value=self.result()) as call, \
                patch.object(d.time, "monotonic", side_effect=[0, 1, 301]), patch.object(d.time, "sleep") as sleep:
            self.stop("completion_timeout", d.update_once, "unused", V, Mock())
            call.assert_called_once()
            sleep.assert_called_once_with(10)

    def test_failed_candidate_keeps_closed_diagnostic_without_replay(self):
        pending = {**view(), "update_tag": V, "update_status": "failed"}
        with patch.object(d, "plan_update"), patch.object(d, "get", side_effect=[view(), pending]), \
                patch.object(d, "call", return_value=self.result()) as call, patch.object(d.time, "sleep") as sleep:
            self.stop("candidate_failed", d.update_once, "unused", V, Mock())
            call.assert_called_once()
            sleep.assert_not_called()
        diagnostic = r.diagnostic("candidate_failed")
        for field in ("initiating cause", "unknown", "no replay"):
            self.assertIn(field, diagnostic["action"])

    def test_every_configuration_change_is_rejected_in_each_cli_plan(self):
        for family in ("variables", "secrets", "ssh_keys"):
            for kind in ("added", "changed", "removed", "unknown"):
                data = plan()
                data["configuration_changes"][family] = {kind: ["SYNTHETIC-SECRET"]}
                self.stop("plan_configuration", d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)
                self.stop("plan_configuration", d.inspect_plan,
                          json.dumps(plan()).encode() + b"\n" + json.dumps(data).encode(), "v1.2.2", V)
        for field in ("cpus", "gpus", "memory_mb"):
            data = plan()
            data["target"][field] += 1
            self.stop("plan_configuration", d.inspect_plan, json.dumps(data).encode(), "v1.2.2", V)

    def test_protected_entry_keeps_approval_provenance_and_final_gate(self):
        events = []
        with patch.object(r, "valid_pins"), patch.object(d, "eligibility", side_effect=lambda *args: events.append("eligibility")), \
                patch.object(d, "approved", side_effect=lambda *args: events.append("approved")), \
                patch.object(r, "verify_public", side_effect=lambda *args: events.append("provenance")), \
                patch.object(d, "serving_tool"), patch.object(d, "install_cli", return_value="pinned-cli"), \
                patch.object(d, "update_once", return_value={}) as update, patch.object(d, "verify_serving"), patch("builtins.print"):
            d.protected(S, R, V, D, "d" * 64, "7")
            self.assertEqual(events, ["eligibility", "approved", "provenance"])
            self.assertEqual(update.call_args.args[:2], ("pinned-cli", V))
            self.assertEqual(update.call_args.kwargs, {})
            update.call_args.args[2]()
            self.assertEqual(events[-2:], ["eligibility", "approved"])

    def test_measured_configuration_enables_tor_with_fixed_grant_and_identity_references(self):
        config = (ROOT / "tinfoil-config.yml").read_text()
        self.assertEqual(config.count('      - POSSUMS_TOR_ENABLED: "true"\n'), 1)
        self.assertIn('attested-keys:\n  - id: possums-tor-endorsement-v1\n'
                      '    key: ed25519\n    uid: 65532\n    gid: 65532\n', config)
        self.assertEqual(config.count('    keys: [possums-tor-endorsement-v1]\n'), 1)
        for reference in ("POSSUMS_TOR_IDENTITY_NATIVE_KEY", "POSSUMS_TOR_EXPECTED_ONION"):
            self.assertEqual(config.count('      - ' + reference + '\n'), 1)
        self.assertNotIn('POSSUMS_TOR_IDENTITY_DIRECTORY', config)
        self.assertNotIn('OTEL_SDK_DISABLED', config)
        self.assertIn('      start_period: 300s\n', config)
        self.assertIn('    egress: open\n', config)
        for required in ("attestation: true", "read_only: true", 'size=512m,mode=1777'):
            self.assertIn(required, config)
        deploy = (ROOT / "scripts/deploy.py").read_text()
        self.assertNotIn("first_tor_activation", deploy)
        self.assertEqual(d.TOR_REFS, {"POSSUMS_TOR_IDENTITY_NATIVE_KEY", "POSSUMS_TOR_EXPECTED_ONION"})
        self.assertIn('torCommon = common // { cargoExtraArgs = "--locked --features tor"; };',
                      (ROOT / "flake.nix").read_text())
        self.assertIn('default = []', (ROOT / "Cargo.toml").read_text())
        self.assertIn('tor-image-probe', (ROOT / ".github/workflows/check.yml").read_text())
        self.assertIn('User = "65532:65532"', (ROOT / "flake.nix").read_text())
        self.assertIn('gatewayAccounts', (ROOT / "flake.nix").read_text())

    def test_pr90_readiness_modules_hooks_and_direct_libc_are_removed(self):
        for name in ("src/telemetry/startup/readiness.rs", "src/telemetry/startup/readiness",
                     "src/tor_ingress/readiness_tests.rs", "src/tor_ingress/readiness_tests"):
            self.assertFalse((ROOT / name).exists(), name)
        for name in ("src/main.rs", "src/telemetry/startup.rs"):
            source = (ROOT / name).read_text()
            for removed in ("startup_readiness", "readiness::", "mod readiness", "libc::"):
                self.assertNotIn(removed, source)
        self.assertNotRegex((ROOT / "Cargo.toml").read_text(), r"(?m)^libc\s*=")


if __name__ == "__main__":
    unittest.main()
