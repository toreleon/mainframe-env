#!/usr/bin/env python3
"""Verify a current, externally pinned IBM IMS 15.6 licensed differential receipt."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys
from typing import Any

import generate_ims_licensed_fixtures as generator


ROOT = Path(__file__).resolve().parents[3]
ADAPTER = Path("conformance/0.14/oracles/ims-licensed-differential.json")
FIXTURES = Path("conformance/0.14/fixtures/ims-licensed-differential-cases.json")
VERIFIER = Path("conformance/0.14/tools/verify_ims_licensed_differential.py")
BASELINE = "ibm-ims-15.6-dli-2026-08-31"
WORK_PACKAGE = "IMS-1406.licensed-harness-contract"
MAX_BYTES = 2 * 1024 * 1024
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
GIT_SHA = re.compile(r"[0-9a-f]{40}\Z")
SAFE_ID = re.compile(r"[A-Za-z0-9_.:@+/-]{1,160}\Z")
STATUS = re.compile(r"[A-Z0-9 ]{2}\Z")
RELEASE = re.compile(r"15\.6\.[0-9]{1,3}\Z")
TIMESTAMP = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z\Z")
SPEC_DOMAIN = b"mainframe-env.ims-licensed-spec@1\0"
FIXTURE_DOMAIN = b"mainframe-env.ims-licensed-fixtures@1\0"
OBSERVATION_FIELDS = [
    "outcome_class", "pcb_status", "pcb_kind", "position_state", "position_digest",
    "mutation_state", "state_digest", "effect_digest", "output_digest", "output_length",
    "output_encoding",
]
MUTANTS = [
    "omitted-row", "duplicate-row", "reordered-cases", "generic-success",
    "malformed-status", "malformed-position", "stale-candidate", "stale-spec",
    "stale-source", "unauthorized-environment", "internal-receipt-path",
    "missing-external-pin", "tampered-observation",
]
PIN_ENV = {
    "receipt_sha256": "MAINFRAME_ENV_IMS_LICENSED_ORACLE_RECEIPT_SHA256",
    "service_identity_sha256": "MAINFRAME_ENV_IMS_LICENSED_SERVICE_IDENTITY_SHA256",
    "environment_manifest_sha256": "MAINFRAME_ENV_IMS_LICENSED_ENVIRONMENT_SHA256",
    "authorization_grant_sha256": "MAINFRAME_ENV_IMS_LICENSED_AUTHORIZATION_SHA256",
    "fixture_bundle_sha256": "MAINFRAME_ENV_IMS_LICENSED_FIXTURE_BUNDLE_SHA256",
    "product_runner_sha256": "MAINFRAME_ENV_IMS_PRODUCT_RUNNER_SHA256",
    "oracle_runner_sha256": "MAINFRAME_ENV_IMS_LICENSED_RUNNER_SHA256",
}


class VerificationError(ValueError):
    """The contract or receipt cannot grant differential credit."""


@dataclass(frozen=True)
class CandidateIdentity:
    commit_sha: str
    tree_sha: str
    tracked_worktree_clean: bool


@dataclass(frozen=True)
class ContractContext:
    adapter: dict[str, Any]
    fixtures: dict[str, Any]
    cases: list[dict[str, Any]]
    source_identity: dict[str, Any]
    spec_digest: str
    verifier_digest: str
    fixture_contract_digest: str


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def unique_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def parse_json(body: bytes, label: str) -> dict[str, Any]:
    require(0 < len(body) <= MAX_BYTES, f"{label} is empty or too large")
    try:
        value = json.loads(body, object_pairs_hook=unique_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"{label}: {error}") from error
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def load(path: Path) -> dict[str, Any]:
    try:
        return parse_json(path.read_bytes(), str(path))
    except OSError as error:
        raise VerificationError(f"{path}: {error}") from error


def sha256(body: bytes) -> str:
    return "sha256:" + hashlib.sha256(body).hexdigest()


def file_digest(path: Path) -> str:
    return sha256(path.read_bytes())


def canonical_digest(value: Any) -> str:
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode())


def checked_digest(value: Any, label: str) -> str:
    require(isinstance(value, str) and DIGEST.fullmatch(value) is not None, f"{label} is not a SHA-256 digest")
    return value


def checked_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and SAFE_ID.fullmatch(value) is not None, f"{label} is not a bounded identity")
    return value


def exact_keys(value: Any, keys: list[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    require(set(value) == set(keys), f"{label} fields differ")
    return value


def safe_path(root: Path, relative: str) -> Path:
    value = PurePosixPath(relative)
    require(
        isinstance(relative, str) and not value.is_absolute() and value.as_posix() == relative
        and all(part not in {"", ".", ".."} for part in value.parts),
        f"unsafe spec path {relative!r}",
    )
    path = root.joinpath(*value.parts)
    require(path.is_file() and not path.is_symlink(), f"missing regular spec path {relative}")
    return path


def bundle_digest(root: Path, paths: list[str], domain: bytes) -> str:
    require(paths and len(paths) == len(set(paths)), "spec paths are empty or duplicate")
    hasher = hashlib.sha256(domain)
    for relative in sorted(paths):
        body = safe_path(root, relative).read_bytes()
        for field in (relative.encode(), body):
            hasher.update(len(field).to_bytes(8, "big"))
            hasher.update(field)
    return "sha256:" + hasher.hexdigest()


def topic_set_digest(manifest: dict[str, Any]) -> str:
    rows = manifest.get("topics")
    require(isinstance(rows, list) and rows, "IMS source manifest has no topics")
    lines = []
    for row in rows:
        exact_keys(row, ["topic_path", "sha256", "bytes", "last_modified"], "IMS topic pin")
        require(isinstance(row["topic_path"], str) and row["topic_path"].startswith("SSEPH2_15.6.0/"), "IMS topic path drifted")
        require(isinstance(row["sha256"], str) and re.fullmatch(r"[0-9a-f]{64}", row["sha256"]) is not None, "IMS topic hash malformed")
        require(type(row["bytes"]) is int and 0 < row["bytes"] <= 2_000_000, "IMS topic byte count invalid")
        lines.append(f"{row['topic_path']} {row['sha256']}\n")
    require(len(lines) == len(set(lines)), "IMS source manifest has duplicate topics")
    require(manifest.get("topic_count") == len(lines), "IMS topic count drifted")
    return sha256("".join(sorted(lines)).encode())


def verify_contract(root: Path = ROOT) -> ContractContext:
    adapter = load(root / ADAPTER)
    fixtures = load(root / FIXTURES)
    require(adapter.get("schema_version") == "mainframe-env.ims-licensed-differential-adapter@1", "IMS adapter version drifted")
    require(adapter.get("target_version") == "0.14.0" and adapter.get("work_package") == WORK_PACKAGE, "IMS adapter target drifted")
    require(fixtures == generator.generated_document(root), "IMS fixture index differs from its independent generator")
    denominator = adapter.get("denominator")
    require(isinstance(denominator, dict) and denominator.get("baseline_id") == BASELINE and denominator.get("unit") == "dli-call-families" and denominator.get("family_count") == 25, "IMS denominator drifted")
    source_identity: dict[str, Any] = {
        "baseline_id": BASELINE,
        "unit": "dli-call-families",
        "documentation_product": "SSEPH2_15.6.0",
        "official_catalog_sha256": None,
        "applicability_rules_sha256": None,
        "source_manifests": [],
        "comparison_manifest_sha256": None,
        "comparison_topic_set_sha256": None,
        "comparison_toc_sha256": None,
        "comparison_topic_path": None,
        "comparison_topic_sha256": None,
        "family_count": 25,
    }
    for key, expected in [
        ("official_catalog", "conformance/0.2/catalogs/ims.json"),
        ("applicability_rules", "conformance/0.14/ims/call-applicability-rules.json"),
    ]:
        pin = exact_keys(denominator.get(key), ["path", "sha256"], f"IMS {key} pin")
        require(pin["path"] == expected and pin["sha256"] == file_digest(safe_path(root, expected)), f"IMS {key} source pin drifted")
        source_identity[key + "_sha256"] = pin["sha256"]
    expected_scopes = ["programming", "database", "tm", "metadata", "recovery-utilities"]
    pins = denominator.get("source_manifests")
    require(isinstance(pins, list) and len(pins) == 5, "IMS source manifest set drifted")
    for scope, pin in zip(expected_scopes, pins, strict=True):
        exact_keys(pin, ["path", "sha256", "baseline_id", "topic_set_sha256", "toc_sha256"], "IMS source manifest pin")
        expected = f"conformance/0.14/manifests/ims-{scope}-contracts-topics.json"
        require(pin["path"] == expected and pin["sha256"] == file_digest(safe_path(root, expected)), f"IMS {scope} manifest pin drifted")
        manifest = load(root / expected)
        require(manifest.get("baseline_id") == pin["baseline_id"] and manifest.get("product") == "SSEPH2_15.6.0", f"IMS {scope} manifest identity drifted")
        require(pin["topic_set_sha256"] == topic_set_digest(manifest) == "sha256:" + str(manifest.get("topic_manifest_digest")), f"IMS {scope} topic set drifted")
        require(pin["toc_sha256"] == "sha256:" + str(manifest.get("toc_sha256")), f"IMS {scope} TOC pin drifted")
        source_identity["source_manifests"].append(pin)
    comparison = exact_keys(denominator.get("comparison_topic"), ["path", "sha256"], "IMS comparison topic")
    base_path = "conformance/0.2/manifests/ims-topics.json"
    base_pin = exact_keys(denominator.get("comparison_manifest"), ["path", "sha256", "topic_set_sha256", "toc_sha256"], "IMS comparison manifest")
    require(base_pin["path"] == base_path and base_pin["sha256"] == file_digest(safe_path(root, base_path)), "IMS comparison manifest pin drifted")
    base_manifest = load(root / base_path)
    require(base_pin["topic_set_sha256"] == topic_set_digest(base_manifest) == "sha256:" + str(base_manifest.get("topic_manifest_digest")), "IMS comparison topic set drifted")
    require(base_pin["toc_sha256"] == "sha256:" + str(base_manifest.get("toc_sha256")), "IMS comparison TOC pin drifted")
    topic = base_manifest["topics"][0]
    require(base_manifest.get("baseline_id") == BASELINE and comparison == {"path": topic["topic_path"], "sha256": "sha256:" + topic["sha256"]}, "IMS comparison source drifted")
    require(denominator.get("reviewed_source_topics") == [{"path": comparison["path"], "sha256": comparison["sha256"], "method": "retained-html-sha256-and-plain-text", "coverage_credit": 0}], "IMS source review identity drifted")
    source_identity["comparison_topic_path"] = comparison["path"]
    source_identity["comparison_topic_sha256"] = comparison["sha256"]
    source_identity["comparison_manifest_sha256"] = base_pin["sha256"]
    source_identity["comparison_topic_set_sha256"] = base_pin["topic_set_sha256"]
    source_identity["comparison_toc_sha256"] = base_pin["toc_sha256"]
    require(adapter.get("licensed_oracle") == {"product_name": "IBM IMS", "major_minor": "15.6", "documentation_product": "SSEPH2_15.6.0", "service_identity_policy": "externally-pinned-exact-release-service-build-installation", "execution_role": "oracle-only", "native_product_fallback_allowed": False}, "IMS licensed oracle policy drifted")
    require(adapter.get("receipt_configuration") == {"receipt_environment_variable": "MAINFRAME_ENV_IMS_LICENSED_ORACLE_RECEIPT", "external_digest_environment_variables": PIN_ENV, "external_regular_file_required": True, "symlink_allowed": False, "maximum_receipt_bytes": MAX_BYTES}, "IMS external receipt policy drifted")
    require(adapter.get("candidate_identity") == {"source": "clean-git-head-and-tree", "required_fields": ["commit_sha", "tree_sha", "tracked_worktree_clean"]}, "IMS candidate policy drifted")
    spec = adapter.get("spec_identity")
    require(isinstance(spec, dict) and spec.get("algorithm") == "sha256" and spec.get("domain") == "mainframe-env.ims-licensed-spec@1", "IMS spec identity policy drifted")
    expected_spec_paths = [
        "conformance/0.14/fixtures/ims-licensed-differential-cases.json",
        "conformance/0.14/oracles/ims-licensed-differential.json",
        "conformance/0.14/schemas/ims-licensed-differential-adapter.schema.json",
        "conformance/0.14/schemas/ims-licensed-differential-fixtures.schema.json",
        "conformance/0.14/schemas/ims-licensed-differential-receipt.schema.json",
        "conformance/0.14/tools/generate_ims_licensed_fixtures.py",
    ]
    require(spec.get("paths") == expected_spec_paths, "IMS spec path set drifted")
    require(adapter.get("runner_identity") == {"contract_verifier": VERIFIER.as_posix(), "protocol": "mainframe-env.ims-licensed-runner@1", "product_and_oracle_runners_must_differ": True}, "IMS runner policy drifted")
    require(adapter.get("fixtures") == {"path": FIXTURES.as_posix(), "schema": "mainframe-env.ims-licensed-differential-fixtures@1", "case_count": 25, "cases_per_family": 1, "external_bundle_digest_required": True, "product_generated_expectations_allowed": False}, "IMS fixture policy drifted")
    capabilities = ["licensed.ibm-ims-15.6", "independent.product-oracle-execution", "normalized.digest-only-capture", "secrets.excluded", "raw-output.excluded"] + [f"dli.family.{i:04d}" for i in range(1, 26)]
    require(adapter.get("required_capabilities") == capabilities, "IMS capability set drifted")
    require(adapter.get("normalization") == {"policy": "ims-normalized-observation@1", "comparison": "exact-canonical-object-equality", "fields": OBSERVATION_FIELDS, "raw_output_retained": False, "maximum_output_length": 16_777_216, "pcb_status_width": 2, "position_digest_required": True}, "IMS observation policy drifted")
    require(adapter.get("required_harness_mutants") == MUTANTS, "IMS harness mutant contract drifted")
    require(adapter.get("secret_and_raw_output_policy") == {"credentials_in_git": False, "raw_licensed_output_in_git": False, "receipts_in_git": False, "external_digest_pins_required": True}, "IMS secret policy drifted")
    require(adapter.get("credit_policy") == {"external_receipt_required_for_pass": True, "absent_receipt_differential_credit": 0, "invalid_or_partial_receipt_differential_credit": 0, "complete_receipt_family_credit": 25, "historical_local_model_synthetic_credit": 0, "native_ibm_product_fallback_credit": 0}, "IMS credit policy drifted")
    require(adapter.get("licensed_campaign_status") == "not-run" and adapter.get("differential_gate") == "pending", "IMS committed licensed status drifted")
    require(sorted(p.name for p in (root / ADAPTER.parent).iterdir()) == [ADAPTER.name], "IMS oracle directory contains retained receipt or raw output")
    cases = fixtures.get("cases")
    require(isinstance(cases, list) and len(cases) == 25, "IMS fixture denominator drifted")
    require(all(case["source_topic_sha256"] == comparison["sha256"] for case in cases), "IMS fixture source pin drifted")
    return ContractContext(adapter, fixtures, cases, source_identity, bundle_digest(root, expected_spec_paths, SPEC_DOMAIN), file_digest(root / VERIFIER), bundle_digest(root, [FIXTURES.as_posix()], FIXTURE_DOMAIN))


def git_output(root: Path, *args: str) -> str:
    result = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=False)
    require(result.returncode == 0, f"git {' '.join(args)} failed")
    return result.stdout.strip()


def current_candidate(root: Path = ROOT) -> CandidateIdentity:
    commit = git_output(root, "rev-parse", "HEAD")
    tree = git_output(root, "rev-parse", "HEAD^{tree}")
    require(GIT_SHA.fullmatch(commit) is not None and GIT_SHA.fullmatch(tree) is not None, "current candidate Git identity malformed")
    return CandidateIdentity(commit, tree, git_output(root, "status", "--porcelain") == "")


def external_pins(environment: dict[str, str] | None = None) -> dict[str, str]:
    source = os.environ if environment is None else environment
    missing = [name for name in PIN_ENV.values() if name not in source]
    require(not missing, f"IMS licensed external pins are missing: {', '.join(missing)}")
    return {key: checked_digest(source[name], name) for key, name in PIN_ENV.items()}


def normalized(value: Any, label: str, scenario: str) -> dict[str, Any]:
    item = exact_keys(value, OBSERVATION_FIELDS, label)
    require(item["outcome_class"] == scenario, f"{label} outcome class differs from independent fixture")
    require(isinstance(item["pcb_status"], str) and STATUS.fullmatch(item["pcb_status"]) is not None, f"{label} PCB status malformed")
    require(item["pcb_kind"] in {"database", "gsam", "io", "alternate", "none"}, f"{label} PCB kind malformed")
    require(item["position_state"] in {"none", "unchanged", "advanced", "reset", "restored", "unknown"}, f"{label} position state malformed")
    require(item["mutation_state"] in {"no-mutation", "pending", "committed", "backed-out", "unknown"}, f"{label} mutation state malformed")
    for field in ("position_digest", "state_digest", "effect_digest", "output_digest"):
        checked_digest(item[field], f"{label} {field}")
    require(type(item["output_length"]) is int and 0 <= item["output_length"] <= 16_777_216, f"{label} output length malformed")
    require(item["output_encoding"] in {"no-output", "binary", "utf-8", "ebcdic-single-byte"}, f"{label} output encoding malformed")
    require((item["output_length"] == 0) == (item["output_encoding"] == "no-output"), f"{label} output length and encoding disagree")
    if scenario in {"condition", "rejected"}:
        require(item["pcb_status"] != "  ", f"{label} generic success status")
    if scenario == "rejected":
        require(item["mutation_state"] == "no-mutation" and item["position_state"] in {"none", "unchanged"}, f"{label} forbidden mutation or position change")
    return item


def verify_receipt_document(receipt: dict[str, Any], context: ContractContext, pins: dict[str, str], candidate: CandidateIdentity) -> dict[str, Any]:
    exact_keys(receipt, ["schema_version", "target_version", "work_package", "source_identity", "licensed_service", "authorization", "candidate", "contract", "runners", "observations", "attestation"], "IMS receipt")
    require(receipt["schema_version"] == "mainframe-env.ims-licensed-differential-receipt@1" and receipt["target_version"] == "0.14.0" and receipt["work_package"] == WORK_PACKAGE, "IMS receipt target drifted")
    require(receipt["source_identity"] == context.source_identity, "IMS receipt source identity drifted")
    service = exact_keys(receipt["licensed_service"], ["product_name", "major_minor", "release", "service_level", "build_id", "installation_digest"], "IMS licensed service")
    require(service["product_name"] == "IBM IMS" and service["major_minor"] == "15.6", "IMS licensed product identity drifted")
    require(isinstance(service["release"], str) and RELEASE.fullmatch(service["release"]) is not None, "IMS licensed release is not 15.6")
    for field in ("service_level", "build_id"):
        checked_id(service[field], f"IMS licensed {field}")
    checked_digest(service["installation_digest"], "IMS installation digest")
    require(canonical_digest(service) == pins["service_identity_sha256"], "IMS licensed service differs from external pin")
    auth = exact_keys(receipt["authorization"], ["authorized", "authority", "grant_id", "grant_digest", "environment_manifest_digest", "capabilities"], "IMS authorization")
    require(auth["authorized"] is True and auth["authority"] == "licensed-environment-controller", "IMS environment unauthorized")
    checked_id(auth["grant_id"], "IMS grant ID")
    require(auth["grant_digest"] == pins["authorization_grant_sha256"] and auth["environment_manifest_digest"] == pins["environment_manifest_sha256"], "IMS environment or grant pin differs")
    require(auth["capabilities"] == context.adapter["required_capabilities"], "IMS environment capabilities differ")
    require(candidate.tracked_worktree_clean, "current IMS candidate has worktree changes")
    require(receipt["candidate"] == {"commit_sha": candidate.commit_sha, "tree_sha": candidate.tree_sha, "tracked_worktree_clean": True}, "IMS receipt candidate is stale")
    require(receipt["contract"] == {"spec_digest": context.spec_digest, "verifier_digest": context.verifier_digest, "fixture_contract_digest": context.fixture_contract_digest, "fixture_bundle_digest": pins["fixture_bundle_sha256"], "normalization_policy": "ims-normalized-observation@1", "runner_protocol": "mainframe-env.ims-licensed-runner@1"}, "IMS receipt spec or fixture identity is stale")
    runners = exact_keys(receipt["runners"], ["product", "oracle"], "IMS runners")
    require(runners["product"] == {"kind": "mainframe-env-owned", "route": "owned-ims-provider", "digest": pins["product_runner_sha256"], "native_ibm_fallback": False}, "IMS product runner identity differs")
    require(runners["oracle"] == {"kind": "licensed-ibm-ims", "route": "licensed-dli-adapter", "digest": pins["oracle_runner_sha256"]}, "IMS oracle runner identity differs")
    require(pins["product_runner_sha256"] != pins["oracle_runner_sha256"], "IMS product and oracle runners must differ")
    attest = exact_keys(receipt["attestation"], ["receipt_id", "run_id", "issued_at", "licensed_execution", "independent_product_and_oracle_runs", "proprietary_bytes_retained_in_receipt", "credentials_retained_in_receipt", "raw_output_retained_in_receipt", "historical_result", "synthetic_or_model_result"], "IMS attestation")
    checked_id(attest["receipt_id"], "IMS receipt ID")
    checked_id(attest["run_id"], "IMS run ID")
    require(isinstance(attest["issued_at"], str) and TIMESTAMP.fullmatch(attest["issued_at"]) is not None, "IMS receipt timestamp malformed")
    require(all(attest[key] is True for key in ("licensed_execution", "independent_product_and_oracle_runs")) and all(attest[key] is False for key in ("proprietary_bytes_retained_in_receipt", "credentials_retained_in_receipt", "raw_output_retained_in_receipt", "historical_result", "synthetic_or_model_result")), "IMS attestation is not current, licensed, independent and redacted")
    observations = receipt["observations"]
    require(isinstance(observations, list) and len(observations) == 25, "IMS receipt must have exactly 25 observations")
    input_digests: set[str] = set()
    for fixture, observation in zip(context.cases, observations, strict=True):
        exact_keys(observation, ["case_id", "row_id", "call_family", "source_locator", "source_topic_path", "source_topic_sha256", "applicability_profiles", "input_profile", "scenario_class", "fixture_bundle_member", "fixture_input_digest", "product", "oracle"], "IMS case observation")
        for field in ("case_id", "row_id", "call_family", "source_locator", "source_topic_path", "source_topic_sha256", "applicability_profiles", "input_profile", "scenario_class", "fixture_bundle_member"):
            require(observation[field] == fixture[field], f"IMS case {fixture['case_id']} {field} differs or order changed")
        input_digest = checked_digest(observation["fixture_input_digest"], "IMS fixture input")
        require(input_digest not in input_digests, "IMS receipt reuses fixture input digest")
        input_digests.add(input_digest)
        product = normalized(observation["product"], "IMS product observation", fixture["scenario_class"])
        oracle = normalized(observation["oracle"], "IMS oracle observation", fixture["scenario_class"])
        require(product == oracle, f"IMS differential mismatch for {fixture['case_id']}")
    return {"schema_version": "mainframe-env.ims-licensed-verification-result@1", "status": "pass", "baseline_id": BASELINE, "candidate_commit": candidate.commit_sha, "candidate_tree": candidate.tree_sha, "spec_digest": context.spec_digest, "verifier_digest": context.verifier_digest, "fixture_contract_digest": context.fixture_contract_digest, "receipt_sha256": pins["receipt_sha256"], "differential": {"credited_families": 25, "denominator": 25}}


def read_external_receipt(path: Path, root: Path, expected_digest: str) -> dict[str, Any]:
    metadata = path.lstat()
    require(stat.S_ISREG(metadata.st_mode) and not stat.S_ISLNK(metadata.st_mode), "IMS receipt must be a regular non-symlink file")
    require(0 < metadata.st_size <= MAX_BYTES, "IMS receipt is empty or too large")
    resolved = path.resolve(strict=True)
    repository = root.resolve(strict=True)
    require(resolved != repository and repository not in resolved.parents, "IMS receipt path is inside candidate tree")
    body = resolved.read_bytes()
    require(sha256(body) == expected_digest, "IMS receipt differs from external digest pin")
    return parse_json(body, "IMS external receipt")


def pending_result(context: ContractContext, candidate: CandidateIdentity) -> dict[str, Any]:
    return {"schema_version": "mainframe-env.ims-licensed-verification-result@1", "status": "pending-external-licensed-receipt", "baseline_id": BASELINE, "candidate_commit": candidate.commit_sha, "candidate_tree": candidate.tree_sha, "tracked_worktree_clean": candidate.tracked_worktree_clean, "spec_digest": context.spec_digest, "verifier_digest": context.verifier_digest, "fixture_contract_digest": context.fixture_contract_digest, "differential": {"credited_families": 0, "denominator": 25}, "blockers": ["external IBM IMS 15.6 licensed receipt and exact external pins are absent"]}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check committed contract, reporting pending if no receipt")
    parser.add_argument("--receipt", type=Path, help="external receipt path")
    parser.add_argument("--require-pass", action="store_true", help="fail if licensed receipt is absent")
    args = parser.parse_args(argv)
    try:
        context = verify_contract()
        candidate = current_candidate()
        configured = os.environ.get("MAINFRAME_ENV_IMS_LICENSED_ORACLE_RECEIPT")
        path = args.receipt or (Path(configured) if configured else None)
        if path is None:
            print(json.dumps(pending_result(context, candidate), sort_keys=True))
            return 1 if args.require_pass else 0
        pins = external_pins()
        receipt = read_external_receipt(path, ROOT, pins["receipt_sha256"])
        print(json.dumps(verify_receipt_document(receipt, context, pins, candidate), sort_keys=True))
        return 0
    except (OSError, VerificationError, ValueError) as error:
        print(f"ims-licensed: fail: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
