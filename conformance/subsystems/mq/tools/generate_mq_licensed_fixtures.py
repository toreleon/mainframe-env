#!/usr/bin/env python3
"""Generate the independent IBM MQ 9.4 licensed-differential fixture index."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys


REPOSITORY = Path(__file__).resolve().parents[4]
CATALOG = REPOSITORY / "conformance/subsystems/coverage/catalogs/mq.json"
MANIFEST = REPOSITORY / "conformance/subsystems/coverage/manifests/mq-topics.json"
SOURCE_CALL_LIST = REPOSITORY / "conformance/subsystems/mq/mq/source-call-list.json"
STRUCTURE_CATALOG = REPOSITORY / "conformance/subsystems/mq/mq/structure-status-catalog.json"
OUTPUT = REPOSITORY / "conformance/subsystems/mq/fixtures/mq-licensed-differential-cases.json"
BASELINE = "ibm-mq-9.4-mqi-2026-08-31"
UNIT = "mqi-calls-unique"
PRODUCT = "SSFKSJ_9.4.0"

# These profiles select independently prepared external fixture members. They are
# deliberately not product outputs, oracle observations, or expected MQ results.
CASE_PROFILES = {
    "MQBACK": "syncpoint-backout",
    "MQBEGIN": "syncpoint-begin",
    "MQBUFMH": "buffer-to-message-handle",
    "MQCB": "callback-register-manage",
    "MQCB_FUNCTION": "callback-dispatch",
    "MQCLOSE": "object-close",
    "MQCMIT": "syncpoint-commit",
    "MQCONN": "queue-manager-connect",
    "MQCONNX": "extended-connect-options",
    "MQCRTMH": "message-handle-create",
    "MQCTL": "callback-control",
    "MQDISC": "queue-manager-disconnect",
    "MQDLTMH": "message-handle-delete",
    "MQDLTMP": "message-property-delete",
    "MQGET": "message-get",
    "MQINQ": "object-attributes-inquire",
    "MQINQMP": "message-property-inquire",
    "MQMHBUF": "message-handle-to-buffer",
    "MQOPEN": "object-open",
    "MQPUT": "message-put",
    "MQPUT1": "message-put-one",
    "MQSET": "object-attributes-set",
    "MQSETMP": "message-property-set",
    "MQSTAT": "status-retrieval",
    "MQSUB": "subscription-register",
    "MQSUBRQ": "subscription-request",
}


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"{path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path}: root must be an object")
    return value


def generated_document() -> dict:
    catalog = read_json(CATALOG)
    manifest = read_json(MANIFEST)
    source_list = read_json(SOURCE_CALL_LIST)
    structure_catalog = read_json(STRUCTURE_CATALOG)
    if (
        catalog.get("baseline_id") != BASELINE
        or catalog.get("mandatory_rows") != 26
        or manifest.get("baseline_id") != BASELINE
        or manifest.get("product") != PRODUCT
        or manifest.get("topic_count") != 27
    ):
        raise ValueError("MQ catalog or source manifest identity drifted")
    units = catalog.get("units")
    if not isinstance(units, list) or len(units) != 1:
        raise ValueError("MQ catalog must contain one unit")
    unit = units[0]
    rows = unit.get("rows")
    if (
        unit.get("id") != UNIT
        or unit.get("denominator") != 26
        or not isinstance(rows, list)
        or len(rows) != 26
    ):
        raise ValueError("MQ call denominator drifted")
    topics = {
        topic.get("topic_path"): topic.get("sha256")
        for topic in manifest.get("topics", [])
        if isinstance(topic, dict)
    }
    labels = [row.get("label") for row in rows if isinstance(row, dict)]
    if len(labels) != 26 or set(labels) != set(CASE_PROFILES):
        raise ValueError("MQ fixture profile set differs from the official calls")
    source_rows = source_list.get("rows")
    if (
        source_list.get("baseline_id") != BASELINE
        or source_list.get("unique_call_count") != 26
        or not isinstance(source_rows, list)
        or {(row.get("label"), row.get("official_row")) for row in source_rows}
        != {(row["label"], row["id"]) for row in rows}
        or structure_catalog.get("baseline_id") != BASELINE
        or structure_catalog.get("unique_call_count") != 26
        or {call.get("label") for call in structure_catalog.get("calls", [])} != set(labels)
    ):
        raise ValueError("MQ M3 call or structure denominator drifted")
    for call in structure_catalog["calls"]:
        if topics.get(call.get("topic_path")) != call.get("topic_sha256"):
            raise ValueError("MQ M3 call topic identity drifted")

    cases = []
    for row in rows:
        label = row["label"]
        locator = row.get("source_locator", "")
        if not locator.startswith("html-link:"):
            raise ValueError(f"MQ row {label} has no HTML topic locator")
        topic_path = f"{PRODUCT}/refdev/{locator.removeprefix('html-link:')}"
        if topic_path not in topics:
            raise ValueError(f"MQ row {label} topic is absent from the pinned manifest")
        profile = CASE_PROFILES[label]
        cases.append(
            {
                "case_id": f"mq.mqi.{label.lower()}.licensed-differential",
                "row_id": row["id"],
                "call": label,
                "source_topic_path": topic_path,
                "input_profile": f"independent-mqi.{profile}.v1",
                "fixture_bundle_member": f"mqi/{label.lower()}.json",
                "observation_profile": "mq-normalized-observation@1",
            }
        )

    return {
        "schema_version": "mainframe-env.mq-licensed-differential-fixtures@1",
        "target_subsystem": "mq.programming",
        "work_package": "MQ-1506.licensed-harness-contract",
        "baseline_id": BASELINE,
        "unit": UNIT,
        "independent_fixture_authority": {
            "kind": "reviewed-external-fixture-bundle",
            "product_output_derived": False,
            "oracle_output_embedded": False,
            "raw_fixture_bytes_retained": False,
            "external_bundle_digest_required": True,
        },
        "case_count": 26,
        "cases_per_call": 1,
        "cases": cases,
    }


def render() -> str:
    return json.dumps(generated_document(), indent=2, ensure_ascii=True) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    args = parser.parse_args(argv)
    generated = render()
    if args.check:
        try:
            current = args.output.read_text(encoding="utf-8")
        except OSError as error:
            print(f"MQ licensed fixture index: {error}", file=sys.stderr)
            return 1
        if current != generated:
            print(f"stale MQ licensed fixture index: {args.output}", file=sys.stderr)
            return 1
        print("mq-licensed-fixtures: pass cases=26 independent=true")
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
