#!/usr/bin/env python3
"""Independently verify the CICS sources-a structural projection.

This verifier treats the generated projection as hostile input.  It parses the
digest-pinned HTML first, derives an expected structural inventory, and only
then reads and compares the projection.  The output is a deterministic,
zero-credit recommendation report; it is not human review authority.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import hashlib
from html import unescape
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
from typing import Any, Iterable, Iterator
from urllib.parse import unquote, urljoin, urlsplit


REPOSITORY = Path(__file__).resolve().parents[3]
MAP_PATH = Path("conformance/0.9/cics/application-api-sources-a-map.json")
MANIFEST_PATH = Path(
    "conformance/0.9/manifests/cics-application-api-sources-a-topics.json"
)
PLAN_PATH = Path("conformance/0.9/cics/application-api-sources-a-extraction.json")
PROJECTION_PATH = Path(
    "conformance/0.9/generated/cics-application-api-sources-a-candidates.json"
)
SUPPLEMENTS_PATH = Path(
    "conformance/0.9/cics/application-api-sources-a-supplements.json"
)

REPORT_SCHEMA = "mainframe-env.cics-source-independent-verification@1"
REPORT_DOMAIN = b"mainframe-env.cics-source-independent-verification@1\0"
FRAGMENT_DOMAIN = b"mainframe-env.cics-source-fragment@1\0"
STRUCTURE_DOMAIN = FRAGMENT_DOMAIN
SUPPLEMENT_DOMAIN = b"mainframe-env.cics-source-supplements@1\0"
VERIFIER_VERSION = "cics-source-independent-verifier@1"

CATEGORIES = ("verified", "requires-reprojection", "product-ambiguity", "mismatch")
DIMENSION_KINDS = {
    "syntax": "source-syntax",
    "options": "source-option",
    "conditions": "source-condition",
}
TOKEN_TYPES = {
    "syntaxkwd": "keyword",
    "syntaxvar": "variable",
    "syntaxdelim": "delimiter",
    "fragref": "fragment",
    "syntaxfragref": "fragment",
}
STRUCTURAL_TAGS = frozenset({"svg", "g", "a"})
VOID_TAGS = frozenset(
    {
        "area",
        "base",
        "br",
        "col",
        "embed",
        "hr",
        "img",
        "input",
        "link",
        "meta",
        "param",
        "source",
        "track",
        "wbr",
    }
)
ARGUMENT_MARKERS = (
    "data-area64",
    "ptr-value64",
    "ptr-ref64",
    "data-value",
    "data-area",
    "ptr-value",
    "ptr-ref",
    "systemname",
    "filename",
    "hhmmss",
    "cvda",
    "label",
    "name",
)
INTRINSIC_INPUT = frozenset(
    {
        "data-value",
        "ptr-value",
        "ptr-value64",
        "name",
        "filename",
        "systemname",
        "label",
        "hhmmss",
    }
)
CONTEXT_SELECTORS = frozenset(
    {
        "global-command-format",
        "global-command-argument-values",
        "mapped-inline-execution-context",
        "mapped-one-hop-context",
        "dpl-server-restrictions",
        "threadsafe-command-list",
        "gap-exec-interface-classification",
        "gap-eibfn-identity",
        "traceid-monitor-compatibility",
        "traceid-dfhcmp-compatibility",
        "source-resolution-context",
    }
)


class VerificationError(ValueError):
    """The immutable inputs cannot be safely verified."""


def canonical_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def file_sha256(path: Path) -> str:
    return "sha256:" + sha256_bytes(path.read_bytes())


def normalize_text(value: str) -> str:
    return " ".join(unescape(value).replace("\xa0", " ").split())


def fragment_sha256(value: str) -> str:
    return "sha256:" + sha256_bytes(FRAGMENT_DOMAIN + value.encode("utf-8"))


def read_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"cannot read JSON object {path}: {error}") from error
    if not isinstance(value, dict):
        raise VerificationError(f"JSON root is not an object: {path}")
    return value


@dataclass(eq=False)
class Element:
    tag: str
    attrs: dict[str, str]
    parent: Element | None = None
    children: list[Element] = field(default_factory=list)
    content: list[str | Element] = field(default_factory=list)

    def classes(self) -> frozenset[str]:
        return frozenset(self.attrs.get("class", "").split())

    def descendants(self) -> Iterator[Element]:
        for child in self.children:
            yield child
            yield from child.descendants()

    def text(self, excluded: frozenset[str] = frozenset()) -> str:
        parts: list[str] = []
        for item in self.content:
            if isinstance(item, str):
                parts.append(item)
            elif item.tag not in excluded:
                parts.append(item.text(excluded))
        return normalize_text(" ".join(parts))


class FragmentParser(HTMLParser):
    """Build a bounded mixed-content tree using only the Python standard library."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.document = Element("__document__", {})
        self.stack = [self.document]

    def _insert(self, tag: str, attrs: list[tuple[str, str | None]]) -> Element:
        node = Element(
            tag.casefold(),
            {key.casefold(): value or "" for key, value in attrs},
            self.stack[-1],
        )
        self.stack[-1].children.append(node)
        self.stack[-1].content.append(node)
        return node

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        node = self._insert(tag, attrs)
        if node.tag not in VOID_TAGS:
            self.stack.append(node)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._insert(tag, attrs)

    def handle_endtag(self, tag: str) -> None:
        wanted = tag.casefold()
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == wanted:
                del self.stack[index:]
                break

    def handle_data(self, data: str) -> None:
        self.stack[-1].content.append(data)


def parse_fragment(body: bytes) -> Element:
    try:
        text = body.decode("utf-8")
    except UnicodeDecodeError as error:
        raise VerificationError("pinned HTML is not UTF-8") from error
    parser = FragmentParser()
    parser.feed(text)
    parser.close()
    return parser.document


def sibling_ordinal(node: Element) -> int:
    if node.parent is None:
        return 1
    ordinal = 1
    for sibling in node.parent.children:
        if sibling is node:
            return ordinal
        if sibling.tag == node.tag:
            ordinal += 1
    raise VerificationError("element is detached from its parent")


def element_path(node: Element) -> str:
    parts: list[str] = []
    current: Element | None = node
    while current is not None and current.tag != "__document__":
        identity = current.attrs.get("id")
        parts.append(
            f"{current.tag}#{identity}"
            if identity
            else f"{current.tag}[{sibling_ordinal(current)}]"
        )
        current = current.parent
    return "/".join(reversed(parts))


PATH_ORDINAL = re.compile(r"^([a-z][a-z0-9-]*)\[([1-9][0-9]*)\]$")
PATH_ID = re.compile(r"^([a-z][a-z0-9-]*)#(.+)$")


