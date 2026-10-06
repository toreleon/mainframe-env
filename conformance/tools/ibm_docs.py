#!/usr/bin/env python3
"""Read digest-pinned IBM publication bodies from an external offline cache.

The tool combines the immutable 0.2 baseline index with explicitly registered
later source-review manifests. It imports existing cache bytes and provides
bounded status, search, and read operations. It never fetches, repins, or
grants semantic or licensed-execution coverage.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass
from html.parser import HTMLParser
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import tempfile
from typing import BinaryIO, Iterable
import urllib.parse

import docs_api


INDEX = docs_api.REPOSITORY / "conformance/subsystems/coverage/catalogs/index.json"
REGISTRY = docs_api.REPOSITORY / "conformance/subsystems/cics/application/manifests/index.json"
ADDITIONAL_REGISTRIES = (
    docs_api.REPOSITORY / "conformance/subsystems/ims/manifests/index.json",
    docs_api.REPOSITORY / "conformance/subsystems/mq/manifests/index.json",
)
CONTENT_TEMPLATE = docs_api.CONTENT_URL
MAX_FILE = 64 * 1024 * 1024
MAX_IMPORT = 512 * 1024 * 1024
MAX_ARCHIVE_ENTRIES = 10_000
MAX_QUERY_CHARS = 200
MAX_HEADING_CHARS = 500
MAX_TEXT_LINE_CHARS = 4_000
HEX = re.compile(r"^[0-9a-f]{64}$")


@dataclass(frozen=True, order=True)
class Scope:
    scope_id: str
    subsystem: str
    baseline: str
    target_subsystem: str
    manifest: str


def cache_key(kind: str, locator: str, sha256: str, suffix: str) -> str:
    """Return a bounded path-and-content-addressed flat cache key."""
    locator_sha256 = docs_api.digest(locator.encode())
    return f"{kind}-{locator_sha256}-sha256-{sha256}{suffix}"


@dataclass(frozen=True)
class Pin:
    topic: str
    url: str
    sha256: str
    size: int
    scopes: tuple[Scope, ...]

    @property
    def key(self) -> str:
        return cache_key("topic", self.topic, self.sha256, ".html")

    @property
    def legacy_key(self) -> str:
        return docs_api._key(self.topic.split("?", 1)[0])

    @property
    def subsystem(self) -> str:
        return ",".join(sorted({scope.subsystem for scope in self.scopes}))

    @property
    def baseline(self) -> str:
        return ",".join(sorted({scope.baseline for scope in self.scopes}))


@dataclass(frozen=True)
class TocPin:
    url: str
    sha256: str
    scopes: tuple[Scope, ...]

    @property
    def key(self) -> str:
        return cache_key("toc", self.url, self.sha256, ".json")

    @property
    def legacy_key(self) -> str:
        return "toc-" + docs_api._key(self.url) + ".json"


@dataclass(frozen=True)
class CacheTarget:
    key: str
    legacy_key: str
    sha256: str
    size: int | None
    label: str


def safe_relative_manifest(value: object, prefix: str, owner: str) -> str:
    if not isinstance(value, str) or "\\" in value:
        raise ValueError(f"unsafe manifest path in {owner}")
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or path.as_posix() != value
        or any(part in {"", ".", ".."} for part in path.parts)
        or not value.startswith(prefix)
        or len(path.parts) != len(PurePosixPath(prefix).parts) + 1
        or path.suffix != ".json"
    ):
        raise ValueError(f"unsafe manifest path in {owner}")
    return value


def safe_topic_path(value: object, product: str, owner: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise ValueError(f"unsafe topic path in {owner}")
    parts = value.split("?", 1)
    clean = parts[0]
    if len(parts) == 2 and re.fullmatch(r"pos=[1-9][0-9]*", parts[1]) is None:
        raise ValueError(f"unsafe topic navigation suffix in {owner}")
    path = PurePosixPath(clean)
    if (
        path.is_absolute()
        or path.as_posix() != clean
        or any(part in {"", ".", ".."} for part in path.parts)
        or len(path.parts) < 2
        or path.parts[0] != product
        or path.suffix not in {".htm", ".html"}
    ):
        raise ValueError(f"unsafe topic path in {owner}")
    return value


def official_toc_url(value: object, owner: str) -> str:
    if not isinstance(value, str):
        raise ValueError(f"invalid TOC URL in {owner}")
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme != "https"
        or parsed.netloc != "www.ibm.com"
        or not parsed.path.startswith("/docs/api/v1/toc/")
        or urllib.parse.parse_qsl(parsed.query, keep_blank_values=True) != [("lang", "en")]
        or parsed.fragment
    ):
        raise ValueError(f"invalid TOC URL in {owner}")
    return value


def read_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain an object")
    return value


def validate_manifest(
    manifest: dict,
    path: Path,
    *,
    target_subsystem: str,
    baseline: str,
    subsystem: str,
) -> tuple[list[dict], str, str]:
    topics = manifest.get("topics")
    if not isinstance(topics, list) or not topics:
        raise ValueError(f"manifest has no topics: {path}")
    product = manifest.get("product")
    toc_url = official_toc_url(manifest.get("toc_url"), str(path))
    toc_sha256 = manifest.get("toc_sha256")
    if (
        manifest.get("schema_version") != "mainframe-env.topic-manifest@1"
        or manifest.get("target_subsystem") != target_subsystem
        or manifest.get("baseline_id") != baseline
        or manifest.get("subsystem") != subsystem
        or not isinstance(product, str)
        or len(product) < 2
        or manifest.get("content_url_template") != CONTENT_TEMPLATE
        or not isinstance(toc_sha256, str)
        or HEX.fullmatch(toc_sha256) is None
        or manifest.get("coverage_credit") != 0
        or manifest.get("retained_in_repository") is not False
        or manifest.get("topic_manifest_digest_definition") != docs_api.DIGEST_DEFINITION
    ):
        raise ValueError(f"manifest identity is invalid: {path}")
    safe_topic_path(manifest.get("book_href"), product, str(path))
    normalized: list[dict] = []
    seen: set[str] = set()
    total = 0
    for raw in topics:
        if not isinstance(raw, dict):
            raise ValueError(f"manifest topic is not an object: {path}")
        if set(raw) != {"topic_path", "sha256", "bytes", "last_modified"}:
            raise ValueError(f"manifest topic fields differ: {path}")
        topic = safe_topic_path(raw.get("topic_path"), product, str(path))
        sha256 = raw.get("sha256")
        size = raw.get("bytes")
        last_modified = raw.get("last_modified")
        if (
            topic in seen
            or not isinstance(sha256, str)
            or HEX.fullmatch(sha256) is None
            or not isinstance(size, int)
            or isinstance(size, bool)
            or not 0 < size <= MAX_FILE
            or not isinstance(last_modified, str)
            or len(last_modified) < 4
        ):
            raise ValueError(f"invalid or duplicate topic pin in {path}")
        seen.add(topic)
        total += size
        normalized.append({"topic_path": topic, "sha256": sha256, "bytes": size})
    digest = docs_api.manifest_digest(normalized)
    if (
        len(normalized) != manifest.get("topic_count")
        or total != manifest.get("total_bytes")
        or digest != manifest.get("topic_manifest_digest")
    ):
        raise ValueError(f"manifest counts or digest disagree: {path}")
    return normalized, toc_url, toc_sha256


def baseline_sources(index: Path) -> list[tuple[Scope, dict, list[dict], str, str]]:
    document = read_json(index)
    baselines = document.get("baselines")
    if not isinstance(baselines, list) or not baselines:
        raise ValueError(f"source index contains no baselines: {index}")
    result = []
    for baseline in baselines:
        source = baseline.get("source", {})
        baseline_id = baseline.get("id")
        subsystem = baseline.get("subsystem")
        relative = safe_relative_manifest(
            source.get("manifest"), "conformance/subsystems/coverage/manifests/", str(index)
        )
        path = docs_api.REPOSITORY / relative
        manifest = read_json(path)
        topics, toc_url, toc_sha256 = validate_manifest(
            manifest,
            path,
            target_subsystem="coverage.foundation",
            baseline=baseline_id,
            subsystem=subsystem,
        )
        digest = manifest["topic_manifest_digest"]
        if (
            source.get("kind") != "documentation-topics"
            or source.get("product") != manifest.get("product")
            or source.get("book_href") != manifest.get("book_href")
            or source.get("url") != toc_url
            or source.get("toc_sha256") != "sha256:" + toc_sha256
            or source.get("content_url_template") != CONTENT_TEMPLATE
            or source.get("topic_count") != len(topics)
            or source.get("bytes") != manifest.get("total_bytes")
            or source.get("sha256") != "sha256:" + digest
        ):
            raise ValueError(f"manifest disagrees with baseline: {baseline_id}")
        scope = Scope(baseline_id, subsystem, baseline_id, "coverage.foundation", relative)
        result.append((scope, manifest, topics, toc_url, toc_sha256))
    return result


def registered_sources(
    registry: Path,
) -> list[tuple[Scope, dict, list[dict], str, str]]:
    document = read_json(registry)
    entries = document.get("manifests")
    target_subsystem = document.get("target_subsystem")
    if (
        document.get("schema_version") != "mainframe-env.topic-manifest-registry@1"
        or not isinstance(target_subsystem, str)
        or re.fullmatch(r"[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)?", target_subsystem) is None
        or document.get("semantic_authority") is not False
        or document.get("coverage_credit") != 0
        or not isinstance(entries, list)
        or not entries
    ):
        raise ValueError(f"invalid topic-manifest registry: {registry}")
    result = []
    scope_ids: set[str] = set()
    paths: set[str] = set()
    prefix = str(registry.parent.relative_to(docs_api.REPOSITORY)) + "/"
    for entry in entries:
        scope_id = entry.get("scope_id")
        subsystem = entry.get("subsystem")
        baseline = entry.get("baseline_id")
        if (
            not isinstance(scope_id, str)
            or not scope_id
            or scope_id in scope_ids
            or not isinstance(subsystem, str)
            or not subsystem
            or not isinstance(baseline, str)
            or not baseline
        ):
            raise ValueError(f"invalid or duplicate scope in {registry}")
        relative = safe_relative_manifest(
            entry.get("manifest"), prefix, str(registry)
        )
        if relative.endswith("/index.json") or relative in paths:
            raise ValueError(f"invalid or duplicate manifest in {registry}")
        scope_ids.add(scope_id)
        paths.add(relative)
        path = docs_api.REPOSITORY / relative
        manifest_bytes = path.read_bytes()
        manifest = read_json(path)
        topics, toc_url, toc_sha256 = validate_manifest(
            manifest,
            path,
            target_subsystem=target_subsystem,
            baseline=baseline,
            subsystem=subsystem,
        )
        if (
            entry.get("manifest_sha256")
            != "sha256:" + docs_api.digest(manifest_bytes)
            or entry.get("topic_manifest_sha256")
            != "sha256:" + manifest["topic_manifest_digest"]
            or entry.get("topic_count") != len(topics)
            or entry.get("coverage_credit") != 0
            or entry.get("semantic_authority") is not False
        ):
            raise ValueError(f"registry entry disagrees with manifest: {scope_id}")
        scope = Scope(scope_id, subsystem, baseline, target_subsystem, relative)
        result.append((scope, manifest, topics, toc_url, toc_sha256))
    present = {
        str(path.relative_to(docs_api.REPOSITORY))
        for path in registry.parent.glob("*.json")
        if path.name != "index.json"
    }
    if present != paths:
        raise ValueError(f"{target_subsystem} topic manifests are unregistered or missing")
    return result


def load_pins(
    index: Path = INDEX, registry: Path = REGISTRY
) -> tuple[list[Pin], list[TocPin]]:
    """Load, coalesce, and collision-check every explicitly owned source pin."""
    registries = [registry]
    if registry == REGISTRY:
        registries.extend(ADDITIONAL_REGISTRIES)
    sources = baseline_sources(index)
    for later_registry in registries:
        sources.extend(registered_sources(later_registry))
    scope_ids: set[str] = set()
    baseline_signatures: dict[str, tuple] = {}
    for scope, manifest, topics, toc_url, toc_sha256 in sources:
        if scope.scope_id in scope_ids:
            raise ValueError(f"duplicate global source scope: {scope.scope_id}")
        scope_ids.add(scope.scope_id)
        signature = (
            scope.subsystem,
            scope.target_subsystem,
            manifest["product"],
            manifest["book_href"],
            toc_url,
            toc_sha256,
            manifest["content_url_template"],
            tuple(
                (topic["topic_path"], topic["sha256"], topic["bytes"])
                for topic in topics
            ),
        )
        prior = baseline_signatures.setdefault(scope.baseline, signature)
        if prior != signature:
            raise ValueError(f"conflicting immutable baseline: {scope.baseline}")
    raw_pins: dict[tuple[str, str, int, str], set[Scope]] = defaultdict(set)
    raw_tocs: dict[tuple[str, str], set[Scope]] = defaultdict(set)
    for scope, manifest, topics, toc_url, toc_sha256 in sources:
        for topic in topics:
            url = docs_api.content_url(topic["topic_path"], manifest["content_url_template"])
            raw_pins[(topic["topic_path"], topic["sha256"], topic["bytes"], url)].add(scope)
        raw_tocs[(toc_url, toc_sha256)].add(scope)
    pins = [
        Pin(topic, url, sha256, size, tuple(sorted(scopes)))
        for (topic, sha256, size, url), scopes in sorted(raw_pins.items())
    ]
    tocs = [
        TocPin(url, sha256, tuple(sorted(scopes)))
        for (url, sha256), scopes in sorted(raw_tocs.items())
    ]
    expected: dict[str, tuple[str, int | None]] = {}
    for item in [*pins, *tocs]:
        size = item.size if isinstance(item, Pin) else None
        prior = expected.setdefault(item.key, (item.sha256, size))
        if prior != (item.sha256, size):
            raise ValueError(f"conflicting content-addressed cache key: {item.key}")
    return pins, tocs


def pin_target(pin: Pin) -> CacheTarget:
    return CacheTarget(pin.key, pin.legacy_key, pin.sha256, pin.size, pin.topic)


def toc_target(pin: TocPin) -> CacheTarget:
    return CacheTarget(pin.key, pin.legacy_key, pin.sha256, None, pin.url)


def cached_bytes(cache: Path, target: CacheTarget) -> bytes:
    directory = docs_api.outside_repository(cache)
    for path in [directory / target.key, directory / target.legacy_key]:
        if path.is_symlink():
            raise ValueError(f"cache symlink refused: {target.label}")
        if not path.exists():
            continue
        if not path.is_file() or path.stat().st_size > MAX_FILE:
            raise ValueError(f"cache size mismatch: {target.label}")
        if target.size is not None and path.stat().st_size != target.size:
            raise ValueError(f"cache size mismatch: {target.label}")
        body = read_bounded(path)
        if docs_api.digest(body) != target.sha256:
            raise ValueError(f"cache SHA-256 mismatch: {target.label}")
        return body
    raise FileNotFoundError(target.label)


def cached_body(cache: Path, pin: Pin) -> bytes:
    return cached_bytes(cache, pin_target(pin))


def cached_toc(cache: Path, pin: TocPin) -> bytes:
    return cached_bytes(cache, toc_target(pin))


def read_bounded(path: Path) -> bytes:
    with path.open("rb") as stream:
        body = stream.read(MAX_FILE + 1)
    if len(body) > MAX_FILE:
        raise ValueError(f"cache file exceeds bounded read limit: {path.name}")
    return body


class PlainText(HTMLParser):
    """Readable excerpts with block boundaries, excluding active HTML content."""

    BLOCKS = {"p", "div", "li", "tr", "pre", "h1", "h2", "h3", "h4", "br"}

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []
        self.hidden = 0

    def handle_starttag(self, tag: str, attrs: list) -> None:
        if tag in {"script", "style"}:
            self.hidden += 1
        if not self.hidden and tag in self.BLOCKS:
            self.parts.append("\n")

    def handle_endtag(self, tag: str) -> None:
        if tag in {"script", "style"}:
            self.hidden = max(0, self.hidden - 1)
        if not self.hidden and tag in self.BLOCKS:
            self.parts.append("\n")
        elif not self.hidden and tag in {"td", "th"}:
            self.parts.append(" | ")

    def handle_data(self, data: str) -> None:
        if not self.hidden:
            self.parts.append(data)


def plain_text(body: bytes) -> list[str]:
    parser = PlainText()
    parser.feed(body.decode("utf-8", "replace"))
    lines = []
    for line in "".join(parser.parts).splitlines():
        clean = " ".join(line.split())
        if clean:
            lines.append(clean[:MAX_TEXT_LINE_CHARS])
    return lines


def publish(cache: Path, target: CacheTarget, body: bytes, counts: Counter[str]) -> None:
    destination = cache / target.key
    if destination.is_symlink():
        counts["rejected_conflict"] += 1
        return
    if destination.exists():
        if existing_matches(destination, target, body):
            counts["already_present"] += 1
        else:
            counts["rejected_conflict"] += 1
        return
    cache.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".import-", dir=cache) as staging:
        staged = docs_api.write_retrieved(Path(staging) / target.key, body)
        try:
            os.link(staged, destination)
        except FileExistsError:
            if existing_matches(destination, target, body):
                counts["already_present"] += 1
            else:
                counts["rejected_conflict"] += 1
            return
    counts["imported"] += 1


def existing_matches(destination: Path, target: CacheTarget, body: bytes) -> bool:
    if destination.is_symlink() or not destination.is_file():
        return False
    size = destination.stat().st_size
    if (
        size > MAX_FILE
        or size != len(body)
        or (target.size is not None and size != target.size)
    ):
        return False
    try:
        return read_bounded(destination) == body
    except (OSError, ValueError):
        return False


def import_cache(
    stream: BinaryIO, cache: Path, pins: list[Pin], tocs: list[TocPin]
) -> Counter[str]:
    """Import a complete selected scope from flat legacy or addressed entries."""
    cache = docs_api.outside_repository(cache)
    targets = [*(pin_target(pin) for pin in pins), *(toc_target(pin) for pin in tocs)]
    unique_targets = {target.key: target for target in targets}
    if len(unique_targets) != len(targets):
        raise ValueError("duplicate selected cache targets")
    if (
        len(unique_targets) > MAX_ARCHIVE_ENTRIES
        or sum(target.size or 0 for target in unique_targets.values()) > MAX_IMPORT
    ):
        raise ValueError("selected cache scope exceeds bounded import count or byte limits")
    archive_names: dict[str, list[CacheTarget]] = defaultdict(list)
    for target in targets:
        archive_names[target.key].append(target)
        archive_names[target.legacy_key].append(target)
    counts: Counter[str] = Counter()
    total = 0
    with tarfile.open(fileobj=stream, mode="r|") as archive:
        for count, member in enumerate(archive, 1):
            total += member.size
            if (
                count > MAX_ARCHIVE_ENTRIES
                or member.size < 0
                or member.size > MAX_FILE
                or total > MAX_IMPORT
            ):
                raise ValueError("cache import exceeds bounded file/count/byte limit")
            name = member.name.removeprefix("./")
            if member.isdir():
                continue
            candidates = archive_names.get(name, [])
            if not member.isfile() or "/" in name or not candidates:
                counts["skipped_unrecognized"] += 1
                continue
            reader = archive.extractfile(member)
            if reader is None:
                counts["rejected_mismatch"] += 1
                continue
            body = reader.read(MAX_FILE + 1)
            digest = docs_api.digest(body)
            matches = [
                target
                for target in candidates
                if target.sha256 == digest
                and (target.size is None or target.size == len(body))
            ]
            if not matches:
                counts["rejected_mismatch"] += 1
                continue
            for target in {match.key: match for match in matches}.values():
                publish(cache, target, body, counts)
    for target in unique_targets.values():
        try:
            cached_bytes(cache, target)
        except FileNotFoundError:
            counts["missing_expected"] += 1
        except ValueError:
            counts["mismatch_expected"] += 1
    return counts


def has_scope(scopes: Iterable[Scope], scope_id: str | None, subsystem: str | None) -> bool:
    return any(
        (scope_id is None or scope.scope_id == scope_id)
        and (subsystem is None or scope.subsystem == subsystem)
        for scope in scopes
    )


def select(
    pins: list[Pin],
    tocs: list[TocPin],
    scope_id: str | None,
    subsystem: str | None,
) -> tuple[list[Pin], list[TocPin]]:
    known_scopes = {scope.scope_id for pin in [*pins, *tocs] for scope in pin.scopes}
    known_subsystems = {scope.subsystem for pin in [*pins, *tocs] for scope in pin.scopes}
    if scope_id is not None and scope_id not in known_scopes:
        raise ValueError(f"unknown source scope: {scope_id}")
    if subsystem is not None and subsystem not in known_subsystems:
        raise ValueError(f"unknown subsystem: {subsystem}")
    selected_pins = [pin for pin in pins if has_scope(pin.scopes, scope_id, subsystem)]
    selected_tocs = [pin for pin in tocs if has_scope(pin.scopes, scope_id, subsystem)]
    if not selected_pins or not selected_tocs:
        raise ValueError("selected source scope has no topic or TOC pins")
    return selected_pins, selected_tocs


def add_filters(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--scope")
    parser.add_argument("--subsystem")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    sub = parser.add_subparsers(dest="command", required=True)
    importer = sub.add_parser("import", help="import a verified complete cache scope")
    add_filters(importer)
    status = sub.add_parser("status")
    add_filters(status)
    search = sub.add_parser("search")
    add_filters(search)
    search.add_argument("query")
    search.add_argument("--limit", type=int, default=10)
    reader = sub.add_parser("read")
    add_filters(reader)
    reader.add_argument("topic")
    reader.add_argument("--sha256")
    reader.add_argument("--start-line", type=int, default=1)
    reader.add_argument("--lines", type=int, default=60)
    args = parser.parse_args(argv)

    cache = args.cache or docs_api.default_cache()
    pins, tocs = select(*load_pins(), args.scope, args.subsystem)
    if args.command == "import":
        counts = import_cache(sys.stdin.buffer, cache, pins, tocs)
        print(json.dumps({"cache": str(cache), **counts}, sort_keys=True))
        failures = sum(
            counts[key]
            for key in [
                "rejected_mismatch",
                "rejected_conflict",
                "missing_expected",
                "mismatch_expected",
            ]
        )
        return int(bool(failures))

    if args.command == "read":
        if args.start_line < 1 or not 1 <= args.lines <= 200:
            parser.error("start-line must be positive; lines must be between 1 and 200")
        wanted_digest = (args.sha256 or "").removeprefix("sha256:")
        if wanted_digest and HEX.fullmatch(wanted_digest) is None:
            parser.error("sha256 must be 64 lowercase hexadecimal characters")
        matches = [
            pin
            for pin in pins
            if pin.topic == args.topic and (not wanted_digest or pin.sha256 == wanted_digest)
        ]
        if len(matches) != 1:
            parser.error(
                "topic must identify exactly one pinned snapshot; use search and --sha256"
            )
        pin = matches[0]
        relevant_scope_ids = {scope.scope_id for scope in pin.scopes}
        relevant_tocs = [
            toc
            for toc in tocs
            if any(scope.scope_id in relevant_scope_ids for scope in toc.scopes)
        ]
        if not relevant_tocs:
            raise ValueError("selected topic has no pinned TOC")
        try:
            for toc in relevant_tocs:
                cached_toc(cache, toc)
        except (FileNotFoundError, ValueError) as error:
            print(f"IBM docs: relevant TOC is not verified: {error}", file=sys.stderr)
            return 1
        body = cached_body(cache, pin)
        scopes = ",".join(scope.scope_id for scope in pin.scopes)
        print(f"Verified scopes: {scopes}\nTopic: {pin.topic}\nURL: {pin.url}")
        print(f"SHA-256: {pin.sha256}\nCache: {cache / pin.key}")
        print("Reference text only; semantic and licensed execution coverage credit: 0.")
        lines = plain_text(body)
        stop = min(len(lines), args.start_line + args.lines - 1)
        for number in range(args.start_line, stop + 1):
            print(f"{number}: {lines[number - 1]}")
        print(f"Excerpt: lines {args.start_line}–{stop} of {len(lines)}")
        return 0

    if args.command == "search" and (
        not args.query.strip()
        or len(args.query) > MAX_QUERY_CHARS
        or not 1 <= args.limit <= 50
    ):
        parser.error("query must be 1-200 characters; limit must be between 1 and 50")

    topic_states: Counter[str] = Counter()
    toc_states: Counter[str] = Counter()
    results: list[tuple[int, str, str, str, str, str]] = []
    for pin in pins:
        try:
            body = cached_body(cache, pin)
        except FileNotFoundError:
            topic_states["missing"] += 1
            continue
        except ValueError:
            topic_states["mismatch"] += 1
            continue
        topic_states["verified"] += 1
        if args.command == "search":
            heading = (
                docs_api.heading_of(body.decode("utf-8", "replace")) or pin.topic
            )[:MAX_HEADING_CHARS]
            query = args.query.casefold()
            rank = (
                0
                if query in heading.casefold()
                else 1
                if query in pin.topic.casefold()
                else 2
                if query in " ".join(plain_text(body)).casefold()
                else 3
            )
            if rank < 3:
                scopes = ",".join(scope.scope_id for scope in pin.scopes)
                results.append((rank, pin.topic, pin.subsystem, heading, pin.sha256, scopes))
    for pin in tocs:
        try:
            cached_toc(cache, pin)
        except FileNotFoundError:
            toc_states["missing"] += 1
        except ValueError:
            toc_states["mismatch"] += 1
        else:
            toc_states["verified"] += 1
    print(
        json.dumps(
            {"cache": str(cache), "topics": topic_states, "tocs": toc_states},
            sort_keys=True,
        )
    )
    if args.command == "search":
        for _, topic, subsystem, heading, sha256, scopes in sorted(results)[: args.limit]:
            print(
                f"[{subsystem}] {heading}\n  {topic}\n  sha256:{sha256}\n  scopes:{scopes}"
            )
        print(f"{len(results)} matching verified topics; showing at most {args.limit}.")
    failures = sum(
        states[key]
        for states in [topic_states, toc_states]
        for key in ["missing", "mismatch"]
    )
    if args.command == "search" and not results:
        return 1
    return int(bool(failures))


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, tarfile.TarError) as error:
        raise SystemExit(f"IBM docs: {error}")
