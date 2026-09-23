#!/usr/bin/env python3
"""Fetch and freeze the zero-credit CIC-901 sources-a IBM HTML corpus.

Raw IBM HTML and navigation JSON stay in an external cache. The repository
receives only an ordinary topic manifest plus a bounded source-role projection.
Automated selection is a review candidate and grants no semantic or
licensed-execution credit.
"""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
from typing import Any, BinaryIO, Iterable
import urllib.parse


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "conformance/0.9/tools"))
sys.path.insert(0, str(ROOT / "conformance/tools"))
sys.path.insert(0, str(ROOT / "tools"))
import docs_api  # noqa: E402
from browser_fetch import establish, fetch_binary, open_tab  # noqa: E402
import cics_application_source_batches as source_batches  # noqa: E402
import generate_cics_source_map as source_map  # noqa: E402
import ibm_docs  # noqa: E402


MAP_PATH = Path("conformance/0.9/cics/application-api-sources-a-map.json")
CORPUS_PATH = Path("conformance/0.9/cics/application-api-sources-a-corpus.json")
BROWSER_RECEIPT_PATH = Path(
    "conformance/0.9/cics/application-api-sources-a-browser-verification.json"
)
MANIFEST_PATH = Path(
    "conformance/0.9/manifests/cics-application-api-sources-a-topics.json"
)
REGISTRY_PATH = Path("conformance/0.9/manifests/index.json")
SCHEMA_VERSION = "mainframe-env.cics-source-corpus@1"
TARGET_VERSION = "0.9.0"
WORK_PACKAGE = "CIC-901.sources-a-corpus"
SCOPE_ID = "cics-application-api-sources-a"
BASELINE_ID = "ibm-cics-ts-6x-application-api-sources-a-2026-09-10"
PRODUCT = "SSJL4D_6.x"
SNAPSHOT_DATE = "2026-09-10"
TOC_URL = "https://www.ibm.com/docs/api/v1/toc/cics-ts/6.x?lang=en"
TOC_SHA256 = "f65c51e52facc390c05f084e1d249ff19e68bf2d7f8d3f32d4d745faf622681a"
MAP_SHA256 = "sha256:c535f2cac1072e16a0f3e252045cac975934c5e24d405a560a4e0ad14e0de726"
SUMMARY_PATH = source_map.SUMMARY_PATH
CORPUS_DOMAIN = b"mainframe-env.cics-source-corpus@1\0"
BROWSER_RECEIPT_DOMAIN = b"mainframe-env.cics-browser-source-verification@1\0"
CORPUS_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-corpus@1, one NUL "
    "byte (0x00), then UTF-8 JSON with ensure_ascii=true, sort_keys=true and "
    "separators=(',', ':') of mapping, topic_manifest, counts, mapped topics, "
    "linked topics, manual topics, excluded navigation, and source gaps"
)
MAX_TOPIC_BYTES = 2 * 1024 * 1024
MAX_HTML_TOPICS = 256
EXPECTED_MAPPED_TOPICS = 109
EXPECTED_LINKED_TOPICS = 58
EXPECTED_MANUAL_TOPICS = 6
EXPECTED_HTML_TOPICS = 173
HEX = frozenset("0123456789abcdef")
MANIFEST_FIELDS = {
    "baseline_id",
    "book_href",
    "book_label",
    "content_url_template",
    "coverage_credit",
    "product",
    "retained_in_repository",
    "schema_version",
    "snapshot_date",
    "subsystem",
    "target_version",
    "toc_sha256",
    "toc_url",
    "topic_count",
    "topic_manifest_digest",
    "topic_manifest_digest_definition",
    "topics",
    "total_bytes",
}
BROWSER_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-browser-source-verification@1, "
    "one NUL byte (0x00), then one UTF-8 '<topic_path> <bytes> <sha256>\\n' "
    "line per observed topic sorted by topic_path"
)

ROW_CICSMESSAGE = "ibm-cics-ts-6x-2026-08-31:api-commands:0027"
ROW_DUMP = "ibm-cics-ts-6x-2026-08-31:api-commands:0056"
ROW_TRACEID = "ibm-cics-ts-6x-2026-08-31:api-commands:0065"

MANUAL_TOPICS = [
    {
        "topic_path": "SSJL4D_6.x/applications/designing/dfhp3c00237.html",
        "role": "execution-context-candidate",
        "applies_to": "all-sources-a-rows",
        "reason": "dpl-command-restrictions",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-applications/commands-api/dfhp4_monitor.html",
        "role": "compatibility-context-candidate",
        "applies_to_rows": [ROW_TRACEID],
        "reason": "legacy-monitoring-successor",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-diagnostics/modules/dfhs3c001248.html",
        "role": "compatibility-context-candidate",
        "applies_to_rows": [ROW_TRACEID],
        "reason": "legacy-monitoring-module-context",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-applications/commands-api/dfhp4_threadsafelist.html",
        "role": "execution-context-candidate",
        "applies_to": "all-sources-a-rows",
        "reason": "threadsafe-command-list",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
        "role": "gap-classification-candidate",
        "applies_to_rows": [ROW_CICSMESSAGE, ROW_DUMP, ROW_TRACEID],
        "reason": "exec-interface-identity-and-availability",
    },
    {
        "topic_path": "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
        "role": "gap-identity-candidate",
        "applies_to_rows": [ROW_CICSMESSAGE, ROW_DUMP, ROW_TRACEID],
        "reason": "pinned-eibfn-command-table",
    },
]


# Sources B/C use the same cache and corpus machinery as sources A.  The
# later batches deliberately keep only the two global applicability topics,
# plus exact row-specific topics that live outside the command-summary tree.
COMMON_APPLICABILITY_TOPICS = (
    {
        "topic_path": (
            "SSJL4D_6.x/reference-applications/commands-api/"
            "dfhp4_argumentvalues.html"
        ),
        "role": "operand-contract-candidate",
        "applies_to": "all-batch-rows",
        "reason": "argument-value-direction-rules",
    },
    {
        "topic_path": (
            "SSJL4D_6.x/reference-applications/commands-api/dfhp4_apiformat.html"
        ),
        "role": "execution-context-candidate",
        "applies_to": "all-batch-rows",
        "reason": "exec-command-format-local-task",
    },
    {
        "topic_path": "SSJL4D_6.x/applications/designing/dfhp3c00237.html",
        "role": "execution-context-candidate",
        "applies_to": "all-batch-rows",
        "reason": "dpl-command-restrictions",
    },
    {
        "topic_path": (
            "SSJL4D_6.x/reference-applications/commands-api/"
            "dfhp4_threadsafelist.html"
        ),
        "role": "execution-context-candidate",
        "applies_to": "all-batch-rows",
        "reason": "threadsafe-command-list",
    },
)

ROW_ASSOCIATION = "ibm-cics-ts-6x-2026-08-31:api-commands:0193"
ROW_TRACE = "ibm-cics-ts-6x-2026-08-31:api-commands:0220"
ASSOCIATION_TOPIC = (
    "SSJL4D_6.x/reference-system-programming/commands-spi/"
    "set-association-usercorrdata.html"
)
INTERFACE_TOPIC = "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html"
EIBFN_TOPIC = "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html"
RESPONSE_CODE_TOPIC = "SSJL4D_6.x/reference-diagnostics/eib/dfha8me.html"
TRACE_TOPIC = "SSNAQ8_11.1.0/reference-api/r_set_trace.html"

