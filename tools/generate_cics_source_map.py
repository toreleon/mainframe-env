#!/usr/bin/env python3
"""Generate zero-credit CICS application command-summary source maps.

The IBM table of contents is an external input.  The repository retains only a
bounded projection of its command-summary children and a row-to-topic mapping;
it never retains the TOC response or command-page publication bodies.
"""

from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Iterable

import generate_cics_descriptors as descriptors


ROOT = Path(__file__).resolve().parents[1]
TOC_PROJECTION_PATH = Path("conformance/0.9/cics/command-summary-topics.json")
MAP_PATH = Path("conformance/0.9/cics/application-api-sources-a-map.json")
TOC_SCHEMA_VERSION = "mainframe-env.cics-command-summary-topics@1"
MAP_SCHEMA_VERSION = "mainframe-env.cics-command-source-map@1"
TARGET_VERSION = "0.9.0"
WORK_PACKAGE = "CIC-901.sources-a-map"
TOC_URL = "https://www.ibm.com/docs/api/v1/toc/cics-ts/6.x?lang=en"
TOC_SHA256 = "sha256:f65c51e52facc390c05f084e1d249ff19e68bf2d7f8d3f32d4d745faf622681a"
SUMMARY_PATH = "SSJL4D_6.x/reference-applications/commands-api/dfhp4_commandsummary.html"
SUMMARY_TOPIC_ID = "reference-cics-command-summary"
SUMMARY_LABEL = "CICS command summary"
TOC_DOMAIN = b"mainframe-env.cics-command-summary-topics@1\0"
MAP_DOMAIN = b"mainframe-env.cics-command-source-map@1\0"
TOC_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-command-summary-topics@1, "
    "one NUL byte (0x00), then UTF-8 JSON with ensure_ascii=true, "
    "sort_keys=true and separators=(',', ':') of source_toc_sha256, summary, "
    "and ordered topics"
)
MAP_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-command-source-map@1, one "
    "NUL byte (0x00), then UTF-8 JSON with ensure_ascii=true, sort_keys=true "
    "and separators=(',', ':') of catalog, toc_projection, row_range, and "
    "ordered rows"
)
MAX_TOC_BYTES = 16 * 1024 * 1024
EXPECTED_TOPIC_COUNT = 345
EXPECTED_ROW_COUNT = 88
EXPECTED_RESOLVED_ROWS = 85
EXPECTED_SOURCE_GAPS = 3
EXPECTED_EDGES = 117
EXPECTED_UNIQUE_TOPICS = 109
EXPECTED_MULTI_TOPIC_ROWS = 9
EXPECTED_SHARED_TOPICS = 7

SOURCE_GAPS = frozenset({"CICSMESSAGE", "DUMP", "ENTER TRACEID"})
VARIANT_LABELS = frozenset(
    {
        "ALLOCATE",
        "BUILD ATTACH",
        "CONVERSE",
        "DELETE CONTAINER",
        "ENDBROWSE CONTAINER",
        "EXTRACT ATTACH",
        "EXTRACT ATTRIBUTES",
        "FREE",
        "GET CONTAINER",
    }
)
EXPECTED_VARIANT_COUNTS = {
    "ALLOCATE": 3,
    "BUILD ATTACH": 2,
    "CONVERSE": 22,
    "DELETE CONTAINER": 2,
    "ENDBROWSE CONTAINER": 2,
    "EXTRACT ATTACH": 2,
    "EXTRACT ATTRIBUTES": 2,
    "FREE": 4,
    "GET CONTAINER": 2,
}
SHARED_LABELS = {
    "ACQUIRE ACTIVITYID": "ACQUIRE",
    "ACQUIRE PROCESS": "ACQUIRE",
    "ASKTIME": "ASKTIME",
    "ASKTIME ABSTIME": "ASKTIME",
    "CANCEL ACQACTIVITY": "CANCEL (BTS)",
    "CANCEL ACQPROCESS": "CANCEL (BTS)",
    "CANCEL ACTIVITY": "CANCEL (BTS)",
    "CHECK ACQACTIVITY": "CHECK ACTIVITY",
    "CHECK ACTIVITY": "CHECK ACTIVITY",
}
COMBINED_LABELS = {
    "DEFINE COUNTER": "DEFINE COUNTER and DEFINE DCOUNTER",
    "DEFINE DCOUNTER": "DEFINE COUNTER and DEFINE DCOUNTER",
    "DELETE COUNTER": "DELETE COUNTER and DELETE DCOUNTER",
    "DELETE DCOUNTER": "DELETE COUNTER and DELETE DCOUNTER",
    "GET COUNTER": "GET COUNTER and GET DCOUNTER",
    "GET DCOUNTER": "GET COUNTER and GET DCOUNTER",
}


