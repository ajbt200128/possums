"""Independent public-only Linux gate; never executes a daemon or network provider.

Policy: PRIVACY.md data-handling, forbidden-telemetry and synthetic-testing sections.
Only closed observations and public artifact identities leave this controller.
Docker/Actions retain public build metadata; no whole-host transience is claimed.
"""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import uuid

from image_health import QualificationError, checked, docker, load, require
import image_tor

SUBJECT = "74b7223c210ba361b0a76962805f5ab764d5395a"
ARCHIVE = "/nix/store/6cgzv3aw8996zszbdlsp6cx7sp559vvf-possums-gateway.tar.gz"
PROBE = "/nix/store/h03i9yg0r2kvpnmkmqk4vddjv2y3181h-possums-0.1.0/bin/tor-image-probe"
SAMPLE_IMAGE = "sha256:" + "a" * 64  # Canned metadata only; never a runtime pin.
SDK = "tor_runtime::enabled::tests::offline_e2e::operator_full_path"
COMPOSED = "server::lifecycle_tests::tor_offline_e2e_tests::full_preparation_shared_ingress_tls_bridge_stream_and_owned_drain"
MARKER = b"POSSUMS_OFFLINE_E2E=PREPARE_AND_SDK_OK"
TMP = "/" + "tmp"
TMP_OPTIONS = "rw,nosuid,nodev,size=128m,mode=0700,uid=65532,gid=65532"
DOCKER_PATH = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"


def digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def libtest_matches(code, out, err, sdk):
    # --format terse removes test-name interleaving. No arbitrary output is allowed.
    if code != 0 or err:
        return False
    lines = out.strip().splitlines()
    expected = [b"running 1 test"]
    if sdk:
        expected += [MARKER]
    expected += [b"."]
    lines = [line for line in lines if line]
    return (len(lines) == len(expected) + 1 and lines[:-1] == expected
            and re.fullmatch(rb"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+\.[0-9]+s", lines[-1]) is not None)


def settings_match(metadata, binary, image, unreadable=None):
    host, config = metadata["HostConfig"], metadata["Config"]
    mounts = metadata["Mounts"]
    wanted = {(str(binary), "/public-libtest")}
    if unreadable is not None:
        wanted.update((str(unreadable), "/etc/" + target) for target in ("passwd", "group"))
    return (host["NetworkMode"] == "none" and host["ReadonlyRootfs"] is True
            and host["CapDrop"] == ["ALL"] and not host.get("CapAdd")
            and host["SecurityOpt"] == ["no-new-privileges"]
            and host["Privileged"] is False
            and host["PidsLimit"] == 128 and host["Memory"] == 2147483648
            and host["Ulimits"] == [{"Name": "core", "Hard": 0, "Soft": 0}]
            and host["Tmpfs"] == {TMP: TMP_OPTIONS}
            and config["User"] == "65532:65532"
            and config["Entrypoint"] == ["/public-libtest"]
            and config["WorkingDir"] == TMP
            and metadata["Image"] == image
            and len(mounts) == len(wanted)
            and {(mount["Source"], mount["Destination"]) for mount in mounts} == wanted
            and all(mount["Type"] == "bind" and mount["RW"] is False for mount in mounts)
            and config["Env"] is not None
            and sorted(e for e in config["Env"] if not e.startswith("SSL_CERT_FILE=") and e != DOCKER_PATH)
            == ["POSSUMS_OFFLINE_MODE=synthetic"])


