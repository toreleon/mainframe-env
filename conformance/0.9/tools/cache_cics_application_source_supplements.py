#!/usr/bin/env python3
"""Import and verify the zero-credit CIC-901 direct HTML supplements.

The receipt intentionally makes no table-of-contents claim.  It pins three
browser-observed IBM content bodies from publications other than the target
CICS TS book and keeps the resulting evidence behind explicit authority
boundaries.  This tool never fetches publication content or grants semantic,
execution, registration, coverage, or differential credit.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
from typing import Any, BinaryIO, Iterable
import urllib.parse


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "conformance/tools"))
import docs_api  # noqa: E402
import ibm_docs  # noqa: E402


RECEIPT_PATH = Path(
    "conformance/0.9/cics/application-api-sources-a-supplements.json"
)
SCHEMA_PATH = Path("conformance/0.9/schemas/cics-source-supplements.schema.json")
CHECKER_PATH = Path(
    "conformance/0.9/tools/cache_cics_application_source_supplements.py"
)
SCHEMA_VERSION = "mainframe-env.cics-source-supplements@1"
CHECKER_VERSION = "cics-source-supplements-cache@1"
TARGET_VERSION = "0.9.0"
WORK_PACKAGE = "CIC-901.sources-a-corpus"
RECEIPT_DOMAIN = b"mainframe-env.cics-source-supplements@1\0"
IDENTITY_DOMAIN = b"mainframe-env.cics-source-supplement-identity@1\0"
RECEIPT_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-supplements@1, one "
    "NUL byte (0x00), then UTF-8 JSON with ensure_ascii=true, sort_keys=true "
    "and separators=(',', ':') of the complete receipt with "
    "supplements_sha256 omitted"
)
IDENTITY_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-supplement-identity@1, "
    "one NUL byte (0x00), then one UTF-8 '<topic_path> <bytes> <sha256>\\n' "
    "line per observed topic sorted by topic_path"
)
SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
BARE_SHA256 = re.compile(r"^[0-9a-f]{64}$")

ROW_CICSMESSAGE = "ibm-cics-ts-6x-2026-08-31:api-commands:0027"
ROW_DUMP = "ibm-cics-ts-6x-2026-08-31:api-commands:0056"
ROW_TRACEID = "ibm-cics-ts-6x-2026-08-31:api-commands:0065"

TARGET = {
    "catalog_baseline": "ibm-cics-ts-6x-2026-08-31",
    "product_key": "SSJL4D_6.x",
    "target_product": "CICS Transaction Server for z/OS 6.x",
}

EQUIVALENCE_CONTROLS = [
    {
        "topic_path": (
            "SSJL4D_6.x/reference-applications/commands-api/"
            "dfhp4_dumptransaction.html"
        ),
        "expected_bytes": 56995,
        "expected_sha256": (
            "165e1964974c535a90fc4d18084012225b4ead00f55b214be59a3ee5aabc12d5"
        ),
        "observed_bytes": 56995,
        "observed_sha256": (
            "165e1964974c535a90fc4d18084012225b4ead00f55b214be59a3ee5aabc12d5"
        ),
        "result": "exact",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-diagnostics/modules/dfhs3c001248.html",
        "expected_bytes": 1847,
        "expected_sha256": (
            "1067e330e46667f52c4ae07002e73d1b13b35f0db8f1a69fbd3809eab5f2d109"
        ),
        "observed_bytes": 1847,
        "observed_sha256": (
            "1067e330e46667f52c4ae07002e73d1b13b35f0db8f1a69fbd3809eab5f2d109"
        ),
        "result": "exact",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
        "expected_bytes": 172664,
        "expected_sha256": (
            "dcd764843508284b5fe4aa5dc7ae0fa47989be8c0b1f1049cb089174d21babea"
        ),
        "observed_bytes": 172664,
        "observed_sha256": (
            "dcd764843508284b5fe4aa5dc7ae0fa47989be8c0b1f1049cb089174d21babea"
        ),
        "result": "exact",
    },
]

EXPECTED_TOPICS = [
    {
        "topic_path": "SSNAQ8_11.1.0/reference-api/r_dump.html",
        "public_url": "https://www.ibm.com/docs/en/cics-tx/11.1.0?topic=commands-dump",
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSNAQ8_11.1.0/"
            "reference-api/r_dump.html?parsebody=true&lang=en"
        ),
        "source_product": "CICS TX 11.1",
        "product_key": "SSNAQ8_11.1.0",
        "heading": "DUMP",
        "last_modified": "2022-04-06",
        "bytes": 44235,
        "sha256": "0234352759c3d24a0e56db00eaa8a0ad509e4e936889dc2af7ebf1cc6bdd14ee",
        "source_role": "cross-product-explicit-compatibility",
        "target_authority_boundary": "cross-product-not-target-authority",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_DUMP],
        "allowed_evidence_dimensions": ["syntax", "options"],
        "limitations": [
            "cross-product-not-target-authority",
            "compatibility-statement-limited-to-dump-option-table-names",
            "condition-section-absent-does-not-prove-target-absence",
        ],
    },
    {
        "topic_path": "SSNAQ8_11.1.0/reference-api/r_enter.html",
        "public_url": "https://www.ibm.com/docs/en/cics-tx/11.1.0?topic=commands-enter",
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSNAQ8_11.1.0/"
            "reference-api/r_enter.html?parsebody=true&lang=en"
        ),
        "source_product": "CICS TX 11.1",
        "product_key": "SSNAQ8_11.1.0",
        "heading": "ENTER",
        "last_modified": "2022-04-06",
        "bytes": 22142,
        "sha256": "60856d9bcd4062d58b54f80080d40b5f3210fcb8d6945b23c9873a70fa6af5dd",
        "source_role": "cross-product-explicit-compatibility",
        "target_authority_boundary": "cross-product-not-target-authority",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_TRACEID],
        "allowed_evidence_dimensions": [
            "syntax",
            "options",
            "operand-directions",
            "conditions",
        ],
        "limitations": [
            "cross-product-not-target-authority",
            "explicit-target-compatibility-limited-to-account-option",
            "target-command-equivalence-not-established",
        ],
    },
    {
        "topic_path": (
            "SSXJAJ_14.1.0/com.ibm.faultanalyzer.doc_14.1/idiug166.html"
        ),
        "public_url": (
            "https://www.ibm.com/docs/en/fafz/14.1.0?"
            "topic=cics-ivp-exec-dump-dumpcodefad1"
        ),
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSXJAJ_14.1.0/"
            "com.ibm.faultanalyzer.doc_14.1/idiug166.html?parsebody=true&lang=en"
        ),
        "source_product": "Fault Analyzer for z/OS 14.1",
        "product_key": "SSXJAJ_14.1.0",
        "heading": "CICS IVP: EXEC CICS DUMP DUMPCODE(FAD1)",
        "last_modified": "2021-05-26",
        "bytes": 1633,
        "sha256": "1dec488da7449f3254cef9181be320d4fcebb2d4e9766cfef3a706d5150a100c",
        "source_role": "target-platform-interface-example",
        "target_authority_boundary": "target-platform-example-only",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_DUMP],
        "allowed_evidence_dimensions": ["syntax", "execution-context"],
        "limitations": [
            "product-adjacent-example-only",
            "exact-invocation-subset-only",
            "no-command-section-closure",
        ],
    },
    {
        "topic_path": (
            "SSGMCP_5.5.0/reference/commands-api/dfhp4_entertracenum.html"
        ),
        "public_url": (
            "https://www.ibm.com/docs/en/SSGMCP_5.5.0/reference/commands-api/"
            "dfhp4_entertracenum.html"
        ),
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSGMCP_5.5.0/reference/"
            "commands-api/dfhp4_entertracenum.html?parsebody=true&lang=en"
        ),
        "source_product": "CICS Transaction Server for z/OS 5.5",
        "product_key": "SSGMCP_5.5.0",
        "heading": "ENTER TRACENUM",
        "last_modified": "2025-01-07",
        "bytes": 19409,
        "sha256": "549e208c306f3b0f5d21038d0e65fc7c1eec2322263023bd6359a63f08fdb712",
        "source_role": "target-product-version-compatibility",
        "target_authority_boundary": "target-product-older-version-compatibility",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_TRACEID],
        "allowed_evidence_dimensions": ["execution-context"],
        "limitations": [
            "older-version-not-current-target-authority",
            "compatibility-statement-only",
            "not-syntax-options-or-conditions-alias",
        ],
    },
    {
        "topic_path": (
            "SSGMCP_5.5.0/reference/resources/transaction/dfha4_attributes.html"
        ),
        "public_url": (
            "https://www.ibm.com/docs/en/SSGMCP_5.5.0/reference/resources/"
            "transaction/dfha4_attributes.html"
        ),
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSGMCP_5.5.0/reference/"
            "resources/transaction/dfha4_attributes.html?parsebody=true&lang=en"
        ),
        "source_product": "CICS Transaction Server for z/OS 5.5",
        "product_key": "SSGMCP_5.5.0",
        "heading": "TRANSACTION attributes",
        "last_modified": "2025-01-07",
        "bytes": 200069,
        "sha256": "90a4db1fb717a6f400b470219f4149b96c73680ade80f2a962f8972f8ca3c6dd",
        "source_role": "target-product-version-compatibility",
        "target_authority_boundary": "target-product-older-version-context",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_DUMP],
        "allowed_evidence_dimensions": ["execution-context"],
        "limitations": [
            "older-version-not-current-target-authority",
            "applicability-statement-only",
            "no-syntax-or-options-closure",
        ],
    },
]

EXPECTED_RESOLUTIONS = [
    {
        "official_row": ROW_CICSMESSAGE,
        "label": "CICSMESSAGE",
        "eibfn": "6C12",
        "state": "target-internal-only",
        "target_topic_sources": [
            "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
            "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
        ],
        "supplemental_topic_sources": [],
    },
    {
        "official_row": ROW_DUMP,
        "label": "DUMP",
        "eibfn": "1C02",
        "state": "triangulated-compatibility-evidence",
        "target_topic_sources": [
            "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
            "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
        ],
        "supplemental_topic_sources": [
            "SSNAQ8_11.1.0/reference-api/r_dump.html",
            "SSXJAJ_14.1.0/com.ibm.faultanalyzer.doc_14.1/idiug166.html",
            "SSGMCP_5.5.0/reference/resources/transaction/dfha4_attributes.html",
        ],
    },
    {
        "official_row": ROW_TRACEID,
        "label": "ENTER TRACEID",
        "eibfn": "1A04",
        "state": "triangulated-compatibility-evidence",
        "target_topic_sources": [
            "SSJL4D_6.x/reference-applications/commands-api/dfhp4_entertracenum.html",
            "SSJL4D_6.x/reference-applications/commands-api/dfhp4_monitor.html",
            "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
            "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
            "SSJL4D_6.x/reference-diagnostics/modules/dfhs3c001248.html",
        ],
        "supplemental_topic_sources": [
            "SSNAQ8_11.1.0/reference-api/r_enter.html",
            "SSGMCP_5.5.0/reference/commands-api/dfhp4_entertracenum.html",
        ],
    },
]

class SupplementError(ValueError):
    """The direct supplement receipt or cache does not match its contract."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise SupplementError(message)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SupplementError(f"invalid JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise SupplementError(f"expected a JSON object: {path}")
    return value


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=True, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def receipt_digest(receipt: dict[str, Any]) -> str:
    material = dict(receipt)
    material.pop("supplements_sha256", None)
    return "sha256:" + hashlib.sha256(
        RECEIPT_DOMAIN + canonical_bytes(material)
    ).hexdigest()


