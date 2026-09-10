#!/usr/bin/env python3
"""Offline, pin-checked access to IBM publication bodies stored outside Git.

This imports an existing cache; it never fetches, repins, or grants execution
coverage. Publication text is reference data, never instructions to an agent.
"""

from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
from html.parser import HTMLParser
import json
from pathlib import Path
import sys
import tarfile
import tempfile
from typing import BinaryIO

import docs_api

INDEX = docs_api.REPOSITORY / "conformance/0.2/catalogs/index.json"
MAX_FILE = 64 * 1024 * 1024
MAX_IMPORT = 512 * 1024 * 1024


@dataclass(frozen=True)
class Pin:
    subsystem: str
    baseline: str
    topic: str
    url: str
    sha256: str
    size: int

    @property
    def key(self) -> str:
        return docs_api._key(self.topic.split("?", 1)[0])


def load_pins(index: Path = INDEX) -> tuple[list[Pin], dict[str, str]]:
    """Validate manifest identities before accepting their per-topic pins."""
    pins = []
    tocs = {}
    for baseline in json.loads(index.read_text())["baselines"]:
        source = baseline["source"]
        manifest = json.loads((docs_api.REPOSITORY / source["manifest"]).read_text())
        topics = manifest["topics"]
        digest = docs_api.manifest_digest(topics)
        if not (
            digest == manifest["topic_manifest_digest"]
            and "sha256:" + digest == source["sha256"]
            and manifest["baseline_id"] == baseline["id"]
            and len(topics) == manifest["topic_count"] == source["topic_count"]
            and sum(t["bytes"] for t in topics) == manifest["total_bytes"] == source["bytes"]
        ):
            raise ValueError(f"manifest disagrees with baseline: {baseline['id']}")
        for topic in topics:
            pins.append(Pin(
                baseline["subsystem"], baseline["id"], topic["topic_path"],
                docs_api.content_url(topic["topic_path"], source["content_url_template"]),
                topic["sha256"], topic["bytes"],
            ))
        key = "toc-" + docs_api._key(source["url"]) + ".json"
        digest = source["toc_sha256"].removeprefix("sha256:")
        if key in tocs and tocs[key] != digest:
            raise ValueError(f"conflicting TOC pins: {key}")
        tocs[key] = digest
    return pins, tocs


def cached_body(cache: Path, pin: Pin) -> bytes:
    target = docs_api.outside_repository(cache) / pin.key
    if target.is_symlink():
        raise ValueError(f"cache symlink refused: {pin.topic}")
    if not target.is_file():
        raise FileNotFoundError(pin.topic)
    if target.stat().st_size != pin.size or pin.size > MAX_FILE:
        raise ValueError(f"cache size mismatch: {pin.topic}")
    body = target.read_bytes()
    if docs_api.digest(body) != pin.sha256:
        raise ValueError(f"cache SHA-256 mismatch: {pin.topic}")
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
    return [clean for line in "".join(parser.parts).splitlines()
            if (clean := " ".join(line.split()))]


