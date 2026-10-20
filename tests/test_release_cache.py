import importlib.util
import os
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "release_cache", Path(__file__).resolve().parents[1] / "scripts/check-release-cache.py"
)
assert spec is not None and spec.loader is not None
release_cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_cache)

APP = "/nix/store/app.drv"
HELPER = "/nix/store/helper.drv"
DEP = "/nix/store/dependency.drv"
APP_OUT = "/nix/store/app"
HELPER_OUT = "/nix/store/helper"
DEP_OUT = "/nix/store/dependency-dev"
ACCOUNTS = "/nix/store/fake-nss.drv"
USER_RECORD = "/nix/store/passwd.drv"
GROUP_RECORD = "/nix/store/group.drv"


def graph():
    return {
        APP: {"outputs": {"out": {"path": APP_OUT}}, "inputDrvs": {DEP: ["dev"], HELPER: ["out"]}},
        HELPER: {"outputs": {"out": {"path": HELPER_OUT}}, "inputDrvs": {}},
        DEP: {"outputs": {"out": {"path": "/nix/store/dependency"}, "dev": {"path": DEP_OUT}}, "inputDrvs": {}},
    }


class ReleaseCacheTests(unittest.TestCase):
    def validate(self, image=None, dependencies=None, valid=None, expected=None):
        return release_cache.validate_boundary(
            image or graph(), dependencies if dependencies is not None else {DEP: graph()[DEP]},
            [APP, HELPER] if expected is None else expected,
            (valid if valid is not None else {DEP_OUT}).__contains__,
        )

    def test_prewarmed_dependencies_and_fresh_application(self):
        self.assertEqual(self.validate(), 2)

    def test_cached_application_in_dependency_graph_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "boundary changed"):
            self.validate(dependencies={DEP: graph()[DEP], APP: graph()[APP]})

    def test_already_realized_application_or_helper_is_rejected(self):
        for output in [APP_OUT, HELPER_OUT]:
            with self.subTest(output=output), self.assertRaisesRegex(ValueError, "already realized"):
                self.validate(valid={DEP_OUT, output})

    def test_missing_selected_dependency_output_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "not prewarmed"):
            self.validate(valid={"/nix/store/dependency"})

    def test_added_unclassified_image_stage_is_rejected(self):
        image = graph()
        image["/nix/store/new-stage.drv"] = image[APP]
        with self.assertRaisesRegex(ValueError, "boundary changed"):
            self.validate(image=image)

    def test_configured_accounts_and_record_files_must_remain_fresh(self):
        image = graph()
        image[APP]["inputDrvs"][ACCOUNTS] = ["out"]
        image[ACCOUNTS] = {"outputs": {"out": {"path": "/nix/store/fake-nss"}},
                           "inputDrvs": {USER_RECORD: ["out"], GROUP_RECORD: ["out"], DEP: ["dev"]}}
        for record in [USER_RECORD, GROUP_RECORD]:
            image[record] = {"outputs": {"out": {"path": record.removesuffix(".drv")}},
                             "inputDrvs": {DEP: ["dev"]}}
        expected = [APP, HELPER, ACCOUNTS, USER_RECORD, GROUP_RECORD]
        self.assertEqual(self.validate(image=image, expected=expected), 5)
        for account_node in [ACCOUNTS, USER_RECORD, GROUP_RECORD]:
            with self.subTest(node=account_node):
                with self.assertRaisesRegex(ValueError, "boundary changed"):
                    self.validate(image=image, expected=[node for node in expected if node != account_node])
                with self.assertRaisesRegex(ValueError, "boundary changed"):
                    self.validate(image=image, expected=expected,
                                  dependencies={DEP: image[DEP], account_node: image[account_node]})
                with self.assertRaisesRegex(ValueError, "already realized"):
                    self.validate(image=image, expected=expected,
                                  valid={DEP_OUT, release_cache.output_path(image[account_node], "out")})

    @unittest.skipUnless(os.environ.get("RELEASE_CACHE_GRAPH_CHECK") == "1", "opt-in pinned Linux graph evaluation")
    def test_pinned_linux_graph_partition_without_realizing_outputs(self):
        image = release_cache.derivations(release_cache.nix_json(
            "derivation", "show", "--recursive", ".#packages.x86_64-linux.gateway-image",
        ))
        dependencies = release_cache.derivations(release_cache.nix_json(
            "derivation", "show", "--recursive", ".#packages.x86_64-linux.release-build-deps",
        ))
        expected = set(release_cache.nix_json(
            "eval", "--json", ".#packages.x86_64-linux.release-build-deps.rebuildDerivations",
        ))
        fresh = set(image) - set(dependencies)
        self.assertTrue(fresh)
        self.assertEqual(fresh, expected)
        accounts = [path for path in fresh if path.endswith("-fake-nss.drv")]
        self.assertEqual(len(accounts), 1)
        account_inputs = release_cache.inputs(image[accounts[0]])
        for name in ["passwd", "group", "nsswitch.conf", "var-empty"]:
            records = {path for path in account_inputs if path.endswith(f"-{name}.drv")}
            self.assertTrue(records)
            self.assertTrue(records <= fresh)
        # Selected external outputs must be reachable from the prewarm recipe.
        required = {(dep, output) for path in fresh for dep, outputs in release_cache.inputs(image[path]).items()
                    if dep not in fresh for output in outputs}
        prewarmed = {(dep, output) for drv in dependencies.values()
                     for dep, outputs in release_cache.inputs(drv).items() for output in outputs}
        self.assertTrue(required <= prewarmed)
        # Graph membership only: not store validity, realization or fresh A/B builds.
        for drv in image.values():
            release_cache.inputs(drv)  # Retain the dynamic-derivation rejection.

    @unittest.skipUnless(os.environ.get("RELEASE_CACHE_GRAPH_CHECK") == "1", "opt-in pinned Linux graph evaluation")
    def test_clippy_reuses_tor_artifacts_without_relaxing_lints(self):
        clippy = release_cache.nix_json(
            "eval", "--json", ".#checks.x86_64-linux.clippy",
            "--apply", "drv: { artifacts = drv.cargoArtifacts.drvPath; command = drv.buildPhase; }",
        )
        tor_artifacts = release_cache.nix_json(
            "eval", "--json", ".#checks.x86_64-linux.tor-tests.cargoArtifacts.drvPath",
        )
        self.assertEqual(clippy["artifacts"], tor_artifacts)
        self.assertIn("cargoWithProfile clippy --locked --all-targets --all-features -- -D warnings",
                      clippy["command"].splitlines())

    def test_modern_store_relative_graph(self):
        modern = {}
        for path, drv in graph().items():
            modern[Path(path).name] = {
                "outputs": {name: {"path": Path(output["path"]).name} for name, output in drv["outputs"].items()},
                "inputs": {"drvs": {
                    Path(dependency).name: {"outputs": outputs, "dynamicOutputs": {}}
                    for dependency, outputs in drv["inputDrvs"].items()
                }},
            }
        image = release_cache.derivations({"version": 4, "derivations": modern})
        self.assertEqual(self.validate(image=image), 2)
        self.assertEqual(release_cache.derivations(graph()), graph())

    def test_modern_fixed_output_path_uses_environment_binding(self):
        image = graph()
        image[DEP]["outputs"]["dev"] = {"hash": "sha256-synthetic", "method": "nar"}
        image[DEP]["env"] = {"dev": DEP_OUT}
        self.assertEqual(self.validate(image=image), 2)

    def test_unresolved_output_path_is_rejected(self):
        image = graph()
        image[DEP]["outputs"]["dev"] = {}
        with self.assertRaisesRegex(ValueError, "unresolved derivation output path"):
            self.validate(image=image)

    def test_invalid_nix_json_does_not_echo_parser_input(self):
        result = type("Result", (), {"stdout": "hostile-parser-sentinel"})()
        with (
            patch.object(release_cache.subprocess, "run", return_value=result),
            self.assertRaisesRegex(ValueError, "^release cache check: invalid Nix derivation JSON$"),
        ):
            release_cache.nix_json("eval", "--json")

    def test_nix_failure_does_not_echo_process_errors(self):
        error = release_cache.subprocess.CalledProcessError(1, "nix", stderr="hostile-error-sentinel")
        with (
            patch.object(release_cache, "nix_json", side_effect=error),
            self.assertRaises(SystemExit) as failure,
        ):
            release_cache.main()
        self.assertEqual(str(failure.exception), "release cache check: unable to inspect Nix build dependencies")

    def test_dynamic_derivation_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "dynamic derivations"):
            release_cache.inputs({"inputs": {"drvs": {"dynamic.drv": {
                "outputs": ["out"], "dynamicOutputs": {"out": {"outputs": ["out"]}},
            }}}})


if __name__ == "__main__":
    unittest.main()
