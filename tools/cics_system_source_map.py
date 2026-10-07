"""Bounded SPI/FEPI source projections for the shared CICS source-map owner.

These locators establish row-to-publication identity only. They never replace
the shared command, resource, condition, security or execution authorities.
"""

from __future__ import annotations

from collections import Counter
from html.parser import HTMLParser
import hashlib
import json
from pathlib import Path
import sys

import generate_cics_source_map as shared

sys.path.insert(0, str(shared.ROOT / "conformance/tools"))
sys.path.insert(0, str(shared.ROOT / "conformance/subsystems/cics/system/tools"))
import ibm_docs  # noqa: E402
import generate_spi1001_catalog as identities  # noqa: E402

PROJECTION_VERSION = "mainframe-env.cics-command-topic-projection@1"
MAP_VERSION = "mainframe-env.cics-command-source-map@2"
PROJECTION_DOMAIN = b"mainframe-env.cics-command-topic-projection@1\0"
MAP_DOMAIN = b"mainframe-env.cics-command-source-map@2\0"
PROJECTION_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-command-topic-projection@1, "
    "one NUL byte, then canonical UTF-8 JSON (ensure_ascii=true, sort_keys=true, "
    "separators=(',', ':')) excluding projection_sha256 and projection_digest_definition"
)
MAP_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-command-source-map@2, "
    "one NUL byte, then canonical UTF-8 JSON (ensure_ascii=true, sort_keys=true, "
    "separators=(',', ':')) excluding mapping_sha256 and mapping_digest_definition"
)
COUNTS = {"spi": (269, 277, 267), "fepi": (39, 36, 36)}
FORM_LOCATORS = Path("conformance/subsystems/cics/system/cics/command-form-locators.json")
BRANCHES = {
    "spi": ("SSJL4D_6.x/reference-system-programming/commands-spi/dfha81j.html",),
    "fepi": (
        "SSJL4D_6.x/reference-applications/commands-fepi/dfhp743.html",
        "SSJL4D_6.x/reference-applications/commands-fepi/dfhp73u.html",
    ),
}
# Reviewed exceptions preserve the official EIBFN label literally. Qualified
# pages remain unresolved candidates; no row here declares accepted grammar.
EXCEPTIONS = {
    "6.3 and later INQUIRE OTEL": ("INQUIRE OTEL", "release-annotation", []),
    "6.2 and later INQUIRE SECDISCOVERY": ("INQUIRE SECDISCOVERY", "release-annotation", []),
    "6.2 and later PERFORM SECDISCOVERY": ("PERFORM SECDISCOVERY", "release-annotation", []),
    "6.3 and later SET OTEL": ("SET OTEL", "release-annotation", []),
    "6.2 and later SET SECDISCOVERY": ("SET SECDISCOVERY", "release-annotation", []),
    "INQUIRE VTAM 1": ("INQUIRE VTAM", "footnote-annotation", []),
    "SET VTAM 1": ("SET VTAM", "footnote-annotation", []),
    "INQUIRE TSQNAME": ("INQUIRE TSQUEUE / TSQNAME", "combined-page", ["TSQNAME"]),
    "INQUIRE TSQUEUE": ("INQUIRE TSQUEUE / TSQNAME", "combined-page", ["TSQUEUE"]),
    "SET TSQNAME": ("SET TSQUEUE / TSQNAME", "combined-page", ["TSQNAME"]),
    "SET TSQUEUE": ("SET TSQUEUE / TSQNAME", "combined-page", ["TSQUEUE"]),
    "PERFORM SECURITY": ("PERFORM SECURITY REBUILD", "qualified-page", ["SECURITY", "REBUILD"]),
    "PERFORM SSL": ("PERFORM SSL REBUILD", "qualified-page", ["SSL", "REBUILD"]),
    "PERFORM STATISTICS": ("PERFORM STATISTICS RECORD", "qualified-page", ["STATISTICS", "RECORD"]),
    "FEPI SET NODELIST": ("FEPI SET NODE", "shared-list-form", ["NODELIST"]),
    "FEPI SET POOLLIST": ("FEPI SET POOL", "shared-list-form", ["POOLLIST"]),
    "FEPI SET TARGETLIST": ("FEPI SET TARGET", "shared-list-form", ["TARGETLIST"]),
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise shared.SourceMapError(message)


def artifact_paths(family: str) -> tuple[Path, Path]:
    require(family in COUNTS, f"unknown administrative family {family}")
    directory = Path("conformance/subsystems/cics/system/cics")
    return directory / f"{family}-command-topics.json", directory / f"{family}-command-source-map.json"


def sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def core(value: dict, digest_field: str) -> dict:
    return {key: item for key, item in value.items()
            if key not in {digest_field, digest_field.replace("sha256", "digest_definition")}}


def source_scope(root: Path, family: str) -> tuple[ibm_docs.Scope, dict, list[dict]]:
    sources = ibm_docs.registered_sources(root / "conformance/subsystems/cics/system/manifests/index.json", root)
    matches = [(scope, manifest, topics) for scope, manifest, topics, _, _ in sources
               if scope.scope_id == f"cics-{family}-command-bodies"]
    require(len(matches) == 1, f"{family} body scope is not uniquely registered")
    scope, manifest, topics = matches[0]
    require(scope.target_subsystem == "cics.system-api" and len(topics) == COUNTS[family][1]
            and "sha256:" + manifest["toc_sha256"] == shared.TOC_SHA256
            and manifest["toc_url"] == shared.TOC_URL, f"{family} source scope drift")
    return scope, manifest, topics


class Headings(HTMLParser):
    """Extract bounded h1 identity while ignoring publication execution."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.active = False
        self.anchor: str | None = None
        self.parts: list[str] = []
        self.headings: list[dict] = []

    def handle_starttag(self, tag: str, attrs: list) -> None:
        if tag == "h1":
            self.active = True
            self.anchor = dict(attrs).get("id")
            self.parts = []

    def handle_endtag(self, tag: str) -> None:
        if tag == "h1" and self.active:
            self.headings.append({"heading_id": self.anchor,
                                  "heading_label": " ".join("".join(self.parts).split())})
            self.active = False

    def handle_data(self, data: str) -> None:
        if self.active:
            self.parts.append(data)


def body_bytes(cache: Path, scope: ibm_docs.Scope, manifest: dict, topic: dict) -> bytes:
    pin = ibm_docs.Pin(topic["topic_path"], ibm_docs.docs_api.content_url(topic["topic_path"]),
                       topic["sha256"], topic["bytes"], (scope,))
    return ibm_docs.cached_body(cache, pin)


def projection_from_source(root: Path, family: str, toc_body: bytes, cache: Path) -> dict:
    require(0 < len(toc_body) <= shared.MAX_TOC_BYTES, "TOC exceeds bounded limit")
    require("sha256:" + hashlib.sha256(toc_body).hexdigest() == shared.TOC_SHA256,
            "TOC SHA-256 differs")
    scope, manifest, pins = source_scope(root, family)
    toc = json.loads(toc_body)
    candidates: list[dict] = []
    for path in BRANCHES[family]:
        matches = [node for node in shared.walk_objects(toc) if node.get("href") == path]
        require(len(matches) == 1, f"TOC branch is not unique: {path}")
        for node in shared.array_value(matches[0].get("topics"), path):
            candidates.append(node)
    topics = []
    for pin in pins:
        matches = [node for node in candidates if node.get("href") == pin["topic_path"]]
        require(len(matches) == 1, f"body pin has no unique immediate TOC child: {pin['topic_path']}")
        node = matches[0]
        heading = Headings()
        heading.feed(body_bytes(cache, scope, manifest, pin).decode("utf-8"))
        require(len(heading.headings) == 1 and heading.headings[0]["heading_id"],
                f"body has no unique anchored heading: {pin['topic_path']}")
        topics.append({"topic_path": pin["topic_path"], "topic_id": node["topicId"],
                       "toc_label": node["label"], **heading.headings[0],
                       "sha256": "sha256:" + pin["sha256"], "bytes": pin["bytes"]})
    result = {
        "schema_version": PROJECTION_VERSION, "target_subsystem": "cics.system-api", "family": family,
        "source_toc_url": shared.TOC_URL, "source_toc_sha256": shared.TOC_SHA256,
        "branches": list(BRANCHES[family]), "manifest": scope.manifest,
        "manifest_sha256": sha256(root / scope.manifest),
        "topic_manifest_sha256": "sha256:" + manifest["topic_manifest_digest"],
        "semantic_authority": False, "coverage_credit": 0, "retained_source_bytes": False,
        "topic_count": len(topics), "topics": topics,
        "projection_digest_definition": PROJECTION_DEFINITION,
    }
    result["projection_sha256"] = shared.canonical_digest(PROJECTION_DOMAIN, core(result, "projection_sha256"))
    validate_projection(root, family, result)
    return result


def validate_projection(root: Path, family: str, projection: dict) -> None:
    scope, manifest, pins = source_scope(root, family)
    require(set(projection) == {
        "schema_version", "target_subsystem", "family", "source_toc_url", "source_toc_sha256",
        "branches", "manifest", "manifest_sha256", "topic_manifest_sha256", "semantic_authority",
        "coverage_credit", "retained_source_bytes", "topic_count", "topics",
        "projection_digest_definition", "projection_sha256",
    }, "administrative projection fields differ")
    require(projection["schema_version"] == PROJECTION_VERSION
            and projection["target_subsystem"] == "cics.system-api" and projection["family"] == family
            and projection["source_toc_url"] == shared.TOC_URL
            and projection["source_toc_sha256"] == shared.TOC_SHA256
            and projection["branches"] == list(BRANCHES[family])
            and projection["manifest"] == scope.manifest
            and projection["manifest_sha256"] == sha256(root / scope.manifest)
            and projection["topic_manifest_sha256"] == "sha256:" + manifest["topic_manifest_digest"]
            and projection["semantic_authority"] is False and projection["coverage_credit"] == 0
            and projection["retained_source_bytes"] is False
            and projection["topic_count"] == len(pins)
            and projection["projection_digest_definition"] == PROJECTION_DEFINITION
            and projection["projection_sha256"] == shared.canonical_digest(
                PROJECTION_DOMAIN, core(projection, "projection_sha256")), "administrative projection identity drift")
    topics = shared.array_value(projection["topics"], "administrative projected topics")
    require(len(topics) == len(pins), "administrative topic count drift")
    ids = set()
    for topic, pin in zip(topics, pins, strict=True):
        require(set(topic) == {"topic_path", "topic_id", "toc_label", "heading_id", "heading_label", "sha256", "bytes"},
                "administrative projected topic fields differ")
        require(topic["topic_path"] == pin["topic_path"] and topic["sha256"] == "sha256:" + pin["sha256"]
                and topic["bytes"] == pin["bytes"], "administrative projected body pin drift")
        for key in ["topic_id", "toc_label", "heading_id", "heading_label"]:
            shared.text_value(topic[key], f"administrative {key}")
        require(topic["topic_id"] not in ids, "duplicate administrative topic ID")
        ids.add(topic["topic_id"])


def build_mapping(root: Path, family: str, projection: dict) -> dict:
    validate_projection(root, family, projection)
    catalog = identities.final_catalog(root)
    path = identities.OUTPUT_PATH
    require((root / path).read_bytes() == identities.pretty_bytes(catalog), "SPI/FEPI identity catalog drift")
    forms = shared.read_json(root / FORM_LOCATORS)
    require(forms["schema_version"] == "mainframe-env.cics-command-form-locators@1"
            and forms["target_subsystem"] == "cics.system-api" and forms["semantic_authority"] is False
            and forms["coverage_credit"] == 0, "command-form locator boundary drift")
    identity_source = catalog["commands"][0]["source"]
    require(forms["identity_topic"] == {"topic_path": identity_source["topic_path"],
                                        "sha256": identity_source["topic_sha256"]},
            "command-form identity topic drift")
    reviewed = {row["label"]: row for row in forms["rows"]}
    require(len(forms["rows"]) == len(reviewed) == 17 and set(reviewed) == set(EXCEPTIONS),
            "command-form locator review rows drift")
    rows = []
    for command in catalog["commands"]:
        if command["interface"].lower() != family:
            continue
        label = command["label"]
        selected_label, kind, symbols = EXCEPTIONS.get(label, (label, "exact", []))
        matches = [topic for topic in projection["topics"] if topic["toc_label"] == selected_label]
        require(len(matches) == 1, f"{label} has no unique reviewed TOC mapping")
        topic = matches[0]
        require(topic["heading_label"] == selected_label, f"{label} mapped body heading differs")
        state = "candidate-unresolved" if kind == "qualified-page" else "mapped"
        review = reviewed.get(label)
        if review is not None:
            require(review["official_row"] == command["official_row"] and review["eibfn"] == command["eibfn"]
                    and review["state"] == state and review["topic_path"] == topic["topic_path"]
                    and review["topic_sha256"] == topic["sha256"] and review["heading_id"] == topic["heading_id"],
                    f"{label} reviewed form locator differs")
        rows.append({"official_row": command["official_row"], "label": label, "eibfn": command["eibfn"],
                     "additional_eibfn_codes": command["additional_eibfn_codes"],
                     "shared_eibfn_rows": command["shared_eibfn_rows"],
                     "state": state, "selection_kind": kind, "required_body_symbols": symbols,
                     "form_review": review, "topic": topic})
    usage = Counter(row["topic"]["topic_path"] for row in rows)
    require(len(rows) == COUNTS[family][0] and len(usage) == COUNTS[family][2], "administrative row denominator drift")
    projection_path, _ = artifact_paths(family)
    result = {
        "schema_version": MAP_VERSION, "target_subsystem": "cics.system-api", "family": family,
        "work_package": "SPI-1001.command-source-maps", "status": "source-identity-only",
        "semantic_authority": False, "automatic_registration": False, "public_routes": False,
        "coverage_credit": 0, "differential_credit": 0,
        "catalog": {"path": str(path), "sha256": sha256(root / path),
                    "identity_sha256": catalog["identity_sha256"],
                    "official_catalog_sha256": catalog["inputs"]["official_catalog_sha256"]},
        "toc_projection": {"path": str(projection_path), "sha256": projection["projection_sha256"]},
        "form_locators": {"path": str(FORM_LOCATORS), "sha256": sha256(root / FORM_LOCATORS)},
        "counts": {"row_count": len(rows), "unique_topic_count": len(usage),
                   "mapped_row_count": sum(row["state"] == "mapped" for row in rows),
                   "unresolved_row_count": sum(row["state"] == "candidate-unresolved" for row in rows),
                   "shared_topic_count": sum(count > 1 for count in usage.values()),
                   "selection_kind_counts": dict(sorted(Counter(row["selection_kind"] for row in rows).items()))},
        "rows": rows, "mapping_digest_definition": MAP_DEFINITION,
    }
    result["mapping_sha256"] = shared.canonical_digest(MAP_DOMAIN, core(result, "mapping_sha256"))
    return result


def verify_bodies(root: Path, family: str, mapping: dict, cache: Path) -> None:
    scope, manifest, pins = source_scope(root, family)
    by_path = {topic["topic_path"]: topic for topic in pins}
    identity = shared.read_json(root / FORM_LOCATORS)["identity_topic"]
    identity_pin = ibm_docs.Pin(identity["topic_path"], ibm_docs.docs_api.content_url(identity["topic_path"]),
                                identity["sha256"].removeprefix("sha256:"), 265761, ())
    identity_body = None
    for row in mapping["rows"]:
        topic = row["topic"]
        body = body_bytes(cache, scope, manifest, by_path[topic["topic_path"]])
        parser = Headings()
        parser.feed(body.decode("utf-8"))
        require(parser.headings == [{"heading_id": topic["heading_id"], "heading_label": topic["heading_label"]}],
                f"{row['official_row']} retained heading differs")
        text = "\n".join(ibm_docs.plain_text(body))
        for symbol in row["required_body_symbols"]:
            require(symbol in text, f"{row['official_row']} missing reviewed body symbol {symbol}")
        if row["form_review"] is not None:
            if identity_body is None:
                identity_body = ibm_docs.cached_body(cache, identity_pin)
            for locator in row["form_review"]["locators"]:
                selected = identity_body if locator["source"] == "identity" else body
                start, end = locator["byte_range"]
                require(isinstance(start, int) and not isinstance(start, bool)
                        and isinstance(end, int) and not isinstance(end, bool)
                        and 0 <= start < end <= len(selected), "form locator byte range is invalid")
                require("sha256:" + hashlib.sha256(selected[start:end]).hexdigest() == locator["fragment_sha256"],
                        f"{row['official_row']} form locator fragment differs")


def run(root: Path, family: str, check: bool, toc: bytes | None, cache: Path | None) -> None:
    projection_path, map_path = artifact_paths(family)
    require((toc is None) == (cache is None), "administrative source reproduction needs both --toc and --cache")
    if toc is not None:
        projection = projection_from_source(root, family, toc, cache)
    else:
        projection = shared.read_json(root / projection_path)
    mapping = build_mapping(root, family, projection)
    if cache is not None:
        verify_bodies(root, family, mapping, cache)
    for path, value in [(projection_path, projection), (map_path, mapping)]:
        output = shared.pretty(value)
        if check:
            require((root / path).read_text() == output, f"{path} differs from canonical source mapping")
        else:
            (root / path).write_text(output)