C_ROW_TOPICS = (
    {
        "topic_path": RESPONSE_CODE_TOPIC,
        "role": "condition-contract-candidate",
        "applies_to": "all-batch-rows",
        "reason": "response-code-table",
    },
    {
        "topic_path": ASSOCIATION_TOPIC,
        "role": "target-command-source",
        "applies_to_rows": [ROW_ASSOCIATION],
        "reason": "target-command-outside-summary-tree",
    },
    {
        "topic_path": INTERFACE_TOPIC,
        "role": "gap-classification-candidate",
        "applies_to_rows": [ROW_TRACE],
        "reason": "exec-interface-identity-and-availability",
    },
    {
        "topic_path": EIBFN_TOPIC,
        "role": "gap-identity-candidate",
        "applies_to_rows": [ROW_TRACE],
        "reason": "pinned-eibfn-command-table",
    },
)

C_SUPPLEMENTAL_TOPICS = (
    {
        "topic_path": TRACE_TOPIC,
        "public_url": (
            "https://www.ibm.com/docs/en/cics-tx/11.1.0?topic=commands-trace"
        ),
        "content_url": (
            "https://www.ibm.com/docs/api/v1/content/SSNAQ8_11.1.0/"
            "reference-api/r_set_trace.html?parsebody=true&lang=en"
        ),
        "source_product": "CICS TX 11.1",
        "product_key": "SSNAQ8_11.1.0",
        "heading": "TRACE",
        "last_modified": "2022-04-06",
        "bytes": 20495,
        "sha256": (
            "28e56eef7556509f9a473ae573f6295a56c170ae9a1dd79041c18b15fafd9c83"
        ),
        "cache_key": (
            "topic-e507e009c2d03078c4c36a92f2626edbabec719ab692aa14e279d09404f97976-"
            "sha256-28e56eef7556509f9a473ae573f6295a56c170ae9a1dd79041c18b15fafd9c83.html"
        ),
        "source_role": "cross-product-command-reference",
        "target_authority_boundary": "cross-product-not-target-authority",
        "target_product_authority": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "applies_to_rows": [ROW_TRACE],
        "allowed_evidence_dimensions": [
            "syntax",
            "options",
            "operand-directions",
            "execution-context",
        ],
        "limitations": [
            "cross-product-not-target-authority",
            "target-command-equivalence-not-established",
            "condition-section-absent-does-not-prove-target-absence",
        ],
    },
)

C_SOURCE_RESOLUTIONS = (
    {
        "official_row": ROW_ASSOCIATION,
        "label": "SET ASSOCIATION USERCORRDATA",
        "eibfn": "C404",
        "state": "target-product-authority",
        "target_topic_sources": [ASSOCIATION_TOPIC],
        "supplemental_topic_sources": [],
    },
    {
        "official_row": ROW_TRACE,
        "label": "TRACE",
        "eibfn": "1A02",
        "state": "cross-product-evidence",
        "target_topic_sources": [INTERFACE_TOPIC, EIBFN_TOPIC],
        "supplemental_topic_sources": [TRACE_TOPIC],
    },
)

LATER_CORPUS_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-corpus@1, one NUL "
    "byte (0x00), then UTF-8 JSON with ensure_ascii=true, sort_keys=true and "
    "separators=(',', ':') of mapping, topic_manifest, counts, mapped topics, "
    "linked topics, manual topics, excluded navigation, supplemental topics, "
    "and source resolutions"
)


class LaterCorpusConfig:
    """The small amount of policy that differs between source batches."""

    def __init__(
        self,
        batch: source_batches.SourceBatch,
        manual_topics: tuple[dict[str, Any], ...],
        supplemental_topics: tuple[dict[str, Any], ...] = (),
        source_resolutions: tuple[dict[str, Any], ...] = (),
    ) -> None:
        self.batch = batch
        self.manual_topics = manual_topics
        self.supplemental_topics = supplemental_topics
        self.source_resolutions = source_resolutions

    @property
    def map_batch_id(self) -> str:
        return f"sources-{self.batch.name}"

    @property
    def work_package(self) -> str:
        return f"CIC-901.sources-{self.batch.name}-corpus"

    @property
    def baseline_id(self) -> str:
        return (
            "ibm-cics-ts-6x-application-api-sources-"
            f"{self.batch.name}-{SNAPSHOT_DATE}"
        )

    @property
    def book_label(self) -> str:
        return (
            "CICS application API sources-"
            f"{self.batch.name} HTML topic corpus"
        )


LATER_BATCHES = {
    "b": LaterCorpusConfig(
        source_batches.source_batch("b"),
        tuple(dict(topic) for topic in COMMON_APPLICABILITY_TOPICS),
    ),
    "c": LaterCorpusConfig(
        source_batches.source_batch("c"),
        tuple(dict(topic) for topic in (*COMMON_APPLICABILITY_TOPICS, *C_ROW_TOPICS)),
        tuple(dict(topic) for topic in C_SUPPLEMENTAL_TOPICS),
        tuple(dict(row) for row in C_SOURCE_RESOLUTIONS),
    ),
}


class CorpusError(ValueError):
    """The source corpus, cache, mapping, or generated projection is invalid."""


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"{path}: {error}") from error
    if not isinstance(value, dict):
        raise CorpusError(f"{path} must contain an object")
    return value


def pretty(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, ensure_ascii=True, separators=(",", ":"), sort_keys=True
    ).encode()
    return f"sha256:{hashlib.sha256(CORPUS_DOMAIN + encoded).hexdigest()}"


def browser_identity_digest(topics: Iterable[dict[str, Any]]) -> str:
    lines = sorted(
        f"{topic['topic_path']} {topic['bytes']} {topic['sha256']}\n"
        for topic in topics
    )
    return "sha256:" + hashlib.sha256(
        BROWSER_RECEIPT_DOMAIN + "".join(lines).encode()
    ).hexdigest()


def expected_browser_receipt(manifest: dict[str, Any]) -> dict[str, Any]:
    manifest_bytes = pretty(manifest).encode()
    count = manifest["topic_count"]
    return {
        "schema_version": "mainframe-env.cics-browser-source-verification@1",
        "target_version": TARGET_VERSION,
        "work_package": WORK_PACKAGE,
        "verified_on": SNAPSHOT_DATE,
        "source_origin": "https://www.ibm.com/docs/",
        "retrieval_method": "user-chrome-direct-content-navigation",
        "response_materialization": (
            "document.body.innerHTML read in 65536-character chunks and UTF-8 encoded"
        ),
        "source_bytes_retained_in_repository": False,
        "semantic_authority": False,
        "coverage_credit": 0,
        "topic_manifest": {
            "path": str(MANIFEST_PATH),
            "file_sha256": "sha256:" + hashlib.sha256(manifest_bytes).hexdigest(),
            "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
            "topic_count": count,
            "total_bytes": manifest["total_bytes"],
        },
        "observation": {
            "topics_requested": count,
            "topics_loaded": count,
            "topic_identity_matches": count,
            "mismatches": [],
            "identity_digest_definition": BROWSER_DIGEST_DEFINITION,
            "identity_sha256": browser_identity_digest(manifest["topics"]),
        },
    }


