"""Synthetic executable-inspection boundaries; no target execution or network."""
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("tor_native_portability", ROOT / "scripts/tor_native_portability.py")
assert spec is not None and spec.loader is not None
portability = importlib.util.module_from_spec(spec)
spec.loader.exec_module(portability)

ELF = """ELF Header:
  Class: ELF64
  Data: 2's complement, little endian
  Type: DYN (Position-Independent Executable file)
  Machine: AArch64
Program Headers:
  Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align
  LOAD 0 0 0 0 0 R 0
Dynamic section at offset 0 contains entries:
  0x0000000000000000 (NULL) 0x0
"""


def macho(*extra, floor="14.0", platform="1", library="/usr/lib/libSystem.B.dylib"):
    blocks = ["cmd LC_LOAD_DYLINKER\nname /usr/lib/dyld (offset 12)",
              f"cmd LC_BUILD_VERSION\nplatform {platform}\nminos {floor}\nsdk 26.4",
              f"cmd LC_LOAD_DYLIB\nname {library} (offset 24)", *extra]
    return "fixture:\n" + "\n".join(f"Load command {i}\n{block}" for i, block in enumerate(blocks))


class NativePortabilityTests(unittest.TestCase):
    def test_static_elf_both_architectures_is_inspection_not_execution(self):
        for arch in ["AArch64", "Advanced Micro Devices X86-64"]:
            result = portability.elf_linkage(ELF.replace("AArch64", arch), arch)
            self.assertEqual(result, {"format": "elf64", "linkage": "static", "execution": "not_tested"})

    def test_elf_rejects_dynamic_loader_dependencies_and_search_paths(self):
        for extra in ["  INTERP 0 0 0 0 0 R 0", " (NEEDED) Shared library: [libc.so]",
                      " (RPATH) /builder/runtime", " (RUNPATH) /builder/runtime", "/nix/store/hostile-canary"]:
            with self.subTest(extra=extra), self.assertRaisesRegex(portability.PortabilityFailure, "possums_build_tor_linkage") as failure:
                portability.elf_linkage(ELF + extra, "AArch64")
            self.assertNotIn("hostile-canary", str(failure.exception))
            self.assertEqual(failure.exception.code, "possums_build_tor_linkage")

    def test_elf_rejects_wrong_architecture_class_endianness_type_and_missing_headers(self):
        for text in [ELF.replace("AArch64", "wrong"), ELF.replace("ELF64", "ELF32"),
                     ELF.replace("little endian", "big endian"), ELF.replace("DYN", "REL"),
                     ELF.replace("Program Headers:", ""), "hostile-canary"]:
            with self.subTest(text=text), self.assertRaisesRegex(portability.PortabilityFailure, "possums_build_tor_format") as failure:
                portability.elf_linkage(text, "AArch64")
            self.assertEqual(failure.exception.code, "possums_build_tor_format")

    def test_macho_accepts_only_platform_libraries_and_records_floor(self):
        result = portability.macho_linkage(macho())
        self.assertEqual(result, {"format": "macho64", "linkage": "platform_only", "deploymentTarget": "14.0", "execution": "not_tested"})
        portability.macho_linkage(macho(library="/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation"))

    def test_macho_rejects_nonplatform_library_paths(self):
        for path in ["/nix/store/hostile-canary/lib.dylib", "@rpath/lib.dylib", "@loader_path/lib.dylib",
                     "/builder/lib.dylib", "/usr/lib/../../builder/lib.dylib"]:
            with self.subTest(path=path), self.assertRaisesRegex(portability.PortabilityFailure, "possums_build_tor_linkage") as failure:
                portability.macho_linkage(macho(library=path))
            self.assertNotIn("hostile-canary", str(failure.exception))

    def test_macho_rejects_search_paths_environment_and_alternate_loaders(self):
        for extra in ["cmd LC_RPATH\npath /builder (offset 12)", "cmd LC_DYLD_ENVIRONMENT\nname hostile-canary (offset 12)",
                      "cmd LC_LOAD_DYLINKER\nname /usr/lib/other (offset 12)",
                      "cmd LC_LOAD_WEAK_DYLIB\nname /builder/lib.dylib (offset 24)",
                      "cmd LC_LAZY_LOAD_DYLIB\nname @executable_path/lib.dylib (offset 24)"]:
            with self.subTest(extra=extra), self.assertRaises(portability.PortabilityFailure):
                portability.macho_linkage(macho(extra))

    def test_macho_rejects_unestablished_or_different_deployment_floor(self):
        for text in [macho(floor="15.0"), macho(platform="2"), macho("cmd LC_BUILD_VERSION\nplatform 1\nminos 14.0"),
                     macho("cmd LC_VERSION_MIN_MACOSX\nversion 14.0"), macho().replace("cmd LC_LOAD_DYLINKER", "cmd LC_UUID"), "hostile-canary"]:
            with self.subTest(text=text), self.assertRaises(portability.PortabilityFailure):
                portability.macho_linkage(text)

    def test_unknown_target_is_rejected_before_file_or_tool_access(self):
        with patch.object(portability, "tool") as tool, self.assertRaises(portability.PortabilityFailure):
            portability.inspect("../hostile-canary", Path("missing"))
        tool.assert_not_called()

    def test_file_mode_symlink_and_magic_rejections_do_not_run_tools(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(portability, "tool") as tool:
            artifact = Path(tmp) / "helper"
            artifact.write_bytes(b"not-an-executable")
            artifact.chmod(0o644)
            with self.assertRaises(portability.PortabilityFailure):
                portability.inspect("aarch64-apple-darwin", artifact)
            artifact.chmod(0o755)
            with self.assertRaises(portability.PortabilityFailure):
                portability.inspect("aarch64-apple-darwin", artifact)
            link = Path(tmp) / "symlink"
            link.symlink_to(artifact)
            with self.assertRaises(portability.PortabilityFailure):
                portability.inspect("aarch64-apple-darwin", link)
            tool.assert_not_called()

    def test_macho_wrong_architecture_and_nonexecutable_filetype_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifact = Path(tmp) / "helper"
            artifact.write_bytes(b"\xcf\xfa\xed\xfe")
            artifact.chmod(0o755)
            with patch.object(portability, "tool", return_value="x86_64"), self.assertRaises(portability.PortabilityFailure):
                portability.inspect("aarch64-apple-darwin", artifact)
            with patch.object(portability, "tool", side_effect=["arm64", "MH_MAGIC_64 ARM64 DYLIB"]), self.assertRaises(portability.PortabilityFailure):
                portability.inspect("aarch64-apple-darwin", artifact)
            with patch.object(portability, "tool", side_effect=["arm64", "MH_MAGIC_64 ARM64 EXECUTE", macho()]):
                self.assertEqual(portability.inspect("aarch64-apple-darwin", artifact)["execution"], "not_tested")

    def test_actual_cli_with_missing_tools_has_safe_content_free_diagnostic(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifact = Path(tmp) / "helper"
            artifact.write_bytes(b"\x7fELF")
            artifact.chmod(0o755)
            result = subprocess.run([sys.executable, str(ROOT / "scripts/tor_native_portability.py"),
                                     "aarch64-unknown-linux-musl", str(artifact)], env={"PATH": tmp}, capture_output=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stderr, b"possums_build_tor_inspection_tool: executable inspection could not complete; cause unknown. Provide the platform readelf or Apple lipo/otool tools. Do not accept the artifact without inspection.\n")
        self.assertNotIn(b"Traceback", result.stderr)
        self.assertEqual(result.stdout, b"")

    def test_tool_errors_cannot_export_hostile_output(self):
        result = subprocess.CompletedProcess(["fixture"], 1, b"hostile-canary", b"hostile-canary")
        with patch.object(subprocess, "run", return_value=result), self.assertRaises(portability.PortabilityFailure) as failure:
            portability.tool(["fixture"])
        self.assertNotIn("hostile-canary", str(failure.exception))
        self.assertEqual(failure.exception.code, "possums_build_tor_inspection_tool")


class NativePortabilityCLITests(unittest.TestCase):
    def invoke(self, argv):
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = portability.main(argv)
        return status, stdout.getvalue(), stderr.getvalue()

    def test_success_envelope_preserves_inspection_and_legacy_projection(self):
        for target, magic, outputs, evidence in [
            ("aarch64-unknown-linux-musl", b"\x7fELF", [ELF],
             {"format": "elf64", "linkage": "static", "execution": "not_tested"}),
            ("aarch64-apple-darwin", b"\xcf\xfa\xed\xfe", ["arm64", "MH_MAGIC_64 ARM64 EXECUTE", macho()],
             {"format": "macho64", "linkage": "platform_only", "deploymentTarget": "14.0", "execution": "not_tested"}),
        ]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as tmp:
                artifact = Path(tmp) / "hostile-canary"
                artifact.write_bytes(magic)
                artifact.chmod(0o755)
                for flags in [[], ["--json"]]:
                    with patch.object(portability, "tool", side_effect=outputs):
                        status, stdout, stderr = self.invoke([*flags, target, str(artifact)])
                    expected = {"ok": True, "evidence": evidence} if flags else evidence
                    self.assertEqual((status, stdout, stderr), (0, json.dumps(expected) + "\n", ""))

    def test_json_carries_only_explicit_code_without_formatting_message(self):
        class UnformattableFailure(portability.PortabilityFailure):
            def __str__(self):
                raise AssertionError("hostile-canary must not be formatted")

        expected_codes = {
            "possums_build_tor_inspection_tool", "possums_build_tor_format",
            "possums_build_tor_linkage", "possums_build_tor_deployment",
            "possums_build_tor_inspection_target", "possums_build_tor_artifact",
            "possums_build_tor_inspection_input", "possums_build_tor_inspection_unknown",
        }
        self.assertEqual(portability.FAILURE_CODES, expected_codes)
        for code in expected_codes:
            with self.subTest(code=code), patch.object(portability, "inspect", side_effect=UnformattableFailure(
                    code, "possums_build_tor_format: hostile-canary /private/path https://private.invalid")) as inspect:
                status, stdout, stderr = self.invoke(["--json", "target", "artifact"])
            self.assertEqual((status, stdout, stderr), (1, json.dumps({"ok": False, "code": code}) + "\n", ""))
            inspect.assert_called_once_with("target", Path("artifact"))

    def test_invalid_codes_fall_back_without_parsing_message_or_exception(self):
        for code in ["hostile-canary", "possums_build_tor_format: hostile-canary", None, ["hostile-canary"]]:
            with self.subTest(code=code):
                failure = portability.PortabilityFailure(code, "possums_build_tor_linkage: hostile-canary")
                self.assertEqual(failure.code, "possums_build_tor_inspection_unknown")
                self.assertEqual(str(failure), portability.UNKNOWN_MESSAGE)
                with patch.object(portability, "inspect", side_effect=failure):
                    self.assertEqual(self.invoke(["--json", "target", "artifact"]),
                                     (1, '{"ok": false, "code": "possums_build_tor_inspection_unknown"}\n', ""))

    def test_legacy_message_only_failure_has_unknown_code_not_inferred_prefix(self):
        failure = portability.PortabilityFailure("possums_build_tor_format: hostile-canary")
        self.assertEqual(failure.code, "possums_build_tor_inspection_unknown")
        self.assertEqual(str(failure), portability.UNKNOWN_MESSAGE)

    def test_unknown_hostile_fault_is_sanitized_in_both_modes_without_retry(self):
        class HostileError(Exception):
            def __str__(self):
                raise AssertionError("hostile-canary must not be formatted")

        for error in [RuntimeError("possums_build_tor_linkage: hostile-canary /private/path"), HostileError()]:
            for flags in [[], ["--json"]]:
                with self.subTest(error_type=type(error), flags=flags), patch.object(portability, "inspect", side_effect=error) as inspect:
                    result = self.invoke([*flags, "target", "artifact"])
                expected = (1, '{"ok": false, "code": "possums_build_tor_inspection_unknown"}\n', "") if flags else (
                    1, "", portability.UNKNOWN_MESSAGE + "\n")
                self.assertEqual(result, expected)
                inspect.assert_called_once()

    def test_json_real_rejections_keep_their_codes(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifact = Path(tmp) / "hostile-canary"
            for target, content, mode, output, code in [
                ("unknown", b"", 0o755, None, "inspection_target"),
                ("aarch64-unknown-linux-musl", b"\x7fELF", 0o644, None, "artifact"),
                ("aarch64-unknown-linux-musl", b"bad!", 0o755, None, "format"),
                ("aarch64-unknown-linux-musl", b"\x7fELF", 0o755, [ELF + " (NEEDED) hostile-canary"], "linkage"),
                ("aarch64-apple-darwin", b"\xcf\xfa\xed\xfe", 0o755,
                 ["arm64", "MH_MAGIC_64 ARM64 EXECUTE", macho(floor="15.0")], "deployment"),
            ]:
                artifact.write_bytes(content)
                artifact.chmod(mode)
                with self.subTest(code=code), patch.object(portability, "tool", side_effect=output) as tool:
                    status, stdout, stderr = self.invoke(["--json", target, str(artifact)])
                self.assertEqual((status, json.loads(stdout), stderr),
                                 (1, {"ok": False, "code": "possums_build_tor_" + code}, ""))
                if output is None:
                    tool.assert_not_called()

    def test_actual_cli_input_and_unknown_failures_and_legacy_message(self):
        for flags in [[], ["--json"]]:
            for args, code, message in [
                ([], "inspection_input", "supply an approved target and artifact path. No artifact was executed."),
                (["hostile-canary", "missing"], "inspection_target", "inspection requires one of the four approved target triples. No artifact was executed."),
                (["aarch64-unknown-linux-musl", "/missing/hostile-canary"], "inspection_unknown", None),
            ]:
                result = subprocess.run([sys.executable, str(ROOT / "scripts/tor_native_portability.py"), *flags, *args],
                                        capture_output=True, check=False)
                code = "possums_build_tor_" + code
                expected = (1, json.dumps({"ok": False, "code": code}).encode() + b"\n", b"") if flags else (
                    1, b"", ((code + ": " + message) if message else portability.UNKNOWN_MESSAGE).encode() + b"\n")
                with self.subTest(flags=flags, args=args):
                    self.assertEqual((result.returncode, result.stdout, result.stderr), expected)

    def test_actual_json_cli_missing_tools_and_nonexecution(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifact = Path(tmp) / "helper"
            marker = Path(tmp) / "must-not-exist"
            for content, code in [(b"\x7fELF", "inspection_tool"),
                                  (f"#!/bin/sh\nprintf executed > '{marker}'\n".encode(), "format")]:
                artifact.write_bytes(content)
                artifact.chmod(0o755)
                result = subprocess.run([sys.executable, str(ROOT / "scripts/tor_native_portability.py"),
                                         "--json", "aarch64-unknown-linux-musl", str(artifact)],
                                        env={"PATH": tmp}, capture_output=True, check=False)
                self.assertEqual((result.returncode, json.loads(result.stdout), result.stderr),
                                 (1, {"ok": False, "code": "possums_build_tor_" + code}, b""))
                self.assertFalse(marker.exists())

    def test_inspection_subprocesses_are_only_static_tools_never_artifact(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifact = Path(tmp) / "helper"
            for target, magic, outputs, commands in [
                ("aarch64-unknown-linux-musl", b"\x7fELF", [ELF],
                 [["readelf", "-W", "-h", "-l", "-d", str(artifact)]]),
                ("aarch64-apple-darwin", b"\xcf\xfa\xed\xfe", ["arm64", "MH_MAGIC_64 ARM64 EXECUTE", macho()],
                 [["/usr/bin/lipo", "-archs", str(artifact)], ["/usr/bin/otool", "-hv", str(artifact)],
                  ["/usr/bin/otool", "-l", str(artifact)]]),
            ]:
                artifact.write_bytes(magic)
                artifact.chmod(0o755)
                responses = [subprocess.CompletedProcess(command, 0, output.encode(), b"hostile-canary")
                             for command, output in zip(commands, outputs, strict=True)]
                with self.subTest(target=target), patch.object(subprocess, "run", side_effect=responses) as run:
                    self.assertEqual(portability.inspect(target, artifact)["execution"], "not_tested")
                self.assertEqual([call.args[0] for call in run.call_args_list], commands)


if __name__ == "__main__":
    unittest.main()