def resolve_path(document: Element, value: str) -> Element:
    current = document
    if not isinstance(value, str) or not value or len(value.encode("utf-8")) > 2048:
        raise VerificationError("invalid structural path")
    for component in value.split("/"):
        ordinal = PATH_ORDINAL.fullmatch(component)
        identity = PATH_ID.fullmatch(component)
        if ordinal:
            tag, index_text = ordinal.groups()
            matches = [child for child in current.children if child.tag == tag]
            index = int(index_text)
            if index > len(matches):
                raise VerificationError(f"locator ordinal does not resolve: {component}")
            current = matches[index - 1]
        elif identity:
            tag, wanted = identity.groups()
            matches = [
                child
                for child in current.children
                if child.tag == tag and child.attrs.get("id") == wanted
            ]
            if len(matches) != 1:
                raise VerificationError(f"locator id is not unique: {component}")
            current = matches[0]
        else:
            raise VerificationError(f"invalid locator component: {component}")
    return current


def section_id(node: Element) -> str:
    current: Element | None = node
    while current is not None:
        if current.tag == "section":
            headings = [
                child
                for child in current.children
                if child.tag == "h2" and "sectiontitle" in child.classes()
            ]
            if len(headings) == 1 and headings[0].attrs.get("id"):
                return headings[0].attrs["id"]
        current = current.parent
    return "__topic__"


def section_title(node: Element) -> str | None:
    current: Element | None = node
    while current is not None:
        if current.tag == "section":
            headings = [
                child
                for child in current.children
                if child.tag == "h2" and "sectiontitle" in child.classes()
            ]
            return headings[0].text() if len(headings) == 1 else None
        current = current.parent
    return None


@dataclass(frozen=True)
class Section:
    title: str
    heading_id: str
    node: Element


def sections(document: Element) -> dict[str, Section]:
    result: dict[str, Section] = {}
    for node in document.descendants():
        if node.tag != "section":
            continue
        headings = [
            child
            for child in node.children
            if child.tag == "h2" and "sectiontitle" in child.classes()
        ]
        if len(headings) > 1:
            raise VerificationError("section has multiple direct section titles")
        if not headings:
            continue
        title = headings[0].text()
        heading_id = headings[0].attrs.get("id", "")
        if not title or not heading_id or title in result:
            raise VerificationError(f"invalid or duplicate section title: {title!r}")
        result[title] = Section(title, heading_id, node)
    return result


def ancestors(node: Element, stop: Element | None = None) -> Iterator[Element]:
    current = node.parent
    while current is not None and current is not stop:
        yield current
        current = current.parent


@dataclass(frozen=True)
class Definition:
    term: Element
    descriptions: tuple[Element, ...]
    stack: tuple[str, ...]
    depth: int


def outer_definition_lists(section: Section) -> list[Element]:
    return [
        node
        for node in section.node.descendants()
        if node.tag == "dl"
        and not any(parent.tag == "dl" for parent in ancestors(node, section.node))
    ]


def definitions_in_list(
    definition_list: Element,
    prefix: tuple[str, ...] = (),
    depth: int = 0,
) -> Iterator[Definition]:
    current: Element | None = None
    descriptions: list[Element] = []
    groups: list[Definition] = []

    def finish() -> None:
        if current is None:
            return
        term = current.text(frozenset({"dl", "ul", "ol"}))
        if not term:
            raise VerificationError("definition term is empty")
        groups.append(Definition(current, tuple(descriptions), prefix + (term,), depth))

    header_pending = False
    for child in definition_list.children:
        if child.tag == "dt" and "dthd" in child.classes():
            if current is not None:
                raise VerificationError("definition header follows a term")
            header_pending = True
        elif child.tag == "dd" and "ddhd" in child.classes() and header_pending:
            header_pending = False
        elif child.tag == "dt":
            finish()
            current = child
            descriptions = []
        elif child.tag == "dd":
            if current is None:
                raise VerificationError("definition description has no term")
            descriptions.append(child)
    finish()
    if header_pending:
        raise VerificationError("definition header is incomplete")

    for group in groups:
        yield group
        for description in group.descriptions:
            nested = [
                node
                for node in description.descendants()
                if node.tag == "dl"
                and not any(parent.tag == "dl" for parent in ancestors(node, description))
            ]
            for child in nested:
                yield from definitions_in_list(child, group.stack, depth + 1)


def definition_inventory(section: Section) -> list[Definition]:
    return [
        group
        for definition_list in outer_definition_lists(section)
        for group in definitions_in_list(definition_list)
    ]


OPTION_NAME = re.compile(r"^([A-Z][A-Z0-9-]{0,31})\b")


def option_name(value: str) -> str | None:
    match = OPTION_NAME.match(value)
    return match.group(1) if match else None


def markers_in_var(value: str) -> list[str]:
    normalized = normalize_text(value).replace("_", "-")
    folded = normalized.casefold().replace("pointer-ref", "ptr-ref")
    found = [
        marker
        for marker in ARGUMENT_MARKERS
        if re.search(rf"(?<![a-z0-9-]){re.escape(marker)}(?![a-z0-9-])", folded)
    ]
    if not found and normalized != normalized.upper() and re.fullmatch(
        r"[A-Za-z][A-Za-z0-9-]*name", normalized
    ):
        return ["name"]
    return found


def argument_markers(term: Element) -> list[str]:
    found: list[str] = []
    for node in term.descendants():
        if "var" not in node.classes():
            continue
        raw = node.text()
        values = markers_in_var(raw)
        normalized = normalize_text(raw)
        non_operand_annotation = bool(
            re.fullmatch(r"[0-9]+|.*\bonly\b.*", normalized, re.IGNORECASE)
            or re.fullmatch(r"[A-Z][A-Z0-9-]*", normalized)
        )
        if not values and not non_operand_annotation:
            values = ["unknown"]
        for value in values:
            if value not in found:
                found.append(value)
    if not found:
        for value in markers_in_var(term.text()):
            if value not in found:
                found.append(value)
    return found or ["none"]


def option_stack(values: Iterable[str]) -> list[str]:
    result: list[str] = []
    for value in values:
        name = option_name(value)
        if name:
            result.append(name)
        else:
            result.append("FRAGMENT-" + fragment_sha256(normalize_text(value))[7:19].upper())
    return result


