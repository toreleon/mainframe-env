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
from typing import Any, Iterable
import urllib.parse


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "conformance/tools"))
sys.path.insert(0, str(ROOT / "tools"))
import docs_api  # noqa: E402
from browser_fetch import establish, fetch_binary, open_tab  # noqa: E402
import generate_cics_source_map as source_map  # noqa: E402
import ibm_docs  # noqa: E402


MAP_PATH = Path("conformance/0.9/cics/application-api-sources-a-map.json")
CORPUS_PATH = Path("conformance/0.9/cics/application-api-sources-a-corpus.json")
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
MAP_SHA256 = "sha256:5bacdef388d9ee405008a895289348a9c95ff21bb271751e4d91407cd4b68b0e"
SUMMARY_PATH = source_map.SUMMARY_PATH
CORPUS_DOMAIN = b"mainframe-env.cics-source-corpus@1\0"
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
EXPECTED_MANUAL_TOPICS = 5
EXPECTED_HTML_TOPICS = 172
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


def write_outputs(root: Path, cache: Path, port: int) -> None:
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


def check(root: Path = ROOT, cache: Path | None = None) -> None:
    source_map.check(root)
    mapping = read_json(root / MAP_PATH)
    manifest_path = root / MANIFEST_PATH
    manifest = read_json(manifest_path)
    validate_manifest(manifest)
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
    for path, value in [(manifest_path, manifest), (root / CORPUS_PATH, corpus)]:
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


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    parser.add_argument(
        "--port",
        type=int,
        default=9222,
        help="Chrome DevTools port used only for fresh generation",
    )
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if not 1 <= args.port <= 65_535:
        parser.error("port must be between 1 and 65535")
    cache = args.cache or docs_api.default_cache()
    try:
        if args.check:
            check(ROOT, cache if args.cache else None)
            print("cics-sources-a-corpus: pass")
        else:
            write_outputs(ROOT, cache, args.port)
            print(
                f"cics-sources-a-corpus: generated {MANIFEST_PATH}, {CORPUS_PATH}, "
                f"and {REGISTRY_PATH}"
            )
    except (
        CorpusError,
        OSError,
        RuntimeError,
        source_map.SourceMapError,
        ValueError,
    ) as error:
        parser.exit(1, f"cics-sources-a-corpus: {error}\n")


if __name__ == "__main__":
    main()
