#!/usr/bin/env python3
"""Verify the fail-closed external IBM MQ 9.4 differential contract."""

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


REPOSITORY = Path(__file__).resolve().parents[3]
ADAPTER_PATH = Path("conformance/0.15/oracles/mq-licensed-differential.json")
FIXTURE_PATH = Path("conformance/0.15/fixtures/mq-licensed-differential-cases.json")
CATALOG_PATH = Path("conformance/0.2/catalogs/mq.json")
MANIFEST_PATH = Path("conformance/0.2/manifests/mq-topics.json")
SOURCE_CALL_LIST_PATH = Path("conformance/0.15/mq/source-call-list.json")
STRUCTURE_CATALOG_PATH = Path("conformance/0.15/mq/structure-status-catalog.json")
VERIFIER_PATH = Path("conformance/0.15/tools/verify_mq_licensed_differential.py")
MAX_JSON_BYTES = 2 * 1024 * 1024
BASELINE = "ibm-mq-9.4-mqi-2026-08-31"
UNIT = "mqi-calls-unique"
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
GIT_SHA = re.compile(r"^[0-9a-f]{40}$")
SAFE_ID = re.compile(r"^[A-Za-z0-9_.:@+/-]{1,160}$")
VRMF = re.compile(r"^9\.4\.[0-9]{1,3}\.[0-9]{1,3}$")
TIMESTAMP = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
CALL = re.compile(r"^MQ[A-Z0-9_]{1,30}$")
REASON = re.compile(r"^MQRC_[A-Z0-9_]{1,90}$")
SPEC_DOMAIN = b"mainframe-env.mq-licensed-spec@1\0"
FIXTURE_DOMAIN = b"mainframe-env.mq-licensed-fixtures@1\0"

GLOBAL_CAPABILITIES = [
    "licensed.ibm-mq-9.4",
    "independent.product-oracle-execution",
    "normalized.digest-only-capture",
    "secrets.excluded",
    "raw-output.excluded",
]
MUTANTS = [
    "omitted-transition",
    "generic-success",
    "authorization-bypass",
    "forbidden-mutation",
    "byte-encoding-error",
]
NORMALIZED_FIELDS = [
    "completion_code",
    "reason_code",
    "handle_state",
    "mutation_state",
    "state_digest",
    "effect_digest",
    "output_digest",
    "output_length",
    "output_encoding",
    "status_digest",
]
SOURCE_FIELDS = [
    "baseline_id",
    "unit",
    "documentation_product",
    "official_catalog_sha256",
    "source_manifest_sha256",
    "topic_manifest_sha256",
    "toc_sha256",
    "source_call_list_sha256",
    "call_list_topic_sha256",
    "unique_call_count",
    "source_row_count",
]
RECEIPT_ENVIRONMENTS = {
    "receipt_sha256": "MAINFRAME_ENV_MQ_LICENSED_ORACLE_RECEIPT_SHA256",
    "service_identity_sha256": "MAINFRAME_ENV_MQ_LICENSED_SERVICE_IDENTITY_SHA256",
    "environment_manifest_sha256": "MAINFRAME_ENV_MQ_LICENSED_ENVIRONMENT_SHA256",
    "authorization_grant_sha256": "MAINFRAME_ENV_MQ_LICENSED_AUTHORIZATION_SHA256",
    "fixture_bundle_sha256": "MAINFRAME_ENV_MQ_LICENSED_FIXTURE_BUNDLE_SHA256",
    "product_runner_sha256": "MAINFRAME_ENV_MQ_PRODUCT_RUNNER_SHA256",
    "oracle_runner_sha256": "MAINFRAME_ENV_MQ_LICENSED_RUNNER_SHA256",
}


class VerificationError(ValueError):
    """A fail-closed contract or receipt finding."""


@dataclass(frozen=True)
class ExternalPins:
    receipt_sha256: str
    service_identity_sha256: str
    environment_manifest_sha256: str
    authorization_grant_sha256: str
    fixture_bundle_sha256: str
    product_runner_sha256: str
    oracle_runner_sha256: str


@dataclass(frozen=True)
class CandidateIdentity:
    commit_sha: str
    tree_sha: str
    tracked_worktree_clean: bool


@dataclass(frozen=True)
class ContractContext:
    adapter: dict[str, Any]
    fixtures: dict[str, Any]
    cases: dict[str, dict[str, Any]]
    calls: tuple[str, ...]
    source_identity: dict[str, Any]
    spec_digest: str
    verifier_digest: str
    fixture_contract_digest: str


