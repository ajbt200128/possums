#!/usr/bin/env python3
"""Protected production gate, single-update wrapper and public serving check.

Activation is off by default. Approval and current main are rechecked before
one update. Control-plane configuration and CLI output remain memory-only.
"""
import io
import json
import os
import re
import sys
import tarfile
import time
from pathlib import Path

import release as r

NAME = "possum-phase0"
CLI_SHA256 = "ef014c67a976b4b47941d084fb0a3837ba6eb59c9f4e662424c6826fb7a70f8c"
CLI_URL = "https://github.com/tinfoilsh/tinfoil-cli/releases/download/v0.19.0/tinfoil-cli_0.19.0_linux_amd64.tar.gz"
LIMIT = 16 << 20
REQUIRED_REFS = {"POSSUMS_ACCOUNTS_JSON", "TINFOIL_API_KEY", "OTEL_EXPORTER_OTLP_HEADERS"}
STABLE = ("id", "name", "repo", "project_id", "cpus", "gpus", "memory_mb", "variables", "secrets",
          "ssh_keys", "debug", "disable_cc_mode", "domain", "held", "volume_slots", "volumes",
          "host_gpu_type", "host_cpu_type")
r.CODES.update({
    "environment_unreadable": ("approval-policy", "Provision and expose read access to the existing production policy; no environment was created."),
    "environment_identity": ("approval-policy", "Restore the separately approved existing production environment."),
    "reviewer": ("approval-policy", "Require only the approved operator account as reviewer, with self-review permitted."),
    "approval": ("approval-policy", "Require the operator's authenticated production review on this exact run; bypassing a gate is not approval."),
    "branch_policy": ("approval-policy", "Restrict production deployment branches to main only, with no tag patterns."),
    "serving_tool": ("serving", "Build the reviewed serving verifier with the pinned Go compiler before requesting approval."),
    "SERVING_ARGUMENTS": ("serving-input", "Provide only the verified paid tag and manifest digest."),
    "SERVING_SDK_CONFIG": ("serving-configuration", "Check the pinned SDK and release reference; do not accept serving verification."),
    "SERVING_SDK_FETCH": ("serving-evidence", "Check public attestation availability; no deployment was replayed."),
    "SERVING_SDK_APPRAISAL": ("serving-appraisal", "Resolve the authenticated release or endpoint binding mismatch; no deployment was replayed."),
    "SERVING_DIGEST": ("serving-appraisal", "Resolve the serving workload's mismatch with the verified manifest."),
    "SERVING_FRESHNESS": ("serving-appraisal", "Obtain fresh authenticated witness evidence; do not accept serving verification."),
    "SERVING_BOUND_CLIENT": ("serving-transport", "Inspect the pinned SDK's bound-client configuration."),
    "SERVING_REDIRECT": ("serving-http", "Check the fixed serving endpoint; no redirect was followed."),
    "SERVING_BOUND_TRANSPORT": ("serving-transport", "Check public endpoint connectivity; the underlying cause is unknown."),
    "SERVING_HTTP_STATUS": ("serving-http", "Check the observed public endpoint status; do not replay deployment."),
    "SERVING_UNKNOWN": ("serving", "The verification cause is unknown. Inspect privately; deployment is not replayed."),
    "rerun": ("mutation", "Do not rerun an update. Reconcile the previous outcome privately, then request a separately reviewed fresh deployment."),
    "credential": ("authorization", "Provision the supported admin credential only as a production environment secret."),
    "control_read": ("control-plane", "Inspect the production state privately; no update was replayed."),
    "control_shape": ("control-plane", "Resolve the unsupported control-plane response without sharing its contents."),
    "not_stable": ("plan", "Reconcile pending, held, debug, non-confidential or non-running production state before a new request."),
    "plan_missing": ("post-call-plan", "The update may have happened. Privately reconcile the missing CLI plan; never replay automatically."),
    "plan_strategy": ("post-call-plan", "Privately reconcile the unexpected strategy, hold or downtime policy; no replay or rollback."),
    "plan_target": ("post-call-plan", "Privately reconcile the unexpected source or target; no replay or rollback."),
    "plan_configuration": ("post-call-plan", "Privately reconcile configuration/reference changes; no replay or rollback."),
    "cli_uncertain": ("mutation", "The update outcome is uncertain. Inspect state privately; never rerun or automatically roll back."),
    "configuration_changed": ("completion", "Privately reconcile changed opaque configuration; no replay or rollback."),
    "completion_timeout": ("completion", "Inspect the pending target privately; no replay or rollback."),
    "cli_checksum": ("tool", "Resolve the pinned official CLI archive mismatch before provisioning credentials."),
})