def condition_symbol(value: str, depth: int) -> str:
    normalized = normalize_text(value)
    top = re.match(r"^([0-9]{1,5})\s+([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if top:
        return f"RESP-{top.group(1)}-{top.group(2)}"
    nested = re.match(r"^([0-9]{1,5})\b", normalized)
    if depth > 0 and nested:
        return f"RESP2-{nested.group(1)}"
    symbol = re.match(r"^([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if symbol:
        return f"SYMBOL-{symbol.group(1)}"
    return "FRAGMENT-" + fragment_sha256(normalized)[7:19].upper()


@dataclass(frozen=True)
class SyntaxToken:
    kind: str
    value: str
    node: Element


@dataclass(frozen=True)
class SyntaxItem:
    relation: str
    tokens: tuple[SyntaxToken, ...]
    node: Element


@dataclass(frozen=True)
class Diagram:
    title: str
    node: Element
    pieces: tuple[Element, ...]


def semantic_tokens(node: Element) -> list[SyntaxToken]:
    result: list[SyntaxToken] = []
    for child in node.descendants():
        if child.tag != "text":
            continue
        kind = next((TOKEN_TYPES[name] for name in TOKEN_TYPES if name in child.classes()), None)
        value = child.text()
        if kind and value:
            result.append(SyntaxToken(kind, value, child))
    return result


def syntax_children(node: Element) -> list[Element]:
    return [child for child in node.children if child.tag in STRUCTURAL_TAGS]


def carries_syntax(node: Element) -> bool:
    return bool(semantic_tokens(node))


def syntax_items(node: Element, relation: str = "required") -> Iterator[SyntaxItem]:
    if node.classes() & TOKEN_TYPES.keys():
        tokens = tuple(semantic_tokens(node))
        if tokens:
            yield SyntaxItem(relation, tokens, node)
        return
    children = syntax_children(node)
    speaking = [child for child in children if carries_syntax(child)]
    if speaking and len(speaking) != len(children):
        child_relation = "optional" if relation == "required" else relation
        for child in speaking:
            yield from syntax_items(child, child_relation)
        return
    if "groupchoice" in node.classes():
        for index, child in enumerate(speaking):
            child_relation = "alternative" if relation == "required" and index else relation
            yield from syntax_items(child, child_relation)
        return
    for child in speaking:
        yield from syntax_items(child, relation)


def diagrams(section: Section) -> list[Diagram]:
    containers = [
        node
        for node in section.node.descendants()
        if node.tag == "div"
        and "syntaxdiagram" in node.classes()
        and not any(
            parent.tag == "div" and "syntaxdiagram" in parent.classes()
            for parent in ancestors(node, section.node)
        )
    ]
    result: list[Diagram] = []
    for container in containers:
        titles = [
            node
            for node in container.descendants()
            if node.tag == "h3" and "syntaxdiagram-title" in node.classes()
        ]
        pieces = tuple(
            node
            for node in container.descendants()
            if node.tag == "svg" and "syntaxdiagram" in node.classes()
        )
        if len(titles) != 1 or not titles[0].text() or not pieces:
            raise VerificationError("syntax diagram lacks one title or any SVG piece")
        result.append(Diagram(titles[0].text(), container, pieces))
    return result


def diagram_items(diagram: Diagram) -> list[SyntaxItem]:
    result: list[SyntaxItem] = []
    for piece in diagram.pieces:
        roots = [
            child for child in piece.children if child.tag == "g" and "diagram" in child.classes()
        ]
        if len(roots) != 1:
            raise VerificationError("syntax SVG lacks one diagram root")
        result.extend(syntax_items(roots[0]))
    return result


def token_group_path(token: SyntaxToken, diagram: Diagram) -> str:
    parts: list[str] = []
    current = token.node.parent
    while current is not None and current is not diagram.node:
        group = next(
            (name for name in ("groupseq", "groupchoice", "groupcomp") if name in current.classes()),
            None,
        )
        if group:
            parts.append(f"{group}[{sibling_ordinal(current)}]")
        current = current.parent
    return "/".join(reversed(parts)) or "__root__"


def token_piece(token: SyntaxToken, diagram: Diagram) -> int:
    current: Element | None = token.node
    while current is not None:
        for index, piece in enumerate(diagram.pieces, 1):
            if current is piece:
                return index
        current = current.parent
    raise VerificationError("syntax token is outside its SVG pieces")


@dataclass(frozen=True)
class Topic:
    path: str
    sha256: str
    size: int
    document: Element
    source_product: str | None = None
    authority_boundary: str | None = None
    allowed_dimensions: frozenset[str] = frozenset()


@dataclass(frozen=True)
class ExpectedFact:
    official_row: str
    dimension: str
    kind: str
    topic_path: str
    structural_path: str
    value: dict[str, Any]

    def key(self) -> tuple[str, str, str, str, str]:
        return (
            self.official_row,
            self.dimension,
            self.kind,
            self.topic_path,
            self.structural_path,
        )


@dataclass
class SourceSnapshot:
    manifest: dict[str, Any]
    mapping: dict[str, Any]
    plan: dict[str, Any]
    topics: dict[str, Topic]
    expected: dict[tuple[str, str, str, str, str], ExpectedFact]
    top_options: dict[str, set[str]]
    option_definitions: dict[tuple[str, str], list[Definition]]
    supplement_entries: dict[str, dict[str, Any]]


def cache_names(topic_path: str, digest: str) -> tuple[str, str]:
    clean = topic_path.split("?", 1)[0]
    canonical = f"topic-{sha256_bytes(topic_path.encode())}-sha256-{digest}.html"
    legacy = re.sub(r"[^A-Za-z0-9._-]", "_", clean)
    return canonical, legacy


def cached_topic(cache: Path, topic_path: str, digest: str, size: int) -> bytes:
    names = cache_names(topic_path, digest)
    paths = [cache / name for name in names if (cache / name).exists()]
    if not paths:
        raise VerificationError(f"missing cached topic: {topic_path}")
    bodies: list[bytes] = []
    for path in paths:
        if path.is_symlink() or not path.is_file():
            raise VerificationError(f"unsafe cached topic entry: {path}")
        bodies.append(path.read_bytes())
    if any(body != bodies[0] for body in bodies[1:]):
        raise VerificationError(f"cache aliases disagree: {topic_path}")
    body = bodies[0]
    if len(body) != size or sha256_bytes(body) != digest:
        raise VerificationError(f"cached topic identity differs: {topic_path}")
    if body.startswith(b"%PDF-"):
        raise VerificationError(f"non-HTML source body: {topic_path}")
    return body


def supplement_digest(value: dict[str, Any]) -> str:
    material = dict(value)
    material.pop("supplements_sha256", None)
    return "sha256:" + sha256_bytes(SUPPLEMENT_DOMAIN + canonical_bytes(material))


def load_supplements(root: Path, cache: Path) -> tuple[dict[str, Topic], dict[str, dict[str, Any]]]:
    path = root / SUPPLEMENTS_PATH
    if not path.exists():
        return {}, {}
    receipt = read_object(path)
    if (
        receipt.get("schema_version") != "mainframe-env.cics-source-supplements@1"
        or receipt.get("target_version") != "0.9.0"
        or receipt.get("source_kind") != "browser-verified-direct-content"
        or receipt.get("supplements_sha256") != supplement_digest(receipt)
    ):
        raise VerificationError("supplement receipt identity differs")
    for field in (
        "semantic_authority",
        "execution_authority",
        "automatic_registration",
    ):
        if receipt.get(field) is not False:
            raise VerificationError(f"supplement improperly grants {field}")
    for field in (
        "semantic_credit",
        "execution_credit",
        "registration_credit",
        "coverage_credit",
        "differential_credit",
    ):
        if receipt.get(field) != 0:
            raise VerificationError(f"supplement improperly grants {field}")
    raw_topics = receipt.get("topics")
    capture = receipt.get("capture")
    if not isinstance(raw_topics, list) or not raw_topics or not isinstance(capture, dict):
        raise VerificationError("supplement receipt has no bounded topic capture")
    if (
        capture.get("topics_requested") != len(raw_topics)
        or capture.get("topics_loaded") != len(raw_topics)
        or capture.get("all_reproductions_identical") is not True
        or capture.get("raw_publication_retained_in_repository") is not False
    ):
        raise VerificationError("supplement browser reproduction is incomplete")

    topics: dict[str, Topic] = {}
    entries: dict[str, dict[str, Any]] = {}
    identity_lines: list[str] = []
    allowed_boundaries = {
        "cross-product-not-target-authority",
        "target-platform-example-only",
        "target-product-older-version-compatibility",
        "target-product-older-version-context",
    }
    for entry in raw_topics:
        if not isinstance(entry, dict):
            raise VerificationError("supplement topic is not an object")
        topic_path = entry.get("topic_path")
        digest = entry.get("sha256")
        size = entry.get("bytes")
        product = entry.get("product_key")
        dimensions = entry.get("allowed_evidence_dimensions")
        boundary = entry.get("target_authority_boundary")
        if (
            not isinstance(topic_path, str)
            or not isinstance(product, str)
            or not topic_path.startswith(product + "/")
            or not isinstance(digest, str)
            or re.fullmatch(r"[0-9a-f]{64}", digest) is None
            or not isinstance(size, int)
            or isinstance(size, bool)
            or size <= 0
            or boundary not in allowed_boundaries
            or entry.get("target_product_authority") is not False
            or entry.get("semantic_authority") is not False
            or entry.get("coverage_credit") != 0
            or not isinstance(dimensions, list)
            or not set(dimensions) <= {
                "syntax",
                "options",
                "operand-directions",
                "conditions",
                "execution-context",
            }
            or topic_path in topics
        ):
            raise VerificationError("supplement topic authority boundary differs")
        canonical, _ = cache_names(topic_path, digest)
        if entry.get("cache_key") != canonical:
            raise VerificationError(f"supplement cache key differs: {topic_path}")
        content_url = urlsplit(str(entry.get("content_url", "")))
        public_url = urlsplit(str(entry.get("public_url", "")))
        if (
            content_url.scheme != "https"
            or content_url.netloc != "www.ibm.com"
            or public_url.scheme != "https"
            or public_url.netloc != "www.ibm.com"
        ):
            raise VerificationError(f"supplement URL is not official IBM HTML: {topic_path}")
        body = cached_topic(cache, topic_path, digest, size)
        topics[topic_path] = Topic(
            topic_path,
            digest,
            size,
            parse_fragment(body),
            str(entry.get("source_product")),
            boundary,
            frozenset(dimensions),
        )
        entries[topic_path] = entry
        identity_lines.append(f"{topic_path} {size} {digest}\n")
    identity = "sha256:" + sha256_bytes(
        b"mainframe-env.cics-source-supplement-identity@1\0"
        + "".join(sorted(identity_lines)).encode("utf-8")
    )
    if capture.get("identity_sha256") != identity:
        raise VerificationError("supplement capture identity digest differs")
    return topics, entries


def row_discriminators(rows: list[dict[str, Any]]) -> dict[str, set[str]]:
    labels = [str(row["label"]).split() for row in rows]
    common = 0
    while labels and all(len(words) > common for words in labels) and len(
        {words[common] for words in labels}
    ) == 1:
        common += 1
    return {
        str(row["official_row"]): set(words[common:])
        for row, words in zip(rows, labels)
    }


def diagram_applies(row: dict[str, Any], siblings: list[dict[str, Any]], diagram: Diagram) -> bool:
    selection = row.get("selection_kind")
    if selection == "combined-page" and len(siblings) > 1:
        return normalize_text(diagram.title).casefold() == normalize_text(str(row["label"])).casefold()
    if selection != "shared-page" or len(siblings) == 1:
        return True
    wanted = row_discriminators(siblings)[str(row["official_row"])]
    words: set[str] = set()
    for item in diagram_items(diagram):
        for token in item.tokens:
            if token.kind != "keyword":
                continue
            match = re.match(r"^[A-Z][A-Z0-9-]*", token.value)
            if match:
                words.add(match.group(0))
    return not wanted or wanted <= words


def diagram_items_for_row(
    row: dict[str, Any], siblings: list[dict[str, Any]], diagram: Diagram
) -> list[SyntaxItem]:
    items = diagram_items(diagram)
    if row.get("selection_kind") != "shared-page" or len(siblings) == 1:
        return items
    discriminators = row_discriminators(siblings)
    selected = discriminators[str(row["official_row"])]
    sibling_only = set().union(*discriminators.values()) - selected
    result: list[SyntaxItem] = []
    for item in items:
        names = {
            match.group(0)
            for token in item.tokens
            if token.kind == "keyword"
            for match in [re.match(r"^[A-Z][A-Z0-9-]*", token.value)]
            if match is not None
        }
        if not names & sibling_only:
            result.append(item)
    return result


def definition_applies(
    row: dict[str, Any], siblings: list[dict[str, Any]], definition: Definition
) -> bool:
    if len(siblings) == 1 or row.get("selection_kind") not in {
        "shared-page",
        "combined-page",
    }:
        return True
    name = option_name(definition.stack[0])
    discriminators = row_discriminators(siblings)
    all_words = set().union(*discriminators.values())
    return name not in all_words or name in discriminators[str(row["official_row"])]


def syntax_value(
    diagram: Diagram, ordinal: int, items: Iterable[SyntaxItem] | None = None
) -> dict[str, Any]:
    tokens = [
        {
            "kind": token.kind,
            "value": token.value,
            "relation": item.relation,
            "piece": token_piece(token, diagram),
            "group_path": token_group_path(token, diagram),
        }
        for item in (list(items) if items is not None else diagram_items(diagram))
        for token in item.tokens
    ]
    encoded = json.dumps(tokens, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    return {
        "type": "syntax",
        "variant": f"diagram-{ordinal:04d}",
        "tokens": tokens,
        "structure_sha256": fragment_sha256(encoded),
    }


def option_value(definition: Definition) -> dict[str, Any]:
    name = option_name(definition.stack[0])
    if name is None:
        raise VerificationError("option definition has no symbolic name")
    return {
        "type": "option",
        "term": name,
        "arguments": argument_markers(definition.term),
        "definition_count": len(definition.descriptions),
        "depth": definition.depth,
        "stack": option_stack(definition.stack),
    }


def condition_value(definition: Definition) -> dict[str, Any]:
    return {
        "type": "condition",
        "condition_stack": [
            condition_symbol(value, depth) for depth, value in enumerate(definition.stack)
        ],
        "definition_count": len(definition.descriptions),
        "depth": definition.depth,
    }


def add_expected(
    target: dict[tuple[str, str, str, str, str], ExpectedFact],
    fact: ExpectedFact,
) -> None:
    key = fact.key()
    if key in target:
        raise VerificationError(f"duplicate independently derived fact: {key}")
    target[key] = fact


def build_source_snapshot(root: Path, cache: Path) -> SourceSnapshot:
    """Derive all expected structural facts before any projection is opened."""

    manifest = read_object(root / MANIFEST_PATH)
    mapping = read_object(root / MAP_PATH)
    plan = read_object(root / PLAN_PATH)
    topic_rows = manifest.get("topics")
    if not isinstance(topic_rows, list) or not topic_rows:
        raise VerificationError("source manifest has no topics")
    topics: dict[str, Topic] = {}
    for raw in topic_rows:
        if not isinstance(raw, dict):
            raise VerificationError("manifest topic is not an object")
        path = raw.get("topic_path")
        digest = raw.get("sha256")
        size = raw.get("bytes")
        if (
            not isinstance(path, str)
            or not isinstance(digest, str)
            or re.fullmatch(r"[0-9a-f]{64}", digest) is None
            or not isinstance(size, int)
            or isinstance(size, bool)
            or size <= 0
            or path in topics
        ):
            raise VerificationError("invalid manifest topic identity")
        body = cached_topic(cache, path, digest, size)
        topics[path] = Topic(path, digest, size, parse_fragment(body))

    supplement_topics, supplement_entries = load_supplements(root, cache)
    if set(topics) & set(supplement_topics):
        raise VerificationError("supplement topic aliases the target corpus")
    topics.update(supplement_topics)

    rows = mapping.get("rows")
    if not isinstance(rows, list) or not rows:
        raise VerificationError("source map has no rows")
    resolutions = {
        str(row["official_row"]): row
        for row in plan.get("source_resolutions", [])
        if isinstance(row, dict) and row.get("official_row")
    }
    projected_sources: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        official_row = str(row["official_row"])
        sources = [dict(source) for source in row.get("topics", [])]
        resolution = resolutions.get(official_row)
        direct = resolution.get("direct_supplement_topic") if resolution else None
        if direct:
            entry = supplement_entries.get(str(direct))
            if entry is None or official_row not in entry.get("applies_to_rows", []):
                raise VerificationError(f"supplement escapes its declared row: {official_row}")
            sources.append({"topic_path": direct, "role": "supplement"})
        projected_sources[official_row] = sources
    by_topic: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        for source in projected_sources[str(row["official_row"])]:
            by_topic.setdefault(str(source["topic_path"]), []).append(row)

    expected: dict[tuple[str, str, str, str, str], ExpectedFact] = {}
    top_options: dict[str, set[str]] = {str(row["official_row"]): set() for row in rows}
    option_definitions: dict[tuple[str, str], list[Definition]] = {}
    for row in rows:
        official_row = str(row["official_row"])
        for source in projected_sources[official_row]:
            path = str(source["topic_path"])
            topic = topics.get(path)
            if topic is None:
                raise VerificationError(f"mapped topic is outside manifest: {path}")
            indexed = sections(topic.document)
            siblings = by_topic[path]
            allowed = (
                topic.allowed_dimensions
                if source.get("role") == "supplement"
                else frozenset({"syntax", "options", "conditions"})
            )
            syntax = indexed.get("Syntax")
            if syntax and "syntax" in allowed:
                for ordinal, diagram in enumerate(diagrams(syntax), 1):
                    if not diagram_applies(row, siblings, diagram):
                        continue
                    add_expected(
                        expected,
                        ExpectedFact(
                            official_row,
                            "syntax",
                            "source-syntax",
                            path,
                            element_path(diagram.node),
                            syntax_value(
                                diagram,
                                ordinal,
                                diagram_items_for_row(row, siblings, diagram),
                            ),
                        ),
                    )
            options = indexed.get("Options")
            if options and "options" in allowed:
                for definition in definition_inventory(options):
                    if not definition_applies(row, siblings, definition):
                        continue
                    name = option_name(definition.stack[0])
                    if name is None:
                        continue
                    add_expected(
                        expected,
                        ExpectedFact(
                            official_row,
                            "options",
                            "source-option",
                            path,
                            element_path(definition.term),
                            option_value(definition),
                        ),
                    )
                    option_definitions.setdefault((official_row, name), []).append(definition)
                    if definition.depth == 0:
                        top_options[official_row].add(name)
            conditions = indexed.get("Conditions")
            if conditions and "conditions" in allowed:
                for definition in definition_inventory(conditions):
                    add_expected(
                        expected,
                        ExpectedFact(
                            official_row,
                            "conditions",
                            "source-condition",
                            path,
                            element_path(definition.term),
                            condition_value(definition),
                        ),
                    )
    return SourceSnapshot(
        manifest,
        mapping,
        plan,
        topics,
        expected,
        top_options,
        option_definitions,
        supplement_entries,
    )


def projected_fact_key(
    official_row: str,
    dimension: str,
    candidate: dict[str, Any],
) -> tuple[str, str, str, str, str]:
    evidence = candidate.get("evidence")
    if not isinstance(evidence, dict):
        raise VerificationError("candidate evidence is not an object")
    return (
        official_row,
        dimension,
        str(candidate.get("kind")),
        str(evidence.get("topic_path")),
        str(evidence.get("structural_path")),
    )


def validate_evidence(candidate: dict[str, Any], snapshot: SourceSnapshot) -> str | None:
    evidence = candidate.get("evidence")
    if not isinstance(evidence, dict):
        return "malformed-evidence"
    path = evidence.get("topic_path")
    topic = snapshot.topics.get(str(path))
    if topic is None:
        return "foreign-topic"
    if evidence.get("topic_sha256") != "sha256:" + topic.sha256:
        return "stale-topic-digest"
    supplement = snapshot.supplement_entries.get(str(path))
    if supplement is not None:
        if (
            evidence.get("source_product") != supplement.get("product_key")
            or evidence.get("target_authority_boundary")
            != supplement.get("target_authority_boundary")
        ):
            return "supplement-authority-boundary-mismatch"
    elif "source_product" in evidence or "target_authority_boundary" in evidence:
        if (
            evidence.get("source_product") != "SSJL4D_6.x"
            or evidence.get("target_authority_boundary") != "target-product-authority"
        ):
            return "target-source-authority-boundary-mismatch"
    try:
        node = resolve_path(topic.document, str(evidence.get("structural_path")))
    except VerificationError:
        return "unresolved-locator"
    if evidence.get("tag") != node.tag:
        return "locator-tag-mismatch"
    if evidence.get("section_id") != section_id(node):
        return "locator-section-mismatch"
    if evidence.get("fragment_sha256") != fragment_sha256(node.text()):
        return "fragment-digest-mismatch"
    return None


def link_target(href: str, source_topic: str) -> tuple[str, str] | None:
    if not href or href.startswith(("#", "javascript:", "mailto:")):
        return None
    base = "https://www.ibm.com/docs/en/" + source_topic
    parsed = urlsplit(urljoin(base, href))
    if parsed.scheme != "https" or parsed.netloc != "www.ibm.com" or parsed.query:
        return None
    prefix = "/docs/en/"
    if not parsed.path.startswith(prefix):
        return None
    topic = unquote(parsed.path[len(prefix) :])
    fragment = unquote(parsed.fragment)
    if (
        not topic.startswith("SSJL4D_6.x/")
        or not topic.endswith((".htm", ".html"))
        or ".." in topic.split("/")
        or any(character.isspace() for character in fragment)
        or len(fragment) > 256
    ):
        return None
    return topic, fragment


def expected_link_target(topic: Topic, fragment: str) -> Element | None:
    if fragment:
        matches = [node for node in topic.document.descendants() if node.attrs.get("id") == fragment]
        if len(matches) == 1:
            return matches[0]
        titles = [
            node
            for node in topic.document.descendants()
            if node.tag == "h1"
            and "topictitle1" in node.classes()
            and node.attrs.get("id", "").startswith(fragment + "__")
        ]
        articles = [node for node in topic.document.descendants() if node.tag == "article"]
        return articles[0] if not matches and len(titles) == 1 and len(articles) == 1 else None
    articles = [node for node in topic.document.descendants() if node.tag == "article"]
    return articles[0] if len(articles) == 1 else None


def one_hop_results(
    candidates: dict[str, tuple[dict[str, Any], dict[str, Any], dict[str, Any]]],
    snapshot: SourceSnapshot,
) -> dict[str, tuple[str, str]]:
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for identity, (row, _, candidate) in candidates.items():
        value = candidate.get("candidate_value")
        if (
            candidate.get("kind") != "source-context"
            or not isinstance(value, dict)
            or value.get("selector_id") != "mapped-one-hop-context"
        ):
            continue
        key = str(candidate.get("key"))
        match = re.fullmatch(r"one-hop-(source|target)-([0-9a-f]{16})", key)
        if not match:
            grouped.setdefault((str(row["official_row"]), "invalid-" + identity), []).append(candidate)
            continue
        grouped.setdefault((str(row["official_row"]), match.group(2)), []).append(candidate)

    results: dict[str, tuple[str, str]] = {}
    for _, members in sorted(grouped.items()):
        identities = [str(member["candidate_id"]) for member in members]
        source = [member for member in members if str(member.get("key", "")).startswith("one-hop-source-")]
        target = [member for member in members if str(member.get("key", "")).startswith("one-hop-target-")]
        if len(source) != 1 or len(target) != 1:
            for identity in identities:
                results[identity] = ("mismatch", "one-hop-pair-is-incomplete")
            continue
        source_evidence = source[0]["evidence"]
        target_evidence = target[0]["evidence"]
        source_topic = snapshot.topics.get(source_evidence["topic_path"])
        if source_topic is None:
            for identity in identities:
                results[identity] = ("mismatch", "one-hop-source-topic-is-foreign")
            continue
        try:
            anchor = resolve_path(source_topic.document, source_evidence["structural_path"])
        except VerificationError:
            anchor = None
        reference = (
            link_target(anchor.attrs.get("href", ""), source_topic.path)
            if anchor is not None and anchor.tag == "a"
            else None
        )
        target_topic = snapshot.topics.get(target_evidence["topic_path"])
        expected_target = (
            expected_link_target(target_topic, reference[1])
            if reference is not None
            and target_topic is not None
            and reference[0] == target_topic.path
            else None
        )
        if expected_target is None or element_path(expected_target) != target_evidence["structural_path"]:
            for identity in identities:
                results[identity] = ("mismatch", "one-hop-target-does-not-follow-href")
        else:
            for identity in identities:
                results[identity] = ("verified", "one-hop-binding")
    return results


def prose_direction(definitions: Iterable[Definition]) -> str | None:
    values: set[str] = set()
    for definition in definitions:
        text = " ".join(item.text(frozenset({"dl", "ul", "ol"})) for item in definition.descriptions)
        folded = text.casefold()
        sends = bool(
            re.search(
                r"\b(specifies|supplies|passes|input field|on input|provided by the (?:application|program))\b",
                folded,
            )
        )
        receives = bool(
            re.search(
                r"\b(returns|returned by cics|cics returns|cics sets|output field|on output|"
                r"buffer to contain|area to contain|into which)\b",
                folded,
            )
        )
        if sends and receives:
            values.add("input-output")
        elif sends:
            values.add("input")
        elif receives:
            values.add("output")
    return next(iter(values)) if len(values) == 1 else None


def candidate_category(
    row: dict[str, Any],
    dimension: dict[str, Any],
    candidate: dict[str, Any],
    snapshot: SourceSnapshot,
    expected: ExpectedFact | None,
    issue_code: str | None,
    context_result: tuple[str, str] | None = None,
) -> tuple[str, str]:
    evidence_error = validate_evidence(candidate, snapshot)
    if evidence_error:
        return "mismatch", evidence_error
    if issue_code == "unmatched-row":
        return "requires-reprojection", "unmatched-row"
    if issue_code in {
        "source-gap",
        "conflicting-argument-kind",
        "target-equivalence-ambiguity",
    }:
        return "product-ambiguity", issue_code

    kind = candidate.get("kind")
    if kind in set(DIMENSION_KINDS.values()):
        if expected is None:
            return "requires-reprojection", "extra-structural-fact"
        if candidate.get("candidate_value") != expected.value:
            return "mismatch", "structural-value-differs"
        return "verified", "independent-structural-match"

    if kind == "source-context":
        value = candidate.get("candidate_value")
        selector = value.get("selector_id") if isinstance(value, dict) else None
        if selector not in CONTEXT_SELECTORS:
            return "mismatch", "unknown-context-selector"
        if selector in {"global-command-format", "global-command-argument-values"}:
            return "verified", "global-context-binding"
        if selector == "mapped-inline-execution-context":
            direct = {
                source["topic_path"] for source in row.get("topics", []) if isinstance(source, dict)
            }
            if candidate["evidence"]["topic_path"] not in direct:
                return "requires-reprojection", "inline-context-outside-row-topic"
            topic = snapshot.topics[candidate["evidence"]["topic_path"]]
            node = resolve_path(topic.document, candidate["evidence"]["structural_path"])
            if node.tag != "p" or node.parent is None or section_title(node) != "Syntax":
                return "requires-reprojection", "inline-context-is-not-a-syntax-paragraph"
            return "verified", "inline-row-context"
        if selector == "mapped-one-hop-context":
            return context_result or ("mismatch", "one-hop-pair-was-not-indexed")
        if selector in {"dpl-server-restrictions", "threadsafe-command-list"}:
            return "verified", "global-applicability-source-binding"
        if selector == "source-resolution-context":
            resolution = next(
                (
                    item
                    for item in snapshot.plan.get("source_resolutions", [])
                    if item.get("official_row") == row.get("official_row")
                ),
                None,
            )
            fragments = {
                item.get("topic_path"): item.get("selector")
                for item in (resolution or {}).get("context_fragments", [])
                if isinstance(item, dict)
            }
            expected_tags = {
                "unique-article-root": "article",
                "unique-h1": "h1",
                "traceid-compatibility-note": "div",
                "dump-applicability-item": "li",
            }
            topic_path = candidate["evidence"]["topic_path"]
            if (
                resolution is None
                or topic_path not in fragments
                or expected_tags.get(fragments[topic_path])
                != candidate["evidence"]["tag"]
            ):
                return "mismatch", "source-resolution-context-escaped-boundary"
            return "verified", "authority-bounded-context-root"
        return "product-ambiguity", "context-applicability-needs-review"

    if kind != "source-operand-direction":
        return "mismatch", "unknown-candidate-kind"
    value = candidate.get("candidate_value")
    if not isinstance(value, dict):
        return "mismatch", "malformed-direction-value"
    marker = value.get("marker")
    direction = value.get("direction")
    option = value.get("option")
    official_row = str(row["official_row"])
    if not isinstance(option, str) or option not in snapshot.top_options.get(official_row, set()):
        return "product-ambiguity", "operand-has-no-row-option"
    if marker in INTRINSIC_INPUT:
        return (
            ("verified", "intrinsic-input-marker")
            if direction == "input"
            else ("mismatch", "intrinsic-input-direction-differs")
        )
    if marker == "none":
        key = projected_fact_key(official_row, "options", {
            "kind": "source-option",
            "evidence": candidate["evidence"],
        })
        option_fact = snapshot.expected.get(key)
        if option_fact and option_fact.value.get("depth", 0) > 0:
            return "verified", "nested-source-detail"
        return (
            ("verified", "bare-option")
            if direction == "none"
            else ("mismatch", "bare-option-has-direction")
        )
    if marker == "unknown":
        return "requires-reprojection", "unrecognized-argument-marker"
    if marker not in {"data-area", "data-area64", "cvda", "ptr-ref", "ptr-ref64"}:
        return "mismatch", "unknown-argument-marker"
    derived = prose_direction(snapshot.option_definitions.get((official_row, option), []))
    if derived is None:
        return "product-ambiguity", "direction-not-explicit"
    if direction in {"unknown", derived}:
        return "verified", "independent-option-direction"
    return "product-ambiguity", "direction-differs-from-option-prose"


def category_digest(values: list[str]) -> str:
    return "sha256:" + sha256_bytes(REPORT_DOMAIN + canonical_bytes(values))


def declared_absence(
    snapshot: SourceSnapshot,
    official_row: str,
    dimension: str,
) -> bool:
    row = next(
        (item for item in snapshot.mapping.get("rows", []) if item.get("official_row") == official_row),
        None,
    )
    if row is None:
        return False
    paths = {item.get("topic_path") for item in row.get("topics", [])}
    return any(
        item.get("dimension") == dimension
        and item.get("state") == "declared-absent"
        and item.get("topic_path") in paths
        and item.get("official_row") in {None, official_row}
        for item in snapshot.plan.get("section_exceptions", [])
        if isinstance(item, dict)
    )


def report_digest(report: dict[str, Any]) -> str:
    material = dict(report)
    material.pop("report_sha256", None)
    return "sha256:" + sha256_bytes(REPORT_DOMAIN + canonical_bytes(material))


def verify(
    root: Path = REPOSITORY,
    cache: Path | None = None,
    projection_path: Path | None = None,
) -> dict[str, Any]:
    cache = cache or Path("/ibm-docs/topic-cache")
    snapshot = build_source_snapshot(root, cache)
    # Deliberately load generated output only after the source-derived model is complete.
    projection_file = projection_path or root / PROJECTION_PATH
    projection = read_object(projection_file)

    projected_rows = projection.get("rows")
    mapped_rows = snapshot.mapping.get("rows")
    if not isinstance(projected_rows, list) or not isinstance(mapped_rows, list):
        raise VerificationError("projection or source map has no row list")
    map_by_id = {str(row["official_row"]): row for row in mapped_rows}
    projected_by_id = {str(row.get("official_row")): row for row in projected_rows}
    if len(map_by_id) != len(mapped_rows) or len(projected_by_id) != len(projected_rows):
        raise VerificationError("row identities are duplicated")

    structural_projected: dict[
        tuple[str, str, str, str, str], tuple[dict[str, Any], dict[str, Any], dict[str, Any]]
    ] = {}
    candidate_locations: dict[str, tuple[dict[str, Any], dict[str, Any], dict[str, Any]]] = {}
    issue_by_candidate: dict[str, str] = {}
    issue_rows: list[tuple[str, str, str, str]] = []
    for official_row, projected_row in projected_by_id.items():
        source_row = map_by_id.get(official_row)
        if source_row is None:
            continue
        dimensions = projected_row.get("dimensions")
        if not isinstance(dimensions, list):
            raise VerificationError(f"projection row lacks dimensions: {official_row}")
        for dimension in dimensions:
            if not isinstance(dimension, dict):
                raise VerificationError("projection dimension is not an object")
            candidates = dimension.get("candidates")
            issues = dimension.get("issues")
            if not isinstance(candidates, list) or not isinstance(issues, list):
                raise VerificationError("projection candidates or issues are not lists")
            for issue in issues:
                if not isinstance(issue, dict):
                    raise VerificationError("projected issue is not an object")
                code = str(issue.get("code"))
                issue_rows.append(
                    (
                        official_row,
                        str(dimension.get("name")),
                        str(issue.get("issue_id")),
                        code,
                    )
                )
                for identity in issue.get("candidate_ids", []):
                    if identity in issue_by_candidate and issue_by_candidate[identity] != code:
                        raise VerificationError("candidate belongs to conflicting issue types")
                    issue_by_candidate[str(identity)] = code
            for candidate in candidates:
                if not isinstance(candidate, dict):
                    raise VerificationError("projected candidate is not an object")
                identity = str(candidate.get("candidate_id"))
                if not identity or identity in candidate_locations:
                    raise VerificationError(f"duplicate or empty candidate id: {identity}")
                candidate_locations[identity] = (source_row, dimension, candidate)
                if candidate.get("kind") in set(DIMENSION_KINDS.values()):
                    key = projected_fact_key(official_row, str(dimension.get("name")), candidate)
                    if key in structural_projected:
                        raise VerificationError(f"duplicate projected structural fact: {key}")
                    structural_projected[key] = (source_row, dimension, candidate)

    one_hop = one_hop_results(candidate_locations, snapshot)

    expected_keys = set(snapshot.expected)
    projected_keys = set(structural_projected)
    missing = sorted(expected_keys - projected_keys)
    extra = sorted(projected_keys - expected_keys)

    categories: dict[str, list[str]] = {name: [] for name in CATEGORIES}
    findings: list[dict[str, str]] = []
    for identity in sorted(candidate_locations):
        row, dimension, candidate = candidate_locations[identity]
        expected = snapshot.expected.get(
            projected_fact_key(str(row["official_row"]), str(dimension.get("name")), candidate)
        )
        category, reason = candidate_category(
            row,
            dimension,
            candidate,
            snapshot,
            expected,
            issue_by_candidate.get(identity),
            one_hop.get(identity),
        )
        categories[category].append(identity)
        if category != "verified":
            findings.append(
                {
                    "candidate_id": identity,
                    "category": category,
                    "reason_code": reason,
                }
            )

    for key in missing:
        findings.append(
            {
                "fact_sha256": category_digest(list(key)),
                "category": "requires-reprojection",
                "reason_code": "missing-structural-fact",
            }
        )
    issue_categories = {name: [] for name in CATEGORIES}
    for official_row, dimension, identity, code in sorted(issue_rows):
        if code == "unmatched-row" and declared_absence(snapshot, official_row, dimension):
            category = "verified"
        elif code == "unmatched-row":
            category = "requires-reprojection"
        elif code in {
            "source-gap",
            "conflicting-argument-kind",
            "target-equivalence-ambiguity",
        }:
            category = "product-ambiguity"
        else:
            category = "mismatch"
        issue_categories[category].append(identity)
        if category != "verified":
            findings.append(
                {
                    "issue_id": identity,
                    "official_row": official_row,
                    "category": category,
                    "reason_code": code,
                }
            )

    for key in extra:
        candidate = structural_projected[key][2]
        identity = str(candidate["candidate_id"])
        if identity in categories["verified"]:
            categories["verified"].remove(identity)
            categories["requires-reprojection"].append(identity)

    categories_report = {
        name: {
            "count": len(sorted(values)),
            "candidate_ids": sorted(values),
            "candidate_ids_sha256": category_digest(sorted(values)),
        }
        for name, values in categories.items()
    }
    issue_report = {
        name: {
            "count": len(values),
            "issue_ids": sorted(values),
            "issue_ids_sha256": category_digest(sorted(values)),
        }
        for name, values in issue_categories.items()
    }
    candidate_kind_counts: dict[str, int] = {}
    for _, _, candidate in candidate_locations.values():
        kind = str(candidate.get("kind"))
        candidate_kind_counts[kind] = candidate_kind_counts.get(kind, 0) + 1
    has_mismatch = bool(
        categories_report["mismatch"]["count"] or issue_report["mismatch"]["count"]
    )
    has_reprojection = bool(
        categories_report["requires-reprojection"]["count"]
        or issue_report["requires-reprojection"]["count"]
        or missing
        or extra
    )
    has_bounded_ambiguity = bool(
        categories_report["product-ambiguity"]["count"]
        or issue_report["product-ambiguity"]["count"]
    )
    status = (
        "mismatch"
        if has_mismatch
        else "requires-reprojection"
        if has_reprojection
        else "verified-with-bounded-ambiguities"
        if has_bounded_ambiguity
        else "verified"
    )
    input_identities = {
        "source_map_sha256": file_sha256(root / MAP_PATH),
        "topic_manifest_sha256": file_sha256(root / MANIFEST_PATH),
        "extraction_plan_sha256": file_sha256(root / PLAN_PATH),
        "candidate_projection_sha256": file_sha256(projection_file),
    }
    if (root / SUPPLEMENTS_PATH).exists():
        input_identities["source_supplements_sha256"] = file_sha256(
            root / SUPPLEMENTS_PATH
        )
    report: dict[str, Any] = {
        "schema_version": REPORT_SCHEMA,
        "target_version": projection.get("target_version"),
        "status": status,
        "semantic_authority": False,
        "execution_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "semantic_credit": 0,
        "differential_credit": 0,
        "verifier": {
            "name": "independent-cics-html-structural-verifier",
            "version": VERIFIER_VERSION,
            "implementation_sha256": file_sha256(Path(__file__).resolve()),
            "projection_loaded_after_source_derivation": True,
        },
        "inputs": input_identities,
        "counts": {
            "topics": len(snapshot.topics),
            "rows": len(projected_rows),
            "candidates": len(candidate_locations),
            "issues": len(issue_rows),
            "unique_evidence_fragments": len(
                {
                    (
                        candidate["evidence"]["topic_path"],
                        candidate["evidence"]["structural_path"],
                        candidate["evidence"]["fragment_sha256"],
                    )
                    for _, _, candidate in candidate_locations.values()
                    if isinstance(candidate.get("evidence"), dict)
                }
            ),
            "candidate_kinds": dict(sorted(candidate_kind_counts.items())),
        },
        "structural_coverage": {
            "expected": len(expected_keys),
            "projected": len(projected_keys),
            "missing": len(missing),
            "extra": len(extra),
            "expected_by_kind": {
                kind: sum(fact.kind == kind for fact in snapshot.expected.values())
                for kind in sorted(set(DIMENSION_KINDS.values()))
            },
            "projected_by_kind": {
                kind: sum(key[2] == kind for key in projected_keys)
                for kind in sorted(set(DIMENSION_KINDS.values()))
            },
        },
        "candidate_categories": categories_report,
        "issue_categories": issue_report,
        "findings": sorted(findings, key=canonical_bytes),
    }
    report["report_sha256"] = report_digest(report)
    return report


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=REPOSITORY)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--projection", type=Path)
    parser.add_argument(
        "--require-verified",
        action="store_true",
        help="fail unless every candidate and issue is independently verified",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        report = verify(args.root.resolve(), args.cache.resolve(), args.projection)
    except VerificationError as error:
        print(f"independent CICS source verification failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, ensure_ascii=True, sort_keys=True))
    if report["status"] in {"mismatch", "requires-reprojection"}:
        return 1
    if args.require_verified and report["status"] != "verified":
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
