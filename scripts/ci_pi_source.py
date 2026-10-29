"""Read-only source CI: public acquisition, isolated offline builds, no publisher."""
import base64
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from contextlib import suppress
from pathlib import Path

from tor_native_build import TARGETS
from tor_native_build import SOURCES as CARGO_SOURCES

REPO = Path(__file__).resolve().parents[1]
PINS = {"node": "v24.13.0", "npm": "11.6.2"}
SDK_PACKAGES = {"@earendil-works/" + name: "1.3.0" for name in
                ("pi-ai", "pi-coding-agent", "pi-agent-core")}
MESSAGES = {
    "input": "Run the source CI driver without arguments in the pinned pi-client shell. No work was started.",
    "source": "The exact clean requested source commit could not be established. Check out the reviewed commit; no candidate is accepted.",
    "tools": "The selected native target does not match this supported source-build host. Enter the pinned pi-client shell; this driver installs no target tools.",
    "tools_node": "Node 24.13.0 was not validated. Select that exact Node pin before source acquisition or compilation.",
    "tools_npm": "npm 11.6.2 was not validated. Select that exact npm pin before source acquisition or compilation.",
    "tools_go": "Go 1.27.2 was not validated. Enter the pinned pi-client shell; no toolchain is downloaded by this driver.",
    "tools_rustc": "Rust 1.92.0 was not validated. Enter the pinned pi-client shell; do not enable rustup auto-install.",
    "tools_cargo": "Cargo 1.92.0 was not validated. Enter the pinned pi-client shell; do not enable rustup auto-install.",
    "certificate": "A public build CA bundle is unavailable. Retain the pinned shell certificate bundle; never disable TLS verification.",
    "sdk_lock": "The compiler SDK fixture differs from its closed public-registry lock contract. Restore the reviewed manifest and lock; no SDK is accepted.",
    "npm_acquisition": "Public compiler dependency acquisition did not complete; its underlying cause is unknown. Check the pinned registry/cache and tools; no client installation or publication was attempted.",
    "go_acquisition": "Public Go module acquisition did not complete; its underlying cause is unknown. Check the pinned modules, CA and public proxy; no verifier is accepted.",
    "cargo_acquisition": "Locked public Cargo acquisition did not complete; its underlying cause is unknown. Check the target, CA and public registry; no helper is accepted.",
    "verifier_build": "Offline verifier compilation did not complete; its underlying cause is unknown. Check the pinned Go toolchain and prepared module cache; no candidate is accepted.",
    "native_build": "Offline native compilation/inspection did not complete; its underlying cause is unknown. Check the pinned target, tools and prepared registry cache; no helper is accepted.",
    "client_build": "Offline source-client compilation did not complete; its underlying cause is unknown. Check the prepared SDK/cache and pinned tools; no client candidate is accepted.",
    "build_output": "A source builder did not return its expected local scratch result. Discard this attempt and check the reviewed producer; no candidate is accepted.",
    "report": "Source/report/artifact correspondence was not established by the build-only validator. Keep the candidate private and rebuild matching inputs; no universal acceptance is claimed.",
    "checks": "The offline client suites did not complete; their underlying failing constraint is unavailable at this subprocess boundary. Keep the source candidate unqualified and run the pinned local suites; no install, inference or publication was attempted.",
    "storage": "Source CI scratch input/output failed; cause unknown. Check the owned runner storage and permissions; no accepted receipt is established.",
    "cleanup": "Source checks finished but public dependency scratch cleanup failed; cause unknown. Check the owned runner storage. Candidate acceptance is withheld; nothing was installed or published.",
    "unknown": "Source CI preparation/reporting failed; cause unknown. Check local scratch storage and pinned tools; no accepted receipt, installation or publication is established.",
}