def canonical_topic_path(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    clean = value.split("#", 1)[0].split("?", 1)[0]
    candidate = PurePosixPath(clean)
    if (
        not clean
        or "\\" in clean
        or candidate.is_absolute()
        or candidate.as_posix() != clean
        or any(part in {"", ".", ".."} for part in candidate.parts)
        or len(candidate.parts) < 2
        or candidate.parts[0] != PRODUCT
        or candidate.suffix not in {".htm", ".html"}
        or any(
            re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", part) is None
            for part in candidate.parts[1:-1]
        )
        or re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]*\.html?", candidate.parts[-1])
        is None
    ):
        return None
    return clean


def linked_topic(href: str, source: str) -> str | None:
    if not href or href.startswith(("#", "javascript:", "mailto:")):
        return None
    parsed = urllib.parse.urlsplit(href)
    if parsed.scheme or parsed.netloc:
        if parsed.scheme != "https" or parsed.netloc != "www.ibm.com":
            return None
        prefix = "/docs/en/"
        if not parsed.path.startswith(prefix):
            return None
        candidate = parsed.path[len(prefix) :]
    elif href.startswith("/docs/en/"):
        candidate = href[len("/docs/en/") :]
    elif href.startswith("/"):
        return None
    else:
        candidate = urllib.parse.urljoin(source, href)
    return canonical_topic_path(candidate)


def mapped_paths(mapping: dict[str, Any]) -> list[str]:
    if mapping.get("mapping_sha256") != MAP_SHA256:
        raise CorpusError("sources-a mapping digest differs")
    paths = sorted(
        {
            topic["topic_path"]
            for row in mapping.get("rows", [])
            for topic in row.get("topics", [])
        }
    )
    gaps = [row for row in mapping.get("rows", []) if row.get("state") == "source-gap"]
    if (
        len(paths) != EXPECTED_MAPPED_TOPICS
        or [(row.get("official_row"), row.get("label")) for row in gaps]
        != [
            (ROW_CICSMESSAGE, "CICSMESSAGE"),
            (ROW_DUMP, "DUMP"),
            (ROW_TRACEID, "ENTER TRACEID"),
        ]
    ):
        raise CorpusError("mapped topic or source-gap set differs")
    return paths


def retrieve_topics(paths: Iterable[str], tab: Any) -> dict[str, bytes]:
    """Retrieve fresh topic bytes through the browser, never from the cache."""
    ordered = sorted(set(paths))
    if not ordered or len(ordered) > MAX_HTML_TOPICS:
        raise CorpusError("HTML topic set is empty or exceeds its bound")
    results = []
    for path in ordered:
        status, body = fetch_binary(tab, docs_api.content_url(path))
        results.append((path, status, body))
    failures = [
        f"{path}:http-{status}"
        for path, status, body in results
        if status != 200 or body is None
    ]
    if failures:
        raise CorpusError(f"IBM topic retrieval failed: {', '.join(failures)}")
    bodies = {path: body for path, _, body in results if isinstance(body, bytes)}
    if len(bodies) != len(ordered):
        raise CorpusError("IBM topic retrieval returned an incomplete set")
    for path, body in bodies.items():
        text = body.decode("utf-8", "replace")
        if (
            not body
            or len(body) > MAX_TOPIC_BYTES
            or not docs_api.heading_of(text)
            or not docs_api.last_modified_of(text)
        ):
            raise CorpusError(f"IBM topic lacks bounded provenance: {path}")
    return bodies


def cache_target(kind: str, locator: str, body: bytes, suffix: str) -> ibm_docs.CacheTarget:
    sha256 = docs_api.digest(body)
    legacy = (
        "toc-" + docs_api._key(locator) + ".json"
        if kind == "toc"
        else docs_api._key(locator.split("?", 1)[0])
    )
    return ibm_docs.CacheTarget(
        ibm_docs.cache_key(kind, locator, sha256, suffix),
        legacy,
        sha256,
        len(body),
        locator,
    )


def publish_bodies(cache: Path, bodies: dict[str, bytes]) -> None:
    counts: Counter[str] = Counter()
    for path, body in sorted(bodies.items()):
        ibm_docs.publish(cache, cache_target("topic", path, body, ".html"), body, counts)
    if counts["rejected_conflict"]:
        raise CorpusError("content-addressed IBM topic cache conflict")
    if counts["imported"] + counts["already_present"] != len(bodies):
        raise CorpusError("not every verified IBM topic reached the external cache")


def retrieve_toc(tab: Any, cache: Path) -> None:
    status, body = fetch_binary(tab, TOC_URL)
    if status != 200 or body is None:
        raise CorpusError(f"IBM table-of-contents retrieval failed: http-{status}")
    if docs_api.digest(body) != TOC_SHA256:
        raise CorpusError("IBM table-of-contents digest differs from the accepted source map")
    counts: Counter[str] = Counter()
    ibm_docs.publish(cache, cache_target("toc", TOC_URL, body, ".json"), body, counts)
    if counts["rejected_conflict"] or counts["imported"] + counts["already_present"] != 1:
        raise CorpusError("verified IBM table of contents did not reach the external cache")