def identity_digest(topics: Iterable[dict[str, Any]]) -> str:
    lines = sorted(
        f"{topic['topic_path']} {topic['bytes']} {topic['sha256']}\n"
        for topic in topics
    )
    return "sha256:" + hashlib.sha256(
        IDENTITY_DOMAIN + "".join(lines).encode("utf-8")
    ).hexdigest()


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def _exact_keys(value: object, keys: set[str], owner: str) -> dict[str, Any]:
    _require(isinstance(value, dict), f"{owner} must be an object")
    actual = set(value)
    _require(
        actual == keys,
        f"{owner} fields differ: missing={sorted(keys - actual)} "
        f"unexpected={sorted(actual - keys)}",
    )
    return value


def _validate_topic(topic: object, expected: dict[str, Any], index: int) -> None:
    owner = f"topics[{index}]"
    value = _exact_keys(
        topic,
        set(expected) | {"cache_key"},
        owner,
    )
    for key, wanted in expected.items():
        _require(value[key] == wanted, f"{owner}.{key} differs")
    _require(
        isinstance(value["bytes"], int) and not isinstance(value["bytes"], bool),
        f"{owner}.bytes must be an integer",
    )
    _require(BARE_SHA256.fullmatch(value["sha256"]) is not None, f"{owner}.sha256 is invalid")
    path = value["topic_path"]
    product_key = value["product_key"]
    _require(
        ibm_docs.safe_topic_path(path, product_key, owner) == path,
        f"{owner}.topic_path is unsafe",
    )
    parsed = urllib.parse.urlsplit(value["content_url"])
    _require(
        parsed.scheme == "https"
        and parsed.netloc == "www.ibm.com"
        and parsed.path == f"/docs/api/v1/content/{path}"
        and urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
        == [("parsebody", "true"), ("lang", "en")]
        and not parsed.fragment,
        f"{owner}.content_url is not the exact IBM content locator",
    )
    pin = ibm_docs.Pin(path, value["content_url"], value["sha256"], value["bytes"], ())
    _require(value["cache_key"] == pin.key, f"{owner}.cache_key differs")