def run_fixture(binary, sdk, image, normal=None, unreadable=None):
    name = "possums-independent-public-" + uuid.uuid4().hex
    created = False
    try:
        command = ["create", "--name", name, "--network", "none", "--read-only",
                   "--user", "65532:65532", "--cap-drop", "ALL",
                   "--security-opt", "no-new-privileges", "--ulimit", "core=0:0",
                   "--pids-limit", "128", "--memory", "2g", "--memory-swap", "2g",
                   "--tmpfs", TMP + ":" + TMP_OPTIONS,
                   "--mount", f"type=bind,src={binary},dst=/public-libtest,readonly",
                   "--env", "POSSUMS_OFFLINE_MODE=synthetic", "--workdir", TMP,
                   "--entrypoint", "/public-libtest"]
        if unreadable is not None:
            for target in ("passwd", "group"):
                command += ["--mount", f"type=bind,src={unreadable},dst=/etc/{target},readonly"]
        command.append(image)
        if normal is None:
            command += ["--exact", SDK if sdk else COMPOSED, "--nocapture", "--format", "terse", "--test-threads=1"]
            if sdk:
                command.append("--ignored")
        require(bool(re.fullmatch(rb"[0-9a-f]{64}\n", checked(command, "fixture_create"))), "fixture_create")
        created = True
        template = '{"HostConfig":{{json .HostConfig}},"Config":{{json .Config}},"Mounts":{{json .Mounts}},"Image":{{json .Image}}}'
        metadata = json.loads(checked(["inspect", "--format", template, name], "fixture_metadata"))
        require(settings_match(metadata, binary, image, unreadable), "fixture_isolation")
        print("stage=fixture_isolation code=readonly_uid65532_capsnone_nonet_private_tmpfs", flush=True)
        checked(["start", name], "fixture_start")
        exit_code = checked(["wait", name], "fixture_wait", timeout=60)
        # Keep loader/SDK/panic text private even when the ELF closure is missing.
        code, out, err = docker(["logs", name], "fixture_output")
        if normal is not None:
            expected_code = 2 if normal else 0
            require(exit_code == f"{expected_code}\n".encode(), "normal_probe_exit")
            require(code == 0 and image_tor.result_matches(expected_code, out, err, normal),
                    image_tor.projection_failure(expected_code, out, err, normal))
            print("stage=image_negative code=nss_io_permission_denied_sdk_builder_panic" if normal else
                  "stage=image_positive code=nss_builder_configure_create_keymgr_ok", flush=True)
        else:
            require(exit_code == b"0\n", "sdk_fixture_exit" if sdk else "composed_fixture_exit")
            require(code == 0 and libtest_matches(0, out, err, sdk),
                    "sdk_fixture_projection" if sdk else "composed_fixture_projection")
            print("stage=full_prepare_sdk code=PREPARE_AND_SDK_OK" if sdk else
                  "stage=composed_local_tls_stream_accounting_drain code=libtest_pass", flush=True)
    finally:
        if created:
            if sys.exc_info()[0] is not None:
                try:
                    checked(["rm", "--force", name], "fixture_cleanup")
                    print("stage=fixture_cleanup code=removed_after_failure", flush=True)
                except QualificationError:
                    print("stage=fixture_cleanup code=unknown_primary_failure_preserved", flush=True)
            else:
                checked(["rm", "--force", name], "fixture_cleanup")
                print("stage=fixture_cleanup code=removed", flush=True)


def build_libtest(subject):
    # Build diagnostics are transient files, never uploaded or echoed.
    # A Cargo exit of zero is not evidence that the right test artifact exists.
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        process = subprocess.Popen(
            ["nix", "develop", "-c", "cargo", "test", "--locked", "--features", "tor",
             "--lib", "--no-run", "--message-format=json"],
            cwd=subject, stdout=out, stderr=err, start_new_session=True)
        try:
            require(process.wait(timeout=2700) == 0, "pinned_libtest_build")
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                with contextlib.suppress(subprocess.TimeoutExpired):
                    process.wait(timeout=5)
        require(out.tell() <= 32 * 1024 * 1024 and err.tell() <= 4 * 1024 * 1024, "build_output_bound")
        out.seek(0)
        artifacts = []
        for line in out:
            event = json.loads(line)
            if (event.get("reason") == "compiler-artifact" and event.get("executable")
                    and event.get("target", {}).get("name") == "possums"
                    and event.get("target", {}).get("kind") == ["lib"]
                    and event.get("profile", {}).get("test") is True):
                artifacts.append(Path(event["executable"]).resolve(strict=True))
        require(len(artifacts) == 1, "libtest_artifact_selection")
    binary = artifacts[0]
    require(binary.is_file(), "libtest_artifact_file")
    with binary.open("rb") as source:
        elf = source.read(20)
    require(elf[:6] == b"\x7fELF\x02\x01" and elf[18:20] == b"\x3e\x00", "libtest_elf_amd64")
    print("stage=libtest_sha256 code=" + digest(binary), flush=True)
    return binary