def derive_linked_topics(
    mapped: list[str], bodies: dict[str, bytes]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    mapped_set = set(mapped)
    references: Counter[str] = Counter()
    for source in mapped:
        targets = {
            target
            for _, href in docs_api.html_links(bodies[source].decode("utf-8", "replace"))
            if (target := linked_topic(href, source)) is not None
        }
        references.update(targets - mapped_set)
    summary_count = references.pop(SUMMARY_PATH, 0)
    linked = [
        {
            "topic_path": path,
            "role": "one-hop-context-candidate",
            "referenced_by_command_topics": references[path],
        }
        for path in sorted(references)
    ]
    excluded = [
        {
            "topic_path": SUMMARY_PATH,
            "role": "navigation-index",
            "referenced_by_command_topics": summary_count,
            "reason": "pinned-toc-is-navigation-authority",
        }
    ]
    if len(linked) != EXPECTED_LINKED_TOPICS or summary_count != EXPECTED_MAPPED_TOPICS:
        raise CorpusError(
            f"one-hop link closure differs: linked={len(linked)} summary={summary_count}"
        )
    return linked, excluded


def manifest_from_bodies(bodies: dict[str, bytes]) -> dict[str, Any]:
    topics = []
    for path in sorted(bodies):
        body = bodies[path]
        modified = docs_api.last_modified_of(body.decode("utf-8", "replace"))
        if modified is None:
            raise CorpusError(f"topic has no Last Updated value: {path}")
        topics.append(
            {
                "bytes": len(body),
                "last_modified": modified,
                "sha256": docs_api.digest(body),
                "topic_path": path,
            }
        )
    manifest = {
        "baseline_id": BASELINE_ID,
        "book_href": SUMMARY_PATH,
        "book_label": "CICS application API sources-a HTML topic corpus",
        "content_url_template": docs_api.CONTENT_URL,
        "coverage_credit": 0,
        "product": PRODUCT,
        "retained_in_repository": False,
        "schema_version": "mainframe-env.topic-manifest@1",
        "snapshot_date": SNAPSHOT_DATE,
        "subsystem": "cics",
        "target_version": TARGET_VERSION,
        "toc_sha256": TOC_SHA256,
        "toc_url": TOC_URL,
        "topic_count": len(topics),
        "topic_manifest_digest": docs_api.manifest_digest(topics),
        "topic_manifest_digest_definition": docs_api.DIGEST_DEFINITION,
        "topics": topics,
        "total_bytes": sum(topic["bytes"] for topic in topics),
    }
    return manifest


def gap_records() -> list[dict[str, Any]]:
    return [
        {
            "official_row": ROW_CICSMESSAGE,
            "label": "CICSMESSAGE",
            "state": "pending-review",
            "topic_sources": [
                "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
                "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
            ],
        },
        {
            "official_row": ROW_DUMP,
            "label": "DUMP",
            "state": "pending-review",
            "topic_sources": [
                "SSJL4D_6.x/reference-applications/commands-api/dfhp4_dumptransaction.html",
                "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
                "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
            ],
        },
        {
            "official_row": ROW_TRACEID,
            "label": "ENTER TRACEID",
            "state": "pending-review",
            "topic_sources": [
                "SSJL4D_6.x/reference-applications/commands-api/dfhp4_entertracenum.html",
                "SSJL4D_6.x/reference-applications/commands-api/dfhp4_monitor.html",
                "SSJL4D_6.x/reference-diagnostics/components/dfhs34k.html",
                "SSJL4D_6.x/reference-diagnostics/eib/dfha8mf.html",
                "SSJL4D_6.x/reference-diagnostics/modules/dfhs3c001248.html",
            ],
        },
    ]


def corpus_core(corpus: dict[str, Any]) -> dict[str, Any]:
    return {
        key: corpus[key]
        for key in [
            "mapping",
            "topic_manifest",
            "counts",
            "mapped_command_topics",
            "linked_context_topics",
            "manual_topics",
            "excluded_navigation",
            "source_gaps",
        ]
    }


def build_corpus(
    mapping: dict[str, Any],
    manifest: dict[str, Any],
    linked: list[dict[str, Any]],
    excluded: list[dict[str, Any]],
) -> dict[str, Any]:
    mapped = mapped_paths(mapping)
    expected_excluded = [
        {
            "topic_path": SUMMARY_PATH,
            "role": "navigation-index",
            "referenced_by_command_topics": EXPECTED_MAPPED_TOPICS,
            "reason": "pinned-toc-is-navigation-authority",
        }
    ]
    if excluded != expected_excluded:
        raise CorpusError("excluded navigation disposition differs")
    if len(linked) != EXPECTED_LINKED_TOPICS:
        raise CorpusError("linked context topic count differs")
    for row in linked:
        if (
            set(row)
            != {"topic_path", "role", "referenced_by_command_topics"}
            or canonical_topic_path(row.get("topic_path", "")) is None
            or row.get("role") != "one-hop-context-candidate"
            or not isinstance(row.get("referenced_by_command_topics"), int)
            or not 1 <= row["referenced_by_command_topics"] <= EXPECTED_MAPPED_TOPICS
        ):
            raise CorpusError("linked context topic metadata differs")
    manual_paths = {row["topic_path"] for row in MANUAL_TOPICS}
    linked_paths = {row["topic_path"] for row in linked}
    if manual_paths & (set(mapped) | linked_paths):
        raise CorpusError("manual topics overlap mapped or one-hop topics")
    expected = set(mapped) | linked_paths | manual_paths
    actual = {topic["topic_path"] for topic in manifest.get("topics", [])}
    if len(manual_paths) != EXPECTED_MANUAL_TOPICS or actual != expected:
        raise CorpusError("topic manifest differs from the selected corpus union")
    gaps = gap_records()
    if any(not set(gap["topic_sources"]) <= actual for gap in gaps):
        raise CorpusError("source-gap evidence falls outside the pinned corpus")
    manifest_bytes = pretty(manifest).encode()
    counts = {
        "mapping_rows": 88,
        "mapped_command_topics": len(mapped),
        "linked_context_topics": len(linked),
        "manual_topics": len(MANUAL_TOPICS),
        "html_topics": len(actual),
        "source_gaps_pending_review": 3,
    }
    if counts["html_topics"] != EXPECTED_HTML_TOPICS:
        raise CorpusError(f"HTML corpus count differs: {counts}")
    corpus: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "work_package": WORK_PACKAGE,
        "status": "candidate",
        "semantic_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "differential_credit": 0,
        "mapping": {
            "path": str(MAP_PATH),
            "sha256": MAP_SHA256,
        },
        "topic_manifest": {
            "path": str(MANIFEST_PATH),
            "file_sha256": f"sha256:{hashlib.sha256(manifest_bytes).hexdigest()}",
            "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
            "topic_count": manifest["topic_count"],
            "total_bytes": manifest["total_bytes"],
        },
        "counts": counts,
        "mapped_command_topics": mapped,
        "linked_context_topics": linked,
        "manual_topics": MANUAL_TOPICS,
        "excluded_navigation": excluded,
        "source_gaps": gaps,
        "corpus_digest_definition": CORPUS_DIGEST_DEFINITION,
        "corpus_sha256": "",
    }
    corpus["corpus_sha256"] = canonical_digest(corpus_core(corpus))
    return corpus


def registry_with_manifest(registry: dict[str, Any], manifest: dict[str, Any]) -> dict[str, Any]:
    if (
        registry.get("schema_version") != "mainframe-env.topic-manifest-registry@1"
        or registry.get("target_version") != TARGET_VERSION
        or registry.get("semantic_authority") is not False
        or registry.get("coverage_credit") != 0
        or not isinstance(registry.get("manifests"), list)
    ):
        raise CorpusError("0.9 topic registry header differs")
    entry = {
        "scope_id": SCOPE_ID,
        "subsystem": "cics",
        "baseline_id": BASELINE_ID,
        "manifest": str(MANIFEST_PATH),
        "manifest_sha256": "sha256:" + hashlib.sha256(pretty(manifest).encode()).hexdigest(),
        "topic_count": manifest["topic_count"],
        "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
        "semantic_authority": False,
        "coverage_credit": 0,
    }
    entries = [row for row in registry.get("manifests", []) if row.get("scope_id") != SCOPE_ID]
    entries.append(entry)
    result = dict(registry)
    result["manifests"] = sorted(entries, key=lambda row: row["scope_id"])
    return result


def later_mapped_paths(
    mapping: dict[str, Any], config: LaterCorpusConfig
) -> list[str]:
    """Return the exact mapped set after checking the batch's gap closure."""
    rows = mapping.get("rows")
    if not isinstance(rows, list) or len(rows) != config.batch.row_count:
        raise CorpusError(f"sources-{config.batch.name} mapping row count differs")
    paths = sorted(
        {
            topic["topic_path"]
            for row in rows
            for topic in row.get("topics", [])
        }
    )
    gaps = [row for row in rows if row.get("state") == "source-gap"]
    resolved = [
        (row["official_row"], row["label"], row["eibfn"])
        for row in config.source_resolutions
    ]
    actual = [
        (row.get("official_row"), row.get("label"), row.get("eibfn"))
        for row in gaps
    ]
    if actual != resolved:
        raise CorpusError(
            f"sources-{config.batch.name} source-gap resolution set differs"
        )
    if not paths:
        raise CorpusError(f"sources-{config.batch.name} mapped topic set is empty")
    return paths