def require(condition: bool, message: str) -> None:
    if not condition:
        raise VerificationError(message)


def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise VerificationError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def load_json_bytes(body: bytes, label: str) -> dict[str, Any]:
    require(0 < len(body) <= MAX_JSON_BYTES, f"{label} is empty or exceeds its byte limit")
    try:
        value = json.loads(body, object_pairs_hook=object_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"{label}: {error}") from error
    require(isinstance(value, dict), f"{label} root must be an object")
    return value


def load_json(path: Path, label: str | None = None) -> dict[str, Any]:
    try:
        body = path.read_bytes()
    except OSError as error:
        raise VerificationError(f"{label or path}: {error}") from error
    return load_json_bytes(body, label or str(path))


def sha256_bytes(body: bytes) -> str:
    return "sha256:" + hashlib.sha256(body).hexdigest()


def file_digest(path: Path) -> str:
    try:
        return sha256_bytes(path.read_bytes())
    except OSError as error:
        raise VerificationError(f"{path}: {error}") from error


def canonical_digest(value: Any) -> str:
    body = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()
    return sha256_bytes(body)


def digest_field(hasher: Any, body: bytes) -> None:
    hasher.update(len(body).to_bytes(8, "big"))
    hasher.update(body)


def safe_repository_path(root: Path, value: str) -> Path:
    candidate = PurePosixPath(value)
    require(
        not candidate.is_absolute()
        and candidate.as_posix() == value
        and all(part not in {"", ".", ".."} for part in candidate.parts),
        f"unsafe contract path {value!r}",
    )
    path = root.joinpath(*candidate.parts)
    require(path.is_file() and not path.is_symlink(), f"contract path is not a regular file: {value}")
    return path


def bundle_digest(root: Path, paths: list[str], domain: bytes) -> str:
    require(paths and len(paths) == len(set(paths)), "contract bundle paths are empty or duplicate")
    hasher = hashlib.sha256()
    hasher.update(domain)
    for relative in sorted(paths):
        path = safe_repository_path(root, relative)
        body = path.read_bytes()
        digest_field(hasher, relative.encode())
        digest_field(hasher, body)
    return "sha256:" + hasher.hexdigest()