def boolean(value, expected):
    """Require a JSON boolean: equality alone would accept numeric zero/one."""
    return isinstance(value, bool) and value == expected


def policy(view, branches, reviewer_id, expected_id=None):
    r.require(view["name"] == "production" and type(view["id"]) is int
              and (expected_id is None or str(view["id"]) == expected_id), "environment_identity")
    rules = [rule for rule in view["protection_rules"] if rule["type"] == "required_reviewers"]
    r.require(len(rules) == 1, "reviewer")
    rule = rules[0]
    r.require(boolean(rule.get("prevent_self_review"), False) and len(rule["reviewers"]) == 1, "reviewer")
    reviewer = rule["reviewers"][0]
    r.require(reviewer["type"] == "User" and reviewer["reviewer"]["login"] == "ajbt200128"
              and reviewer["reviewer"]["id"] == reviewer_id, "reviewer")
    # REST does not expose the administrator-bypass setting. The protected
    # entry separately requires an authenticated review, not merely gate entry.
    r.require(view["deployment_branch_policy"] == {"protected_branches": False, "custom_branch_policies": True}, "branch_policy")
    r.require(branches["total_count"] == 1 and len(branches["branch_policies"]) == 1
              and branches["branch_policies"][0]["name"] == "main"
              and branches["branch_policies"][0]["type"] == "branch", "branch_policy")
    return str(view["id"])


def live_policy(expected_id=None):
    try:
        view = r.api("environments/production")
        branches = r.api("environments/production/deployment-branch-policies?per_page=100")
        reviewer = json.loads(r.command(["gh", "api", "users/ajbt200128"]))
        return policy(view, branches, reviewer["id"], expected_id)
    except r.Stop as error:
        if error.code == "command":
            raise r.Stop("environment_unreadable") from None
        raise
    except Exception:
        raise r.Stop("environment_unreadable") from None


def eligibility(source, expected_id=None):
    r.require(os.environ.get("GITHUB_REPOSITORY") == r.REPO
              and os.environ.get("GITHUB_REF") == "refs/heads/main"
              and os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "event")
    # GitHub supplies activation snapshots; disabling a switch is not guaranteed
    # to revoke an already queued/approved run. Approval and main are rechecked.
    for name in ("PRODUCTION_DEPLOYMENT_ENABLED", "RELEASE_AUTOMATION_ENABLED"):
        r.require(os.environ.get(name) == "true", "disabled")
    environment_id = live_policy(expected_id)
    r.latest(source)
    return environment_id


def approved(environment_id):
    run = os.environ.get("GITHUB_RUN_ID", "")
    r.require(re.fullmatch(r"[1-9][0-9]*", run) is not None, "approval")
    reviews = r.api(f"actions/runs/{run}/approvals")
    r.require(isinstance(reviews, list), "approval")
    decisions = [review for review in reviews if review.get("state") in {"approved", "rejected"}
                 and any(str(env.get("id")) == environment_id and env.get("name") == "production"
                         for env in review.get("environments", []))]
    # Run attempts >1 are refused separately: a previous review cannot authorize
    # a mutation rerun. A bypass has no matching operator approval receipt.
    r.require(len(decisions) == 1 and decisions[0]["state"] == "approved", "approval")
    user = decisions[0]["user"]
    reviewer = r.parse_json(r.command(["gh", "api", "users/ajbt200128"]))
    r.require(user["login"] == "ajbt200128" and user["id"] == reviewer["id"], "approval")