def qualify(subject, archive, probe):
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=subject, capture_output=True, check=False)
    require(head.returncode == 0 and head.stdout == (SUBJECT + "\n").encode() and not head.stderr,
            "exact_subject_head")
    require(Path(probe).resolve(strict=True) == Path(PROBE), "literal_normal_probe_pin")
    require(Path(archive).resolve(strict=True) == Path(ARCHIVE), "literal_image_archive_pin")
    print("stage=subject_head code=" + SUBJECT, flush=True)
    for filename in ("flake.lock", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"):
        print("stage=public_source_" + filename.replace(".", "_") + " code=" + digest(subject / filename), flush=True)
    print("stage=build_scope code=locked_tor_lib_devdeps_not_normal_feature_equivalence", flush=True)
    print("stage=sdk_pin code=arti_0_47_0_fs_mistrust_0_16_0_rust_1_92_0", flush=True)
    load(archive, image_tor.IMAGE)
    actual = checked(["image", "inspect", "--format", "{{.Id}}", image_tor.IMAGE], "image_identity").strip()
    require(bool(re.fullmatch(rb"sha256:[0-9a-f]{64}", actual)), "image_identity")
    image = actual.decode("ascii")
    require(checked(["image", "inspect", "--format", "{{.Os}}/{{.Architecture}}", image], "image_architecture")
            == b"linux/amd64\n", "image_architecture")
    # The new subject is built independently; all runs now use this immutable ID.
    print("stage=image_identity code=" + image, flush=True)
    print("stage=normal_probe_sha256 code=" + digest(probe), flush=True)
    with tempfile.TemporaryDirectory(prefix="possums-independent-public-") as directory:
        unreadable = Path(directory) / "unreadable"
        unreadable.write_bytes(b"")
        unreadable.chmod(0o000)
        run_fixture(Path(probe).resolve(strict=True), False, image, normal=False)
        run_fixture(Path(probe).resolve(strict=True), False, image, normal=True, unreadable=unreadable)
    binary = build_libtest(subject)
    run_fixture(binary, True, image)
    run_fixture(binary, False, image)
    print("stage=independent_linux_gate code=public_fixture_pair_pass", flush=True)


def main(argv):
    try:
        require(len(argv) == 4, "arguments")
        qualify(Path(argv[1]).resolve(strict=True), argv[2], argv[3])
    except QualificationError as failure:
        print(f"code=independent_linux_failed stage={failure} action=inspect_public_controller_checks", flush=True)
        return 1
    except subprocess.TimeoutExpired:
        print("code=independent_linux_failed stage=pinned_libtest_build_timeout action=inspect_public_controller_checks", flush=True)
        return 1
    except Exception:
        print("code=independent_linux_failed stage=controller_cause_unknown action=inspect_public_controller_checks", flush=True)
        return 1
    return 0


class PublicProjectionTests(unittest.TestCase):
    def output(self, sdk):
        return (b"\nrunning 1 test\n" + (MARKER + b"\n" if sdk else b"")
                + b".\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 409 filtered out; finished in 0.01s\n\n")

    def test_exact_results(self):
        for sdk in (True, False):
            self.assertTrue(libtest_matches(0, self.output(sdk), b"", sdk))

    def test_zero_exit_alone_never_passes(self):
        for sdk in (True, False):
            for output in (b"", b"running 0 tests\n", self.output(sdk).replace(b"1 passed", b"0 passed"),
                           self.output(sdk).replace(b"0 ignored", b"1 ignored")):
                self.assertFalse(libtest_matches(0, output, b"", sdk))
        self.assertFalse(libtest_matches(0, self.output(True).replace(MARKER, b"POSSUMS_OFFLINE_E2E=SDK_BUILDER_PANIC"), b"", True))
        self.assertFalse(libtest_matches(0, self.output(False), b"", True))

    def test_hostile_output_and_wrong_exit(self):
        hostile = b"PUBLIC_HOSTILE credential=synthetic-only"
        for sdk in (True, False):
            for code, out, err in ((2, self.output(sdk), b""), (137, self.output(sdk), b""),
                                   (0, self.output(sdk) + hostile, b""), (0, self.output(sdk), hostile)):
                self.assertFalse(libtest_matches(code, out, err, sdk))

    def test_unknown_exception_is_not_echoed(self):
        with patch(__name__ + ".qualify", side_effect=RuntimeError("PUBLIC_HOSTILE")), patch("builtins.print") as emit:
            self.assertEqual(main(["gate", ".", "public", "public"]), 1)
            self.assertNotIn("PUBLIC_HOSTILE", str(emit.call_args))
            self.assertIn("controller_cause_unknown", str(emit.call_args))

    def test_only_synthetic_mode_and_one_elf_are_requested(self):
        observed = []
        def fake_checked(command, stage, **kwargs):
            observed.append(command)
            if command[0] == "create":
                return b"a" * 64 + b"\n"
            if command[0] == "inspect":
                return b"{}"
            return b"0\n"
        for sdk in (True, False):
            with patch(__name__ + ".checked", side_effect=fake_checked), patch(__name__ + ".settings_match", return_value=True), \
                    patch(__name__ + ".docker", return_value=(0, self.output(sdk), b"")), patch("builtins.print"):
                run_fixture(Path("/public/test-elf"), sdk, SAMPLE_IMAGE)
        creates = [cmd for cmd in observed if cmd[0] == "create"]
        self.assertEqual(len(creates), 2)
        for command in creates:
            self.assertEqual(command.count("--mount"), 1)
            self.assertEqual(command.count("--env"), 1)
            self.assertIn("POSSUMS_OFFLINE_MODE=synthetic", command)
            self.assertNotIn("private", command)
            self.assertIn(SAMPLE_IMAGE, command)

    def test_isolation_metadata_cannot_downgrade(self):
        binary = Path("/public/test-elf")
        metadata = {
            "Image": SAMPLE_IMAGE,
            "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": True, "CapDrop": ["ALL"],
                           "SecurityOpt": ["no-new-privileges"], "Privileged": False,
                           "PidsLimit": 128, "Memory": 2147483648,
                           "Ulimits": [{"Name": "core", "Hard": 0, "Soft": 0}],
                           "Tmpfs": {TMP: TMP_OPTIONS}},
            "Config": {"User": "65532:65532", "Entrypoint": ["/public-libtest"],
                       "WorkingDir": TMP, "Env": ["POSSUMS_OFFLINE_MODE=synthetic", DOCKER_PATH]},
            "Mounts": [{"Type": "bind", "Source": str(binary), "Destination": "/public-libtest", "RW": False}],
        }
        self.assertTrue(settings_match(metadata, binary, SAMPLE_IMAGE))
        for section, field, value in (("HostConfig", "ReadonlyRootfs", False),
                                      ("HostConfig", "NetworkMode", "bridge"),
                                      ("HostConfig", "PidsLimit", 0),
                                      ("HostConfig", "Memory", 0),
                                      ("HostConfig", "CapAdd", ["SYS_ADMIN"]),
                                      ("Config", "User", "0:0"),
                                      ("Config", "Env", ["POSSUMS_OFFLINE_MODE=private"])):
            changed = json.loads(json.dumps(metadata))
            changed[section][field] = value
            self.assertFalse(settings_match(changed, binary, SAMPLE_IMAGE))
        metadata["Mounts"][0]["RW"] = True
        self.assertFalse(settings_match(metadata, binary, SAMPLE_IMAGE))

    def test_primary_failure_survives_cleanup_failure(self):
        def fake_checked(command, stage, **kwargs):
            if command[0] == "create":
                return b"a" * 64 + b"\n"
            if command[0] == "inspect":
                raise QualificationError("fixture_metadata")
            raise QualificationError("fixture_cleanup")
        with patch(__name__ + ".checked", side_effect=fake_checked), \
                self.assertRaisesRegex(QualificationError, "^fixture_metadata$"), patch("builtins.print"):
            run_fixture(Path("/public/test-elf"), True, SAMPLE_IMAGE)


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        unittest.main(argv=[sys.argv[0]])
    else:
        sys.exit(main(sys.argv))
