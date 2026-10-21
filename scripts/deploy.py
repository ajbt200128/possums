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
import urllib.error
import urllib.request
from contextlib import suppress
from pathlib import Path

import release as r

NAME = "gateway"
OLD_DOMAIN = "possum-phase0.possums.containers.tinfoil.dev"
DOMAIN = "gateway.possums.dev"
CLI_SHA256 = "ef014c67a976b4b47941d084fb0a3837ba6eb59c9f4e662424c6826fb7a70f8c"
CLI_URL = "https://github.com/tinfoilsh/tinfoil-cli/releases/download/v0.19.0/tinfoil-cli_0.19.0_linux_amd64.tar.gz"
LIMIT = 16 << 20
COMPLETION_TIMEOUT_SECONDS = 900
INTAKE_REFS = {"POSSUMS_INTAKE_ORIGIN", "POSSUMS_INTAKE_LEDGER_KEY"}
LEGACY_REF = "POSSUMS_ACCOUNTS_JSON"
TOR_REFS = {"POSSUMS_TOR_IDENTITY_NATIVE_KEY", "POSSUMS_TOR_EXPECTED_ONION"}
REQUIRED_REFS = INTAKE_REFS | {"TINFOIL_API_KEY", "OTEL_EXPORTER_OTLP_HEADERS"}
STABLE = ("id", "name", "repo", "project_id", "cpus", "gpus", "memory_mb", "variables", "secrets",
          "ssh_keys", "debug", "disable_cc_mode", "domain", "held", "volume_slots", "volumes",
          "host_gpu_type", "host_cpu_type")
