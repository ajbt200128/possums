"""Fail-closed projection tests: public sentinels, no Docker or inference."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import image_tor as probe


class TorImageProjectionTests(unittest.TestCase):
    def test_positive_and_real_database_negative_are_distinct(self):
        self.assertTrue(probe.result_matches(0, probe.POSITIVE, b"", False))
        self.assertTrue(probe.result_matches(2, probe.NEGATIVE, b"", True))
        self.assertFalse(probe.result_matches(0, probe.POSITIVE, b"", True))
        self.assertFalse(probe.result_matches(2, probe.NEGATIVE, b"", False))

    def test_every_positive_stage_and_exit_are_required(self):
        for line in probe.POSITIVE.splitlines(keepends=True):
            self.assertFalse(probe.result_matches(0, probe.POSITIVE.replace(line, b""), b"", False))
        for code in (1, 2, 137):
            self.assertFalse(probe.result_matches(code, probe.POSITIVE, b"", False))

    def test_negative_requires_typed_io_error_and_sdk_default_builder(self):
        for line in probe.NEGATIVE.splitlines(keepends=True):
            self.assertFalse(probe.result_matches(2, probe.NEGATIVE.replace(line, b""), b"", True))
        for code in (0, 1, 137):
            self.assertFalse(probe.result_matches(code, probe.NEGATIVE, b"", True))
        for code in (b"configuration_panic", b"client_panic", b"cause_unknown"):
            self.assertFalse(probe.result_matches(2, probe.NEGATIVE.replace(b"sdk_builder_panic", code), b"", True))
        for code in (b"absent", b"records_unmatched", b"present", b"cause_unknown"):
            self.assertFalse(probe.result_matches(2, probe.NEGATIVE.replace(b"io_permission_denied", code), b"", True))

    def test_known_wrong_fixtures_remain_failures_with_closed_diagnostics(self):
        for output, stage in probe.NEGATIVE_FAILURES.items():
            self.assertFalse(probe.result_matches(2, output, b"", True))
            self.assertEqual(probe.projection_failure(2, output, b"", True), stage)
            for code, out, err in ((0, output, b""), (2, output + b"hostile", b""),
                                   (2, output, b"hostile"), (2, output[:-1], b"")):
                self.assertEqual(probe.projection_failure(code, out, err, True),
                                 "tor_negative_output_cause_unknown")
        self.assertEqual(probe.projection_failure(0, b"hostile", b"", False),
                         "tor_positive_output_cause_unknown")

    def test_mode_zero_fixture_and_same_public_identity_for_both_runs(self):
        identity = b"sha256:" + b"a" * 64
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "probe"
            binary.touch()
            observed = []

            def inspect_fixture(image, artifact, fixture, negative):
                self.assertEqual(image, identity.decode("ascii"))
                self.assertEqual(artifact, binary.resolve())
                self.assertEqual(fixture.stat().st_mode & 0o777, 0)
                observed.append(negative)
                self.assertEqual(emit.call_args_list[0].args,
                                 ("stage=image_identity code=" + image,))
                self.assertEqual(emit.call_args_list[1].args,
                                 ("stage=image_architecture code=linux_amd64",))

            with patch.object(probe, "load"), patch.object(probe, "checked", side_effect=[
                    identity + b"\n", b"linux/amd64\n"]), patch.object(
                    probe, "probe", side_effect=inspect_fixture), patch("builtins.print") as emit:
                probe.qualify("public-archive", binary)
            self.assertEqual(observed, [False, True])

    def test_public_identity_is_available_before_probe_failure(self):
        identity = b"sha256:" + b"b" * 64
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "probe"
            binary.touch()
            with patch.object(probe, "load"), patch.object(probe, "checked", side_effect=[
                    identity, b"linux/amd64"]), patch.object(
                    probe, "probe", side_effect=probe.QualificationError("tor_negative_fixture_no_match")), \
                    patch("builtins.print") as emit:
                self.assertEqual(probe.main(["probe", "public-archive", str(binary)]), 1)
            self.assertEqual(emit.call_args_list[0].args,
                             ("stage=image_identity code=" + identity.decode("ascii"),))

    def test_hostile_output_is_not_accepted_or_echoed(self):
        hostile = b"PUBLIC_HOSTILE_SDK credential=synthetic-only"
        for negative, output, code in ((False, probe.POSITIVE, 0), (True, probe.NEGATIVE, 2)):
            self.assertFalse(probe.result_matches(code, output + hostile, b"", negative))
            self.assertFalse(probe.result_matches(code, output, hostile, negative))
            stage = probe.projection_failure(code, output + hostile, b"", negative)
            self.assertNotIn("PUBLIC_HOSTILE", stage)
            self.assertTrue(stage.endswith("output_cause_unknown"))
        with patch.object(probe, "qualify", side_effect=RuntimeError(hostile)), patch("builtins.print") as emit:
            self.assertEqual(probe.main(["probe", "archive", "binary"]), 1)
            self.assertNotIn("PUBLIC_HOSTILE", str(emit.call_args))
            self.assertIn("stage=qualification cause=unknown", str(emit.call_args))


if __name__ == "__main__":
    unittest.main()