def validate_receipt(receipt: dict[str, Any], root: Path = ROOT) -> dict[str, Any]:
    top = _exact_keys(
        receipt,
        {
            "schema_version",
            "target_version",
            "work_package",
            "source_kind",
            "toc_claimed",
            "target",
            "semantic_authority",
            "execution_authority",
            "automatic_registration",
            "semantic_credit",
            "execution_credit",
            "registration_credit",
            "coverage_credit",
            "differential_credit",
            "capture",
            "receipt_contract",
            "topics",
            "source_resolutions",
            "supplement_digest_definition",
            "supplements_sha256",
        },
        "receipt",
    )
    constants = {
        "schema_version": SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "work_package": WORK_PACKAGE,
        "source_kind": "browser-verified-direct-content",
        "toc_claimed": False,
        "target": TARGET,
        "semantic_authority": False,
        "execution_authority": False,
        "automatic_registration": False,
        "semantic_credit": 0,
        "execution_credit": 0,
        "registration_credit": 0,
        "coverage_credit": 0,
        "differential_credit": 0,
        "source_resolutions": EXPECTED_RESOLUTIONS,
        "supplement_digest_definition": RECEIPT_DIGEST_DEFINITION,
    }
    for key, expected in constants.items():
        _require(top[key] == expected, f"receipt.{key} differs")

    capture = _exact_keys(
        top["capture"],
        {
            "observed_on",
            "browser",
            "representation",
            "source_origin",
            "raw_publication_retained_in_repository",
            "topics_requested",
            "topics_loaded",
            "reproduction_count",
            "all_reproductions_identical",
            "identity_digest_definition",
            "identity_sha256",
            "equivalence_controls",
        },
        "capture",
    )
    capture_constants = {
        "observed_on": "2026-09-10",
        "browser": "user-chrome-browser-control",
        "representation": "document.body.innerHTML encoded as UTF-8",
        "source_origin": "https://www.ibm.com/docs/",
        "raw_publication_retained_in_repository": False,
        "topics_requested": 5,
        "topics_loaded": 5,
        "reproduction_count": 2,
        "all_reproductions_identical": True,
        "identity_digest_definition": IDENTITY_DIGEST_DEFINITION,
        "equivalence_controls": EQUIVALENCE_CONTROLS,
    }
    for key, expected in capture_constants.items():
        _require(capture[key] == expected, f"capture.{key} differs")

    contract = _exact_keys(top["receipt_contract"], {"schema", "checker"}, "receipt_contract")
    schema = _exact_keys(contract["schema"], {"path", "file_sha256"}, "receipt_contract.schema")
    checker = _exact_keys(
        contract["checker"], {"path", "version", "file_sha256"}, "receipt_contract.checker"
    )
    _require(schema["path"] == SCHEMA_PATH.as_posix(), "receipt schema path differs")
    _require(checker["path"] == CHECKER_PATH.as_posix(), "receipt checker path differs")
    _require(checker["version"] == CHECKER_VERSION, "receipt checker version differs")
    _require(
        schema["file_sha256"] == file_sha256(root / SCHEMA_PATH),
        "receipt schema binding is stale",
    )
    _require(
        checker["file_sha256"] == file_sha256(root / CHECKER_PATH),
        "receipt checker binding is stale",
    )

    topics = top["topics"]
    _require(isinstance(topics, list), "receipt.topics must be an array")
    _require(len(topics) == len(EXPECTED_TOPICS), "receipt topic count differs")
    for index, (topic, expected) in enumerate(zip(topics, EXPECTED_TOPICS)):
        _validate_topic(topic, expected, index)
    paths = [topic["topic_path"] for topic in topics]
    _require(len(paths) == len(set(paths)), "duplicate supplement topic")
    _require(
        capture["identity_sha256"] == identity_digest(topics),
        "capture identity digest is stale",
    )
    _require(
        isinstance(top["supplements_sha256"], str)
        and SHA256.fullmatch(top["supplements_sha256"]) is not None,
        "receipt supplement digest is malformed",
    )
    _require(
        top["supplements_sha256"] == receipt_digest(top),
        "receipt supplement digest is stale",
    )
    return top


