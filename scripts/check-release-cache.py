#!/usr/bin/env python3
"""Fail closed if dependency prewarming weakens application/image reproduction."""

import argparse
import json
import subprocess


def store_path(path):
    return path if path.startswith("/nix/store/") else f"/nix/store/{path}"


def derivations(payload):
    # Nix 2.34 uses version-4 store-relative JSON; older installers use absolute paths.
    return {store_path(path): drv for path, drv in payload.get("derivations", payload).items()}


def inputs(drv):
    if "inputDrvs" in drv:
        return {store_path(path): outputs for path, outputs in drv["inputDrvs"].items()}
    result = {}
    for path, selection in drv["inputs"]["drvs"].items():
        if selection["dynamicOutputs"]:
            raise ValueError("release cache check: dynamic derivations are not supported")
        result[store_path(path)] = selection["outputs"]
    return result


def output_path(drv, name):
    # Version 4 omits fixed-output paths from outputs, but retains their env binding.
    path = drv["outputs"][name].get("path") or drv.get("env", {}).get(name)
    if not path:
        raise ValueError("release cache check: unresolved derivation output path")
    return store_path(path)


def validate_boundary(image, dependencies, expected, is_valid):
    fresh = set(image) - set(dependencies)
    if not fresh or fresh != set(expected):
        raise ValueError("release cache check: application/image dependency boundary changed")
    for path in fresh:
        for output in image[path]["outputs"]:
            if is_valid(output_path(image[path], output)):
                raise ValueError("release cache check: application/image output already realized")
        for dependency, outputs in inputs(image[path]).items():
            if dependency in fresh:
                continue
            for output in outputs:
                if not is_valid(output_path(image[dependency], output)):
                    raise ValueError("release cache check: external build dependency not prewarmed")
    return len(fresh)


def nix_json(*arguments):
    result = subprocess.run(["nix", *arguments], capture_output=True, text=True, check=True)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        raise ValueError("release cache check: invalid Nix derivation JSON") from None


def is_valid(path):
    return subprocess.run(
        ["nix-store", "--check-validity", path],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
    ).returncode == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--free", action="store_true", help="check the free image boundary")
    args = parser.parse_args()
    image_name = "free-gateway-image" if args.free else "gateway-image"
    deps_name = "free-release-build-deps" if args.free else "release-build-deps"
    try:
        image = derivations(nix_json("derivation", "show", "--recursive", f".#{image_name}"))
        dependencies = derivations(nix_json("derivation", "show", "--recursive", f".#{deps_name}"))
        expected = nix_json("eval", "--json", f".#{deps_name}.rebuildDerivations")
        count = validate_boundary(image, dependencies, expected, is_valid)
    except ValueError as error:
        # Only locally authored boundary messages may be reported, not parser payloads.
        if str(error).startswith("release cache check:"):
            raise SystemExit(str(error)) from None
        raise SystemExit("release cache check: invalid Nix derivation JSON") from None
    except (KeyError, TypeError, AttributeError, OSError, subprocess.CalledProcessError):
        raise SystemExit("release cache check: unable to inspect Nix build dependencies") from None
    print(f"Release cache boundary verified: {count} fresh application/image derivations")


if __name__ == "__main__":
    main()