r.CODES.update({
    "environment_unreadable": ("approval-policy", "Provision and expose read access to the existing production policy; no environment was created."),
    "environment_identity": ("approval-policy", "Restore the separately approved existing production environment."),
    "reviewer": ("approval-policy", "Require only the approved operator account as reviewer, with self-review permitted."),
    "approval": ("approval-policy", "Require the operator's authenticated production review on this exact run; bypassing a gate is not approval."),
    "branch_policy": ("approval-policy", "Restrict production deployment branches to main only, with no tag patterns."),
    "serving_tool": ("serving", "Local executable qualification failed; cause unknown. Restore the reviewed verifier built with the pinned Go compiler and check runner compatibility; do not accept serving verification."),
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
    "not_stable": ("plan", "Verify approved intake reference delivery and absence of legacy account authority, as well as stable running, confidential, non-held state, before a separately authorized deployment."),
    "plan_http": ("pre-update-plan", "Check production admin planning access and the observed HTTP status; no update was sent."),
    "plan_fetch": ("pre-update-plan", "The read-only plan could not be acquired; cause unknown. Check control-plane connectivity; no update was sent."),
    "plan_rejected": ("pre-update-plan", "The read-only plan did not match the approved source, hostname, reference and configuration constraints; no update was sent."),
    "intake_cutover_input": ("cutover-selection", "Select only the ordinary update or the explicitly approved intake cutover; no update was sent."),
    "intake_cutover_config": ("cutover-config", "The signed candidate does not declare exactly the approved intake and preserved service/Tor references; no update was sent."),
    "intake_cutover_state": ("cutover-state", "Require stable confidential legacy production with both new intake selections absent and the approved service/Tor selections retained. Do not repeat an already completed or uncertain cutover; no update was sent."),
    "intake_cutover_custody": ("cutover-custody", "Confirm exclusive gateway configuration mutation custody from protected approval until the outcome is reconciled. Keep all other console/API/project writers idle; this assertion is not a server lock. No update was sent."),
    "plan_missing": ("post-call-plan", "The update may have happened. Privately reconcile the missing CLI plan; never replay automatically."),
    "plan_strategy": ("post-call-plan", "The update may have happened; completion is unknown. Privately reconcile the unexpected strategy, hold or downtime policy; no replay or rollback."),
    "plan_target": ("post-call-plan", "The update may have happened; completion is unknown. Privately reconcile the unexpected source or target; no replay or rollback."),
    "plan_configuration": ("post-call-plan", "The update may have happened; completion is unknown. Privately reconcile configuration/reference changes; no replay or rollback."),
    "cli_uncertain": ("mutation", "The update outcome is uncertain. Inspect state privately; never rerun or automatically roll back."),
    "configuration_changed": ("completion", "The update may have happened; completion is unknown. Privately reconcile changed opaque configuration; no replay or rollback."),
    "candidate_failed": ("completion-candidate", "The requested update candidate is reported failed; its initiating cause, cleanup and serving are unknown. Inspect supported startup diagnostics privately; no replay or rollback."),
    "domain_source": ("plan-domain", "Production must use the approved old or new hostname; reconcile its configuration before a new deployment."),
    "domain_completion": ("completion-domain", "The release is running but the approved hostname was not confirmed; reconcile privately without replay or rollback."),
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


def serving_environment():
    # No admin/GitHub keys, cookies or ambient SDK configuration reach this tool.
    env = {key: os.environ[key] for key in ("PATH", "SSL_CERT_FILE") if key in os.environ}
    env["HOME"] = os.environ["RUNNER_TEMP"]
    return env


def serving_tool():
    try:
        path = Path(os.environ["RUNNER_TEMP"]) / "possums-verify-serving"
        r.require(path.is_file() and os.access(path, os.X_OK), "serving_tool")
        # Empty arguments fail before SDK construction: qualify local execution,
        # not serving, without credentials or network requests on either runner.
        result = r.capture([str(path)], env=serving_environment(), timeout=10, limit=65536,
                           code="serving_tool")
        r.require(result.returncode == 1 and result.stdout == b"", "serving_tool")
        failure = r.parse_json(result.stderr, "serving_tool")
        r.require(isinstance(failure, dict) and failure.get("code") == "SERVING_ARGUMENTS", "serving_tool")
        return str(path)
    except Exception:
        raise r.Stop("serving_tool") from None


def verify_serving(tag, manifest):
    result = r.capture([serving_tool(), tag, manifest], env=serving_environment(), timeout=180, limit=65536,
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


def cutover_custody():
    r.require(os.environ.get("INTAKE_CONFIGURATION_CUSTODY") == "true", "intake_cutover_custody")


def reference_transition():
    value = os.environ.get("REFERENCE_TRANSITION", "none")
    r.require(value in ("none", "intake"), "intake_cutover_input")
    if value == "intake":
        cutover_custody()
    return value == "intake"


def cutover_config(release):
    config = r.config_at(release)
    blocks = re.findall(rb"(?m)^    secrets:\n((?:      - [A-Z][A-Z0-9_]*\n)+)", config)
    r.require(len(blocks) == 1, "intake_cutover_config")
    names = [line[8:].decode("ascii") for line in blocks[0].splitlines()]
    r.require(len(names) == len(set(names)) and set(names) == REQUIRED_REFS | TOR_REFS,
              "intake_cutover_config")


def preflight(source, release, tag, image, manifest):
    intake_cutover = reference_transition()
    if intake_cutover:
        r.require(os.environ.get("GITHUB_SHA") == source, "stale")
    r.valid_pins(source, release, tag, image)
    r.require(re.fullmatch(r"[0-9a-f]{64}", manifest) is not None, "arguments")
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    environment_id = eligibility(source)
    r.verify_public(source, release, tag, image, manifest)
    if intake_cutover:
        cutover_config(release)
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


def get(cli, identifier=NAME):
    try:
        result = call(cli, ["container", "get", identifier, "--output", "json"])
        r.require(result.returncode == 0, "control_read")
        view = json.loads(result.stdout)
        r.require(isinstance(view, dict), "control_shape")
        # The official CLI omits these fields for a volume-free container.
        # Normalize only absence; preserve populated fields for comparison.
        view.setdefault("volume_slots", [])
        view.setdefault("volumes", {})
        r.require(all(field in view for field in STABLE)
                  and (identifier == NAME or view["id"] == identifier), "control_shape")
        return view
    except r.Stop:
        raise
    except Exception:
        raise r.Stop("control_read") from None


def unique_names(value):
    return (isinstance(value, list) and all(isinstance(name, str) and name for name in value)
            and len(value) == len(set(value)))


def stable_running(view, tag):
    return (view.get("repo") == r.REPO and view.get("current_tag") == tag
            and str(view.get("status")).lower() == "running" and not view.get("update_tag")
            and boolean(view.get("debug"), False) and boolean(view.get("disable_cc_mode"), False)
            and boolean(view.get("held"), False) and type(view.get("mark_latest_release")) is bool
            and unique_names(view.get("secrets"))
            and REQUIRED_REFS.issubset(set(view["secrets"]))
            and "POSSUMS_ACCOUNTS_JSON" not in view["secrets"]
            and all(field in view for field in STABLE))


def cutover_references(view):
    names = view.get("secrets")
    r.require(unique_names(names), "intake_cutover_state")
    selected = set(names)
    r.require(isinstance(view.get("id"), str)
              and re.fullmatch(r"[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}", view["id"]),
              "intake_cutover_state")
    r.require(LEGACY_REF in selected and not selected & INTAKE_REFS
              and ((REQUIRED_REFS - INTAKE_REFS) | TOR_REFS).issubset(selected)
              and view.get("domain") == DOMAIN and view.get("name") == NAME,
              "intake_cutover_state")
    target = sorted((selected - {LEGACY_REF}) | INTAKE_REFS)
    previous = view.get("current_tag")
    r.require(isinstance(previous, str) and re.fullmatch(r.VERSION, previous) is not None
              and stable_running({**view, "secrets": target}, previous), "intake_cutover_state")
    return target


def admit_cutover_snapshot(cli, before, target_refs):
    # The supported API has no state-bound update. Exclusive operator custody
    # must cover the remaining race; this refresh is not a lock or CAS.
    cutover_custody()
    current = get(cli, before["id"])
    r.require(cutover_references(current) == target_refs
              and current["current_tag"] == before["current_tag"]
              and current["mark_latest_release"] == before["mark_latest_release"]
              and all(current[field] == before[field] for field in STABLE if field != "secrets"),
              "intake_cutover_state")


def inspect_plan(raw, previous, tag, previous_domain=DOMAIN, current_resources=None, *, intake_cutover=False):
    try:
        target_refs = []
        cutover_id = None
        if intake_cutover:
            if not isinstance(current_resources, dict) or current_resources.get("current_tag") != previous:
                raise r.Stop("plan_configuration")
            target_refs = cutover_references(current_resources)
            cutover_id = current_resources["id"]
        text = raw.decode("utf-8").lstrip()
        plans = 0
        while text.startswith("{"):
            plan, end = json.JSONDecoder().raw_decode(text)
            plans += 1
            if intake_cutover:
                r.require(plan.get("instance_id") == cutover_id
                          and plan.get("name") == NAME, "plan_target")
            r.require(boolean(plan.get("read_only"), True) and plan.get("current", {}).get("tag") == previous
                      and plan.get("target", {}).get("tag") == tag, "plan_target")
            r.require(plan.get("update_strategy") == "blue_green" and boolean(plan.get("downtime_required"), False)
                      and boolean(plan.get("hold"), False), "plan_strategy")
            r.require(type(plan.get("mark_latest_release")) is bool, "plan_configuration")
            if current_resources is not None:
                r.require(type(current_resources.get("mark_latest_release")) is bool
                          and plan["mark_latest_release"] == current_resources["mark_latest_release"],
                          "plan_configuration")
            changes = plan["configuration_changes"]
            r.require(isinstance(changes, dict) and set(changes) <=
                      {"variables", "secrets", "ssh_keys", "settings", "secrets_refreshed"}, "plan_configuration")
            expected_settings = ["custom_domain"] if previous_domain == OLD_DOMAIN else []
            r.require(changes.get("settings") == expected_settings, "plan_configuration")
            for family in ("variables", "secrets", "ssh_keys"):
                change = changes[family]
                r.require(isinstance(change, dict) and set(change) <= {"added", "changed", "removed"},
                          "plan_configuration")
                for kind in ("added", "changed", "removed"):
                    names = change.get(kind, [])
                    # The official Go CLI serializes nil change slices as null.
                    # Preserve zero-change semantics.
                    if names is None:
                        names = []
                    expected = set()
                    if intake_cutover and family == "secrets":
                        if kind == "added":
                            expected = INTAKE_REFS
                        elif kind == "removed":
                            expected = {LEGACY_REF}
                    r.require(unique_names(names) and set(names) == expected, "plan_configuration")
            if intake_cutover:
                refreshed = changes.get("secrets_refreshed")
                if refreshed is not None:
                    r.require(unique_names(refreshed) and set(refreshed).issubset(target_refs),
                              "plan_configuration")
            for field in ("cpus", "gpus", "memory_mb"):
                r.require(type(plan["current"].get(field)) is int and type(plan["target"].get(field)) is int
                          and plan["current"][field] == plan["target"][field], "plan_configuration")
                if current_resources is not None:
                    r.require(plan["current"][field] == current_resources[field], "plan_configuration")
            text = text[end:].lstrip()
        r.require(plans > 0, "plan_missing")
    except r.Stop:
        raise
    except Exception:
        raise r.Stop("plan_missing") from None


class NoPlanRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def plan_update(before, tag, *, intake_cutover=False):
    """The documented Admin API has a read-only plan route; the CLI has no plan-only command."""
    try:
        r.require(isinstance(before["id"], str) and re.fullmatch(r"[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}", before["id"]), "plan_rejected")
        r.require(type(before.get("mark_latest_release")) is bool, "plan_rejected")
        fields = {"tag": tag, "custom_domain": DOMAIN,
                  "mark_latest_release": before["mark_latest_release"]}
        if intake_cutover:
            fields["secrets"] = cutover_references(before)
        body = json.dumps(fields).encode()
        request = urllib.request.Request("https://api.tinfoil.sh/api/containers/" + before["id"] + "/update/plan",
            data=body, headers={"Authorization": "Bearer " + os.environ["TINFOIL_ADMIN_KEY"],
                               "Content-Type": "application/json",
                               "Accept": "application/json",
                               "User-Agent": "possums-release-verifier/1"}, method="POST")
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoPlanRedirect())
        with opener.open(request, timeout=30) as response:
            raw = response.read(LIMIT + 1)
        r.require(len(raw) <= LIMIT, "plan_rejected")
        plan = json.loads(raw)
        r.require(isinstance(plan, dict), "plan_rejected")
        r.require(plan.get("instance_id") == before["id"] and plan.get("name") == NAME, "plan_rejected")
        try:
            inspect_plan(raw, before["current_tag"], tag, before["domain"], current_resources=before,
                         intake_cutover=intake_cutover)
        except r.Stop:
            raise r.Stop("plan_rejected") from None
    except urllib.error.HTTPError as error:
        status = error.code
        # Cleanup must not erase the observed HTTP failure or expose raw errors.
        with suppress(Exception):
            error.close()
        raise r.Stop("plan_http", status) from None
    except r.Stop:
        raise
    except (ValueError, KeyError, TypeError, AttributeError):
        raise r.Stop("plan_rejected") from None
    except Exception:
        raise r.Stop("plan_fetch") from None


def update_once(cli, tag, final_gate, *, intake_cutover=False):
    """Testable mutation primitive; entry point verifies policy and public provenance.

    CLI's plan is reviewed POST-CALL: stable blue/green can already execute.
    Closed stdin refuses interactive downtime/replacement consent. Never replay.
    """
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    r.require(bool(os.environ.get("TINFOIL_ADMIN_KEY")), "credential")
    if intake_cutover:
        cutover_custody()
    before = get(cli)
    r.require(type(before.get("mark_latest_release")) is bool, "control_shape")
    previous = before.get("current_tag")
    r.require(before.get("domain") in (OLD_DOMAIN, DOMAIN), "domain_source")
    target_refs = cutover_references(before) if intake_cutover else []
    r.require(isinstance(previous, str) and re.fullmatch(r.VERSION, previous) is not None
              and previous != tag and (intake_cutover or stable_running(before, previous)), "not_stable")
    if intake_cutover:
        plan_update(before, tag, intake_cutover=True)
    else:
        plan_update(before, tag)
    identifier = before["id"] if intake_cutover else NAME
    arguments = ["container", "update", identifier, "--tag", tag,
                 "--custom-domain", DOMAIN, "--output", "json",
                 "--mark-latest=" + str(before["mark_latest_release"]).lower()]
    if intake_cutover:
        for name in target_refs:
            arguments.extend(["--secret", name])
        admit_cutover_snapshot(cli, before, target_refs)
    final_gate()  # Main/policy recheck AFTER approval and read-only planning, immediately before invocation.
    try:
        result = call(cli, arguments, timeout=1800)
    except Exception:
        raise r.Stop("cli_uncertain") from None
    r.require(result.returncode == 0, "cli_uncertain")
    inspect_plan(result.stderr, previous, tag, before["domain"], current_resources=before,
                 intake_cutover=intake_cutover)
    deadline = time.monotonic() + COMPLETION_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        after = get(cli, identifier) if intake_cutover else get(cli)
        r.require(type(after.get("mark_latest_release")) is bool
                  and after["mark_latest_release"] == before["mark_latest_release"], "configuration_changed")
        unchanged = [field for field in STABLE
                     if field != "domain" and not (intake_cutover and field == "secrets")]
        r.require(all(before[field] == after[field] for field in unchanged)
                  and after.get("domain") in (before["domain"], DOMAIN), "configuration_changed")
        if intake_cutover:
            names = after.get("secrets")
            r.require(unique_names(names) and set(names) in (set(before["secrets"]), set(target_refs)),
                      "configuration_changed")
        r.require(not (after.get("update_tag") == tag and after.get("update_status") == "failed"),
                  "candidate_failed")
        if stable_running(after, tag):
            r.require(after["domain"] == DOMAIN, "domain_completion")
            if intake_cutover:
                r.require(set(after["secrets"]) == set(target_refs), "configuration_changed")
            result = {"cliSucceeded": True, "postCallPlanReviewed": True,
                      "opaqueConfigurationPreserved": not intake_cutover, "approvedDomainConfirmed": True,
                      "targetRunningNoPending": True, "mutationCommandCount": 1, "servingVerified": False}
            if intake_cutover:
                result.update(approvedIntakeReferencesConfirmed=True, unrelatedConfigurationPreserved=True,
                              exclusiveConfigurationCustodyAsserted=True)
            return result
        time.sleep(10)
    raise r.Stop("completion_timeout")


def protected(source, release, tag, image, manifest, environment_id):
    intake_cutover = reference_transition()
    if intake_cutover:
        r.require(os.environ.get("GITHUB_SHA") == source, "stale")
    r.valid_pins(source, release, tag, image)
    r.require(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "rerun")
    eligibility(source, environment_id)
    approved(environment_id)
    r.verify_public(source, release, tag, image, manifest)
    if intake_cutover:
        cutover_config(release)
    serving_tool()
    cli = install_cli()

    def final_gate():
        eligibility(source, environment_id)
        approved(environment_id)

    if intake_cutover:
        result = update_once(cli, tag, final_gate, intake_cutover=True)
    else:
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
    elif operation == "qualify-serving-tool" and not args:
        serving_tool()
        print(json.dumps({"passed": True, "stage": "serving-tool"}))
    else:
        raise r.Stop("arguments")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps(r.diagnostic(error.code, error.status) if isinstance(error, r.Stop) else r.diagnostic("internal")))
        sys.exit(1)
