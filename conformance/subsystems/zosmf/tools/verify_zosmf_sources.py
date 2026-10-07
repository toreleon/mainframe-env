#!/usr/bin/env python3
"""Verify the ZMF-1101 catalog against its immutable rows and retained HTML.

This command is offline.  It never fetches, repins, or grants coverage credit.
The optional archive check reads only content-addressed bodies whose identities
are already committed in the z/OSMF 3.2 manifest.
"""

from __future__ import annotations

import argparse
from collections import Counter
import copy
import hashlib
import json
from pathlib import Path
import sys
from typing import Any


REPOSITORY = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPOSITORY / "conformance/tools"))
import ibm_docs  # noqa: E402  (repository-owned bounded HTML parser)


CATALOG = REPOSITORY / "conformance/subsystems/zosmf/catalogs/zosmf-normalization.json"
OFFICIAL = REPOSITORY / "conformance/subsystems/coverage/catalogs/zosmf.json"
MANIFEST = REPOSITORY / "conformance/subsystems/coverage/manifests/zosmf-topics.json"
OFFICIAL_ROUTES = REPOSITORY / "conformance/subsystems/coverage/routes/official-route-bindings.json"
CUSTOM_ROUTES = REPOSITORY / "conformance/subsystems/coverage/routes/custom-routes.json"
EXPECTED_DISPOSITIONS = {
    "concrete-operation": 174,
    "context-reference": 1,
    "error-reference": 4,
    "operation-bundle": 2,
    "operation-group": 6,
    "schema-reference": 2,
}
GATES = {
    "recognized",
    "validated",
    "executed",
    "conditioned",
    "recovered",
    "differential",
}


