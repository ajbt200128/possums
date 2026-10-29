"""Scratch-only native connector builds; no installation, Tor launch or inference."""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TARGETS = {
    "aarch64-apple-darwin": ("darwin", "arm64", "apple"),
    "x86_64-apple-darwin": ("darwin", "x64", "apple"),
    "aarch64-unknown-linux-musl": ("linux", "arm64", "musl"),
    "x86_64-unknown-linux-musl": ("linux", "x64", "musl"),
}
LOCAL_TARGETS = TARGETS | {
    "aarch64-unknown-linux-gnu": ("linux", "arm64", "gnu"),
    "x86_64-unknown-linux-gnu": ("linux", "x64", "gnu"),
}
SOURCES = ("Cargo.toml", "Cargo.lock", "src/main.rs")


class BuildFailure(Exception):
    """Only locally authored content-free messages may cross the CLI boundary."""


def run(command, *, env=None, cwd=None):
    try:
        result = subprocess.run(command, env=env, cwd=cwd, capture_output=True, check=False)
    except OSError:
        raise BuildFailure("possums_build_tor_tool_start: a required build tool could not start; cause unknown. Check the pinned toolchain and execution permissions.") from None
    if result.returncode:
        raise BuildFailure("possums_build_tor_tool_exit: a required build tool exited unsuccessfully; cause unknown. Check the pinned toolchain, target libraries and offline dependency cache. No compiler output is exported.")
    return result.stdout.decode("utf-8")


def select_target(arguments, host):
    if not arguments:
        if host in LOCAL_TARGETS:
            return host, False
    elif len(arguments) == 2 and arguments[0] == "--target" and arguments[1] in TARGETS:
        return arguments[1], True
    raise BuildFailure("possums_build_tor_target: select one of the four documented --target triples, or use a supported native host. No build was started.")


def sha(file):
    return hashlib.sha256(file.read_bytes()).hexdigest()


def target_std_digest(directory):
    # Bind the selected installed Rust target runtime, including musl CRT objects.
    # Only relative public toolchain paths/digests enter the report.
    files = sorted(p for p in directory.rglob("*") if p.is_file())
    if not any(p.name.startswith("libstd-") and p.suffix == ".rlib" for p in files):
        raise BuildFailure("possums_build_tor_target_std: the selected target standard library is unavailable. Provision the pinned target toolchain separately; this script installs nothing.")
    entries = [(str(p.relative_to(directory)), sha(p)) for p in files]
    return hashlib.sha256(json.dumps(entries, separators=(",", ":")).encode()).hexdigest()


def toolchain():
    rust = dict(line.split(": ", 1) for line in run(["rustc", "-vV"]).splitlines() if ": " in line)
    if (rust.get("release") != "1.92.0" or rust.get("host") not in LOCAL_TARGETS
            or not re.fullmatch(r"[0-9a-f]{40}", rust.get("commit-hash", ""))):
        raise BuildFailure("possums_build_tor_compiler: Rust 1.92.0 on a supported host is required. Enter the pinned devenv shell; do not relax the pin.")
    # Resolve rustup proxies before clearing HOME; no toolchain acquisition occurs.
    root = Path(run(["rustc", "--print", "sysroot"]).strip())
    cargo = root / "bin/cargo"
    compiler = root / "bin/rustc"
    if not run([str(cargo), "--version"]).startswith("cargo 1.92.0 "):
        raise BuildFailure("possums_build_tor_cargo: Cargo 1.92.0 is required. Enter the pinned devenv shell.")
    return rust, root, compiler, cargo