def load_receipt(root: Path = ROOT) -> dict[str, Any]:
    return validate_receipt(read_json(root / RECEIPT_PATH), root)


def pins_from_receipt(receipt: dict[str, Any]) -> list[ibm_docs.Pin]:
    return [
        ibm_docs.Pin(
            topic["topic_path"],
            topic["content_url"],
            topic["sha256"],
            topic["bytes"],
            (),
        )
        for topic in receipt["topics"]
    ]


def check(root: Path, cache: Path) -> dict[str, Any]:
    """Validate the receipt and all exact bodies in an external cache."""
    receipt = load_receipt(root)
    directory = docs_api.outside_repository(cache)
    for topic, pin in zip(receipt["topics"], pins_from_receipt(receipt)):
        try:
            body = ibm_docs.cached_body(directory, pin)
        except (FileNotFoundError, ValueError, OSError) as error:
            raise SupplementError(
                f"supplement cache does not verify {pin.topic}: {error}"
            ) from error
        try:
            text = body.decode("utf-8")
        except UnicodeDecodeError as error:
            raise SupplementError(f"supplement is not UTF-8: {pin.topic}") from error
        _require(
            docs_api.heading_of(text) == topic["heading"],
            f"supplement heading differs: {pin.topic}",
        )
        _require(
            docs_api.last_modified_of(text) == topic["last_modified"],
            f"supplement last-modified date differs: {pin.topic}",
        )
    return receipt