def serving_tool():
    try:
        path = Path(os.environ["RUNNER_TEMP"]) / "possums-verify-serving"
        r.require(path.is_file() and os.access(path, os.X_OK), "serving_tool")
        return str(path)
    except (KeyError, OSError):
        raise r.Stop("serving_tool") from None


def verify_serving(tag, manifest):
    # No admin/GitHub keys, cookies or ambient SDK configuration reach this tool.
    env = {key: os.environ[key] for key in ("PATH", "SSL_CERT_FILE") if key in os.environ}
    env["HOME"] = os.environ["RUNNER_TEMP"]
    result = r.capture([serving_tool(), tag, manifest], env=env, timeout=180, limit=65536,
                       code="SERVING_UNKNOWN")
    if result.returncode == 0:
        r.require(r.parse_json(result.stdout, "SERVING_UNKNOWN") == {"stage": "serving", "passed": True},
                  "SERVING_UNKNOWN")
        return
    failure = r.parse_json(result.stderr, "SERVING_UNKNOWN")
    code = failure.get("code") if isinstance(failure, dict) else None
    if code not in r.CODES or not code.startswith("SERVING_"):
        raise r.Stop("SERVING_UNKNOWN")
    raise r.Stop(code, failure.get("status"))


def preflight(source, release, tag, image, manifest):
    r.valid_pins(source, release, tag, image)
    r.require(re.fullmatch(r"[0-9a-f]{64}", manifest) is not None, "arguments")
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    environment_id = eligibility(source)
    r.verify_public(source, release, tag, image, manifest)
    serving_tool()
    r.output(environment_id=environment_id)


def install_cli():
    archive = r.command(["curl", "--fail", "--silent", "--show-error", "--location", "--max-time", "120", CLI_URL])
    r.require(r.hashlib.sha256(archive).hexdigest() == CLI_SHA256, "cli_checksum")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as file:
        members = [member for member in file.getmembers() if member.name == "tinfoil" and member.isfile()]
        r.require(len(members) == 1 and members[0].size <= 96 << 20, "cli_checksum")
        stream = file.extractfile(members[0])
        if stream is None:
            raise r.Stop("cli_checksum")
        binary = stream.read()
    path = Path(os.environ["RUNNER_TEMP"]) / "possums-tinfoil-0.19.0"
    path.write_bytes(binary)
    path.chmod(0o700)
    return str(path)


def call(cli, arguments, timeout=120):
    # Only documented admin env authentication; no login/config persistence,
    # inference key fallback, custom controlplane endpoint or passive update check.
    env = {key: os.environ[key] for key in ("PATH", "HOME", "SSL_CERT_FILE") if key in os.environ}
    config = Path(os.environ["RUNNER_TEMP"]) / "possums-unused-admin-config.json"
    r.require(not config.exists(), "credential")
    env.update(TINFOIL_ADMIN_KEY=os.environ.get("TINFOIL_ADMIN_KEY", ""), TINFOIL_NO_UPDATE_CHECK="1",
               TINFOIL_CONFIG=str(config))
    return r.capture([cli, *arguments], env=env, timeout=timeout, limit=LIMIT, code="cli_uncertain")


def get(cli):
    try:
        result = call(cli, ["container", "get", NAME, "--output", "json"])
        r.require(result.returncode == 0, "control_read")
        view = json.loads(result.stdout)
        r.require(isinstance(view, dict), "control_shape")
        # The official CLI omits these fields for a volume-free container.
        # Normalize only absence; preserve populated fields for comparison.
        view.setdefault("volume_slots", [])
        view.setdefault("volumes", {})
        r.require(all(field in view for field in STABLE), "control_shape")
        return view
    except r.Stop:
        raise
    except Exception:
        raise r.Stop("control_read") from None


