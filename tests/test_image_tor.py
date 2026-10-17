"""Fail-closed projection tests: public sentinels, no Docker or inference."""
import unittest
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

    def test_negative_cannot_skip_sdk_default_builder(self):
        self.assertFalse(probe.result_matches(2, b"stage=nss code=absent\n", b"", True))
        for code in (b"configuration_panic", b"client_panic", b"cause_unknown"):
            self.assertFalse(probe.result_matches(2, probe.NEGATIVE.replace(b"sdk_builder_panic", code), b"", True))

    def test_hostile_output_is_not_accepted_or_echoed(self):
        hostile = b"PUBLIC_HOSTILE_SDK credential=synthetic-only"
        for negative, output, code in ((False, probe.POSITIVE, 0), (True, probe.NEGATIVE, 2)):
            self.assertFalse(probe.result_matches(code, output + hostile, b"", negative))
            self.assertFalse(probe.result_matches(code, output, hostile, negative))
        with patch.object(probe, "qualify", side_effect=RuntimeError(hostile)), patch("builtins.print") as emit:
            self.assertEqual(probe.main(["probe", "archive", "binary"]), 1)
            self.assertNotIn("PUBLIC_HOSTILE", str(emit.call_args))
            self.assertIn("stage=qualification cause=unknown", str(emit.call_args))


if __name__ == "__main__":
    unittest.main()