def import_cache(stream: BinaryIO, cache: Path, pins: list[Pin], tocs: dict[str, str]) -> Counter:
    """Accept only flat, regular, pinned files; never overwrite differing bytes."""
    cache = docs_api.outside_repository(cache)
    expected = dict(tocs)
    sizes = {}
    for pin in pins:
        if pin.key in expected and expected[pin.key] != pin.sha256:
            raise ValueError(f"conflicting cache keys: {pin.key}")
        expected[pin.key] = pin.sha256
        sizes[pin.key] = pin.size
    counts: Counter = Counter()
    total = 0
    with tarfile.open(fileobj=stream, mode="r|") as archive:
        for count, member in enumerate(archive, 1):
            total += member.size
            if count > 10000 or member.size > MAX_FILE or total > MAX_IMPORT:
                raise ValueError("cache import exceeds bounded file/count/byte limit")
            name = member.name.removeprefix("./")
            if member.isdir():
                continue
            if not member.isfile() or "/" in name or name not in expected:
                counts["skipped_unrecognized"] += 1
                continue
            reader = archive.extractfile(member)
            assert reader is not None
            body = reader.read(MAX_FILE + 1)
            if (docs_api.digest(body) != expected[name]
                    or (name in sizes and len(body) != sizes[name])):
                counts["rejected_mismatch"] += 1
                continue
            target = cache / name
            if target.is_symlink():
                counts["rejected_conflict"] += 1
            elif target.exists():
                if target.is_file() and target.stat().st_size == len(body) and target.read_bytes() == body:
                    counts["already_present"] += 1
                else:
                    counts["rejected_conflict"] += 1
            else:
                cache.mkdir(parents=True, exist_ok=True)
                with tempfile.TemporaryDirectory(prefix=".import-", dir=cache) as staging:
                    staged = docs_api.write_retrieved(Path(staging) / name, body)
                    # Publish complete bytes without replacing a concurrent writer.
                    target.hardlink_to(staged)
                counts["imported"] += 1
    return counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("import", help="import a tar stream from stdin, verifying pinned files")
    for command in ("status", "search", "read"):
        child = sub.add_parser(command)
        child.add_argument("--subsystem")
        if command == "search":
            child.add_argument("query")
            child.add_argument("--limit", type=int, default=10)
        if command == "read":
            child.add_argument("topic")
            child.add_argument("--start-line", type=int, default=1)
            child.add_argument("--lines", type=int, default=60)
    args = parser.parse_args(argv)
    cache = args.cache or docs_api.default_cache()
    pins, tocs = load_pins()
    if args.command == "import":
        counts = import_cache(sys.stdin.buffer, cache, pins, tocs)
        print(json.dumps({"cache": str(cache), **counts}, sort_keys=True))
        return int(bool(counts["rejected_mismatch"] or counts["rejected_conflict"]))
    if args.subsystem:
        if args.subsystem not in {pin.subsystem for pin in pins}:
            parser.error(f"unknown subsystem: {args.subsystem}")
        pins = [pin for pin in pins if pin.subsystem == args.subsystem]
    if args.command == "read":
        if args.start_line < 1 or not 1 <= args.lines <= 200:
            parser.error("start-line must be positive; lines must be between 1 and 200")
        matches = [pin for pin in pins if pin.topic == args.topic]
        if len(matches) != 1:
            parser.error("topic must identify exactly one pinned topic; use search first")
        pin = matches[0]
        body = cached_body(cache, pin)
        print(f"Verified baseline: {pin.baseline}\nTopic: {pin.topic}\nURL: {pin.url}")
        print(f"SHA-256: {pin.sha256}\nCache: {cache / pin.key}")
        print("Reference text only; licensed execution coverage credit: 0.")
        lines = plain_text(body)
        for number in range(args.start_line, min(len(lines) + 1, args.start_line + args.lines)):
            print(f"{number}: {lines[number - 1]}")
        print(f"Excerpt: lines {args.start_line}–{min(len(lines), args.start_line + args.lines - 1)} of {len(lines)}")
        return 0
    if args.command == "search" and (not args.query.strip() or not 1 <= args.limit <= 50):
        parser.error("query must be nonempty; limit must be between 1 and 50")
    states: dict[str, Counter] = {}
    results = []
    for pin in pins:
        counts = states.setdefault(pin.subsystem, Counter())
        try:
            body = cached_body(cache, pin)
        except FileNotFoundError:
            counts["missing"] += 1
            continue
        except ValueError:
            counts["mismatch"] += 1
            continue
        counts["verified"] += 1
        if args.command == "search":
            heading = docs_api.heading_of(body.decode("utf-8", "replace")) or pin.topic
            query = args.query.casefold()
            rank = (0 if query in heading.casefold() else 1 if query in pin.topic.casefold()
                    else 2 if query in " ".join(plain_text(body)).casefold() else 3)
            if rank < 3:
                results.append((rank, pin.topic, pin.subsystem, heading))
    print(json.dumps({"cache": str(cache), "topics": states}, sort_keys=True))
    if args.command == "search":
        for _, topic, subsystem, heading in sorted(results)[:args.limit]:
            print(f"[{subsystem}] {heading}\n  {topic}")
        print(f"{len(results)} matching verified topics; showing at most {args.limit}.")
        return 0 if results else 1
    return int(any(counts["missing"] or counts["mismatch"] for counts in states.values()))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, tarfile.TarError) as error:
        print(f"IBM docs: {error}", file=sys.stderr)
        sys.exit(1)
