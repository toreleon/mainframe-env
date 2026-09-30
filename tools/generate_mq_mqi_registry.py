#!/usr/bin/env python3
"""Generate the identity-only IBM MQ 9.4 MQI call registry."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SOURCE_LIST_PATH = Path("conformance/0.15/mq/source-call-list.json")
CONTRACT_CATALOG_PATH = Path("conformance/0.15/mq/structure-status-catalog.json")
OFFICIAL_CATALOG_PATH = Path("conformance/0.2/catalogs/mq.json")
TOPIC_MANIFEST_PATH = Path("conformance/0.2/manifests/mq-topics.json")
OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-host-api/src/generated/mq_mqi_calls.rs"
)
CONTRACT_OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-host-api/src/generated/mq_mqi_contracts.rs"
)
SCHEMA_VERSION = "mainframe-env.mq-source-call-list@1"
CONTRACT_SCHEMA_VERSION = "mainframe-env.mq-structure-status-catalog@1"
BASELINE = "ibm-mq-9.4-mqi-2026-08-31"
UNIT = "mqi-calls-unique"
PRODUCT = "SSFKSJ_9.4.0"
DIGEST_DOMAIN = b"mainframe-env.mq-mqi-call-identities@1\0"
CONTRACT_DIGEST_DOMAIN = b"mainframe-env.mq-mqi-contract-identities@1\0"
TOKEN = re.compile(r"^[A-Za-z][A-Za-z0-9]*$")
SYMBOL = re.compile(r"^MQ[A-Z0-9_]+$")
VERSION_SYMBOL = re.compile(r"^MQ[A-Z0-9_]+_VERSION$")
DIRECTIONS = {"input", "output", "input-output"}
ROLES = {
    "scalar",
    "name",
    "length",
    "data",
    "handle",
    "structure",
    "options",
    "selector",
    "completion",
    "reason",
}
HANDLE_ROLES = {"connection", "object", "subscription", "message"}
HANDLE_ACTIONS = {"use", "create", "release", "use-or-create"}
MQBUFMH_HMSG_ANOMALY = {
    "source_topic_path": "SSFKSJ_9.4.0/refdev/q101710_.html",
    "source_topic_sha256": "8a94879a9c9f2e18ddb2171b0dd5ea477ebaf26684a760c9e1e31931f2084156",
    "source_line": 13,
    "literal_data_type": "MQHMQSG",
    "canonical_topic_path": "SSFKSJ_9.4.0/refdev/q101780_.html",
    "canonical_topic_sha256": "66cf482573408e227aec23ec2219cd52bf7bf879591f33acb04dc3f33fb386db",
    "canonical_first_line": 37,
    "canonical_last_line": 49,
}


def _json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    return value


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _field(digest: Any, value: bytes) -> None:
    digest.update(len(value).to_bytes(8, "big"))
    digest.update(value)


def _identity_digest(rows: list[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    digest.update(DIGEST_DOMAIN)
    digest.update(len(rows).to_bytes(8, "big"))
    for row in rows:
        for key in ("official_row", "label", "topic_path", "topic_sha256"):
            _field(digest, row[key].encode())
        positions = row["source_positions"]
        digest.update(len(positions).to_bytes(8, "big"))
        for position in positions:
            digest.update(position.to_bytes(2, "big"))
    return "sha256:" + digest.hexdigest()


def _optional_field(digest: Any, value: str | None) -> None:
    _field(digest, (value or "").encode())


def _anomaly_field(digest: Any, anomaly: dict[str, Any] | None) -> None:
    if anomaly is None:
        digest.update(b"\0")
        return
    digest.update(b"\1")
    for key in (
        "source_topic_path",
        "source_topic_sha256",
        "literal_data_type",
        "canonical_topic_path",
        "canonical_topic_sha256",
    ):
        _field(digest, anomaly[key].encode())
    for key in ("source_line", "canonical_first_line", "canonical_last_line"):
        digest.update(anomaly[key].to_bytes(2, "big"))


def _contract_digest(rows: list[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    digest.update(CONTRACT_DIGEST_DOMAIN)
    digest.update(len(rows).to_bytes(8, "big"))
    for row in rows:
        for key in (
            "official_row",
            "label",
            "topic_path",
            "topic_sha256",
            "source_status",
            "signature_status",
        ):
            _field(digest, row[key].encode())
        parameters = row["parameters"]
        digest.update(len(parameters).to_bytes(8, "big"))
        for parameter in parameters:
            digest.update(parameter["position"].to_bytes(2, "big"))
            for key in ("name", "data_type", "direction"):
                _field(digest, parameter[key].encode())
            roles = parameter["roles"]
            digest.update(len(roles).to_bytes(8, "big"))
            for role in roles:
                _field(digest, role.encode())
            identities = parameter["symbolic_identities"]
            digest.update(len(identities).to_bytes(8, "big"))
            for identity in identities:
                _field(digest, identity.encode())
            _optional_field(digest, parameter["handle_role"])
            _optional_field(digest, parameter["handle_action"])
            _optional_field(digest, parameter["structure_version_identity"])
            _anomaly_field(digest, parameter["source_spelling_anomaly"])
    return "sha256:" + digest.hexdigest()


def load(root: Path = ROOT) -> list[dict[str, Any]]:
    source_path = root / SOURCE_LIST_PATH
    catalog_path = root / OFFICIAL_CATALOG_PATH
    manifest_path = root / TOPIC_MANIFEST_PATH
    source = _json(source_path)
    catalog = _json(catalog_path)
    manifest = _json(manifest_path)

    if (
        source.get("schema_version") != SCHEMA_VERSION
        or source.get("target_version") != "0.15.0"
        or source.get("work_package") != "MQ-1501.call-denominator"
        or source.get("baseline_id") != BASELINE
        or source.get("source_row_count") != 27
        or source.get("unique_call_count") != 26
        or source.get("semantic_authority") is not False
        or source.get("coverage_credit") != 0
    ):
        raise ValueError("MQ source-list identity or zero-credit boundary differs")

    catalog_binding = source.get("official_catalog")
    if not isinstance(catalog_binding, dict):
        raise ValueError("MQ source list has no official catalog binding")
    if (
        catalog_binding.get("path") != OFFICIAL_CATALOG_PATH.as_posix()
        or catalog_binding.get("sha256") != _sha256(catalog_path)
        or catalog_binding.get("unit") != UNIT
    ):
        raise ValueError("MQ official catalog binding differs")

    if (
        catalog.get("schema_version") != "mainframe-env.official-catalog@1"
        or catalog.get("baseline_id") != BASELINE
        or catalog.get("subsystem") != "mq"
        or catalog.get("mandatory_rows") != 26
    ):
        raise ValueError("MQ official catalog identity differs")
    units = catalog.get("units")
    if not isinstance(units, list) or len(units) != 1:
        raise ValueError("MQ official catalog must contain exactly one unit")
    unit = units[0]
    if unit.get("id") != UNIT or unit.get("denominator") != 26:
        raise ValueError("MQ official denominator differs")
    official = unit.get("rows")
    if not isinstance(official, list) or len(official) != 26:
        raise ValueError("MQ official row set differs")

    source_binding = source.get("source")
    if not isinstance(source_binding, dict):
        raise ValueError("MQ source list has no source binding")
    if (
        source_binding.get("topic_path")
        != "SSFKSJ_9.4.0/refdev/q101650_.html"
        or source_binding.get("first_line") != 4
        or source_binding.get("last_line") != 30
    ):
        raise ValueError("MQ call-list source locator differs")

    topics = manifest.get("topics")
    if (
        manifest.get("baseline_id") != BASELINE
        or manifest.get("product") != PRODUCT
        or not isinstance(topics, list)
    ):
        raise ValueError("MQ topic manifest identity differs")
    topic_digests = {
        topic.get("topic_path"): topic.get("sha256")
        for topic in topics
        if isinstance(topic, dict)
    }
    if topic_digests.get(source_binding["topic_path"]) != source_binding.get("sha256"):
        raise ValueError("MQ call-list topic digest differs from its pinned manifest")

    rows = source.get("rows")
    if not isinstance(rows, list) or len(rows) != 27:
        raise ValueError("MQ source list must retain exactly 27 rows")
    first_by_label: dict[str, int] = {}
    positions_by_official: dict[str, list[int]] = {}
    unique_rows: list[dict[str, Any]] = []
    for expected_position, row in enumerate(rows, 1):
        if not isinstance(row, dict) or row.get("source_position") != expected_position:
            raise ValueError("MQ source positions must be contiguous and ordered")
        label = row.get("label")
        official_row = row.get("official_row")
        duplicate = row.get("duplicate_of_source_position")
        if not isinstance(label, str) or not isinstance(official_row, str):
            raise ValueError("MQ source row label or official identity is invalid")
        if label in first_by_label:
            if duplicate != first_by_label[label]:
                raise ValueError(f"MQ duplicate provenance differs for {label}")
        else:
            if duplicate is not None:
                raise ValueError(f"first MQ source occurrence is marked duplicate: {label}")
            first_by_label[label] = expected_position
            unique_rows.append(row)
        positions_by_official.setdefault(official_row, []).append(expected_position)

    if len(unique_rows) != 26 or len(first_by_label) != 26:
        raise ValueError("MQ source rows do not normalize to 26 unique calls")
    for projected, catalog_row in zip(unique_rows, official):
        if (
            projected["official_row"] != catalog_row.get("id")
            or projected["label"] != catalog_row.get("label")
            or catalog_row.get("mandatory") is not True
        ):
            raise ValueError("MQ normalized source order differs from the official catalog")

    result = []
    for catalog_row in official:
        locator = catalog_row.get("source_locator")
        if not isinstance(locator, str) or not locator.startswith("html-link:"):
            raise ValueError("MQ official source locator is invalid")
        topic_path = f"{PRODUCT}/refdev/{locator.removeprefix('html-link:')}"
        topic_sha256 = topic_digests.get(topic_path)
        if not isinstance(topic_sha256, str):
            raise ValueError(f"MQ topic is not pinned: {topic_path}")
        result.append(
            {
                "official_row": catalog_row["id"],
                "label": catalog_row["label"],
                "topic_path": topic_path,
                "topic_sha256": topic_sha256,
                "source_positions": positions_by_official[catalog_row["id"]],
            }
        )
    return result


def load_contract(root: Path = ROOT) -> list[dict[str, Any]]:
    identity_rows = load(root)
    catalog_path = root / CONTRACT_CATALOG_PATH
    manifest_path = root / TOPIC_MANIFEST_PATH
    source_list_path = root / SOURCE_LIST_PATH
    catalog = _json(catalog_path)
    manifest = _json(manifest_path)

    if (
        catalog.get("schema_version") != CONTRACT_SCHEMA_VERSION
        or catalog.get("target_version") != "0.15.0"
        or catalog.get("work_package") != "MQ-1501.structure-status-catalog"
        or catalog.get("baseline_id") != BASELINE
        or catalog.get("unique_call_count") != 26
        or catalog.get("verified_call_topic_count") != 26
        or catalog.get("missing_call_topic_count") != 0
        or catalog.get("identity_authority") is not True
        or catalog.get("behavioral_coverage_credit") != 0
        or catalog.get("licensed_execution_credit") != 0
    ):
        raise ValueError("MQ structure/status catalog identity or zero-credit boundary differs")

    for key, expected_path, actual_path in [
        ("topic_manifest", TOPIC_MANIFEST_PATH, manifest_path),
        ("source_call_list", SOURCE_LIST_PATH, source_list_path),
    ]:
        binding = catalog.get(key)
        if (
            not isinstance(binding, dict)
            or set(binding) != {"path", "sha256"}
            or binding.get("path") != expected_path.as_posix()
            or binding.get("sha256") != _sha256(actual_path)
        ):
            raise ValueError(f"MQ structure/status {key} binding differs")

    topics = manifest.get("topics")
    if not isinstance(topics, list):
        raise ValueError("MQ topic manifest has no topic list")
    topic_digests = {
        topic.get("topic_path"): topic.get("sha256")
        for topic in topics
        if isinstance(topic, dict)
    }

    calls = catalog.get("calls")
    if not isinstance(calls, list) or len(calls) != len(identity_rows):
        raise ValueError("MQ structure/status catalog must contain exactly 26 calls")

    normalized: list[dict[str, Any]] = []
    verified = 0
    missing = 0
    total_parameters = 0
    spelling_anomaly_count = 0
    for call, identity in zip(calls, identity_rows):
        if not isinstance(call, dict) or set(call) != {
            "official_row",
            "label",
            "topic_path",
            "topic_sha256",
            "source_status",
            "signature_status",
            "parameters",
        }:
            raise ValueError("MQ structure/status call fields differ")
        for key in ("official_row", "label", "topic_path", "topic_sha256"):
            if call.get(key) != identity[key]:
                raise ValueError(f"MQ structure/status call identity differs for {identity['label']}")
        if topic_digests.get(call["topic_path"]) != call["topic_sha256"]:
            raise ValueError(f"MQ structure/status topic pin differs for {call['label']}")

        source_status = call.get("source_status")
        signature_status = call.get("signature_status")
        parameters = call.get("parameters")
        if not isinstance(parameters, list):
            raise ValueError(f"MQ parameter signature is not an array for {call['label']}")
        if source_status == "verified" and signature_status == "source-verified":
            verified += 1
            if len(parameters) < 3:
                raise ValueError(f"verified MQ signature is empty for {call['label']}")
        elif source_status == "missing" and signature_status == "pending-source":
            missing += 1
            if parameters or call["label"] != "MQINQ":
                raise ValueError("only source-missing MQINQ may have a pending empty signature")
        else:
            raise ValueError(f"MQ source/signature status differs for {call['label']}")

        normalized_parameters: list[dict[str, Any]] = []
        completion_count = 0
        reason_count = 0
        for expected_position, parameter in enumerate(parameters, 1):
            if not isinstance(parameter, dict):
                raise ValueError(f"MQ parameter is not an object for {call['label']}")
            required = {"position", "name", "data_type", "direction", "roles"}
            optional = {
                "symbolic_identities",
                "handle_role",
                "handle_action",
                "structure_version_identity",
                "source_spelling_anomaly",
            }
            if not required.issubset(parameter) or not set(parameter).issubset(required | optional):
                raise ValueError(f"MQ parameter fields differ for {call['label']}")
            position = parameter.get("position")
            name = parameter.get("name")
            data_type = parameter.get("data_type")
            direction = parameter.get("direction")
            roles = parameter.get("roles")
            identities = parameter.get("symbolic_identities", [])
            handle_role = parameter.get("handle_role")
            handle_action = parameter.get("handle_action")
            version_identity = parameter.get("structure_version_identity")
            spelling_anomaly = parameter.get("source_spelling_anomaly")
            if (
                position != expected_position
                or not isinstance(name, str)
                or TOKEN.fullmatch(name) is None
                or not isinstance(data_type, str)
                or not 2 <= len(data_type) <= 64
                or direction not in DIRECTIONS
                or not isinstance(roles, list)
                or not 1 <= len(roles) <= 2
                or len(roles) != len(set(roles))
                or any(role not in ROLES for role in roles)
                or not isinstance(identities, list)
                or len(identities) > 2
                or len(identities) != len(set(identities))
                or any(not isinstance(value, str) or SYMBOL.fullmatch(value) is None for value in identities)
            ):
                raise ValueError(f"MQ parameter identity differs for {call['label']} position {expected_position}")

            is_handle = "handle" in roles
            if is_handle:
                if (
                    roles != ["handle"]
                    or handle_role not in HANDLE_ROLES
                    or handle_action not in HANDLE_ACTIONS
                    or identities != [data_type]
                    or version_identity is not None
                ):
                    raise ValueError(f"MQ handle role differs for {call['label']} parameter {name}")
            elif handle_role is not None or handle_action is not None:
                raise ValueError(f"non-handle MQ parameter has handle metadata: {call['label']} {name}")

            is_mqbufmh_hmsg = call["label"] == "MQBUFMH" and name == "Hmsg"
            if spelling_anomaly is not None:
                spelling_anomaly_count += 1
                if (
                    not is_mqbufmh_hmsg
                    or spelling_anomaly != MQBUFMH_HMSG_ANOMALY
                    or topic_digests.get(spelling_anomaly["source_topic_path"])
                    != spelling_anomaly["source_topic_sha256"]
                    or topic_digests.get(spelling_anomaly["canonical_topic_path"])
                    != spelling_anomaly["canonical_topic_sha256"]
                    or data_type != "MQHMSG"
                    or identities != ["MQHMSG"]
                ):
                    raise ValueError("MQBUFMH Hmsg source-spelling normalization differs")
            elif is_mqbufmh_hmsg:
                raise ValueError("MQBUFMH Hmsg source-spelling anomaly is missing")

            if "structure" in roles:
                if not identities or identities[0] != data_type:
                    raise ValueError(f"MQ structure identity differs for {call['label']} parameter {name}")
            elif version_identity is not None:
                raise ValueError(f"non-structure MQ parameter has a version identity: {call['label']} {name}")
            if version_identity is not None and (
                not isinstance(version_identity, str)
                or VERSION_SYMBOL.fullmatch(version_identity) is None
                or version_identity != f"{data_type}_VERSION"
            ):
                raise ValueError(f"MQ structure version identity differs for {call['label']} parameter {name}")

            identity_roles = {"options", "selector", "completion", "reason"}
            if identity_roles.intersection(roles) and not identities:
                raise ValueError(f"MQ symbolic identity is missing for {call['label']} parameter {name}")
            if "completion" in roles:
                completion_count += 1
                if roles != ["completion"] or identities != ["MQCC"] or data_type != "MQLONG" or direction != "output":
                    raise ValueError(f"MQ completion identity differs for {call['label']}")
            if "reason" in roles:
                reason_count += 1
                if roles != ["reason"] or identities != ["MQRC"] or data_type != "MQLONG" or direction != "output":
                    raise ValueError(f"MQ reason identity differs for {call['label']}")

            normalized_parameters.append(
                {
                    "position": position,
                    "name": name,
                    "data_type": data_type,
                    "direction": direction,
                    "roles": roles,
                    "symbolic_identities": identities,
                    "handle_role": handle_role,
                    "handle_action": handle_action,
                    "structure_version_identity": version_identity,
                    "source_spelling_anomaly": spelling_anomaly,
                }
            )

        expected_status_count = 0 if call["label"] == "MQCB_FUNCTION" else 1
        if completion_count != expected_status_count or reason_count != expected_status_count:
            raise ValueError(f"MQ completion/reason parameter set differs for {call['label']}")
        total_parameters += len(normalized_parameters)
        normalized.append({**{key: call[key] for key in call if key != "parameters"}, "parameters": normalized_parameters})

    if (
        verified != 26
        or missing != 0
        or total_parameters < 100
        or spelling_anomaly_count != 1
    ):
        raise ValueError("MQ structure/status source or parameter counts differ")
    return normalized


def render(root: Path = ROOT) -> str:
    rows = load(root)
    rendered = [
        "// @generated by `tools/generate_mq_mqi_registry.py`; do not edit.\n",
        f'pub(super) const MQ_MQI_CALL_IDENTITY_SET_SHA256: &str = "{_identity_digest(rows)}";\n',
        "pub(super) const MQ_MQI_CALL_IDENTITIES: &[MqMqiCallIdentityDescriptor] = &[\n",
    ]
    for row in rows:
        positions = ", ".join(str(value) for value in row["source_positions"])
        rendered.extend(
            [
                "    MqMqiCallIdentityDescriptor {\n",
                f'        official_row: {json.dumps(row["official_row"])},\n',
                f'        label: {json.dumps(row["label"])},\n',
                f'        topic_path: {json.dumps(row["topic_path"])},\n',
                f'        topic_sha256: {json.dumps(row["topic_sha256"])},\n',
                f"        source_positions: &[{positions}],\n",
                "    },\n",
            ]
        )
    rendered.append("];\n")
    return "".join(rendered)


def _rust_variant(value: str) -> str:
    variants = {
        "verified": "Verified",
        "missing": "Missing",
        "source-verified": "SourceVerified",
        "pending-source": "PendingSource",
        "input": "Input",
        "output": "Output",
        "input-output": "InputOutput",
        "scalar": "Scalar",
        "name": "Name",
        "length": "Length",
        "data": "Data",
        "handle": "Handle",
        "structure": "Structure",
        "options": "Options",
        "selector": "Selector",
        "completion": "CompletionCode",
        "reason": "ReasonCode",
        "connection": "Connection",
        "object": "Object",
        "subscription": "Subscription",
        "message": "Message",
        "use": "Use",
        "create": "Create",
        "release": "Release",
        "use-or-create": "UseOrCreate",
    }
    try:
        return variants[value]
    except KeyError as error:
        raise ValueError(f"no generated Rust variant for {value}") from error


def _rust_strings(values: list[str]) -> str:
    return "&[" + ", ".join(json.dumps(value) for value in values) + "]"


def _rust_optional_enum(type_name: str, value: str | None) -> str:
    if value is None:
        return "None"
    return f"Some({type_name}::{_rust_variant(value)})"


def _rust_optional_string(value: str | None) -> str:
    return "None" if value is None else f"Some({json.dumps(value)})"


def _rust_spelling_anomaly(value: dict[str, Any] | None) -> str:
    if value is None:
        return "None"
    return (
        "Some(MqMqiSourceSpellingAnomaly { "
        f'source_topic_path: {json.dumps(value["source_topic_path"])}, '
        f'source_topic_sha256: {json.dumps(value["source_topic_sha256"])}, '
        f'source_line: {value["source_line"]}, '
        f'literal_data_type: {json.dumps(value["literal_data_type"])}, '
        f'canonical_topic_path: {json.dumps(value["canonical_topic_path"])}, '
        f'canonical_topic_sha256: {json.dumps(value["canonical_topic_sha256"])}, '
        f'canonical_first_line: {value["canonical_first_line"]}, '
        f'canonical_last_line: {value["canonical_last_line"]} '
        "})"
    )


def render_contract(root: Path = ROOT) -> str:
    rows = load_contract(root)
    parameter_count = sum(len(row["parameters"]) for row in rows)
    rendered = [
        "// @generated by `tools/generate_mq_mqi_registry.py`; do not edit.\n",
        f'pub(super) const MQ_MQI_CONTRACT_CATALOG_SHA256: &str = "sha256:{_sha256(root / CONTRACT_CATALOG_PATH)}";\n',
        f'pub(super) const MQ_MQI_CONTRACT_SET_SHA256: &str = "{_contract_digest(rows)}";\n',
        f"pub(super) const MQ_MQI_PARAMETER_COUNT: usize = {parameter_count};\n",
        "pub(super) const MQ_MQI_CONTRACTS: &[MqMqiContractDescriptor] = &[\n",
    ]
    for row in rows:
        rendered.extend(
            [
                "    MqMqiContractDescriptor {\n",
                f'        official_row: {json.dumps(row["official_row"])},\n',
                f'        label: {json.dumps(row["label"])},\n',
                f'        topic_path: {json.dumps(row["topic_path"])},\n',
                f'        topic_sha256: {json.dumps(row["topic_sha256"])},\n',
                f'        source_status: MqMqiSourceStatus::{_rust_variant(row["source_status"])},\n',
                f'        signature_status: MqMqiSignatureStatus::{_rust_variant(row["signature_status"])},\n',
                "        parameters: &[\n",
            ]
        )
        for parameter in row["parameters"]:
            roles = ", ".join(
                f"MqMqiParameterRole::{_rust_variant(role)}"
                for role in parameter["roles"]
            )
            rendered.extend(
                [
                    "            MqMqiParameterDescriptor { "
                    f'position: {parameter["position"]}, '
                    f'name: {json.dumps(parameter["name"])}, '
                    f'data_type: {json.dumps(parameter["data_type"])},\n',
                    "                direction: "
                    f'MqMqiParameterDirection::{_rust_variant(parameter["direction"])}, '
                    f"roles: &[{roles}], "
                    f'symbolic_identities: {_rust_strings(parameter["symbolic_identities"])},\n',
                    "                handle_role: "
                    + _rust_optional_enum("MqMqiHandleRole", parameter["handle_role"])
                    + ", handle_action: "
                    + _rust_optional_enum("MqMqiHandleAction", parameter["handle_action"])
                    + ", structure_version_identity: "
                    + _rust_optional_string(parameter["structure_version_identity"])
                    + ",\n",
                    "                source_spelling_anomaly: "
                    + _rust_spelling_anomaly(parameter["source_spelling_anomaly"])
                    + ",\n",
                    "            },\n",
                ]
            )
        rendered.extend(["        ],\n", "    },\n"])
    rendered.append("];\n")
    return "".join(rendered)


def check(root: Path = ROOT) -> None:
    for output_path, expected in [
        (OUTPUT_PATH, render(root)),
        (CONTRACT_OUTPUT_PATH, render_contract(root)),
    ]:
        output = root / output_path
        if not output.is_file() or output.read_text() != expected:
            raise ValueError(f"stale generated MQ MQI registry: {output}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        check(ROOT)
    else:
        for output_path, rendered in [
            (OUTPUT_PATH, render(ROOT)),
            (CONTRACT_OUTPUT_PATH, render_contract(ROOT)),
        ]:
            output = ROOT / output_path
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