def import_receipt(root: Path, cache: Path, stream: BinaryIO) -> dict[str, Any]:
    """Import only receipt-pinned bodies from a streaming tar archive."""
    receipt = load_receipt(root)
    directory = docs_api.outside_repository(cache)
    counts = ibm_docs.import_cache(stream, directory, pins_from_receipt(receipt), [])
    failures = sum(
        counts[key]
        for key in (
            "rejected_mismatch",
            "rejected_conflict",
            "missing_expected",
            "mismatch_expected",
        )
    )
    _require(failures == 0, f"supplement import failed: {dict(counts)}")
    check(root, directory)
    return {"cache": str(directory), "topics": len(receipt["topics"]), **counts}


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--import", dest="import_mode", action="store_true")
    mode.add_argument("--check", action="store_true")
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    args = parser.parse_args(list(argv) if argv is not None else None)
    try:
        cache = args.cache or docs_api.default_cache()
        if args.import_mode:
            result = import_receipt(ROOT, cache, sys.stdin.buffer)
            print(json.dumps(result, sort_keys=True))
        else:
            receipt = check(ROOT, cache)
            print(
                "cics-source-supplements: verified "
                f"topics={len(receipt['topics'])} toc_claimed=false "
                f"cache={docs_api.outside_repository(cache)}"
            )
        return 0
    except (
        AttributeError,
        FileNotFoundError,
        KeyError,
        OSError,
        SupplementError,
        TypeError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        parser.exit(1, f"cics-source-supplements: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