@dataclass(frozen=True)
class BatchConfig:
    batch_id: str
    first_index: int
    row_count: int
    map_path: Path
    work_package: str
    source_gaps: frozenset[str]
    variant_counts: dict[str, int]
    variant_topic_labels: dict[str, tuple[str, ...]]
    shared_labels: dict[str, str]
    combined_labels: dict[str, str]
    aliased_labels: dict[str, str]
    expected_counts: dict[str, Any]


SOURCES_A = BatchConfig(
    batch_id="sources-a",
    first_index=0,
    row_count=EXPECTED_ROW_COUNT,
    map_path=MAP_PATH,
    work_package=WORK_PACKAGE,
    source_gaps=SOURCE_GAPS,
    variant_counts=EXPECTED_VARIANT_COUNTS,
    variant_topic_labels={},
    shared_labels=SHARED_LABELS,
    combined_labels=COMBINED_LABELS,
    aliased_labels={},
    expected_counts={
        "row_count": 88,
        "resolved_row_count": 85,
        "source_gap_count": 3,
        "edge_count": 117,
        "unique_topic_count": 109,
        "multi_topic_row_count": 9,
        "shared_topic_count": 7,
        "selection_kind_counts": {
            "exact": 61,
            "variant-set": 9,
            "shared-page": 9,
            "combined-page": 6,
            "source-gap": 3,
        },
    },
)

SOURCES_B = BatchConfig(
    batch_id="sources-b",
    first_index=88,
    row_count=88,
    map_path=Path("conformance/0.9/cics/application-api-sources-b-map.json"),
    work_package="CIC-901.sources-b-map",
    source_gaps=frozenset(),
    variant_counts={
        "GETNEXT CONTAINER": 2,
        "ISSUE COPY": 1,
        "ISSUE DISCONNECT": 2,
        "ISSUE SIGNAL": 2,
        "MOVE CONTAINER": 2,
        "PUT CONTAINER": 2,
        "RECEIVE": 20,
        "RECEIVE MAP": 2,
    },
    variant_topic_labels={
        "RECEIVE MAP": ("RECEIVE MAP", "RECEIVE MAP MAPPINGDEV"),
    },
    shared_labels={
        "LINK ACQACTIVITY": "LINK ACTIVITY",
        "LINK ACTIVITY": "LINK ACTIVITY",
        "RESUME ACQACTIVITY": "RESUME",
        "RESUME ACQPROCESS": "RESUME",
        "RESUME ACTIVITY": "RESUME",
    },
    combined_labels={
        "QUERY COUNTER": "QUERY COUNTER and QUERY DCOUNTER",
        "QUERY DCOUNTER": "QUERY COUNTER and QUERY DCOUNTER",
    },
    aliased_labels={},
    expected_counts={
        "row_count": 88,
        "resolved_row_count": 88,
        "source_gap_count": 0,
        "edge_count": 113,
        "unique_topic_count": 109,
        "multi_topic_row_count": 7,
        "shared_topic_count": 3,
        "selection_kind_counts": {
            "exact": 73,
            "variant-set": 8,
            "shared-page": 5,
            "combined-page": 2,
            "source-gap": 0,
        },
    },
)