def build_environment(scratch, root, rust, target):
    cargo_home = scratch / "cargo-home"
    cargo_home.mkdir()
    # Reuse only public registry cache bytes, never personal Cargo config/tokens.
    registry = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "registry"
    if registry.is_dir():
        (cargo_home / "registry").symlink_to(registry.resolve(), target_is_directory=True)
    for name in ("home", "tmp"):
        (scratch / name).mkdir()
    env = {
        "HOME": str(scratch / "home"), "TMPDIR": str(scratch / "tmp"),
        "CARGO_HOME": str(cargo_home), "RUSTC": str(root / "bin/rustc"),
        "PATH": os.pathsep.join([str(root / "bin"), str(root / "lib/rustlib" / rust["host"] / "bin"), "/usr/bin", "/bin"]),
        "CARGO_NET_OFFLINE": "true", "LC_ALL": "C",
    }
    platform, _, abi = LOCAL_TARGETS[target]
    details = {"kind": "host", "abi": abi}
    if platform == "darwin":
        if not rust["host"].endswith("-apple-darwin"):
            raise BuildFailure("possums_build_tor_sdk: Apple targets require a macOS host and its platform SDK. Cross-OS Apple builds are not supported.")
        # Ignore enclosing Nix SDK overrides; select platform developer tools
        # without forwarding personal environment to those tools.
        sdk_env = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}
        sdk = run(["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path"], env=sdk_env).strip()
        version = run(["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-version"], env=sdk_env).strip()
        linker = run(["/usr/bin/xcrun", "--sdk", "macosx", "--find", "clang"], env=sdk_env).strip()
        clang = run([linker, "--version"], env=sdk_env)
        match = re.match(r"Apple clang version ([0-9.]+)\b", clang)
        if not Path(sdk).is_dir() or not re.fullmatch(r"[0-9.]+", version) or not match:
            raise BuildFailure("possums_build_tor_sdk: a supported Apple SDK/compiler was not established. Check the selected developer tools.")
        env.update({"SDKROOT": sdk, "MACOSX_DEPLOYMENT_TARGET": "14.0",
                    "CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER": linker})
        details = {"kind": "apple-clang", "version": match[1], "sdk": version, "deploymentTarget": "14.0", "abi": abi}
    elif abi == "musl":
        linker = root / "lib/rustlib" / rust["host"] / "bin/rust-lld"
        if not linker.is_file():
            raise BuildFailure("possums_build_tor_linker: the pinned Rust self-contained linker is unavailable. Provision the complete compiler toolchain separately.")
        # Do not inherit Nix compiler wrappers, RUSTFLAGS or dynamic libc defaults.
        env["RUSTFLAGS"] = f"-C target-feature=+crt-static -C link-self-contained=yes -C linker={linker}"
        details = {"kind": "rust-lld", "abi": abi, "selfContained": True}
    else:
        # Local GNU host builds remain supported, but are not distributable targets.
        env["CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"] = "/usr/bin/cc"
    return env, details


def build(arguments):
    repo = Path(__file__).resolve().parents[1]
    rust, root, compiler, cargo = toolchain()
    target, distribution = select_target(arguments, rust["host"])
    platform, arch, _ = LOCAL_TARGETS[target]
    target_lib = Path(run([str(compiler), "--print", "target-libdir", "--target", target]).strip())
    std_digest = target_std_digest(target_lib)
    scratch = Path(tempfile.mkdtemp(prefix="possums-tor-native-build-", dir="/tmp"))
    source = scratch / "source"
    output = scratch / "package"
    (source / "src").mkdir(parents=True)
    output.mkdir()
    sources = {}
    for name in SOURCES:
        original = repo / "clients/pi/tor-native" / name
        shutil.copyfile(original, source / name)
        sources["clients/pi/tor-native/" + name] = sha(source / name)
    env, linker = build_environment(scratch, root, rust, target)
    run([str(cargo), "build", "--offline", "--locked", "--release", "--target", target,
         "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(scratch / "target")], env=env, cwd=source)
    artifact = output / "possums-tor-native"
    shutil.copyfile(scratch / "target" / target / "release/possums-tor-native", artifact)
    artifact.chmod(0o755)
    report = {"rust": rust["release"], "tor": "0.47.0", "platform": platform, "arch": arch,
              "target": target, "distributionTarget": distribution, "rustHost": rust["host"],
              "rustCommit": rust["commit-hash"], "targetStdSha256": std_digest, "linker": linker,
              "sources": sources, "artifacts": {"possums-tor-native": {"sha256": sha(artifact), "size": artifact.stat().st_size}}}
    (output / "tor-native-build-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"torNativeBuild": str(output), "platform": platform, "arch": arch, "target": target}))


if __name__ == "__main__":
    try:
        build(sys.argv[1:])
    except BuildFailure as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except Exception:
        print("possums_build_tor_unknown: native build preparation or reporting failed; cause unknown. Check scratch storage and the pinned tools; no installation was performed.", file=sys.stderr)
        sys.exit(1)