def stable_running(view, tag):
    return (view.get("repo") == r.REPO and view.get("current_tag") == tag
            and str(view.get("status")).lower() == "running" and not view.get("update_tag")
            and boolean(view.get("debug"), False) and boolean(view.get("disable_cc_mode"), False)
            and boolean(view.get("held"), False) and isinstance(view.get("secrets"), list)
            and REQUIRED_REFS.issubset(set(view["secrets"]))
            and all(field in view for field in STABLE))


def inspect_plan(raw, previous, tag):
    try:
        text = raw.decode("utf-8").lstrip()
        plans = 0
        while text.startswith("{"):
            plan, end = json.JSONDecoder().raw_decode(text)
            plans += 1
            r.require(boolean(plan.get("read_only"), True) and plan.get("current", {}).get("tag") == previous
                      and plan.get("target", {}).get("tag") == tag, "plan_target")
            r.require(plan.get("update_strategy") == "blue_green" and boolean(plan.get("downtime_required"), False)
                      and boolean(plan.get("hold"), False), "plan_strategy")
            changes = plan["configuration_changes"]
            r.require(not changes.get("settings"), "plan_configuration")
            for family in ("variables", "secrets", "ssh_keys"):
                change = changes[family]
                r.require(not any(change.get(kind) for kind in ("added", "changed", "removed")), "plan_configuration")
            for field in ("cpus", "gpus", "memory_mb"):
                r.require(field in plan["current"] and field in plan["target"]
                          and plan["current"][field] == plan["target"][field], "plan_configuration")
            text = text[end:].lstrip()
        r.require(plans > 0, "plan_missing")
    except r.Stop:
        raise
    except Exception:
        raise r.Stop("plan_missing") from None


def update_once(cli, tag, final_gate):
    """Testable mutation primitive; entry point verifies policy and public provenance.

    CLI's plan is reviewed POST-CALL: stable blue/green can already execute.
    Closed stdin refuses interactive downtime/replacement consent. Never replay.
    """
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    r.require(bool(os.environ.get("TINFOIL_ADMIN_KEY")), "credential")
    before = get(cli)
    previous = before.get("current_tag")
    r.require(isinstance(previous, str) and re.fullmatch(r.VERSION, previous) is not None
              and previous != tag and stable_running(before, previous), "not_stable")
    final_gate()  # Main/policy recheck AFTER approval, immediately before invocation.
    try:
        result = call(cli, ["container", "update", NAME, "--tag", tag, "--output", "json"], timeout=1800)
    except Exception:
        raise r.Stop("cli_uncertain") from None
    r.require(result.returncode == 0, "cli_uncertain")
    inspect_plan(result.stderr, previous, tag)
    deadline = time.monotonic() + 300
    while time.monotonic() < deadline:
        after = get(cli)
        r.require(all(before[field] == after[field] for field in STABLE), "configuration_changed")
        if stable_running(after, tag):
            return {"cliSucceeded": True, "postCallPlanReviewed": True, "opaqueConfigurationPreserved": True,
                    "targetRunningNoPending": True, "mutationCommandCount": 1, "servingVerified": False}
        time.sleep(10)
    raise r.Stop("completion_timeout")


def protected(source, release, tag, image, manifest, environment_id):
    r.valid_pins(source, release, tag, image)
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    eligibility(source, environment_id)
    approved(environment_id)
    r.verify_public(source, release, tag, image, manifest)
    serving_tool()
    cli = install_cli()

    def final_gate():
        eligibility(source, environment_id)
        approved(environment_id)

    result = update_once(cli, tag, final_gate)
    # Preserve observed deployment success even if the later serving check fails.
    print(json.dumps(result), flush=True)
    verify_serving(tag, manifest)
    print(json.dumps({"passed": True, "stage": "serving", "servingVerified": True}), flush=True)


def main():
    operation, *args = sys.argv[1:]
    if operation == "preflight" and len(args) == 5:
        preflight(*args)
    elif operation == "protected" and len(args) == 6:
        protected(*args)
    else:
        raise r.Stop("arguments")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps(r.diagnostic(error.code, error.status) if isinstance(error, r.Stop) else r.diagnostic("internal")))
        sys.exit(1)