SOURCES_C = BatchConfig(
    batch_id="sources-c",
    first_index=176,
    row_count=87,
    map_path=Path("conformance/0.9/cics/application-api-sources-c-map.json"),
    work_package="CIC-901.sources-c-map",
    source_gaps=frozenset({"SET ASSOCIATION USERCORRDATA", "TRACE"}),
    variant_counts={
        "SEND": 26,
        "SEND MAP": 2,
        "SEND TEXT": 3,
        "SIGNON": 2,
        "START": 2,
        "STARTBROWSE CONTAINER": 2,
        "SYNCPOINT": 2,
        "WAIT CONVID": 1,
        "WEB ENDBROWSE": 3,
        "WEB READ": 3,
        "WEB READNEXT": 3,
        "WEB RECEIVE": 2,
        "WEB SEND": 2,
        "WEB STARTBROWSE": 3,
        "WEB WRITE": 1,
    },
    variant_topic_labels={
        "SEND MAP": ("SEND MAP", "SEND MAP MAPPINGDEV"),
        "SEND TEXT": ("SEND TEXT", "SEND TEXT MAPPED", "SEND TEXT NOEDIT"),
        "SIGNON": ("SIGNON", "SIGNON TOKEN"),
        "START": ("START", "START CHANNEL"),
        "SYNCPOINT": ("SYNCPOINT", "SYNCPOINT ROLLBACK"),
        "WEB ENDBROWSE": (
            "WEB ENDBROWSE FORMFIELD",
            "WEB ENDBROWSE HTTPHEADER",
            "WEB ENDBROWSE QUERYPARM",
        ),
        "WEB READ": ("WEB READ FORMFIELD", "WEB READ HTTPHEADER", "WEB READ QUERYPARM"),
        "WEB READNEXT": (
            "WEB READNEXT FORMFIELD",
            "WEB READNEXT HTTPHEADER",
            "WEB READNEXT QUERYPARM",
        ),
        "WEB STARTBROWSE": (
            "WEB STARTBROWSE FORMFIELD",
            "WEB STARTBROWSE HTTPHEADER",
            "WEB STARTBROWSE QUERYPARM",
        ),
        "WEB WRITE": ("WEB WRITE HTTPHEADER",),
    },
    shared_labels={
        "RUN ACQACTIVITY": "RUN",
        "RUN ACQPROCESS": "RUN",
        "RUN ACTIVITY": "RUN",
        "SUSPEND ACQACTIVITY": "SUSPEND (BTS)",
        "SUSPEND ACQPROCESS": "SUSPEND (BTS)",
        "SUSPEND ACTIVITY": "SUSPEND (BTS)",
    },
    combined_labels={
        "REWIND COUNTER": "REWIND COUNTER and REWIND DCOUNTER",
        "REWIND DCOUNTER": "REWIND COUNTER and REWIND DCOUNTER",
        "UPDATE COUNTER": "UPDATE COUNTER and UPDATE DCOUNTER",
        "UPDATE DCOUNTER": "UPDATE COUNTER and UPDATE DCOUNTER",
    },
    aliased_labels={"WAIT": "GDS WAIT", "WRITE FILE": "WRITE"},
    expected_counts={
        "row_count": 87,
        "resolved_row_count": 85,
        "source_gap_count": 2,
        "edge_count": 127,
        "unique_topic_count": 121,
        "multi_topic_row_count": 13,
        "shared_topic_count": 4,
        "selection_kind_counts": {
            "exact": 58,
            "variant-set": 15,
            "shared-page": 6,
            "combined-page": 4,
            "aliased-page": 2,
            "source-gap": 2,
        },
    },
)

BATCHES = {config.batch_id: config for config in (SOURCES_A, SOURCES_B, SOURCES_C)}


class SourceMapError(ValueError):
    """The pinned TOC projection or source map is malformed or stale."""


def canonical_digest(domain: bytes, value: object) -> str:
    encoded = json.dumps(
        value, ensure_ascii=True, separators=(",", ":"), sort_keys=True
    ).encode()
    return f"sha256:{hashlib.sha256(domain + encoded).hexdigest()}"


