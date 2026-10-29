"""Synthetic executable-inspection boundaries; no target execution or network."""
import importlib.util
import subprocess
import sys
import tempfile
import unittest
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

    def test_elf_rejects_wrong_architecture_class_endianness_type_and_missing_headers(self):
        for text in [ELF.replace("AArch64", "wrong"), ELF.replace("ELF64", "ELF32"),
                     ELF.replace("little endian", "big endian"), ELF.replace("DYN", "REL"),
                     ELF.replace("Program Headers:", ""), "hostile-canary"]:
            with self.subTest(text=text), self.assertRaisesRegex(portability.PortabilityFailure, "possums_build_tor_format"):
                portability.elf_linkage(text, "AArch64")

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
        self.assertIn(b"possums_build_tor_inspection_tool", result.stderr)
        self.assertNotIn(b"Traceback", result.stderr)
        self.assertEqual(result.stdout, b"")

    def test_tool_errors_cannot_export_hostile_output(self):
        result = subprocess.CompletedProcess(["fixture"], 1, b"hostile-canary", b"hostile-canary")
        with patch.object(subprocess, "run", return_value=result), self.assertRaises(portability.PortabilityFailure) as failure:
            portability.tool(["fixture"])
        self.assertNotIn("hostile-canary", str(failure.exception))


if __name__ == "__main__":
    unittest.main()