def derive_later_linked_topics(
    mapped: list[str],
    bodies: dict[str, bytes],
    manual_topics: Iterable[dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Derive one bounded link hop, excluding explicit manual source roles."""
    mapped_set = set(mapped)
    manual_paths = {row["topic_path"] for row in manual_topics}
    references: Counter[str] = Counter()
    for source in mapped:
        try:
            text = bodies[source].decode("utf-8")
        except (KeyError, UnicodeDecodeError) as error:
            raise CorpusError(f"mapped topic is unavailable or non-UTF-8: {source}") from error
        targets = {
            target
            for _, href in docs_api.html_links(text)
            if (target := linked_topic(href, source)) is not None
        }
        references.update(targets - mapped_set - manual_paths)
    summary_count = references.pop(SUMMARY_PATH, 0)
    linked = [
        {
            "topic_path": path,
            "role": "one-hop-context-candidate",
            "referenced_by_command_topics": references[path],
        }
        for path in sorted(references)
    ]
    excluded = [
        {
            "topic_path": SUMMARY_PATH,
            "role": "navigation-index",
            "referenced_by_command_topics": summary_count,
            "reason": "pinned-toc-is-navigation-authority",
        }
    ]
    if summary_count != len(mapped):
        raise CorpusError(
            "one-hop link closure differs: "
            f"summary={summary_count} mapped={len(mapped)}"
        )
    return linked, excluded


def later_manifest_from_bodies(
    bodies: dict[str, bytes], config: LaterCorpusConfig
) -> dict[str, Any]:
    topics = []
    for path in sorted(bodies):
        if canonical_topic_path(path) is None:
            raise CorpusError(f"non-target topic cannot enter target manifest: {path}")
        body = bodies[path]
        text = body.decode("utf-8", "replace")
        modified = docs_api.last_modified_of(text)
        if not docs_api.heading_of(text) or modified is None:
            raise CorpusError(f"topic has no bounded provenance: {path}")
        topics.append(
            {
                "bytes": len(body),
                "last_modified": modified,
                "sha256": docs_api.digest(body),
                "topic_path": path,
            }
        )
    return {
        "baseline_id": config.baseline_id,
        "book_href": SUMMARY_PATH,
        "book_label": config.book_label,
        "content_url_template": docs_api.CONTENT_URL,
        "coverage_credit": 0,
        "product": PRODUCT,
        "retained_in_repository": False,
        "schema_version": "mainframe-env.topic-manifest@1",
        "snapshot_date": SNAPSHOT_DATE,
        "subsystem": "cics",
        "target_version": TARGET_VERSION,
        "toc_sha256": TOC_SHA256,
        "toc_url": TOC_URL,
        "topic_count": len(topics),
        "topic_manifest_digest": docs_api.manifest_digest(topics),
        "topic_manifest_digest_definition": docs_api.DIGEST_DEFINITION,
        "topics": topics,
        "total_bytes": sum(topic["bytes"] for topic in topics),
    }


def validate_later_manifest(
    manifest: dict[str, Any], config: LaterCorpusConfig
) -> None:
    topics = manifest.get("topics")
    if set(manifest) != MANIFEST_FIELDS or not isinstance(topics, list) or not topics:
        raise CorpusError("topic manifest fields or topic set differ")
    paths: list[str] = []
    total = 0
    for topic in topics:
        if set(topic) != {"bytes", "last_modified", "sha256", "topic_path"}:
            raise CorpusError("topic manifest fields differ")
        path = canonical_topic_path(topic.get("topic_path", ""))
        if (
            path is None
            or path in paths
            or not isinstance(topic.get("bytes"), int)
            or isinstance(topic.get("bytes"), bool)
            or not 0 < topic["bytes"] <= MAX_TOPIC_BYTES
            or not isinstance(topic.get("last_modified"), str)
            or not isinstance(topic.get("sha256"), str)
            or len(topic["sha256"]) != 64
            or any(character not in HEX for character in topic["sha256"])
        ):
            raise CorpusError("topic manifest contains invalid metadata")
        paths.append(path)
        total += topic["bytes"]
    constants = {
        "schema_version": "mainframe-env.topic-manifest@1",
        "target_version": TARGET_VERSION,
        "baseline_id": config.baseline_id,
        "book_href": SUMMARY_PATH,
        "book_label": config.book_label,
        "snapshot_date": SNAPSHOT_DATE,
        "subsystem": "cics",
        "product": PRODUCT,
        "toc_url": TOC_URL,
        "toc_sha256": TOC_SHA256,
        "content_url_template": docs_api.CONTENT_URL,
        "coverage_credit": 0,
        "retained_in_repository": False,
    }
    if (
        any(manifest.get(key) != value for key, value in constants.items())
        or manifest.get("topic_count") != len(topics)
        or manifest.get("total_bytes") != total
        or manifest.get("topic_manifest_digest") != docs_api.manifest_digest(topics)
        or manifest.get("topic_manifest_digest_definition") != docs_api.DIGEST_DEFINITION
        or paths != sorted(paths)
    ):
        raise CorpusError("topic manifest identity, counts, or digest differs")


def later_corpus_core(corpus: dict[str, Any]) -> dict[str, Any]:
    return {
        key: corpus[key]
        for key in [
            "mapping",
            "topic_manifest",
            "counts",
            "mapped_command_topics",
            "linked_context_topics",
            "manual_topics",
            "excluded_navigation",
            "supplemental_topics",
            "source_resolutions",
        ]
    }


def build_later_corpus(
    mapping: dict[str, Any],
    manifest: dict[str, Any],
    linked: list[dict[str, Any]],
    excluded: list[dict[str, Any]],
    config: LaterCorpusConfig,
) -> dict[str, Any]:
    mapped = later_mapped_paths(mapping, config)
    manual = [dict(topic) for topic in config.manual_topics]
    supplemental = [dict(topic) for topic in config.supplemental_topics]
    resolutions = [dict(row) for row in config.source_resolutions]
    if excluded != [
        {
            "topic_path": SUMMARY_PATH,
            "role": "navigation-index",
            "referenced_by_command_topics": len(mapped),
            "reason": "pinned-toc-is-navigation-authority",
        }
    ]:
        raise CorpusError("excluded navigation disposition differs")
    linked_paths: set[str] = set()
    for row in linked:
        path = row.get("topic_path")
        if (
            set(row) != {"topic_path", "role", "referenced_by_command_topics"}
            or canonical_topic_path(path) is None
            or path in linked_paths
            or row.get("role") != "one-hop-context-candidate"
            or not isinstance(row.get("referenced_by_command_topics"), int)
            or not 1 <= row["referenced_by_command_topics"] <= len(mapped)
        ):
            raise CorpusError("linked context topic metadata differs")
        linked_paths.add(path)
    manual_paths = {row["topic_path"] for row in manual}
    if (
        len(manual_paths) != len(manual)
        or any(canonical_topic_path(path) is None for path in manual_paths)
        or (set(mapped) & linked_paths)
        or (set(mapped) & manual_paths)
        or (linked_paths & manual_paths)
    ):
        raise CorpusError("target corpus topic roles overlap or are invalid")
    expected_paths = set(mapped) | linked_paths | manual_paths
    actual_paths = {topic["topic_path"] for topic in manifest.get("topics", [])}
    if actual_paths != expected_paths:
        raise CorpusError("topic manifest differs from selected corpus union")
    supplement_paths = [topic["topic_path"] for topic in supplemental]
    if len(supplement_paths) != len(set(supplement_paths)):
        raise CorpusError("duplicate supplemental topic")
    manifest_bytes = pretty(manifest).encode()
    counts = {
        "mapping_rows": config.batch.row_count,
        "mapped_command_topics": len(mapped),
        "linked_context_topics": len(linked),
        "manual_topics": len(manual),
        "html_topics": len(actual_paths),
        "mapping_source_gaps": len(resolutions),
        "source_gaps_unresolved": 0,
        "supplemental_topics": len(supplemental),
    }
    corpus: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "work_package": config.work_package,
        "status": "candidate",
        "semantic_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "differential_credit": 0,
        "mapping": {
            "path": str(config.batch.map_path),
            "sha256": mapping["mapping_sha256"],
        },
        "topic_manifest": {
            "path": str(config.batch.manifest_path),
            "file_sha256": f"sha256:{hashlib.sha256(manifest_bytes).hexdigest()}",
            "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
            "topic_count": manifest["topic_count"],
            "total_bytes": manifest["total_bytes"],
        },
        "counts": counts,
        "mapped_command_topics": mapped,
        "linked_context_topics": linked,
        "manual_topics": manual,
        "excluded_navigation": excluded,
        "supplemental_topics": supplemental,
        "source_resolutions": resolutions,
        "corpus_digest_definition": LATER_CORPUS_DIGEST_DEFINITION,
        "corpus_sha256": "",
    }
    corpus["corpus_sha256"] = canonical_digest(later_corpus_core(corpus))
    return corpus


def later_registry_with_manifest(
    registry: dict[str, Any],
    manifest: dict[str, Any],
    config: LaterCorpusConfig,
) -> dict[str, Any]:
    if (
        registry.get("schema_version") != "mainframe-env.topic-manifest-registry@1"
        or registry.get("target_version") != TARGET_VERSION
        or registry.get("semantic_authority") is not False
        or registry.get("coverage_credit") != 0
        or not isinstance(registry.get("manifests"), list)
    ):
        raise CorpusError("0.9 topic registry header differs")
    entry = {
        "scope_id": config.batch.cache_scope,
        "subsystem": "cics",
        "baseline_id": config.baseline_id,
        "manifest": str(config.batch.manifest_path),
        "manifest_sha256": "sha256:" + hashlib.sha256(pretty(manifest).encode()).hexdigest(),
        "topic_count": manifest["topic_count"],
        "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
        "semantic_authority": False,
        "coverage_credit": 0,
    }
    entries = [
        row
        for row in registry.get("manifests", [])
        if row.get("scope_id") != config.batch.cache_scope
    ]
    entries.append(entry)
    result = dict(registry)
    result["manifests"] = sorted(entries, key=lambda row: row["scope_id"])
    return result


def supplemental_pins(config: LaterCorpusConfig) -> list[ibm_docs.Pin]:
    pins = []
    for topic in config.supplemental_topics:
        pin = ibm_docs.Pin(
            topic["topic_path"],
            topic["content_url"],
            topic["sha256"],
            topic["bytes"],
            (),
        )
        if pin.key != topic["cache_key"]:
            raise CorpusError(f"supplement cache key differs: {pin.topic}")
        pins.append(pin)
    return pins


def read_browser_capture(stream: BinaryIO) -> dict[str, bytes]:
    """Read a bounded browser capture archive without extracting it to disk."""
    members: dict[str, bytes] = {}
    total = 0
    with tarfile.open(fileobj=stream, mode="r|*") as archive:
        for member in archive:
            if not member.isfile():
                continue
            name = member.name.removeprefix("./")
            if "/" in name or name in members:
                raise CorpusError("browser capture contains an unsafe or duplicate member")
            if name != "browser-meta.json" and not name.endswith((".htm", ".html")):
                continue
            if not 0 < member.size <= MAX_TOPIC_BYTES:
                raise CorpusError(f"browser capture member exceeds its bound: {name}")
            total += member.size
            if total > 512 * 1024 * 1024 or len(members) >= 1_024:
                raise CorpusError("browser capture archive exceeds its bound")
            source = archive.extractfile(member)
            if source is None:
                raise CorpusError(f"browser capture member is unreadable: {name}")
            body = source.read(MAX_TOPIC_BYTES + 1)
            if len(body) != member.size:
                raise CorpusError(f"browser capture member is truncated: {name}")
            members[name] = body
    try:
        metadata = json.loads(members.pop("browser-meta.json").decode("utf-8"))
    except (KeyError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CorpusError(f"browser capture metadata is invalid: {error}") from error
    if not isinstance(metadata, list) or not metadata:
        raise CorpusError("browser capture metadata must be a non-empty array")
    bodies: dict[str, bytes] = {}
    supplement_paths = {
        topic["topic_path"]
        for config in LATER_BATCHES.values()
        for topic in config.supplemental_topics
    }
    for index, row in enumerate(metadata):
        if not isinstance(row, dict):
            raise CorpusError(f"browser capture metadata row {index} is invalid")
        path = row.get("topic_path")
        if canonical_topic_path(path) is None and path not in supplement_paths:
            raise CorpusError(f"browser capture topic path is invalid: {path}")
        name = docs_api._key(path.split("?", 1)[0])
        body = members.get(name)
        if (
            path in bodies
            or body is None
            or row.get("bytes") != len(body)
            or row.get("sha256") != docs_api.digest(body)
        ):
            raise CorpusError(f"browser capture identity differs: {path}")
        text = body.decode("utf-8", "replace")
        if not docs_api.heading_of(text) or not docs_api.last_modified_of(text):
            raise CorpusError(f"browser capture provenance differs: {path}")
        bodies[path] = body
    return bodies


def write_browser_capture_outputs(
    root: Path,
    cache: Path,
    stream: BinaryIO,
    batches: Iterable[str],
) -> None:
    """Generate later-batch corpora from user-Chrome bodies streamed on stdin."""
    configs = []
    for batch in batches:
        try:
            configs.append(LATER_BATCHES[batch])
        except KeyError as error:
            raise CorpusError(
                "browser capture import supports only sources-b and sources-c"
            ) from error
    bodies = read_browser_capture(stream)
    registered_pins, _ = ibm_docs.load_pins()
    pins_by_topic = {pin.topic: pin for pin in registered_pins}
    registry = read_json(root / REGISTRY_PATH)
    outputs: list[tuple[Path, dict[str, Any]]] = []
    for config in configs:
        source_map.check_batch(root, config.map_batch_id)
        mapping = read_json(root / config.batch.map_path)
        mapped = later_mapped_paths(mapping, config)
        command_bodies = {
            path: bodies[path]
            for path in mapped
            if path in bodies
        }
        if len(command_bodies) != len(mapped):
            missing = sorted(set(mapped) - set(command_bodies))
            raise CorpusError(f"browser capture lacks mapped topics: {missing}")
        linked, excluded = derive_later_linked_topics(
            mapped, command_bodies, config.manual_topics
        )
        target_paths = (
            set(mapped)
            | {row["topic_path"] for row in linked}
            | {row["topic_path"] for row in config.manual_topics}
        )
        for path in sorted(target_paths - set(bodies)):
            pin = pins_by_topic.get(path)
            if pin is not None:
                bodies[path] = ibm_docs.cached_body(cache, pin)
        missing = sorted(target_paths - set(bodies))
        if missing:
            raise CorpusError(f"browser capture lacks selected target topics: {missing}")
        target_bodies = {path: bodies[path] for path in target_paths}
        supplement_bodies = {}
        for topic in config.supplemental_topics:
            body = bodies.get(topic["topic_path"])
            if (
                body is None
                or len(body) != topic["bytes"]
                or docs_api.digest(body) != topic["sha256"]
            ):
                raise CorpusError(
                    f"browser capture supplement differs: {topic['topic_path']}"
                )
            supplement_bodies[topic["topic_path"]] = body
        publish_bodies(cache, target_bodies | supplement_bodies)
        manifest = later_manifest_from_bodies(target_bodies, config)
        corpus = build_later_corpus(mapping, manifest, linked, excluded, config)
        registry = later_registry_with_manifest(registry, manifest, config)
        outputs.extend(
            [
                (root / config.batch.manifest_path, manifest),
                (root / config.batch.corpus_path, corpus),
            ]
        )
    outputs.append((root / REGISTRY_PATH, registry))
    for path, value in outputs:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(pretty(value))


def _write_sources_a_outputs(root: Path, cache: Path, port: int) -> None:
    source_map.check(root)
    mapping = read_json(root / MAP_PATH)
    mapped = mapped_paths(mapping)
    tab = open_tab(port)
    try:
        establish(tab)
        retrieve_toc(tab, cache)
        command_bodies = retrieve_topics(mapped, tab)
        linked, excluded = derive_linked_topics(mapped, command_bodies)
        all_paths = set(mapped) | {row["topic_path"] for row in linked} | {
            row["topic_path"] for row in MANUAL_TOPICS
        }
        additional_bodies = retrieve_topics(all_paths - set(mapped), tab)
    finally:
        tab.close()
    bodies = command_bodies | additional_bodies
    publish_bodies(cache, bodies)
    manifest = manifest_from_bodies(bodies)
    corpus = build_corpus(mapping, manifest, linked, excluded)
    registry = registry_with_manifest(read_json(root / REGISTRY_PATH), manifest)
    for path, value in [
        (root / MANIFEST_PATH, manifest),
        (root / CORPUS_PATH, corpus),
        (root / REGISTRY_PATH, registry),
    ]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(pretty(value))


def _write_later_outputs(
    root: Path, cache: Path, port: int, config: LaterCorpusConfig
) -> None:
    source_map.check_batch(root, config.map_batch_id)
    mapping = read_json(root / config.batch.map_path)
    mapped = later_mapped_paths(mapping, config)
    manual_paths = {row["topic_path"] for row in config.manual_topics}
    tab = open_tab(port)
    try:
        establish(tab)
        retrieve_toc(tab, cache)
        command_bodies = retrieve_topics(mapped, tab)
        linked, excluded = derive_later_linked_topics(
            mapped, command_bodies, config.manual_topics
        )
        target_paths = set(mapped) | {row["topic_path"] for row in linked} | manual_paths
        target_bodies = command_bodies | retrieve_topics(
            target_paths - set(command_bodies), tab
        )
        supplement_paths = {row["topic_path"] for row in config.supplemental_topics}
        supplement_bodies = (
            retrieve_topics(supplement_paths, tab) if supplement_paths else {}
        )
    finally:
        tab.close()
    for topic in config.supplemental_topics:
        body = supplement_bodies.get(topic["topic_path"])
        if (
            body is None
            or len(body) != topic["bytes"]
            or docs_api.digest(body) != topic["sha256"]
        ):
            raise CorpusError(f"supplement identity differs: {topic['topic_path']}")
    publish_bodies(cache, target_bodies | supplement_bodies)
    manifest = later_manifest_from_bodies(target_bodies, config)
    corpus = build_later_corpus(mapping, manifest, linked, excluded, config)
    registry = later_registry_with_manifest(
        read_json(root / REGISTRY_PATH), manifest, config
    )
    for path, value in [
        (root / config.batch.manifest_path, manifest),
        (root / config.batch.corpus_path, corpus),
        (root / REGISTRY_PATH, registry),
    ]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(pretty(value))


def write_outputs(
    root: Path, cache: Path, port: int, batch: str = "a"
) -> None:
    if batch == "a":
        _write_sources_a_outputs(root, cache, port)
        return
    try:
        config = LATER_BATCHES[batch]
    except KeyError as error:
        raise CorpusError(f"unknown CICS source corpus batch: {batch}") from error
    _write_later_outputs(root, cache, port, config)


def validate_manifest(manifest: dict[str, Any]) -> None:
    topics = manifest.get("topics")
    if (
        set(manifest) != MANIFEST_FIELDS
        or not isinstance(topics, list)
        or len(topics) != EXPECTED_HTML_TOPICS
    ):
        raise CorpusError("topic manifest count differs")
    paths: set[str] = set()
    ordered_paths: list[str] = []
    total = 0
    for topic in topics:
        if set(topic) != {"bytes", "last_modified", "sha256", "topic_path"}:
            raise CorpusError("topic manifest fields differ")
        path = canonical_topic_path(topic.get("topic_path", ""))
        if (
            path is None
            or path in paths
            or not isinstance(topic.get("bytes"), int)
            or not 0 < topic["bytes"] <= MAX_TOPIC_BYTES
            or not isinstance(topic.get("last_modified"), str)
            or len(topic["last_modified"]) < 4
            or not isinstance(topic.get("sha256"), str)
            or len(topic["sha256"]) != 64
            or any(character not in HEX for character in topic["sha256"])
        ):
            raise CorpusError("topic manifest contains invalid metadata")
        paths.add(path)
        ordered_paths.append(path)
        total += topic["bytes"]
    if (
        manifest.get("schema_version") != "mainframe-env.topic-manifest@1"
        or manifest.get("target_version") != TARGET_VERSION
        or manifest.get("baseline_id") != BASELINE_ID
        or manifest.get("book_href") != SUMMARY_PATH
        or manifest.get("book_label")
        != "CICS application API sources-a HTML topic corpus"
        or manifest.get("snapshot_date") != SNAPSHOT_DATE
        or manifest.get("subsystem") != "cics"
        or manifest.get("product") != PRODUCT
        or manifest.get("toc_url") != TOC_URL
        or manifest.get("toc_sha256") != TOC_SHA256
        or manifest.get("content_url_template") != docs_api.CONTENT_URL
        or manifest.get("topic_count") != len(topics)
        or manifest.get("total_bytes") != total
        or manifest.get("topic_manifest_digest") != docs_api.manifest_digest(topics)
        or manifest.get("topic_manifest_digest_definition") != docs_api.DIGEST_DEFINITION
        or manifest.get("coverage_credit") != 0
        or manifest.get("retained_in_repository") is not False
        or ordered_paths != sorted(ordered_paths)
    ):
        raise CorpusError("topic manifest identity, counts, or digest differs")


def _check_sources_a(root: Path = ROOT, cache: Path | None = None) -> None:
    source_map.check(root)
    mapping = read_json(root / MAP_PATH)
    manifest_path = root / MANIFEST_PATH
    manifest = read_json(manifest_path)
    validate_manifest(manifest)
    receipt_path = root / BROWSER_RECEIPT_PATH
    receipt = read_json(receipt_path)
    if receipt != expected_browser_receipt(manifest):
        raise CorpusError("browser source verification receipt differs")
    corpus = read_json(root / CORPUS_PATH)
    expected = build_corpus(
        mapping,
        manifest,
        corpus.get("linked_context_topics", []),
        corpus.get("excluded_navigation", []),
    )
    if corpus != expected or corpus.get("corpus_sha256") != canonical_digest(corpus_core(corpus)):
        raise CorpusError("committed corpus differs from its deterministic projection")
    registry = read_json(root / REGISTRY_PATH)
    expected_registry = registry_with_manifest(registry, manifest)
    if registry != expected_registry:
        raise CorpusError("0.9 topic registry differs from the corpus manifest")
    for path, value in [
        (manifest_path, manifest),
        (receipt_path, receipt),
        (root / CORPUS_PATH, corpus),
    ]:
        if path.read_text() != pretty(value):
            raise CorpusError(f"{path} is not canonical generated JSON")
    if cache is None:
        return
    pins, tocs = ibm_docs.select(*ibm_docs.load_pins(), SCOPE_ID, None)
    bodies = {pin.topic: ibm_docs.cached_body(cache, pin) for pin in pins}
    for toc in tocs:
        ibm_docs.cached_toc(cache, toc)
    linked, excluded = derive_linked_topics(mapped_paths(mapping), bodies)
    if linked != corpus["linked_context_topics"] or excluded != corpus["excluded_navigation"]:
        raise CorpusError("cached one-hop link closure differs from the corpus")
    for topic in manifest["topics"]:
        body = bodies.get(topic["topic_path"])
        if (
            body is None
            or len(body) != topic["bytes"]
            or docs_api.digest(body) != topic["sha256"]
        ):
            raise CorpusError(f"cached topic differs: {topic['topic_path']}")


def _check_later(
    root: Path, cache: Path | None, config: LaterCorpusConfig
) -> None:
    source_map.check_batch(root, config.map_batch_id)
    mapping = read_json(root / config.batch.map_path)
    manifest_path = root / config.batch.manifest_path
    manifest = read_json(manifest_path)
    validate_later_manifest(manifest, config)
    corpus_path = root / config.batch.corpus_path
    corpus = read_json(corpus_path)
    expected = build_later_corpus(
        mapping,
        manifest,
        corpus.get("linked_context_topics", []),
        corpus.get("excluded_navigation", []),
        config,
    )
    if (
        corpus != expected
        or corpus.get("corpus_sha256")
        != canonical_digest(later_corpus_core(corpus))
    ):
        raise CorpusError("committed corpus differs from its deterministic projection")
    registry = read_json(root / REGISTRY_PATH)
    if registry != later_registry_with_manifest(registry, manifest, config):
        raise CorpusError("0.9 topic registry differs from the corpus manifest")
    for path, value in [(manifest_path, manifest), (corpus_path, corpus)]:
        if path.read_text() != pretty(value):
            raise CorpusError(f"{path} is not canonical generated JSON")
    supplemental_pins(config)
    if cache is None:
        return
    pins, tocs = ibm_docs.select(
        *ibm_docs.load_pins(), config.batch.cache_scope, None
    )
    bodies = {pin.topic: ibm_docs.cached_body(cache, pin) for pin in pins}
    for toc in tocs:
        ibm_docs.cached_toc(cache, toc)
    linked, excluded = derive_later_linked_topics(
        later_mapped_paths(mapping, config), bodies, config.manual_topics
    )
    if (
        linked != corpus["linked_context_topics"]
        or excluded != corpus["excluded_navigation"]
    ):
        raise CorpusError("cached one-hop link closure differs from the corpus")
    for topic in manifest["topics"]:
        body = bodies.get(topic["topic_path"])
        if (
            body is None
            or len(body) != topic["bytes"]
            or docs_api.digest(body) != topic["sha256"]
        ):
            raise CorpusError(f"cached topic differs: {topic['topic_path']}")
    for topic, pin in zip(config.supplemental_topics, supplemental_pins(config)):
        body = ibm_docs.cached_body(cache, pin)
        text = body.decode("utf-8")
        if (
            docs_api.heading_of(text) != topic["heading"]
            or docs_api.last_modified_of(text) != topic["last_modified"]
        ):
            raise CorpusError(f"cached supplement provenance differs: {pin.topic}")


def check(
    root: Path = ROOT,
    cache: Path | None = None,
    batch: str = "a",
) -> None:
    """Check one batch; retained positional arguments remain sources-A compatible."""
    if batch == "a":
        _check_sources_a(root, cache)
        return
    try:
        config = LATER_BATCHES[batch]
    except KeyError as error:
        raise CorpusError(f"unknown CICS source corpus batch: {batch}") from error
    _check_later(root, cache, config)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    parser.add_argument(
        "--port",
        type=int,
        default=9222,
        help="Chrome DevTools port used only for fresh generation",
    )
    parser.add_argument(
        "--batch",
        choices=("a", "b", "c", "all", "all-later"),
        default="a",
        help="source corpus batch to generate or check (default: a)",
    )
    parser.add_argument("--check", action="store_true")
    parser.add_argument(
        "--import-browser-capture",
        action="store_true",
        help="stream a tar archive of browser HTML and browser-meta.json on stdin",
    )
    args = parser.parse_args()
    if args.check and args.import_browser_capture:
        parser.error("--check and --import-browser-capture are mutually exclusive")
    if not 1 <= args.port <= 65_535:
        parser.error("port must be between 1 and 65535")
    cache = args.cache or docs_api.default_cache()
    try:
        selected = (
            ("a", "b", "c")
            if args.batch == "all"
            else (("b", "c") if args.batch == "all-later" else (args.batch,))
        )
        if args.check:
            for batch in selected:
                check(ROOT, cache if args.cache else None, batch)
            print(f"cics-sources-{args.batch}-corpus: pass")
        elif args.import_browser_capture:
            if "a" in selected:
                raise CorpusError(
                    "browser capture import supports only --batch b, c, or all-later"
                )
            write_browser_capture_outputs(ROOT, cache, sys.stdin.buffer, selected)
            print(f"cics-sources-{args.batch}-corpus: imported browser capture")
        else:
            for batch in selected:
                write_outputs(ROOT, cache, args.port, batch)
            print(f"cics-sources-{args.batch}-corpus: generated")
    except (
        CorpusError,
        OSError,
        RuntimeError,
        source_map.SourceMapError,
        ValueError,
    ) as error:
        parser.exit(1, f"cics-sources-{args.batch}-corpus: {error}\n")


if __name__ == "__main__":
    main()
