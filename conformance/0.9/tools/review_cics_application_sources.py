#!/usr/bin/env python3
"""Generate and enforce an automatic CICS application-source batch review.

The review is derived from a separate HTML verifier. Clear evidence is
accepted automatically. Missing source, source reprojection, or a
byte/structure mismatch blocks the ordinary check; bounded product ambiguity
remains explicit and no manual approval or model attestation can override a
real failure.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_cics_application_sources as independent  # noqa: E402
from cics_application_source_batches import SourceBatch, source_batch  # noqa: E402


DEFAULT_BATCH = source_batch("a")
# Backwards-compatible alias for focused sources-a tests and callers.
REVIEW_PATH = DEFAULT_BATCH.review_path
SCHEMA_PATH = Path("conformance/0.9/schemas/cics-source-review.schema.json")
CHECKER_PATH = Path("conformance/0.9/tools/review_cics_application_sources.py")
VERIFIER_PATH = Path("conformance/0.9/tools/verify_cics_application_sources.py")

SCHEMA_VERSION = "mainframe-env.cics-source-review@2"
TARGET_VERSION = "0.9.0"
WORK_PACKAGE = DEFAULT_BATCH.review_work_package
CHECKER_VERSION = "cics-source-auto-review-checker@2"
REVIEW_DOMAIN = b"mainframe-env.cics-source-review@2\0"
CATEGORIES = independent.CATEGORIES
BLOCKING_CATEGORIES = frozenset(
    {"requires-reprojection", "mismatch"}
)
FORBIDDEN_AUTHORITY_KEYS = frozenset(
    {
        "approval",
        "approved_by",
        "human_maintainer",
        "reviewed_on",
        "reviewer_id",
        "rationale",
        "winning_candidate_ids",
    }
)


class ReviewError(ValueError):
    """The automatic review is stale, malformed, or blocked by source evidence."""


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def pretty(value: object) -> str:
    return json.dumps(value, indent=2, ensure_ascii=True, sort_keys=True) + "\n"


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def review_digest(value: dict[str, Any]) -> str:
    material = dict(value)
    material.pop("review_sha256", None)
    return "sha256:" + hashlib.sha256(REVIEW_DOMAIN + canonical_bytes(material)).hexdigest()


def read_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ReviewError(f"cannot read review receipt {path}: {error}") from error
    if not isinstance(value, dict):
        raise ReviewError(f"review receipt is not an object: {path}")
    return value


def _category_total(categories: dict[str, Any]) -> int:
    return sum(int(categories[name]["count"]) for name in CATEGORIES)


def _blocking_count(report: dict[str, Any]) -> int:
    candidate_categories = report["candidate_categories"]
    issue_categories = report["issue_categories"]
    return sum(
        int(candidate_categories[name]["count"]) + int(issue_categories[name]["count"])
        for name in BLOCKING_CATEGORIES
    ) + int(report["structural_coverage"]["missing"]) + int(
        report["structural_coverage"]["extra"]
    ) + int(report["applicability_coverage"]["missing"]) + int(
        report["applicability_coverage"]["extra"]
    )


def _assert_verifier_report(report: dict[str, Any]) -> None:
    required = {
        "schema_version",
        "target_version",
        "status",
        "semantic_authority",
        "execution_authority",
        "automatic_registration",
        "coverage_credit",
        "semantic_credit",
        "differential_credit",
        "verifier",
        "inputs",
        "counts",
        "structural_coverage",
        "applicability_coverage",
        "candidate_categories",
        "issue_categories",
        "ambiguity_scope",
        "findings",
        "report_sha256",
    }
    if set(report) != required:
        raise ReviewError("independent verifier report fields differ")
    if report["schema_version"] != independent.REPORT_SCHEMA:
        raise ReviewError("independent verifier schema differs")
    if report["target_version"] != TARGET_VERSION:
        raise ReviewError("independent verifier target differs")
    for field in (
        "semantic_authority",
        "execution_authority",
        "automatic_registration",
    ):
        if report[field] is not False:
            raise ReviewError(f"independent verifier improperly grants {field}")
    for field in ("coverage_credit", "semantic_credit", "differential_credit"):
        if report[field] != 0:
            raise ReviewError(f"independent verifier improperly grants {field}")
    if report.get("report_sha256") != independent.report_digest(report):
        raise ReviewError("independent verifier report digest differs")
    if set(report["candidate_categories"]) != set(CATEGORIES):
        raise ReviewError("candidate verification categories differ")
    if set(report["issue_categories"]) != set(CATEGORIES):
        raise ReviewError("issue verification categories differ")
    if _category_total(report["candidate_categories"]) != report["counts"]["candidates"]:
        raise ReviewError("candidate verification categories are not exhaustive")
    if _category_total(report["issue_categories"]) != report["counts"]["issues"]:
        raise ReviewError("issue verification categories are not exhaustive")


def compact_category(value: dict[str, Any], ids_key: str) -> dict[str, Any]:
    return {
        "count": value["count"],
        f"{ids_key}_sha256": value[f"{ids_key}_sha256"],
    }


def build_receipt(
    root: Path = ROOT,
    cache: Path | None = None,
    *,
    report: dict[str, Any] | None = None,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    source_report = report or independent.verify(root, cache, batch=config)
    _assert_verifier_report(source_report)
    blockers = _blocking_count(source_report)
    ambiguities = (
        source_report["candidate_categories"]["product-ambiguity"]["count"]
        + source_report["issue_categories"]["product-ambiguity"]["count"]
    )
    accepted = blockers == 0
    receipt: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "work_package": config.review_work_package,
        "review_status": (
            "auto-accepted-with-bounded-ambiguities"
            if accepted and ambiguities
            else "auto-accepted"
            if accepted
            else "blocked"
        ),
        "review_authority": {
            "kind": "independent-deterministic-verifier",
            "automatic_acceptance": True,
            "manual_approval": False,
            "source_derived_before_projection": source_report["verifier"][
                "projection_loaded_after_source_derivation"
            ],
        },
        "semantic_authority": False,
        "execution_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "semantic_credit": 0,
        "differential_credit": 0,
        "inputs": source_report["inputs"],
        "review_contract": {
            "schema": {
                "path": SCHEMA_PATH.as_posix(),
                "file_sha256": file_sha256(root / SCHEMA_PATH),
            },
            "checker": {
                "path": CHECKER_PATH.as_posix(),
                "version": CHECKER_VERSION,
                "file_sha256": file_sha256(root / CHECKER_PATH),
            },
            "independent_verifier": {
                "path": VERIFIER_PATH.as_posix(),
                "version": source_report["verifier"]["version"],
                "file_sha256": file_sha256(root / VERIFIER_PATH),
            },
        },
        "independent_verification_sha256": source_report["report_sha256"],
        "counts": {
            **source_report["counts"],
            "blocking_findings": blockers,
        },
        "structural_coverage": source_report["structural_coverage"],
        "applicability_coverage": source_report["applicability_coverage"],
        "candidate_dispositions": {
            category: compact_category(source_report["candidate_categories"][category], "candidate_ids")
            for category in CATEGORIES
        },
        "issue_dispositions": {
            category: compact_category(source_report["issue_categories"][category], "issue_ids")
            for category in CATEGORIES
        },
        "ambiguity_scope": source_report["ambiguity_scope"],
        "blocker_ids": sorted(
            {
                str(finding["issue_id"])
                for finding in source_report["findings"]
                if finding.get("issue_id")
            }
        ),
    }
    receipt["review_sha256"] = review_digest(receipt)
    return receipt


def _walk_keys(value: object) -> Iterable[str]:
    if isinstance(value, dict):
        for key, child in value.items():
            yield key
            yield from _walk_keys(child)
    elif isinstance(value, list):
        for child in value:
            yield from _walk_keys(child)


def validate_receipt(
    receipt: dict[str, Any],
    expected: dict[str, Any],
    *,
    require_accepted: bool = True,
) -> dict[str, Any]:
    if any(key in FORBIDDEN_AUTHORITY_KEYS for key in _walk_keys(receipt)):
        raise ReviewError("manual or model approval fields are forbidden")
    if receipt.get("review_sha256") != review_digest(receipt):
        raise ReviewError("review receipt digest differs")
    if receipt != expected:
        raise ReviewError("review receipt is stale relative to independent verification")
    for field in (
        "semantic_authority",
        "execution_authority",
        "automatic_registration",
    ):
        if receipt.get(field) is not False:
            raise ReviewError(f"review improperly grants {field}")
    for field in ("coverage_credit", "semantic_credit", "differential_credit"):
        if receipt.get(field) != 0:
            raise ReviewError(f"review improperly grants {field}")
    if require_accepted and receipt.get("review_status") not in {
        "auto-accepted",
        "auto-accepted-with-bounded-ambiguities",
    }:
        counts = receipt.get("counts", {})
        raise ReviewError(
            "automatic source review is blocked: "
            f"blocking_findings={counts.get('blocking_findings', 'unknown')}"
        )
    return receipt


def check_committed(
    root: Path = ROOT,
    cache: Path | None = None,
    *,
    require_accepted: bool = True,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    expected = build_receipt(root, cache, batch=config)
    path = root / config.review_path
    receipt = read_object(path)
    validate_receipt(receipt, expected, require_accepted=require_accepted)
    if path.read_text(encoding="utf-8") != pretty(receipt):
        raise ReviewError("review receipt is not canonical JSON")
    return receipt


def write_receipt(
    root: Path = ROOT,
    cache: Path | None = None,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    receipt = build_receipt(root, cache, batch=config)
    output = root / config.review_path
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(pretty(receipt), encoding="utf-8")
    return receipt


def default_cache() -> Path:
    return Path(os.environ.get("MAINFRAME_ENV_IBM_DOCS_CACHE", "/ibm-docs/topic-cache"))


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--generate", action="store_true")
    mode.add_argument("--check", action="store_true")
    parser.add_argument("--batch", choices=("a", "b", "c"), default="a")
    parser.add_argument("--cache", type=Path, default=default_cache())
    args = parser.parse_args(list(argv) if argv is not None else None)
    try:
        if args.generate:
            receipt = write_receipt(ROOT, args.cache, args.batch)
            print(
                f"cics-sources-{args.batch}-auto-review: generated "
                f"status={receipt['review_status']} "
                f"candidates={receipt['counts']['candidates']} "
                f"blockers={receipt['counts']['blocking_findings']}"
            )
            return 0
        receipt = check_committed(
            ROOT,
            args.cache,
            require_accepted=True,
            batch=args.batch,
        )
        print(
            f"cics-sources-{args.batch}-auto-review: accepted "
            f"rows={receipt['counts']['rows']} "
            f"candidates={receipt['counts']['candidates']}"
        )
        return 0
    except (
        OSError,
        ReviewError,
        independent.VerificationError,
        ValueError,
        KeyError,
        TypeError,
    ) as error:
        parser.exit(1, f"cics-sources-{args.batch}-auto-review: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
