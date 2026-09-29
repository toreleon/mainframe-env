#!/usr/bin/env python3
"""Generate the identity-only IBM MQ 9.4 MQI call registry."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SOURCE_LIST_PATH = Path("conformance/0.15/mq/source-call-list.json")
OFFICIAL_CATALOG_PATH = Path("conformance/0.2/catalogs/mq.json")
TOPIC_MANIFEST_PATH = Path("conformance/0.2/manifests/mq-topics.json")
OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-host-api/src/generated/mq_mqi_calls.rs"
)
SCHEMA_VERSION = "mainframe-env.mq-source-call-list@1"
BASELINE = "ibm-mq-9.4-mqi-2026-08-31"
UNIT = "mqi-calls-unique"
PRODUCT = "SSFKSJ_9.4.0"
DIGEST_DOMAIN = b"mainframe-env.mq-mqi-call-identities@1\0"


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


def render(root: Path = ROOT) -> str:
    rows = load(root)
    rendered = [
        "// @generated by tools/generate_mq_mqi_registry.py; do not edit.\n",
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


def check(root: Path = ROOT) -> None:
    output = root / OUTPUT_PATH
    expected = render(root)
    if not output.is_file() or output.read_text() != expected:
        raise ValueError(f"stale generated MQ MQI registry: {output}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        check(ROOT)
    else:
        output = ROOT / OUTPUT_PATH
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(render(ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