def exact_keys(value: Any, keys: list[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    actual = set(value)
    expected = set(keys)
    require(
        actual == expected,
        f"{label} fields differ: missing={sorted(expected - actual)} extra={sorted(actual - expected)}",
    )
    return value


def exact_list(value: Any, expected: list[str], label: str) -> list[str]:
    require(isinstance(value, list), f"{label} must be an array")
    require(value == expected, f"{label} differs from the exact contract order")
    return value


def checked_digest(value: Any, label: str) -> str:
    require(isinstance(value, str) and DIGEST.fullmatch(value) is not None, f"{label} is not a digest")
    return value


def checked_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and SAFE_ID.fullmatch(value) is not None, f"{label} is not a bounded identity")
    return value


def source_manifest_digest(manifest: dict[str, Any]) -> str:
    topics = manifest.get("topics")
    require(isinstance(topics, list) and len(topics) == 27, "MQ source manifest must have 27 topics")
    rows: list[str] = []
    for topic in topics:
        exact_keys(topic, ["bytes", "last_modified", "sha256", "topic_path"], "MQ source topic")
        path = topic["topic_path"]
        digest = topic["sha256"]
        require(isinstance(path, str) and isinstance(digest, str), "MQ source topic identity is malformed")
        require(re.fullmatch(r"SSFKSJ_9\.4\.0/refdev/q[0-9]+_\.html", path) is not None, "MQ source topic path is malformed")
        require(re.fullmatch(r"[0-9a-f]{64}", digest) is not None, "MQ source topic digest is malformed")
        require(isinstance(topic["bytes"], int) and 0 < topic["bytes"] <= 2_000_000, "MQ source topic size is invalid")
        rows.append(f"{path} {digest}\n")
    require(len(set(rows)) == 27, "MQ source manifest topics are duplicate")
    return hashlib.sha256("".join(sorted(rows)).encode()).hexdigest()


def expected_source_identity(adapter: dict[str, Any]) -> dict[str, Any]:
    denominator = adapter["denominator"]
    return {
        "baseline_id": denominator["baseline_id"],
        "unit": denominator["unit"],
        "documentation_product": denominator["source_manifest"]["documentation_product"],
        "official_catalog_sha256": denominator["official_catalog"]["sha256"],
        "source_manifest_sha256": denominator["source_manifest"]["sha256"],
        "topic_manifest_sha256": denominator["source_manifest"]["topic_manifest_sha256"],
        "toc_sha256": denominator["source_manifest"]["toc_sha256"],
        "source_call_list_sha256": denominator["source_call_list"]["sha256"],
        "call_list_topic_sha256": denominator["source_call_list"]["topic_sha256"],
        "unique_call_count": 26,
        "source_row_count": 27,
    }


def verify_contract(root: Path = REPOSITORY) -> ContractContext:
    adapter = load_json(root / ADAPTER_PATH)
    fixtures = load_json(root / FIXTURE_PATH)
    catalog = load_json(root / CATALOG_PATH)
    manifest = load_json(root / MANIFEST_PATH)
    source_list = load_json(root / SOURCE_CALL_LIST_PATH)
    structure_catalog = load_json(root / STRUCTURE_CATALOG_PATH)

    require(adapter.get("schema_version") == "mainframe-env.mq-licensed-differential-adapter@1", "MQ licensed adapter version drifted")
    require(adapter.get("target_version") == "0.15.0", "MQ licensed adapter targets the wrong version")
    require(adapter.get("work_package") == "MQ-1506.licensed-harness-contract", "MQ licensed adapter work package drifted")
    denominator = adapter.get("denominator")
    require(isinstance(denominator, dict), "MQ licensed adapter denominator is missing")
    require(
        denominator.get("baseline_id") == BASELINE
        and denominator.get("unit") == UNIT
        and denominator.get("unique_call_count") == 26
        and denominator.get("source_row_count") == 27,
        "MQ licensed adapter denominator drifted",
    )

    for identity, expected_path in [
        (denominator.get("official_catalog"), CATALOG_PATH),
        (denominator.get("source_manifest"), MANIFEST_PATH),
        (denominator.get("source_call_list"), SOURCE_CALL_LIST_PATH),
    ]:
        require(isinstance(identity, dict), f"MQ source identity {expected_path} is missing")
        require(identity.get("path") == expected_path.as_posix(), f"MQ source identity path drifted: {expected_path}")
        require(identity.get("sha256") == file_digest(root / expected_path), f"MQ source identity digest drifted: {expected_path}")

    require(manifest.get("baseline_id") == BASELINE, "MQ source manifest baseline drifted")
    require(manifest.get("product") == "SSFKSJ_9.4.0", "MQ documentation product drifted")
    require(manifest.get("topic_count") == 27, "MQ source topic count drifted")
    require(
        denominator["source_manifest"].get("topic_manifest_sha256")
        == "sha256:" + source_manifest_digest(manifest)
        == "sha256:" + str(manifest.get("topic_manifest_digest")),
        "MQ source topic-manifest identity drifted",
    )
    require(
        denominator["source_manifest"].get("toc_sha256")
        == "sha256:" + str(manifest.get("toc_sha256")),
        "MQ source TOC identity drifted",
    )
    require(
        denominator["source_call_list"].get("topic_sha256")
        == "sha256:24025a9f40dfc613b8243fef16f902a94794a9fbe517730ae1b6c489a338009f",
        "MQ call-list topic identity drifted",
    )
    reviewed_source_topics = [
        {
            "call": "MQCONNX",
            "topic_path": "SSFKSJ_9.4.0/refdev/q101770_.html",
            "sha256": "sha256:41e9da41eb766141814ba1b2c3dc9c649450d1ab6cc64d574165f239ea8ed633",
            "review_method": "ibm_docs.py-read",
            "coverage_credit": 0,
        },
        {
            "call": "MQSTAT",
            "topic_path": "SSFKSJ_9.4.0/refdev/q101920_.html",
            "sha256": "sha256:4f19dab47ed3e325ec894cc74a8d88db265a7c27f8ff2a7c507f94cf4bedd1d9",
            "review_method": "ibm_docs.py-read",
            "coverage_credit": 0,
        },
        {
            "call": "MQINQ",
            "topic_path": "SSFKSJ_9.4.0/refdev/q101840_.html",
            "sha256": "sha256:03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b",
            "review_method": "retained-html-sha256",
            "coverage_credit": 0,
        },
    ]
    require(
        denominator.get("reviewed_source_topics") == reviewed_source_topics,
        "MQ licensed reviewed source-topic identities drifted",
    )
    manifest_topic_digests = {
        topic["topic_path"]: "sha256:" + topic["sha256"] for topic in manifest["topics"]
    }
    require(
        all(
            manifest_topic_digests.get(topic["topic_path"]) == topic["sha256"]
            for topic in reviewed_source_topics
        ),
        "MQ licensed reviewed source topics differ from the pinned manifest",
    )
    require(
        denominator.get("unavailable_source_topics") == [],
        "MQ retained-source blocker drifted",
    )

    require(catalog.get("baseline_id") == BASELINE and catalog.get("mandatory_rows") == 26, "MQ official catalog identity drifted")
    units = catalog.get("units")
    require(isinstance(units, list) and len(units) == 1, "MQ official catalog unit set drifted")
    unit = units[0]
    rows = unit.get("rows") if isinstance(unit, dict) else None
    require(
        isinstance(rows, list)
        and len(rows) == 26
        and unit.get("id") == UNIT
        and unit.get("denominator") == 26,
        "MQ official call denominator drifted",
    )
    calls = tuple(row.get("label") for row in rows if isinstance(row, dict))
    require(len(calls) == 26 and len(set(calls)) == 26 and all(isinstance(call, str) and CALL.fullmatch(call) for call in calls), "MQ official call identities are malformed")
    row_by_call = {row["label"]: row for row in rows}
    structure_calls = structure_catalog.get("calls")
    require(
        structure_catalog.get("baseline_id") == BASELINE
        and structure_catalog.get("unique_call_count") == 26
        and structure_catalog.get("missing_call_topic_count") == 0
        and isinstance(structure_calls, list)
        and {(call.get("label"), call.get("official_row")) for call in structure_calls}
        == {(call, row_by_call[call]["id"]) for call in calls},
        "MQ M3 structure/status denominator drifted",
    )
    manifest_topic_digests = {topic["topic_path"]: topic["sha256"] for topic in manifest["topics"]}
    require(
        all(
            manifest_topic_digests.get(call.get("topic_path")) == call.get("topic_sha256")
            for call in structure_calls
        ),
        "MQ M3 call topics differ from the pinned manifest",
    )

    require(
        source_list.get("baseline_id") == BASELINE
        and source_list.get("source_row_count") == 27
        and source_list.get("unique_call_count") == 26
        and source_list.get("semantic_authority") is False
        and source_list.get("coverage_credit") == 0,
        "MQ source call-list boundary drifted",
    )
    source_rows = source_list.get("rows")
    require(isinstance(source_rows, list) and len(source_rows) == 27, "MQ source call-list must retain 27 rows")
    normalized = {(row.get("label"), row.get("official_row")) for row in source_rows if isinstance(row, dict)}
    require(normalized == {(call, row_by_call[call]["id"]) for call in calls}, "MQ source call-list does not normalize to the 26 official calls")
    require(
        sum(1 for row in source_rows if row.get("label") == "MQMHBUF") == 2
        and sum(1 for row in source_rows if row.get("duplicate_of_source_position") is not None) == 1,
        "MQ source call-list duplicate provenance drifted",
    )

    require(fixtures.get("schema_version") == "mainframe-env.mq-licensed-differential-fixtures@1", "MQ licensed fixture contract drifted")
    require(
        fixtures.get("baseline_id") == BASELINE
        and fixtures.get("unit") == UNIT
        and fixtures.get("case_count") == 26
        and fixtures.get("cases_per_call") == 1,
        "MQ licensed fixture denominator drifted",
    )
    require(
        fixtures.get("independent_fixture_authority")
        == {
            "kind": "reviewed-external-fixture-bundle",
            "product_output_derived": False,
            "oracle_output_embedded": False,
            "raw_fixture_bytes_retained": False,
            "external_bundle_digest_required": True,
        },
        "MQ fixtures are not independent and external",
    )
    fixture_cases = fixtures.get("cases")
    require(isinstance(fixture_cases, list) and len(fixture_cases) == 26, "MQ licensed fixture cases drifted")
    cases: dict[str, dict[str, Any]] = {}
    manifest_topics = {topic["topic_path"] for topic in manifest["topics"]}
    observed_calls: set[str] = set()
    observed_members: set[str] = set()
    for case in fixture_cases:
        exact_keys(
            case,
            [
                "case_id",
                "row_id",
                "call",
                "source_topic_path",
                "input_profile",
                "fixture_bundle_member",
                "observation_profile",
            ],
            "MQ licensed fixture case",
        )
        call = case["call"]
        require(call in row_by_call and call not in observed_calls, f"MQ fixture call is unknown or duplicate: {call}")
        require(case["row_id"] == row_by_call[call]["id"], f"MQ fixture row differs for {call}")
        expected_topic = "SSFKSJ_9.4.0/refdev/" + row_by_call[call]["source_locator"].removeprefix("html-link:")
        require(case["source_topic_path"] == expected_topic and expected_topic in manifest_topics, f"MQ fixture topic differs for {call}")
        require(case["case_id"] == f"mq.mqi.{call.lower()}.licensed-differential", f"MQ fixture case identity differs for {call}")
        require(case["observation_profile"] == "mq-normalized-observation@1", f"MQ fixture observation profile differs for {call}")
        require(isinstance(case["input_profile"], str) and case["input_profile"].startswith("independent-mqi."), f"MQ fixture input is not independent for {call}")
        require(case["fixture_bundle_member"] == f"mqi/{call.lower()}.json", f"MQ fixture member differs for {call}")
        require(case["fixture_bundle_member"] not in observed_members, f"MQ fixture member is duplicate for {call}")
        observed_calls.add(call)
        observed_members.add(case["fixture_bundle_member"])
        require(case["case_id"] not in cases, f"MQ fixture case identity is duplicate: {case['case_id']}")
        cases[case["case_id"]] = case
    require(observed_calls == set(calls), "MQ fixtures do not cover every official call exactly once")

    expected_capabilities = GLOBAL_CAPABILITIES + [f"mqi.call.{call}" for call in calls]
    exact_list(adapter.get("required_capabilities"), expected_capabilities, "MQ licensed capabilities")
    exact_list(adapter.get("required_harness_mutants"), MUTANTS, "MQ licensed harness mutants")
    normalization = adapter.get("normalization")
    require(isinstance(normalization, dict), "MQ normalization policy is missing")
    exact_list(normalization.get("fields"), NORMALIZED_FIELDS, "MQ normalized observation fields")
    require(
        normalization.get("policy") == "mq-normalized-observation@1"
        and normalization.get("comparison") == "exact-canonical-object-equality"
        and normalization.get("raw_output_retained") is False
        and normalization.get("maximum_output_length") == 16_777_216,
        "MQ normalization bounds drifted",
    )
    require(
        adapter.get("licensed_oracle")
        == {
            "product_name": "IBM MQ",
            "major_minor": "9.4",
            "documentation_product": "SSFKSJ_9.4.0",
            "service_identity_policy": "externally-pinned-exact-vrmf-command-level-build-installation",
            "execution_role": "oracle-only",
            "native_product_fallback_allowed": False,
        },
        "MQ licensed oracle product/fallback boundary drifted",
    )
    require(
        adapter.get("credit_policy")
        == {
            "external_receipt_required_for_pass": True,
            "absent_receipt_differential_credit": 0,
            "invalid_or_partial_receipt_differential_credit": 0,
            "complete_receipt_call_credit": 26,
            "historical_local_model_synthetic_credit": 0,
            "native_ibm_product_fallback_credit": 0,
        },
        "MQ licensed credit policy is not fail closed",
    )
    require(adapter.get("licensed_campaign_status") == "not-run" and adapter.get("differential_gate") == "pending", "MQ licensed campaign must remain pending in Git")

    oracle_directory = root / ADAPTER_PATH.parent
    retained = sorted(path.name for path in oracle_directory.iterdir() if path.is_file())
    require(retained == [ADAPTER_PATH.name], "candidate tree must not retain an MQ licensed receipt or raw oracle output")

    spec = adapter.get("spec_identity")
    require(isinstance(spec, dict), "MQ licensed spec identity is missing")
    spec_paths = spec.get("paths")
    require(isinstance(spec_paths, list) and all(isinstance(path, str) for path in spec_paths), "MQ licensed spec paths are malformed")
    require(spec.get("domain") == "mainframe-env.mq-licensed-spec@1", "MQ licensed spec domain drifted")
    spec_digest = bundle_digest(root, spec_paths, SPEC_DOMAIN)
    verifier_digest = file_digest(root / VERIFIER_PATH)
    fixture_contract_digest = bundle_digest(root, [FIXTURE_PATH.as_posix()], FIXTURE_DOMAIN)
    source_identity = expected_source_identity(adapter)
    exact_keys(source_identity, SOURCE_FIELDS, "MQ expected source identity")
    return ContractContext(
        adapter=adapter,
        fixtures=fixtures,
        cases=cases,
        calls=calls,
        source_identity=source_identity,
        spec_digest=spec_digest,
        verifier_digest=verifier_digest,
        fixture_contract_digest=fixture_contract_digest,
    )


def git_output(root: Path, arguments: list[str]) -> str:
    process = subprocess.run(
        ["git", *arguments],
        cwd=root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    require(process.returncode == 0, f"git {' '.join(arguments)} failed")
    return process.stdout.strip()


def current_candidate(root: Path = REPOSITORY) -> CandidateIdentity:
    commit = git_output(root, ["rev-parse", "HEAD"])
    tree = git_output(root, ["rev-parse", "HEAD^{tree}"])
    require(GIT_SHA.fullmatch(commit) is not None and GIT_SHA.fullmatch(tree) is not None, "current Git candidate identity is malformed")
    clean = git_output(root, ["status", "--porcelain"]) == ""
    return CandidateIdentity(commit, tree, clean)


def external_pins(environment: dict[str, str] | None = None) -> ExternalPins:
    source = os.environ if environment is None else environment
    values: dict[str, str] = {}
    missing: list[str] = []
    for field, name in RECEIPT_ENVIRONMENTS.items():
        value = source.get(name)
        if value is None:
            missing.append(name)
        else:
            values[field] = checked_digest(value, name)
    require(not missing, f"MQ licensed receipt external pins are missing: {', '.join(missing)}")
    return ExternalPins(**values)


def validate_service(service: Any, pins: ExternalPins) -> dict[str, Any]:
    value = exact_keys(
        service,
        ["product_name", "major_minor", "vrmf", "command_level", "build_id", "installation_digest"],
        "MQ licensed service",
    )
    require(value["product_name"] == "IBM MQ" and value["major_minor"] == "9.4", "MQ licensed product identity drifted")
    require(isinstance(value["vrmf"], str) and VRMF.fullmatch(value["vrmf"]) is not None, "MQ licensed VRMF is not an exact 9.4 service level")
    require(isinstance(value["command_level"], int) and not isinstance(value["command_level"], bool) and 940 <= value["command_level"] <= 999, "MQ licensed command level is invalid")
    checked_id(value["build_id"], "MQ licensed build identity")
    checked_digest(value["installation_digest"], "MQ licensed installation identity")
    require(canonical_digest(value) == pins.service_identity_sha256, "MQ licensed service identity differs from its external pin")
    return value


def validate_normalized_observation(value: Any, label: str) -> dict[str, Any]:
    observation = exact_keys(value, NORMALIZED_FIELDS, label)
    require(observation["completion_code"] in {"MQCC_OK", "MQCC_WARNING", "MQCC_FAILED"}, f"{label} completion code is invalid")
    require(isinstance(observation["reason_code"], str) and REASON.fullmatch(observation["reason_code"]) is not None, f"{label} reason code is invalid")
    require(observation["handle_state"] in {"none", "valid", "invalid", "unchanged", "released"}, f"{label} handle state is invalid")
    require(observation["mutation_state"] in {"no-mutation", "pending", "committed", "backed-out", "unknown"}, f"{label} mutation state is invalid")
    for field in ["state_digest", "effect_digest", "output_digest", "status_digest"]:
        checked_digest(observation[field], f"{label} {field}")
    require(isinstance(observation["output_length"], int) and not isinstance(observation["output_length"], bool) and 0 <= observation["output_length"] <= 16_777_216, f"{label} output length is invalid")
    require(observation["output_encoding"] in {"no-output", "binary", "utf-8", "ebcdic-single-byte"}, f"{label} output encoding is invalid")
    require(
        not (observation["output_length"] == 0 and observation["output_encoding"] != "no-output")
        and not (observation["output_length"] > 0 and observation["output_encoding"] == "no-output"),
        f"{label} output length and encoding disagree",
    )
    return observation


def verify_receipt_document(
    receipt: dict[str, Any],
    context: ContractContext,
    pins: ExternalPins,
    candidate: CandidateIdentity,
) -> dict[str, Any]:
    exact_keys(
        receipt,
        [
            "schema_version",
            "target_version",
            "work_package",
            "source_identity",
            "licensed_service",
            "authorization",
            "candidate",
            "contract",
            "runners",
            "observations",
            "attestation",
        ],
        "MQ licensed receipt",
    )
    require(receipt["schema_version"] == "mainframe-env.mq-licensed-differential-receipt@1", "MQ licensed receipt version drifted")
    require(receipt["target_version"] == "0.15.0" and receipt["work_package"] == "MQ-1506.licensed-harness-contract", "MQ licensed receipt target drifted")
    exact_keys(receipt["source_identity"], SOURCE_FIELDS, "MQ receipt source identity")
    require(receipt["source_identity"] == context.source_identity, "MQ licensed receipt source identities drifted")
    validate_service(receipt["licensed_service"], pins)

    authorization = exact_keys(
        receipt["authorization"],
        ["authorized", "authority", "grant_id", "grant_digest", "environment_manifest_digest", "capabilities"],
        "MQ licensed authorization",
    )
    require(authorization["authorized"] is True and authorization["authority"] == "licensed-environment-controller", "MQ licensed environment is not authorized")
    checked_id(authorization["grant_id"], "MQ licensed grant identity")
    require(authorization["grant_digest"] == pins.authorization_grant_sha256, "MQ licensed authorization grant differs from its external pin")
    require(authorization["environment_manifest_digest"] == pins.environment_manifest_sha256, "MQ licensed environment differs from its external pin")
    exact_list(authorization["capabilities"], context.adapter["required_capabilities"], "MQ licensed environment capabilities")

    receipt_candidate = exact_keys(receipt["candidate"], ["commit_sha", "tree_sha", "tracked_worktree_clean"], "MQ licensed candidate")
    require(candidate.tracked_worktree_clean, "current MQ licensed candidate has worktree changes")
    require(
        receipt_candidate
        == {
            "commit_sha": candidate.commit_sha,
            "tree_sha": candidate.tree_sha,
            "tracked_worktree_clean": True,
        },
        "MQ licensed receipt was not produced from the current clean candidate",
    )

    contract = exact_keys(
        receipt["contract"],
        ["spec_digest", "verifier_digest", "fixture_contract_digest", "fixture_bundle_digest", "normalization_policy", "runner_protocol"],
        "MQ licensed receipt contract",
    )
    require(
        contract
        == {
            "spec_digest": context.spec_digest,
            "verifier_digest": context.verifier_digest,
            "fixture_contract_digest": context.fixture_contract_digest,
            "fixture_bundle_digest": pins.fixture_bundle_sha256,
            "normalization_policy": "mq-normalized-observation@1",
            "runner_protocol": "mainframe-env.mq-licensed-runner@1",
        },
        "MQ licensed receipt spec, verifier, fixture, or runner identity drifted",
    )

    runners = exact_keys(receipt["runners"], ["product", "oracle"], "MQ licensed runners")
    product_runner = exact_keys(runners["product"], ["kind", "route", "digest", "native_ibm_fallback"], "MQ product runner")
    oracle_runner = exact_keys(runners["oracle"], ["kind", "route", "digest"], "MQ oracle runner")
    require(
        product_runner
        == {
            "kind": "mainframe-env-owned",
            "route": "owned-mq-provider",
            "digest": pins.product_runner_sha256,
            "native_ibm_fallback": False,
        },
        "MQ product runner is not the externally pinned owned provider",
    )
    require(
        oracle_runner
        == {
            "kind": "licensed-ibm-mq",
            "route": "licensed-mqi-adapter",
            "digest": pins.oracle_runner_sha256,
        },
        "MQ oracle runner is not the externally pinned licensed adapter",
    )
    require(product_runner["digest"] != oracle_runner["digest"], "MQ product and oracle runner identities must differ")

    attestation = exact_keys(
        receipt["attestation"],
        [
            "receipt_id",
            "run_id",
            "issued_at",
            "licensed_execution",
            "independent_product_and_oracle_runs",
            "proprietary_bytes_retained_in_receipt",
            "credentials_retained_in_receipt",
            "raw_output_retained_in_receipt",
            "historical_result",
            "synthetic_or_model_result",
        ],
        "MQ licensed attestation",
    )
    checked_id(attestation["receipt_id"], "MQ receipt identity")
    checked_id(attestation["run_id"], "MQ run identity")
    require(isinstance(attestation["issued_at"], str) and TIMESTAMP.fullmatch(attestation["issued_at"]) is not None, "MQ receipt timestamp is malformed")
    require(
        attestation["licensed_execution"] is True
        and attestation["independent_product_and_oracle_runs"] is True
        and attestation["proprietary_bytes_retained_in_receipt"] is False
        and attestation["credentials_retained_in_receipt"] is False
        and attestation["raw_output_retained_in_receipt"] is False
        and attestation["historical_result"] is False
        and attestation["synthetic_or_model_result"] is False,
        "MQ receipt attestation is not current, licensed, independent, and redacted",
    )

    observations = receipt["observations"]
    require(isinstance(observations, list) and len(observations) == 26, "MQ licensed receipt must contain exactly 26 observations")
    by_case: dict[str, dict[str, Any]] = {}
    fixture_digests: set[str] = set()
    for observation in observations:
        exact_keys(
            observation,
            [
                "case_id",
                "row_id",
                "call",
                "source_topic_path",
                "input_profile",
                "fixture_bundle_member",
                "fixture_input_digest",
                "product",
                "oracle",
            ],
            "MQ licensed case observation",
        )
        case_id = observation["case_id"]
        require(isinstance(case_id, str) and case_id in context.cases and case_id not in by_case, f"MQ licensed observation case is missing, duplicate, or unknown: {case_id!r}")
        fixture = context.cases[case_id]
        for field in ["row_id", "call", "source_topic_path", "input_profile", "fixture_bundle_member"]:
            require(observation[field] == fixture[field], f"MQ licensed observation {case_id} {field} differs from its fixture")
        input_digest = checked_digest(observation["fixture_input_digest"], f"MQ licensed observation {case_id} fixture input")
        require(input_digest not in fixture_digests, f"MQ licensed receipt reuses one fixture input for {case_id}")
        fixture_digests.add(input_digest)
        product = validate_normalized_observation(observation["product"], f"MQ product observation {case_id}")
        oracle = validate_normalized_observation(observation["oracle"], f"MQ oracle observation {case_id}")
        require(product == oracle, f"MQ licensed differential mismatch for {observation['call']}")
        by_case[case_id] = observation
    require(set(by_case) == set(context.cases), "MQ licensed receipt omits or adds per-call coverage")
    require({observation["call"] for observation in observations} == set(context.calls), "MQ licensed receipt does not cover every call exactly once")
    return {
        "schema_version": "mainframe-env.mq-licensed-verification-result@1",
        "status": "pass",
        "baseline_id": BASELINE,
        "candidate_commit": candidate.commit_sha,
        "candidate_tree": candidate.tree_sha,
        "spec_digest": context.spec_digest,
        "verifier_digest": context.verifier_digest,
        "fixture_contract_digest": context.fixture_contract_digest,
        "receipt_sha256": pins.receipt_sha256,
        "differential": {"credited_calls": 26, "denominator": 26},
    }


def read_external_receipt(path: Path, root: Path, expected_digest: str) -> dict[str, Any]:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise VerificationError(f"MQ licensed receipt: {error}") from error
    require(not stat.S_ISLNK(metadata.st_mode), "MQ licensed receipt must not be a symlink")
    require(stat.S_ISREG(metadata.st_mode), "MQ licensed receipt must be a regular file")
    require(0 < metadata.st_size <= MAX_JSON_BYTES, "MQ licensed receipt is empty or exceeds its byte limit")
    resolved = path.resolve(strict=True)
    repository = root.resolve(strict=True)
    require(resolved != repository and repository not in resolved.parents, "MQ licensed receipt must remain outside the candidate tree")
    body = resolved.read_bytes()
    require(sha256_bytes(body) == expected_digest, "MQ licensed receipt bytes differ from their external digest pin")
    return load_json_bytes(body, "MQ licensed receipt")


def pending_result(context: ContractContext, candidate: CandidateIdentity) -> dict[str, Any]:
    return {
        "schema_version": "mainframe-env.mq-licensed-verification-result@1",
        "status": "pending-external-licensed-receipt",
        "baseline_id": BASELINE,
        "candidate_commit": candidate.commit_sha,
        "candidate_tree": candidate.tree_sha,
        "tracked_worktree_clean": candidate.tracked_worktree_clean,
        "spec_digest": context.spec_digest,
        "verifier_digest": context.verifier_digest,
        "fixture_contract_digest": context.fixture_contract_digest,
        "differential": {"credited_calls": 0, "denominator": 26},
        "blockers": [
            "external IBM MQ 9.4 licensed receipt and exact external pins are absent",
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the committed contract")
    parser.add_argument("--receipt", type=Path, help="external receipt path; environment is the default")
    parser.add_argument("--require-pass", action="store_true", help="treat an absent receipt as a failing gate")
    args = parser.parse_args(argv)
    try:
        context = verify_contract(REPOSITORY)
        candidate = current_candidate(REPOSITORY)
        receipt_path = args.receipt
        if receipt_path is None:
            configured = os.environ.get("MAINFRAME_ENV_MQ_LICENSED_ORACLE_RECEIPT")
            receipt_path = Path(configured) if configured else None
        if receipt_path is None:
            print(json.dumps(pending_result(context, candidate), sort_keys=True))
            return 1 if args.require_pass else 0
        pins = external_pins()
        receipt = read_external_receipt(receipt_path, REPOSITORY, pins.receipt_sha256)
        result = verify_receipt_document(receipt, context, pins, candidate)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (OSError, VerificationError) as error:
        print(f"mq-licensed: fail: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
