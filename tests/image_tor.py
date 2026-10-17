"""Public, key-free Arti constructor acceptance in the actual production rootfs.

No daemon, bootstrap, service launch, attestation, inference or telemetry export.
Only the probe binary and (negative case) unreadable public account files are mounted.
Docker's local image/metadata persistence is not whole-host transience.
"""
import contextlib
import json
import os
import re
import sys
import tempfile
import uuid
from pathlib import Path

from image_health import QualificationError, checked, docker, load, require

IMAGE = "possums-gateway:phase0"
POSITIVE = (
    b"stage=nss code=present\n"
    b"stage=sdk_builder code=ok\n"
    b"stage=configure code=ok\n"
    b"stage=create_unbootstrapped code=ok\n"
    b"stage=ephemeral_keymgr code=ok\n"
    b"stage=terminal code=ok\n"
)
NEGATIVE = b"stage=nss code=io_permission_denied\nstage=terminal code=sdk_builder_panic\n"
# Exact locally authored transcripts only: never project arbitrary probe output.
NEGATIVE_FAILURES = {
    b"stage=nss code=records_unmatched\nstage=sdk_builder code=ok\nstage=terminal code=nss_not_rejected\n":
        "tor_negative_fixture_no_match",
    b"stage=nss code=records_unmatched\nstage=terminal code=sdk_builder_panic\n":
        "tor_negative_fixture_no_match_sdk_builder_panic",
    b"stage=nss code=io_permission_denied\nstage=sdk_builder code=ok\nstage=terminal code=nss_not_rejected\n":
        "tor_negative_sdk_builder_not_rejected",
}


def result_matches(exit_code, stdout, stderr, negative):
    return (exit_code == (2 if negative else 0) and not stderr
            and stdout == (NEGATIVE if negative else POSITIVE))


def projection_failure(exit_code, stdout, stderr, negative):
    if negative and exit_code == 2 and not stderr:
        return NEGATIVE_FAILURES.get(stdout, "tor_negative_output_cause_unknown")
    return "tor_negative_output_cause_unknown" if negative else "tor_positive_output_cause_unknown"


def probe(image, binary, unreadable, negative):
    name = "possums-tor-image-probe-" + uuid.uuid4().hex
    created = False
    try:
        command = ["create", "--name", name, "--network", "none", "--read-only",
                   "--user", "65532:65532", "--cap-drop", "ALL",
                   "--security-opt", "no-new-privileges", "--ulimit", "core=0:0",
                   "--tmpfs", os.path.join(os.sep, "tmp") + ":rw,nosuid,nodev,size=64m,mode=0700,uid=65532,gid=65532",
                   "--mount", f"type=bind,src={binary},dst=/tor-image-probe,readonly",
                   "--entrypoint", "/tor-image-probe"]
        if negative:
            for target in ("passwd", "group"):
                command += ["--mount", f"type=bind,src={unreadable},dst=/etc/{target},readonly"]
        command += [image]
        require(bool(re.fullmatch(rb"[0-9a-f]{64}\n", checked(command, "tor_container_create"))),
                "tor_container_create")
        created = True
        settings = json.loads(checked(["inspect", "--format", "{{json .HostConfig}}", name],
                                      "tor_container_settings"))
        require(settings["NetworkMode"] == "none" and settings["ReadonlyRootfs"]
                and settings["CapDrop"] == ["ALL"]
                and "no-new-privileges" in settings["SecurityOpt"], "tor_container_settings")
        checked(["start", name], "tor_container_start")
        # Bounded wait; a lost/stuck SDK task cannot silently pass acceptance.
        require(checked(["wait", name], "tor_container_wait", timeout=20)
                == (b"2\n" if negative else b"0\n"), "tor_container_exit")
        code, out, err = docker(["logs", name], "tor_container_output")
        require(code == 0, "tor_container_output")
        exit_code = 2 if negative else 0
        require(result_matches(exit_code, out, err, negative),
                projection_failure(exit_code, out, err, negative))
        print("stage=image_negative code=nss_io_permission_denied_sdk_builder_panic" if negative
              else "stage=image_positive code=nss_builder_configure_create_keymgr_ok")
    finally:
        if created:
            # Preserve the primary failure if cleanup also fails.
            if sys.exc_info()[0] is not None:
                with contextlib.suppress(QualificationError):
                    docker(["rm", "--force", name], "tor_container_cleanup")
            else:
                checked(["rm", "--force", name], "tor_container_cleanup")


def qualify(archive, binary):
    binary = Path(binary).resolve(strict=True)
    require(binary.is_file(), "tor_probe_artifact")
    load(archive, IMAGE)
    identity = checked(["image", "inspect", "--format", "{{.Id}}", IMAGE], "tor_image_identity").strip()
    require(bool(re.fullmatch(rb"sha256:[0-9a-f]{64}", identity)), "tor_image_identity")
    architecture = checked(["image", "inspect", "--format", "{{.Os}}/{{.Architecture}}", IMAGE],
                           "tor_image_architecture").strip()
    require(architecture == b"linux/amd64", "tor_image_architecture")
    # Public identity remains available even if a subsequent probe fails.
    print("stage=image_identity code=" + identity.decode("ascii"), flush=True)
    print("stage=image_architecture code=linux_amd64", flush=True)
    # Only a single ELF is mounted; no host libraries or host NSS are available.
    with tempfile.TemporaryDirectory(prefix="possums-tor-public-") as directory:
        unreadable = Path(directory) / "unreadable"
        unreadable.write_bytes(b"")
        # UID 65532 with CAP_DROP ALL cannot read this, unlike an empty 0644 DB.
        unreadable.chmod(0o000)
        probe(identity.decode("ascii"), binary, unreadable, False)
        probe(identity.decode("ascii"), binary, unreadable, True)


def main(argv):
    try:
        require(len(argv) == 3, "tor_arguments")
        qualify(argv[1], argv[2])
    except QualificationError as failure:
        # Stage strings originate only at locally authored require/docker calls.
        print(f"code=tor_image_acceptance_failed stage={failure} action=inspect_public_CI_checks")
        return 1
    except Exception:
        print("code=tor_image_acceptance_unknown stage=qualification cause=unknown action=inspect_public_CI_checks")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
