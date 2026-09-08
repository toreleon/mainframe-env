#!/usr/bin/env python3
"""Compile a bounded CICS documentation corpus into review-only rule candidates.

The input directory contains IBM topic bytes and must remain outside the
repository.  The committed output retains structural locators, fragment
digests, classifications, semantic cues and reviewer-authored interpretations;
it never retains the publication text and grants zero coverage credit.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from typing import Iterable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
import docs_api  # noqa: E402

MAX_TOPIC_BYTES = 2 * 1024 * 1024
MAX_TOPICS = 32
MAX_FRAGMENTS = 20_000
BLOCKS = frozenset({"h2", "p", "li", "dt", "dd", "tr"})


@dataclass(eq=False)
class Node:
    tag: str
    attrs: dict[str, str]
    parent: "Node | None" = None
    children: list["Node"] = field(default_factory=list)
    chunks: list[str] = field(default_factory=list)

    def text(self, exclude_lists: bool = False) -> str:
        values = list(self.chunks)
        for child in self.children:
            if exclude_lists and child.tag in {"dl", "ul", "ol"}:
                continue
            values.append(child.text(exclude_lists))
        return normalize(" ".join(values))


class TopicParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = Node("root", {})
        self.stack = [self.root]

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        node = Node(tag.lower(), {key: value or "" for key, value in attrs}, self.stack[-1])
        self.stack[-1].children.append(node)
        self.stack.append(node)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        node = Node(tag.lower(), {key: value or "" for key, value in attrs}, self.stack[-1])
        self.stack[-1].children.append(node)

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == tag:
                del self.stack[index:]
                return

    def handle_data(self, data: str) -> None:
        self.stack[-1].chunks.append(data)


def normalize(value: str) -> str:
    return " ".join(html.unescape(value).split())


def digest(value: bytes | str) -> str:
    data = value.encode("utf-8") if isinstance(value, str) else value
    return hashlib.sha256(data).hexdigest()


def walk(node: Node) -> Iterable[Node]:
    for child in node.children:
        yield child
        yield from walk(child)


def ancestor(node: Node, tags: set[str]) -> Node | None:
    current = node.parent
    while current is not None:
        if current.tag in tags:
            return current
        current = current.parent
    return None


def fragment_node(node: Node) -> bool:
    return node.tag in BLOCKS or (
        node.tag == "div" and "p" in node.attrs.get("class", "").split()
    )


def sibling_ordinal(node: Node) -> int:
    if node.parent is None:
        return 1
    ordinal = 1
    for child in node.parent.children:
        if child is node:
            break
        if child.tag == node.tag:
            ordinal += 1
    return ordinal


def structural_path(node: Node) -> str:
    parts: list[str] = []
    current: Node | None = node
    while current is not None and current.tag != "root":
        identity = current.attrs.get("id")
        parts.append(f"{current.tag}#{identity}" if identity else f"{current.tag}[{sibling_ordinal(current)}]")
        current = current.parent
    return "/".join(reversed(parts))


def references(node: Node) -> list[str]:
    found = {
        child.attrs["href"]
        for child in walk(node)
        if child.tag == "a" and child.attrs.get("href")
    }
    return sorted(found)


def condition_stack(node: Node) -> list[str]:
    values: list[str] = []
    current: Node | None = node
    while current is not None:
        if current.tag == "dd" and current.parent is not None:
            siblings = current.parent.children
            position = siblings.index(current)
            prior = next((item for item in reversed(siblings[:position]) if item.tag == "dt"), None)
            if prior is not None:
                value = prior.text(exclude_lists=True)
                if value:
                    values.append(value)
        current = current.parent
    return list(reversed(values))


def semantic_cues(text: str, node: Node) -> dict[str, object]:
    lower = text.casefold()
    return {
        "negated": bool(re.search(r"\b(no|not|never|without|cannot|must not)\b", lower)),
        "only_when": "only when" in lower,
        "unless": "unless" in lower,
        "condition_stack": condition_stack(node),
        "references": references(node),
    }


def selected_fragments(root: Node, section_ids: set[str]) -> list[tuple[str, Node, str]]:
    current_section = "__lead__"
    result: list[tuple[str, Node, str]] = []
    for node in walk(root):
        if node.tag == "h2":
            current_section = node.attrs.get("id", "__idless_h2__")
        if current_section not in section_ids or not fragment_node(node):
            continue
        if node.tag == "p" or (
            node.tag == "div" and "p" in node.attrs.get("class", "").split()
        ):
            current = node.parent
            nested = False
            while current is not None:
                if current.tag in {"p", "li", "dt", "dd"}:
                    nested = True
                    break
                current = current.parent
            if nested:
                continue
        text = node.text(
            exclude_lists=node.tag in {"li", "dd"}
            or (node.tag == "div" and "p" in node.attrs.get("class", "").split())
        )
        if text:
            result.append((current_section, node, text))
    return result


def rule_matches(rule: dict[str, object], topic_path: str, section: str, text: str) -> bool:
    if rule["topic_path"] != topic_path or section not in rule["sections"]:
        return False
    flags = re.IGNORECASE
    return all(re.search(pattern, text, flags) for pattern in rule.get("all", [])) and (
        not rule.get("any") or any(re.search(pattern, text, flags) for pattern in rule["any"])
    )


def compile_topic(topic: dict[str, object], body: bytes, config: dict[str, object]) -> list[dict[str, object]]:
    parser = TopicParser()
    parser.feed(body.decode("utf-8", "replace"))
    topic_path = str(topic["topic_path"])
    source = next(item for item in config["sources"] if item["topic_path"] == topic_path)
    wanted_sections = set(source["sections"])
    fragments = selected_fragments(parser.root, wanted_sections)
    records: list[dict[str, object]] = []
    available_sections = {"__lead__"} | {
        node.attrs["id"]
        for node in walk(parser.root)
        if node.tag == "h2" and node.attrs.get("id")
    }
    for missing in sorted(wanted_sections - available_sections):
        records.append(
            {
                "topic_path": topic_path,
                "topic_sha256": topic["sha256"],
                "section": missing,
                "tag": "section",
                "locator": f"topic:{topic_path};section:{missing};missing",
                "fragment_sha256": digest(""),
                "classification": "unsupported",
                "reason": "missing-section",
                "rule_ids": [],
            }
        )
    for section, node, text in fragments:
        fragment_digest = digest(text)
        locator = (
            f"topic:{topic_path};section:{section};path:{structural_path(node)};"
            f"fragment-sha256:{fragment_digest}"
        )
        matches = [rule for rule in config["compile_rules"] if rule_matches(rule, topic_path, section, text)]
        if len(matches) > 1 and len({rule["interpretation"] for rule in matches}) > 1:
            classification = "conflicting"
        elif matches:
            classification = "candidate"
        elif node.tag == "h2":
            classification = "informative"
        else:
            classification = "outside-scope"
        record: dict[str, object] = {
            "topic_path": topic_path,
            "topic_sha256": topic["sha256"],
            "section": section,
            "tag": node.tag,
            "locator": locator,
            "fragment_sha256": fragment_digest,
            "classification": classification,
            "rule_ids": [rule["id"] for rule in matches],
        }
        if matches:
            record["cues"] = semantic_cues(text, node)
            record["interpretations"] = [rule["interpretation"] for rule in matches]
            record["applicability"] = [rule["applicability"] for rule in matches]
        records.append(record)
    return records


def compile_corpus(manifest: dict[str, object], config: dict[str, object], topics: Path) -> dict[str, object]:
    entries = manifest["topics"]
    if not 0 < len(entries) <= MAX_TOPICS:
        raise ValueError("pilot topic count is empty or exceeds the bounded limit")
    configured = {item["topic_path"] for item in config["sources"]}
    pinned = {item["topic_path"] for item in entries}
    if configured != pinned:
        raise ValueError("compile-rule source set differs from the pinned pilot manifest")
    inventory: list[dict[str, object]] = []
    for entry in entries:
        path = topics / Path(str(entry["topic_path"])).name
        body = path.read_bytes()
        if len(body) > MAX_TOPIC_BYTES:
            raise ValueError(f"topic exceeds byte limit: {entry['topic_path']}")
        if len(body) != entry["bytes"] or digest(body) != entry["sha256"]:
            raise ValueError(f"topic digest mismatch: {entry['topic_path']}")
        inventory.extend(compile_topic(entry, body, config))
    if len(inventory) > MAX_FRAGMENTS:
        raise ValueError("source-fragment inventory exceeds the bounded limit")
    candidates = [record for record in inventory if record["classification"] == "candidate"]
    conflicts = [record for record in inventory if record["classification"] == "conflicting"]
    unsupported = [record for record in inventory if record["classification"] == "unsupported"]
    return {
        "schema_version": "mainframe-env.cics-semantic-candidates@1",
        "extractor_version": config["extractor_version"],
        "baseline_id": manifest["baseline_id"],
        "product": manifest["product"],
        "release": config["release"],
        "topic_manifest_digest": manifest["topic_manifest_digest"],
        "compile_rules_sha256": digest(json.dumps(config, sort_keys=True, separators=(",", ":"))),
        "coverage_credit": 0,
        "retained_publication_bytes": False,
        "totals": {
            "topics": len(entries),
            "normative_fragments": len(inventory),
            "candidates": len(candidates),
            "unsupported": len(unsupported),
            "conflicting": len(conflicts),
            "outside_scope": sum(item["classification"] == "outside-scope" for item in inventory),
            "informative": sum(item["classification"] == "informative" for item in inventory),
        },
        "inventory": inventory,
        "candidates": candidates,
    }


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--topics", type=docs_api.retrieval_path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--compile-rules", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(list(argv) if argv is not None else None)
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    config = json.loads(args.compile_rules.read_text(encoding="utf-8"))
    rendered = pretty(compile_corpus(manifest, config, args.topics))
    if args.check:
        if not args.output.is_file() or args.output.read_bytes() != rendered:
            print(f"stale CICS pilot candidate projection: {args.output}", file=sys.stderr)
            return 1
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(rendered)
    totals = json.loads(rendered)["totals"]
    print(" ".join(f"{key}={value}" for key, value in totals.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