class SourceFailure(Exception):
    def __init__(self, stage, constraint="validation"):
        self.stage = stage if type(stage) is str and stage in MESSAGES else "unknown"
        self.constraint = constraint if type(constraint) is str and constraint in {"validation", "start", "exit", "read", "decode", "timeout", "cleanup"} else "validation"
        self.code = "possums_pi_source_ci_" + self.stage
        message = MESSAGES[self.stage]
        if self.constraint == "timeout":
            message = "The local stage exceeded its subprocess time bound. Check the pinned tools and public inputs; no candidate is accepted. Descendant termination is not established."
        elif self.constraint == "cleanup":
            message = "The subprocess finished but local pipe cleanup failed. Keep any outputs private; candidate acceptance is withheld."
        super().__init__(f"{self.code}: stage={self.stage}; constraint={self.constraint}. {message}")


def stop(process):
    # Own the fresh process group, not just a shell that can leave pipes open.
    with suppress(OSError, KeyboardInterrupt):
        os.killpg(process.pid, signal.SIGKILL)
    with suppress(OSError, subprocess.TimeoutExpired, KeyboardInterrupt):
        process.communicate(timeout=5)


def run(stage, command, env, cwd=REPO, timeout=600):
    process = None
    failure = None
    interrupted = False
    output = ""
    stdout = b""
    try:
        process = subprocess.Popen(command, env=env, cwd=cwd, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            stdout, _ = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            failure = SourceFailure(stage, "timeout")
            stop(process)
        if failure is None:
            if process.returncode:
                failure = SourceFailure(stage, "exit")
            else:
                try:
                    output = stdout.decode("utf-8")
                except UnicodeError:
                    failure = SourceFailure(stage, "decode")
    except OSError:
        failure = failure or SourceFailure(stage, "start" if process is None else "read")
        if process is not None:
            stop(process)
    except KeyboardInterrupt:
        interrupted = True
        if process is not None:
            stop(process)
    finally:
        if process is not None:
            for pipe in (process.stdout, process.stderr):
                if pipe is None:
                    continue
                try:
                    pipe.close()
                except KeyboardInterrupt:
                    interrupted = True
                except OSError:
                    if not interrupted:
                        failure = failure or SourceFailure(stage, "cleanup")
    if failure is not None:
        raise failure
    if interrupted:
        raise KeyboardInterrupt
    return output


def snapshot(env):
    commit = run("source", ["git", "rev-parse", "HEAD"], env).strip()
    expected = os.environ.get("GITHUB_SHA", commit)
    if (not re.fullmatch(r"[a-f0-9]{40}", commit) or commit != expected
            or run("source", ["git", "-c", "core.fsmonitor=false", "status", "--porcelain", "--untracked-files=all"], env).strip()):
        raise SourceFailure("source")
    return commit


def environment(root):
    certificate = os.environ.get("SSL_CERT_FILE") or os.environ.get("NIX_SSL_CERT_FILE")
    if not certificate or not Path(certificate).is_absolute() or not Path(certificate).is_file():
        raise SourceFailure("certificate")
    for name in ("home", "tmp", "npm-cache", "go-modules", "go-build", "cargo-home"):
        (root / name).mkdir()
    for name in ("user.npmrc", "global.npmrc"):
        (root / name).write_text("")
    return {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(root / "home"),
            "TMPDIR": str(root / "tmp"), "LC_ALL": "C", "SSL_CERT_FILE": certificate,
            "CARGO_HTTP_CAINFO": certificate, "CARGO_HOME": str(root / "cargo-home"),
            "RUSTUP_AUTO_INSTALL": "0", "GOENV": "off", "GOWORK": "off", "GOTOOLCHAIN": "local", "GOAUTH": "off",
            "GOMODCACHE": str(root / "go-modules"), "GOCACHE": str(root / "go-build"),
            "GOPROXY": "https://proxy.golang.org", "GOSUMDB": "sum.golang.org",
            "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null", "GIT_OPTIONAL_LOCKS": "0",
            "npm_config_cache": str(root / "npm-cache"), "npm_config_userconfig": str(root / "user.npmrc"),
            "npm_config_globalconfig": str(root / "global.npmrc"), "npm_config_cafile": certificate,
            "npm_config_registry": "https://registry.npmjs.org", "npm_config_logs_max": "0",
            "npm_config_update_notifier": "false", "npm_config_audit": "false", "npm_config_fund": "false",
            "npm_config_fetch_retries": "0"}


def tools(env):
    for name, expected in PINS.items():
        if run("tools_" + name, [name, "--version"], env).strip() != expected:
            raise SourceFailure("tools_" + name)
    for name, arguments, expected in (("go", ["version"], "go version go1.27.2 "),
                                      ("rustc", ["--version"], "rustc 1.92.0 "),
                                      ("cargo", ["--version"], "cargo 1.92.0 ")):
        if not run("tools_" + name, [name, *arguments], env).startswith(expected):
            raise SourceFailure("tools_" + name)
    target = os.environ.get("POSSUMS_PI_NATIVE_TARGET")
    arch = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64", "AMD64": "x64"}.get(platform.machine())
    if target not in TARGETS or TARGETS[target][:2] != (sys.platform, arch):
        raise SourceFailure("tools")
    return target


def validate_sdk(directory):
    documents = {}
    try:
        documents = {name: (directory / name).read_bytes() for name in ("package.json", "package-lock.json")}
        manifest = json.loads(documents["package.json"])
        lock = json.loads(documents["package-lock.json"])
    except (OSError, UnicodeError, ValueError):
        manifest = lock = None
    if (not isinstance(manifest, dict) or set(manifest) != {"name", "version", "private", "engines", "dependencies"}
            or manifest.get("name") != "possums-pi-ci-sdk" or manifest.get("version") != "0.0.0"
            or manifest.get("private") is not True or manifest.get("engines") != {"node": "24.13.0"}
            or manifest.get("dependencies") != SDK_PACKAGES or not isinstance(lock, dict)
            or lock.get("lockfileVersion") != 3 or lock.get("name") != manifest["name"]
            or lock.get("version") != manifest["version"]):
        raise SourceFailure("sdk_lock")
    packages = lock.get("packages")
    if (not isinstance(packages, dict) or not isinstance(packages.get(""), dict)
            or packages[""].get("dependencies") != SDK_PACKAGES
            or packages[""].get("name") != manifest["name"] or packages[""].get("version") != manifest["version"]
            or packages[""].get("engines") != manifest["engines"]):
        raise SourceFailure("sdk_lock")
    for name, entry in packages.items():
        if not name:
            continue
        if not isinstance(entry, dict):
            raise SourceFailure("sdk_lock")
        resolved, integrity = entry.get("resolved"), entry.get("integrity")
        if (not name.startswith("node_modules/") or ".." in name.split("/") or entry.get("link")
                or not isinstance(resolved, str) or not isinstance(integrity, str)
                or not re.fullmatch(r"https://registry\.npmjs\.org/[a-zA-Z0-9@%_./~-]+", resolved)
                or not re.fullmatch(r"sha512-[A-Za-z0-9+/]{86}==", integrity)):
            raise SourceFailure("sdk_lock")
        encoded = integrity.removeprefix("sha512-")
        if base64.b64encode(base64.b64decode(encoded, validate=True)).decode("ascii") != encoded:
            raise SourceFailure("sdk_lock")
    for name, version in SDK_PACKAGES.items():
        if not isinstance(packages.get("node_modules/" + name), dict) or packages["node_modules/" + name].get("version") != version:
            raise SourceFailure("sdk_lock")
    return documents


def builder_result(text, key):
    results = []
    for line in text.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict) and key in value:
            results.append(value[key])
    if len(results) != 1 or not isinstance(results[0], str):
        raise SourceFailure("build_output")
    result = Path(results[0])
    try:
        valid = result.is_absolute() and not result.is_symlink() and result.is_dir()
    except (OSError, ValueError):
        valid = False
    if not valid:
        raise SourceFailure("build_output")
    return result