def pretty(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def object_value(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise SourceMapError(f"{label} must be an object")
    return value


def array_value(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise SourceMapError(f"{label} must be an array")
    return value


def text_value(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise SourceMapError(f"{label} must be non-empty text")
    return value


def topic_path(value: Any, label: str) -> str:
    path = text_value(value, label)
    candidate = PurePosixPath(path)
    if (
        "\\" in path
        or candidate.is_absolute()
        or candidate.as_posix() != path
        or len(candidate.parts) != 4
        or candidate.parts[:2] != ("SSJL4D_6.x", "reference-applications")
        or candidate.parts[2] not in {"commands-api", "commands-bts"}
        or any(part in {"", ".", ".."} for part in candidate.parts)
        or re.fullmatch(r"[A-Za-z0-9_-]+\.html", candidate.parts[3]) is None
    ):
        raise SourceMapError(f"{label} is not a canonical command-summary path")
    return path


def read_json(path: Path) -> dict[str, Any]:
    try:
        return object_value(json.loads(path.read_text()), str(path))
    except (OSError, json.JSONDecodeError) as error:
        raise SourceMapError(f"{path}: {error}") from error


def walk_objects(value: Any) -> Iterable[dict[str, Any]]:
    pending = [value]
    visited = 0
    while pending:
        current = pending.pop()
        visited += 1
        if visited > 1_000_000:
            raise SourceMapError("TOC exceeds the bounded object/value count")
        if isinstance(current, dict):
            yield current
            pending.extend(reversed(list(current.values())))
        elif isinstance(current, list):
            pending.extend(reversed(current))


def projection_core(projection: dict[str, Any]) -> dict[str, Any]:
    return {
        "source_toc_sha256": projection["source_toc_sha256"],
        "summary": projection["summary"],
        "topics": projection["topics"],
    }


def projection_from_toc(
    body: bytes, expected_sha256: str = TOC_SHA256
) -> dict[str, Any]:
    if not body or len(body) > MAX_TOC_BYTES:
        raise SourceMapError("TOC byte count is empty or exceeds the bounded limit")
    actual_sha256 = f"sha256:{hashlib.sha256(body).hexdigest()}"
    if actual_sha256 != expected_sha256:
        raise SourceMapError(
            f"TOC SHA-256 differs: expected {expected_sha256}, got {actual_sha256}"
        )
    try:
        toc = json.loads(body)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SourceMapError(f"TOC is not valid JSON: {error}") from error
    matches = [node for node in walk_objects(toc) if node.get("href") == SUMMARY_PATH]
    if len(matches) != 1:
        raise SourceMapError(f"expected one command-summary node, found {len(matches)}")
    summary = matches[0]
    if (
        summary.get("label") != SUMMARY_LABEL
        or summary.get("topicId") != SUMMARY_TOPIC_ID
    ):
        raise SourceMapError("command-summary identity differs from the pinned locator")
    children = array_value(summary.get("topics"), "command-summary topics")
    if len(children) != EXPECTED_TOPIC_COUNT:
        raise SourceMapError(
            f"command-summary topic count differs: {len(children)}"
        )
    topics = []
    paths: set[str] = set()
    topic_ids: set[str] = set()
    for ordinal, raw in enumerate(children, 1):
        child = object_value(raw, f"command-summary topic {ordinal}")
        if "topics" in child and child["topics"]:
            raise SourceMapError(f"command-summary topic {ordinal} is not an immediate leaf")
        topic = {
            "ordinal": ordinal,
            "label": text_value(child.get("label"), f"topic {ordinal} label"),
            "topic_path": topic_path(child.get("href"), f"topic {ordinal} path"),
            "topic_id": text_value(child.get("topicId"), f"topic {ordinal} ID"),
        }
        if (
            topic["topic_path"] in paths
            or topic["topic_id"] in topic_ids
        ):
            raise SourceMapError(f"invalid or duplicate command-summary topic {ordinal}")
        paths.add(topic["topic_path"])
        topic_ids.add(topic["topic_id"])
        topics.append(topic)
    projection: dict[str, Any] = {
        "schema_version": TOC_SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "source_toc_url": TOC_URL,
        "source_toc_sha256": actual_sha256,
        "retained_source_bytes": False,
        "coverage_credit": 0,
        "summary": {
            "label": SUMMARY_LABEL,
            "topic_path": SUMMARY_PATH,
            "topic_id": SUMMARY_TOPIC_ID,
        },
        "topic_count": len(topics),
        "projection_digest_definition": TOC_DIGEST_DEFINITION,
        "projection_sha256": "",
        "topics": topics,
    }
    projection["projection_sha256"] = canonical_digest(
        TOC_DOMAIN, projection_core(projection)
    )
    return projection


def validate_projection(projection: dict[str, Any]) -> None:
    expected_fields = {
        "schema_version",
        "target_version",
        "source_toc_url",
        "source_toc_sha256",
        "retained_source_bytes",
        "coverage_credit",
        "summary",
        "topic_count",
        "projection_digest_definition",
        "projection_sha256",
        "topics",
    }
    if set(projection) != expected_fields:
        raise SourceMapError("command-summary projection fields differ")
    topics = array_value(projection["topics"], "projected topics")
    if (
        projection["schema_version"] != TOC_SCHEMA_VERSION
        or projection["target_version"] != TARGET_VERSION
        or projection["source_toc_url"] != TOC_URL
        or projection["source_toc_sha256"] != TOC_SHA256
        or projection["retained_source_bytes"] is not False
        or projection["coverage_credit"] != 0
        or projection["summary"]
        != {
            "label": SUMMARY_LABEL,
            "topic_path": SUMMARY_PATH,
            "topic_id": SUMMARY_TOPIC_ID,
        }
        or projection["topic_count"] != EXPECTED_TOPIC_COUNT
        or len(topics) != EXPECTED_TOPIC_COUNT
        or projection["projection_digest_definition"] != TOC_DIGEST_DEFINITION
        or projection["projection_sha256"]
        != canonical_digest(TOC_DOMAIN, projection_core(projection))
    ):
        raise SourceMapError("command-summary projection identity or digest differs")
    paths: set[str] = set()
    topic_ids: set[str] = set()
    for ordinal, raw in enumerate(topics, 1):
        topic = object_value(raw, f"projected topic {ordinal}")
        if set(topic) != {"ordinal", "label", "topic_path", "topic_id"}:
            raise SourceMapError(f"projected topic {ordinal} fields differ")
        path = topic_path(topic["topic_path"], f"projected topic {ordinal} path")
        topic_id = text_value(topic["topic_id"], f"projected topic {ordinal} ID")
        text_value(topic["label"], f"projected topic {ordinal} label")
        if (
            topic["ordinal"] != ordinal
            or path in paths
            or topic_id in topic_ids
        ):
            raise SourceMapError(f"invalid projected topic {ordinal}")
        paths.add(path)
        topic_ids.add(topic_id)


def selected_topics(
    label: str, topics: list[dict[str, Any]], config: BatchConfig = SOURCES_A
) -> tuple[str, str, str, list[dict[str, Any]]]:
    by_label: dict[str, list[dict[str, Any]]] = {}
    for topic in topics:
        by_label.setdefault(topic["label"], []).append(topic)

    if label in config.source_gaps:
        return "source-gap", "no-command-summary-node", "source-gap", []
    if label in config.shared_labels:
        selected = by_label.get(config.shared_labels[label], [])
        kind, rationale, role = "shared-page", "shared-command-form", "shared"
    elif label in config.combined_labels:
        selected = by_label.get(config.combined_labels[label], [])
        kind, rationale, role = "combined-page", "combined-command-page", "combined"
    elif label in config.aliased_labels:
        selected = by_label.get(config.aliased_labels[label], [])
        kind, rationale, role = "aliased-page", "catalog-label-alias", "alias"
    elif label in config.variant_counts:
        explicit = config.variant_topic_labels.get(label)
        if explicit is None:
            selected = [
                topic
                for topic in topics
                if topic["label"] == label
                or topic["label"].startswith(f"{label} (")
                or topic["label"].startswith(f"{label}:")
            ]
        else:
            selected = [
                topic
                for topic_label in explicit
                for topic in by_label.get(topic_label, [])
            ]
        kind, rationale, role = "variant-set", "qualified-variant-set", "variant"
    else:
        selected = by_label.get(label, [])
        kind, rationale, role = "exact", "exact-toc-label", "primary"
    expected_count = config.variant_counts[label] if kind == "variant-set" else 1
    if len(selected) != expected_count:
        raise SourceMapError(
            f"{label} {kind} mapping has {len(selected)} topics; expected {expected_count}"
        )
    return kind, rationale, "mapped", [
        {
            "topic_path": topic["topic_path"],
            "topic_id": topic["topic_id"],
            "toc_label": topic["label"],
            "role": role,
        }
        for topic in selected
    ]


def map_core(mapping: dict[str, Any]) -> dict[str, Any]:
    return {
        "catalog": mapping["catalog"],
        "toc_projection": mapping["toc_projection"],
        "row_range": mapping["row_range"],
        "rows": mapping["rows"],
    }


def batch_config(batch_id: str) -> BatchConfig:
    try:
        return BATCHES[batch_id]
    except KeyError as error:
        raise SourceMapError(f"unknown source-map batch {batch_id!r}") from error


def build_mapping(
    root: Path, projection: dict[str, Any], batch_id: str = "sources-a"
) -> dict[str, Any]:
    validate_projection(projection)
    config = batch_config(batch_id)
    # Source identity intentionally excludes the separate runtime-admission
    # catalog so option-readiness work cannot invalidate frozen IBM receipts.
    catalog = descriptors.load_catalog(root, include_runtime_admission=False)
    descriptor_path = root / descriptors.CATALOG_PATH
    descriptor_sha256 = f"sha256:{hashlib.sha256(descriptor_path.read_bytes()).hexdigest()}"
    commands = catalog["_application_commands"][
        config.first_index : config.first_index + config.row_count
    ]
    rows = []
    for command in commands:
        kind, rationale, state, topics = selected_topics(
            command["label"], projection["topics"], config
        )
        rows.append(
            {
                "official_row": command["official_row"],
                "label": command["label"],
                "eibfn": command["eibfn"],
                "state": state,
                "selection_kind": kind,
                "rationale": rationale,
                "topics": topics,
            }
        )

    topic_usage = Counter(
        topic["topic_path"] for row in rows for topic in row["topics"]
    )
    kind_counts = Counter(row["selection_kind"] for row in rows)
    selection_kinds = ["exact", "variant-set", "shared-page", "combined-page"]
    if config.aliased_labels:
        selection_kinds.append("aliased-page")
    selection_kinds.append("source-gap")
    counts = {
        "row_count": len(rows),
        "resolved_row_count": sum(row["state"] == "mapped" for row in rows),
        "source_gap_count": sum(row["state"] == "source-gap" for row in rows),
        "edge_count": sum(len(row["topics"]) for row in rows),
        "unique_topic_count": len(topic_usage),
        "multi_topic_row_count": sum(len(row["topics"]) > 1 for row in rows),
        "shared_topic_count": sum(count > 1 for count in topic_usage.values()),
        "selection_kind_counts": {
            key: kind_counts[key] for key in selection_kinds
        },
    }
    if counts != config.expected_counts:
        raise SourceMapError(f"{batch_id} mapping counts differ: {counts}")

    mapping: dict[str, Any] = {
        "schema_version": MAP_SCHEMA_VERSION,
        "target_version": TARGET_VERSION,
        "work_package": config.work_package,
        "status": "candidate",
        "semantic_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "differential_credit": 0,
        "catalog": {
            "descriptor_path": str(descriptors.CATALOG_PATH),
            "descriptor_sha256": descriptor_sha256,
            "official_catalog_sha256": catalog["official_catalog_sha256"],
            "application_identity_set_sha256": descriptors.application_identity_digest(
                catalog["_application_commands"]
            ),
        },
        "toc_projection": {
            "path": str(TOC_PROJECTION_PATH),
            "source_toc_sha256": projection["source_toc_sha256"],
            "projection_sha256": projection["projection_sha256"],
            "summary_topic_path": SUMMARY_PATH,
        },
        "row_range": {
            "first": commands[0]["official_row"],
            "last": commands[-1]["official_row"],
            "count": config.row_count,
        },
        "counts": counts,
        "mapping_digest_definition": MAP_DIGEST_DEFINITION,
        "mapping_sha256": "",
        "rows": rows,
    }
    mapping["mapping_sha256"] = canonical_digest(MAP_DOMAIN, map_core(mapping))
    return mapping


def validate_mapping(
    root: Path,
    mapping: dict[str, Any],
    projection: dict[str, Any],
    batch_id: str = "sources-a",
) -> None:
    config = batch_config(batch_id)
    expected = build_mapping(root, projection, batch_id)
    if mapping != expected:
        raise SourceMapError(f"{config.map_path} differs from the deterministic mapping")
    if mapping["mapping_sha256"] != canonical_digest(MAP_DOMAIN, map_core(mapping)):
        raise SourceMapError(f"{batch_id} mapping digest differs")


def write_outputs(
    root: Path,
    projection: dict[str, Any],
    batch_ids: Iterable[str] = ("sources-a",),
    *,
    write_projection: bool = True,
) -> None:
    outputs = [
        (batch_config(batch_id).map_path, build_mapping(root, projection, batch_id))
        for batch_id in batch_ids
    ]
    if write_projection:
        outputs.insert(0, (TOC_PROJECTION_PATH, projection))
    for relative, value in outputs:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(pretty(value))


def check(
    root: Path = ROOT,
    external_toc: bytes | None = None,
    batch_id: str = "sources-a",
) -> None:
    config = batch_config(batch_id)
    projection = read_json(root / TOC_PROJECTION_PATH)
    validate_projection(projection)
    if external_toc is not None:
        from_source = projection_from_toc(external_toc)
        if from_source != projection:
            raise SourceMapError("committed TOC projection differs from the pinned source")
    mapping = read_json(root / config.map_path)
    validate_mapping(root, mapping, projection, batch_id)
    for relative, value in [
        (TOC_PROJECTION_PATH, projection),
        (config.map_path, mapping),
    ]:
        if (root / relative).read_text() != pretty(value):
            raise SourceMapError(f"{relative} is not canonical generated JSON")


def check_batch(
    root: Path, batch_id: str, external_toc: bytes | None = None
) -> None:
    check(root, external_toc, batch_id)


def read_toc(argument: str) -> bytes:
    if argument == "-":
        body = sys.stdin.buffer.read(MAX_TOC_BYTES + 1)
    else:
        path = Path(argument).resolve()
        try:
            path.relative_to(ROOT.resolve())
        except ValueError:
            pass
        else:
            raise SourceMapError("raw IBM TOC input must remain outside the repository")
        body = path.read_bytes()
    if len(body) > MAX_TOC_BYTES:
        raise SourceMapError("TOC exceeds the bounded byte limit")
    return body


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--toc",
        metavar="PATH|-",
        help="external pinned IBM TOC JSON; use - to read stdin",
    )
    parser.add_argument(
        "--batch",
        choices=[*BATCHES, "all"],
        default="sources-a",
        help="source-map batch to generate or check (default: sources-a)",
    )
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        body = read_toc(args.toc) if args.toc else None
        batch_ids = tuple(BATCHES) if args.batch == "all" else (args.batch,)
        if args.check:
            for batch_id in batch_ids:
                check(ROOT, body, batch_id)
            print(f"cics-source-map ({args.batch}): pass")
        else:
            projection = (
                projection_from_toc(body)
                if body is not None
                else read_json(ROOT / TOC_PROJECTION_PATH)
            )
            validate_projection(projection)
            write_outputs(
                ROOT,
                projection,
                batch_ids,
                write_projection=body is not None,
            )
            generated = ", ".join(str(batch_config(item).map_path) for item in batch_ids)
            print(f"cics-source-map: generated {generated}")
    except (OSError, SourceMapError, descriptors.DescriptorError) as error:
        parser.exit(1, f"cics-source-map: {error}\n")


if __name__ == "__main__":
    main()