class CatalogError(ValueError):
    """A normalization or source-identity invariant failed."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CatalogError(message)


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CatalogError(f"invalid JSON {path}: {error}") from error
    require(isinstance(value, dict), f"{path} must contain an object")
    return value


def digest(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def _unique(rows: list[dict[str, Any]], key: str, owner: str) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for row in rows:
        value = row.get(key)
        require(isinstance(value, str) and value, f"{owner} has an invalid {key}")
        require(value not in result, f"{owner} duplicates {key} {value}")
        result[value] = row
    return result


def _official_units(official: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    units = official.get("units")
    require(isinstance(units, list) and len(units) == 2, "official z/OSMF units changed")
    require(
        [unit.get("id") for unit in units]
        == ["rest-service-families", "direct-guide-operation-headings"],
        "official z/OSMF unit order or identity changed",
    )
    families, headings = units[0].get("rows"), units[1].get("rows")
    require(isinstance(families, list) and len(families) == 27, "official family denominator changed")
    require(isinstance(headings, list) and len(headings) == 189, "official heading denominator changed")
    return families, headings


def validate_structure(
    root: Path = REPOSITORY,
    *,
    catalog_override: dict[str, Any] | None = None,
) -> dict[str, int]:
    catalog = copy.deepcopy(catalog_override) if catalog_override is not None else load_json(
        root / CATALOG.relative_to(REPOSITORY)
    )
    official = load_json(root / OFFICIAL.relative_to(REPOSITORY))
    manifest = load_json(root / MANIFEST.relative_to(REPOSITORY))
    official_routes = load_json(root / OFFICIAL_ROUTES.relative_to(REPOSITORY))
    custom_routes = load_json(root / CUSTOM_ROUTES.relative_to(REPOSITORY))

    require(catalog.get("schema_version") == "mainframe-env.zosmf-normalization@1", "catalog version changed")
    require(catalog.get("target_subsystem") == "zosmf.rest", "catalog target changed")
    require(catalog.get("work_package") == "ZMF-1101", "catalog work package changed")
    require(official.get("baseline_id") == catalog.get("baseline", {}).get("id"), "baseline mismatch")
    require(
        catalog.get("baseline", {}).get("topic_manifest_sha256")
        == "sha256:" + str(manifest.get("topic_manifest_digest")),
        "topic-manifest identity mismatch",
    )
    require(
        catalog.get("baseline", {}).get("toc_sha256")
        == "sha256:" + str(manifest.get("toc_sha256")),
        "TOC identity mismatch",
    )

    official_families, official_headings = _official_units(official)
    families = catalog.get("families")
    headings = catalog.get("headings")
    operations = catalog.get("operations")
    require(isinstance(families, list) and len(families) == 27, "normalization must contain 27 families")
    require(isinstance(headings, list) and len(headings) == 189, "normalization must contain 189 headings")
    require(isinstance(operations, list) and len(operations) == 278, "normalization must contain 278 operations")

    family_by_id = _unique(families, "id", "families")
    heading_by_row = _unique(headings, "row_id", "headings")
    operation_by_id = _unique(operations, "id", "operations")
    require(list(operation_by_id) == sorted(operation_by_id), "operations are not canonically ordered")

    for actual, expected in zip(families, official_families, strict=True):
        source = actual.get("source", {})
        require(source.get("row_id") == expected.get("id"), "family row order/identity differs")
        require(actual.get("label") == expected.get("label"), f"family label differs for {expected.get('id')}")
        require(source.get("locator") == expected.get("source_locator"), f"family locator differs for {expected.get('id')}")
    for expected in official_headings:
        row_id = expected.get("id")
        actual = heading_by_row.get(row_id)
        require(actual is not None, f"heading row is absent: {row_id}")
        source = actual.get("source", {})
        require(actual.get("label") == expected.get("label"), f"heading label differs for {row_id}")
        require(source.get("row_id") == row_id, f"heading source row differs for {row_id}")
        require(source.get("locator") == expected.get("source_locator"), f"heading locator differs for {row_id}")

    dispositions = Counter(row.get("source", {}).get("disposition") for row in headings)
    require(dict(dispositions) == EXPECTED_DISPOSITIONS, f"heading dispositions differ: {dict(dispositions)}")

    source_rows = {
        row["source"]["row_id"]: row["source"] for row in [*families, *headings]
    }
    require(len(source_rows) == 216, "source row closure must contain 216 distinct rows")
    manifest_rows = _unique(manifest.get("topics", []), "topic_path", "z/OSMF manifest")
    for row_id, source in source_rows.items():
        pin = manifest_rows.get(source.get("topic_path"))
        require(pin is not None, f"{row_id} topic is absent from the manifest")
        require(
            source.get("sha256") == pin.get("sha256") and source.get("bytes") == pin.get("bytes"),
            f"{row_id} topic pin differs from the manifest",
        )

    operation_refs: set[str] = set()
    for family_id, family in family_by_id.items():
        family_headings = set(family.get("heading_row_ids", []))
        expected_headings = {
            row_id for row_id, heading in heading_by_row.items() if heading.get("family_id") == family_id
        }
        require(family_headings == expected_headings, f"{family_id} heading closure differs")
        family_operations = set(family.get("normalized_operation_ids", []))
        expected_operations = {
            operation_id
            for operation_id, operation in operation_by_id.items()
            if operation.get("family_id") == family_id
        }
        require(family_operations == expected_operations, f"{family_id} operation closure differs")
        operation_refs.update(family_operations)
    require(operation_refs == set(operation_by_id), "family operation closure is incomplete")

    for row_id, heading in heading_by_row.items():
        family_id = heading.get("family_id")
        require(family_id in family_by_id, f"{row_id} references an unknown family")
        disposition = heading.get("source", {}).get("disposition")
        refs = heading.get("normalized_operation_ids")
        require(isinstance(refs, list), f"{row_id} operation references are invalid")
        if disposition in {"context-reference", "schema-reference", "error-reference"}:
            require(not refs, f"{row_id} reference heading must not become an operation")
        else:
            require(bool(refs), f"{row_id} operation heading has no normalized operation")
        for operation_id in refs:
            operation = operation_by_id.get(operation_id)
            require(operation is not None, f"{row_id} references unknown operation {operation_id}")
            require(operation.get("family_id") == family_id, f"{row_id} crosses family ownership")
            require(row_id in operation.get("source_row_ids", []), f"{operation_id} omits source {row_id}")

    obligation_ids: set[str] = set()
    route_variants = 0
    for operation_id, operation in operation_by_id.items():
        family_id = operation.get("family_id")
        require(family_id in family_by_id, f"{operation_id} references an unknown family")
        routes = operation.get("routes")
        require(isinstance(routes, list) and routes, f"{operation_id} has no route variants")
        route_variants += len(routes)
        route_keys: set[tuple[str, str, str]] = set()
        for route in routes:
            key = (route.get("method"), route.get("source_uri"), route.get("normalized_path"))
            require(key not in route_keys, f"{operation_id} duplicates route {key}")
            route_keys.add(key)
            require(str(route.get("source_uri", "")).startswith("/zosmf/"), f"{operation_id} source URI escapes /zosmf")
            require(str(route.get("normalized_path", "")).startswith("/zosmf/"), f"{operation_id} normalized path escapes /zosmf")
            evidence = route.get("source_evidence")
            require(isinstance(evidence, list) and evidence, f"{operation_id} route lacks source evidence")
            for item in evidence:
                require(item.get("row_id") in source_rows, f"{operation_id} route evidence is unknown")
                require(item.get("row_id") in operation.get("source_row_ids", []), f"{operation_id} route/source closure differs")
        backend = operation.get("backend", {})
        publication = operation.get("publication", {})
        schemas = operation.get("schemas", {})
        require(publication.get("new_route_advertised") is False, f"{operation_id} advertises a new route")
        if backend.get("acceptance") != "accepted" or schemas.get("resolution") == "identity-only-pending-detail":
            require(publication.get("state") == "withheld", f"{operation_id} bypasses a backend/schema blocker")
        obligations = operation.get("mandatory_obligations")
        require(isinstance(obligations, list) and obligations, f"{operation_id} has no obligations")
        operation_gates = Counter(item.get("gate") for item in obligations)
        require(set(operation_gates).issubset(GATES), f"{operation_id} has an unknown gate")
        for gate in {"recognized", "validated", "executed", "conditioned", "differential"}:
            require(operation_gates[gate] > 0, f"{operation_id} omits mandatory {gate}")
        for obligation in obligations:
            obligation_id = obligation.get("id")
            require(isinstance(obligation_id, str) and obligation_id, f"{operation_id} has invalid obligation")
            require(obligation_id not in obligation_ids, f"duplicate obligation {obligation_id}")
            obligation_ids.add(obligation_id)

    require(route_variants == 352, f"route-variant count changed: {route_variants}")
    require(len(obligation_ids) == 1727, f"obligation count changed: {len(obligation_ids)}")

    official_rows = official_routes.get("routes")
    custom_rows = custom_routes.get("routes")
    legacy_rows = catalog.get("legacy_official_routes")
    require(isinstance(official_rows, list) and len(official_rows) == 23, "official route denominator changed")
    require(isinstance(custom_rows, list) and len(custom_rows) == 7, "custom route denominator changed")
    require(isinstance(legacy_rows, list) and len(legacy_rows) == 23, "legacy route preservation changed")
    require(
        [(row.get("id"), row.get("handler")) for row in legacy_rows]
        == [(row.get("id"), row.get("handler")) for row in official_rows],
        "legacy route bindings differ from the frozen official bindings",
    )
    official_ids = {row.get("id") for row in official_rows}
    custom_ids = {row.get("id") for row in custom_rows}
    require(official_ids.isdisjoint(custom_ids), "official and custom route IDs overlap")
    require(all(" /zosmf/" in str(value) for value in official_ids), "official route escaped /zosmf")
    require(all(" /mainframe-env/" in str(value) for value in custom_ids), "custom route escaped /mainframe-env")
    require(
        sum(not row.get("normalized_operation_ids") for row in legacy_rows) == 1,
        "exactly the frozen dataset-search route must remain source-unmapped",
    )
    policy = catalog.get("publication_policy", {})
    require(policy.get("new_official_routes_advertised") == 0, "ZMF-1101 must not advertise routes")
    require(policy.get("custom_routes_can_count_as_official") is False, "custom routes gained official credit")

    return {
        "families": len(families),
        "headings": len(headings),
        "source_rows": len(source_rows),
        "operations": len(operations),
        "route_variants": route_variants,
        "obligations": len(obligation_ids),
        "official_routes": len(official_rows),
        "custom_routes": len(custom_rows),
    }


def read_verified_body(path: Path, expected_sha256: str, expected_bytes: int) -> bytes:
    require(path.is_file() and not path.is_symlink(), f"retained body is absent or unsafe: {path}")
    body = path.read_bytes()
    require(len(body) == expected_bytes, f"retained body byte count differs: {path.name}")
    require(hashlib.sha256(body).hexdigest() == expected_sha256, f"retained body digest differs: {path.name}")
    return body


def validate_sources(archive_root: Path, root: Path = REPOSITORY) -> dict[str, int]:
    catalog = load_json(root / CATALOG.relative_to(REPOSITORY))
    manifest = load_json(root / MANIFEST.relative_to(REPOSITORY))
    html_root = archive_root / "raw/html/sha256"
    toc_root = archive_root / "raw/toc/sha256"
    toc_sha256 = str(manifest.get("toc_sha256"))
    read_verified_body(toc_root / toc_sha256[:2] / f"{toc_sha256}.json", toc_sha256, (toc_root / toc_sha256[:2] / f"{toc_sha256}.json").stat().st_size)

    source_rows = {
        row["source"]["row_id"]: (row["label"], row["source"])
        for row in [*catalog["families"], *catalog["headings"]]
    }
    parsed: dict[str, list[str]] = {}
    for row_id, (label, source) in source_rows.items():
        sha256 = source["sha256"]
        body = read_verified_body(
            html_root / sha256[:2] / f"{sha256}.html", sha256, source["bytes"]
        )
        lines = ibm_docs.plain_text(body)
        require(bool(lines), f"{row_id} retained body has no readable text")
        heading_lines = []
        for line in lines:
            if line.startswith("Last Updated:"):
                break
            heading_lines.append(line)
        retained_heading = " ".join(" ".join(line.split()) for line in heading_lines)
        require(retained_heading == " ".join(label.split()), f"{row_id} heading differs in retained HTML")
        parsed[row_id] = lines

    evidence_count = 0
    for operation in catalog["operations"]:
        for route in operation["routes"]:
            expected = f"{route['method']} {route['source_uri']}"
            for evidence in route["source_evidence"]:
                lines = parsed[evidence["row_id"]]
                line_number = evidence["plain_text_line"]
                require(line_number <= len(lines), f"{operation['id']} source line is out of range")
                actual = lines[line_number - 1].removesuffix(" |").strip()
                require(actual == expected, f"{operation['id']} route differs at {evidence['row_id']}:{line_number}: {actual!r}")
                evidence_count += 1

    return {"verified_bodies": len(parsed), "verified_route_evidence": evidence_count, "verified_tocs": 1}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive-root", type=Path, help="retained ibm-docs-archive root")
    args = parser.parse_args(argv)
    counts = validate_structure()
    if args.archive_root is not None:
        counts.update(validate_sources(args.archive_root.resolve()))
    counts["catalog_sha256"] = digest(CATALOG)
    print(json.dumps(counts, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (CatalogError, OSError) as error:
        raise SystemExit(f"z/OSMF source verification: {error}")