def prepare():
    temporary = None
    failure = None
    interrupted = False
    result = {}
    try:
        temporary = tempfile.TemporaryDirectory(prefix="possums-pi-source-ci-", dir="/tmp")
        result = qualify(Path(temporary.name))
    except SourceFailure as error:
        failure = error
    except OSError:
        failure = SourceFailure("storage")
    except KeyboardInterrupt:
        interrupted = True
    except Exception:
        failure = SourceFailure("unknown")
    finally:
        if temporary is not None:
            try:
                temporary.cleanup()
            except KeyboardInterrupt:
                interrupted = True
            except Exception:
                if failure is None and not interrupted:
                    failure = SourceFailure("cleanup")
    if failure is not None:
        raise failure
    if interrupted:
        raise KeyboardInterrupt
    return result


def qualify(root):
    env = environment(root)
    commit = snapshot(env)
    target = tools(env)
    sdk = root / "sdk"
    sdk.mkdir()
    for name, contents in validate_sdk(REPO / "clients/pi/ci-sdk").items():
        (sdk / name).write_bytes(contents)
    npm_args = ["npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund", "--workspaces=false"]
    run("npm_acquisition", npm_args, env, sdk)
    compiler = root / "compiler"
    compiler.mkdir()
    for name in ("package.json", "package-lock.json"):
        shutil.copyfile(REPO / "clients/pi" / name, compiler / name)
    run("npm_acquisition", [*npm_args, "--legacy-peer-deps"], env, compiler)
    go = root / "go-source"
    go.mkdir()
    for name in ("go.mod", "go.sum"):
        shutil.copyfile(REPO / "attestation-helper" / name, go / name)
    run("go_acquisition", ["go", "mod", "download", "all"], env, go)
    crate = root / "crate"
    (crate / "src").mkdir(parents=True)
    for name in CARGO_SOURCES:
        shutil.copyfile(REPO / "clients/pi/tor-native" / name, crate / name)
    run("cargo_acquisition", ["cargo", "fetch", "--locked", "--target", target,
                              "--manifest-path", str(crate / "Cargo.toml")], env, crate)
    offline = dict(env, GOPROXY="off", GOSUMDB="off", CARGO_NET_OFFLINE="true", npm_config_offline="true")
    verifier = builder_result(run("verifier_build", ["sh", "scripts/build-v3-verifier.sh"], offline, timeout=1200), "verifierBuild")
    native = builder_result(run("native_build", [sys.executable, "scripts/tor_native_build.py", "--target", target], offline, timeout=1200), "torNativeBuild")
    build_env = dict(offline, POSSUMS_PI_ROOT=str(sdk), POSSUMS_VERIFIER_BUILD=str(verifier),
                     POSSUMS_TOR_NATIVE_BUILD=str(native), POSSUMS_PI_NPM_CACHE=env["npm_config_cache"], POSSUMS_PI_NPM_OFFLINE="1")
    package = builder_result(run("client_build", ["sh", "clients/pi/build.sh"], build_env, timeout=600), "output")
    check = package.parent / "source/clients/pi/check.mjs"
    report_program = ('import {clientReport,publicJSON} from "./clients/pi/distribution-reports.mjs";'
                      'const p=process.argv[1];clientReport(publicJSON(p+"/build-report.json"),p,p+"/../source/clients/pi",process.cwd());')
    run("report", ["node", "--input-type=module", "-e", report_program, str(package)], offline)
    run("checks", ["node", str(check)], build_env, timeout=1200)
    if snapshot(env) != commit:
        raise SourceFailure("source")
    return {"schema": 1, "sourceCommit": commit, "node": "24.13.0", "npm": "11.6.2", "pi": "1.3.0",
            "nativeTarget": target, "sdkLockSha256": hashlib.sha256((sdk / "package-lock.json").read_bytes()).hexdigest(),
            "sourceChecks": "passed", "packedChecks": "synthetic_only", "universalCandidate": "not_established",
            "installation": "not_attempted", "publication": "not_attempted"}


if __name__ == "__main__":
    try:
        if len(sys.argv) != 1:
            raise SourceFailure("input")
        print(json.dumps(prepare()))
    except SourceFailure as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print("possums_pi_source_ci_interrupted: source CI interrupted; no accepted receipt. Check the owned build job; descendant termination is unknown. No install or publication was attempted.", file=sys.stderr)
        sys.exit(130)
    except Exception:
        print(str(SourceFailure("unknown")), file=sys.stderr)
        sys.exit(1)
