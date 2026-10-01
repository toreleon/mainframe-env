#!/usr/bin/env python3
"""Generate the 25-row IMS licensed fixture index from pinned identities.

This index selects external inputs; it contains no expected IBM observations.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
CATALOG = ROOT / "conformance/0.2/catalogs/ims.json"
RULES = ROOT / "conformance/0.14/ims/call-applicability-rules.json"
MANIFEST = ROOT / "conformance/0.2/manifests/ims-topics.json"
OUTPUT = ROOT / "conformance/0.14/fixtures/ims-licensed-differential-cases.json"
BASELINE = "ibm-ims-15.6-dli-2026-08-31"
UNIT = "dli-call-families"

# Reviewed scenario selectors are independent of product outputs. External
# fixture bytes and all expected IBM observations require a separate pin.
SCENARIOS = (
    ("availability-init", "completed"),
    ("basic-checkpoint", "completed"),
    ("deq-forbidden-context", "rejected"),
    ("delete-without-hold", "rejected"),
    ("qualified-get-not-found", "condition"),
    ("get-hold-position", "completed"),
    ("schedule-code-get", "completed"),
    ("database-insert", "completed"),
    ("initial-load-insert", "completed"),
    ("log-record", "completed"),
    ("dedb-position", "completed"),
    ("availability-accept", "completed"),
    ("availability-query", "completed"),
    ("availability-refresh", "completed"),
    ("replace-without-hold", "rejected"),
    ("restart-retrieve", "completed"),
    ("rollback", "completed"),
    ("rollback-without-savepoint", "rejected"),
    ("pcb-schedule", "completed"),
    ("set-savepoint", "completed"),
    ("setu-without-token", "rejected"),
    ("database-statistics", "completed"),
    ("extended-checkpoint", "completed"),
    ("terminate", "completed"),
    ("restart-without-checkpoint", "condition"),
)


def load(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path}: expected object")
    return value


def generated_document(root: Path = ROOT) -> dict:
    catalog = load(root / CATALOG.relative_to(ROOT))
    rules = load(root / RULES.relative_to(ROOT))
    manifest = load(root / MANIFEST.relative_to(ROOT))
    if catalog.get("baseline_id") != BASELINE or catalog.get("mandatory_rows") != 25:
        raise ValueError("IMS official denominator drifted")
    units = catalog.get("units")
    if not isinstance(units, list) or len(units) != 1:
        raise ValueError("IMS official unit set drifted")
    unit = units[0]
    rows = unit.get("rows")
    if unit.get("id") != UNIT or unit.get("denominator") != 25 or not isinstance(rows, list) or len(rows) != 25:
        raise ValueError("IMS official rows drifted")
    if rules.get("baseline_id") != BASELINE or len(rules.get("families", [])) != 25:
        raise ValueError("IMS applicability matrix drifted")
    if manifest.get("baseline_id") != BASELINE or manifest.get("product") != "SSEPH2_15.6.0":
        raise ValueError("IMS comparison source drifted")
    topic = manifest.get("topics", [None])[0]
    if not isinstance(topic, dict) or topic.get("topic_path") != manifest.get("book_href"):
        raise ValueError("IMS comparison topic drifted")
    cases = []
    for number, (row, family, scenario) in enumerate(zip(rows, rules["families"], SCENARIOS, strict=True), 1):
        profile, expected_class = scenario
        if row.get("id") != f"{BASELINE}:{UNIT}:{number:04d}" or family[0] != number:
            raise ValueError(f"IMS row {number} identity drifted")
        if row.get("source_locator", "").split(";")[1] != f"row:{number}":
            raise ValueError(f"IMS row {number} source locator drifted")
        if not isinstance(family[1], list) or not family[1]:
            raise ValueError(f"IMS row {number} has no applicability profile")
        cases.append({
            "case_id": f"ims.dli.{number:04d}.licensed-differential",
            "row_id": row["id"],
            "call_family": row["label"],
            "source_locator": row["source_locator"],
            "source_topic_path": topic["topic_path"],
            "source_topic_sha256": "sha256:" + topic["sha256"],
            "applicability_profiles": family[1],
            "input_profile": f"independent-ims.{profile}.v1",
            "scenario_class": expected_class,
            "fixture_bundle_member": f"dli/{number:04d}.json",
            "observation_profile": "ims-normalized-observation@1",
        })
    return {
        "schema_version": "mainframe-env.ims-licensed-differential-fixtures@1",
        "target_version": "0.14.0",
        "work_package": "IMS-1406.licensed-harness-contract",
        "baseline_id": BASELINE,
        "unit": UNIT,
        "independent_fixture_authority": {
            "kind": "reviewed-external-fixture-bundle",
            "product_output_derived": False,
            "oracle_output_embedded": False,
            "raw_fixture_bytes_retained": False,
            "external_bundle_digest_required": True,
        },
        "case_count": 25,
        "cases_per_family": 1,
        "cases": cases,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    body = json.dumps(generated_document(), indent=2, ensure_ascii=True) + "\n"
    if args.check:
        if OUTPUT.read_text(encoding="utf-8") != body:
            raise ValueError("IMS licensed fixture index is stale")
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        OUTPUT.write_text(body, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
