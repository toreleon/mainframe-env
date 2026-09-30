#!/usr/bin/env python3
"""Independently verify a CICS application-source batch projection.

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
sys.path.insert(0, str(Path(__file__).resolve().parent))
from cics_application_source_batches import SourceBatch, source_batch  # noqa: E402


DEFAULT_BATCH = source_batch("a")
# Backwards-compatible aliases for focused sources-a tests and callers.
MAP_PATH = DEFAULT_BATCH.map_path
MANIFEST_PATH = DEFAULT_BATCH.manifest_path
PLAN_PATH = DEFAULT_BATCH.plan_path
PROJECTION_PATH = DEFAULT_BATCH.projection_path
SUPPLEMENTS_PATH = DEFAULT_BATCH.supplements_path

REPORT_SCHEMA = "mainframe-env.cics-source-independent-verification@1"
REPORT_DOMAIN = b"mainframe-env.cics-source-independent-verification@1\0"
FRAGMENT_DOMAIN = b"mainframe-env.cics-source-fragment@1\0"
CONDITION_NAME_DOMAIN = b"mainframe-env.cics-condition-name-authority@1\0"
CONDITION_NAME_PROFILE = "cics-eibresp-condition-name@1"
CONDITION_NAME_OPTION = "CONDITION-NAME"
CONDITION_NAME_TOPIC = (
    "SSJL4D_6.x/reference-diagnostics/eib/dfhp4_eibfields.html"
)
CONDITION_NAME_TABLE = "dfhp4au__table_nt5_kw4_b1c"
CONDITION_NAME_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-condition-name-authority@1, "
    "one NUL byte (0x00), then UTF-8 compact JSON of the lexicographically "
    "sorted condition-name array"
)
CONDITION_PAIR_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-condition-name-authority@1, "
    "one NUL byte (0x00), then ASCII 'pairs', one NUL byte (0x00), then UTF-8 "
    "compact JSON of lexicographically name-sorted [name,code] pairs"
)
STRUCTURE_DOMAIN = FRAGMENT_DOMAIN
SUPPLEMENT_DOMAIN = b"mainframe-env.cics-source-supplements@1\0"
CORPUS_DOMAIN = b"mainframe-env.cics-source-corpus@1\0"
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
        "mapped-context-predicate",
        "mapped-dpl-applicability",
        "mapped-language-applicability",
        "mapped-one-hop-context",
        "dpl-server-restrictions",
        "threadsafe-command-list",
        "gap-exec-interface-classification",
        "gap-eibfn-identity",
        "traceid-monitor-compatibility",
        "traceid-dfhcmp-compatibility",
        "source-resolution-context",
        "global-response-codes",
        "gds-send-response-contract",
        "appc-basic-state-transitions",
        "appc-basic-state-transitions-sl0",
        "appc-basic-state-transitions-sl1",
        "appc-basic-state-transitions-sl2",
        "appc-mapped-state-transitions",
        "appc-mapped-state-transitions-sl0",
        "appc-mapped-state-transitions-sl1",
        "appc-mapped-state-transitions-sl2",
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
    if re.match(r"^condition(?:\s*\(|$)", normalize_text(value), re.IGNORECASE):
        return CONDITION_NAME_OPTION
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


def condition_symbol(
    value: str, depth: int, response_codes: dict[str, int] | None = None
) -> str:
    normalized = normalize_text(value)
    top = re.match(r"^([0-9]{1,5})\s+([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if top:
        return f"RESP-{top.group(1)}-{top.group(2)}"
    nested = re.match(r"^([0-9]{1,5})\b", normalized)
    if depth > 0 and nested:
        return f"RESP2-{nested.group(1)}"
    symbol = re.match(r"^([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if symbol:
        name = symbol.group(1)
        code = (response_codes or {}).get(name)
        return f"RESP-{code}-{name}" if code is not None else f"SYMBOL-{name}"
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
        if kind == "keyword" and value == "condition":
            kind = "variable"
        elif kind == "keyword" and value == "today":
            kind = "fragment"
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
        if len(titles) > 1 or not pieces:
            raise VerificationError("syntax diagram title or SVG cardinality differs")
        fragment_names = [
            node
            for node in container.descendants()
            if node.tag == "span" and "fragmentname" in node.classes()
        ]
        if titles:
            title = titles[0].text()
        elif len(fragment_names) == 1:
            title = fragment_names[0].text()
        else:
            title = container.attrs.get("id", "")
        if not title:
            raise VerificationError("untitled syntax fragment lacks a stable identity")
        result.append(Diagram(title, container, pieces))
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
    corpus: dict[str, Any]
    plan: dict[str, Any]
    topics: dict[str, Topic]
    expected: dict[tuple[str, str, str, str, str], ExpectedFact]
    top_options: dict[str, set[str]]
    option_definitions: dict[tuple[str, str], list[Definition]]
    supplement_entries: dict[str, dict[str, Any]]
    response_codes: dict[str, int]
    condition_name_authority: dict[str, Any] | None


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


def manifest_digest(topics: Iterable[dict[str, Any]]) -> str:
    lines = sorted(f"{row['topic_path']} {row['sha256']}\n" for row in topics)
    return sha256_bytes("".join(lines).encode("utf-8"))


def corpus_digest(corpus: dict[str, Any]) -> str:
    common = [
        "mapping",
        "topic_manifest",
        "counts",
        "mapped_command_topics",
        "linked_context_topics",
        "manual_topics",
        "excluded_navigation",
    ]
    suffix = (
        ["source_gaps"]
        if "source_gaps" in corpus
        else ["supplemental_topics", "source_resolutions"]
    )
    try:
        material = {key: corpus[key] for key in common + suffix}
    except KeyError as error:
        raise VerificationError(f"source corpus closure field is missing: {error}") from error
    return "sha256:" + sha256_bytes(CORPUS_DOMAIN + canonical_bytes(material))


def load_supplements(
    root: Path,
    cache: Path,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> tuple[dict[str, Topic], dict[str, dict[str, Any]]]:
    config = source_batch(batch)
    if config.supplements_path is None:
        return {}, {}
    path = root / config.supplements_path
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
    repin = receipt.get("repin")
    if repin is not None:
        if not isinstance(repin, dict) or repin.get("historical_receipt_path") != (
            "conformance/0.9/cics/application-api-sources-a-supplements-2026-09-10.json"
        ):
            raise VerificationError("supplement repin history path differs")
        historical_path = root / repin["historical_receipt_path"]
        historical = read_object(historical_path)
        if (
            repin.get("historical_receipt_sha256") != file_sha256(historical_path)
            or historical.get("capture") != capture
            or not isinstance(historical.get("topics"), list)
        ):
            raise VerificationError("supplement historical capture differs")
        old_lines = sorted(
            f"{row['topic_path']} {row['bytes']} {row['sha256']}\n"
            for row in historical["topics"]
        )
        old_identity = "sha256:" + sha256_bytes(
            b"mainframe-env.cics-source-supplement-identity@1\0"
            + "".join(old_lines).encode("utf-8")
        )
        changed = [
            (old, new)
            for old, new in zip(historical["topics"], raw_topics)
            if old != new
        ]
        if (
            capture.get("identity_sha256") != old_identity
            or len(changed) != 1
            or changed[0][0].get("topic_path") != repin.get("topic_path")
            or changed[0][1].get("topic_path") != repin.get("topic_path")
            or changed[0][1].get("sha256") != repin.get("topic_sha256")
            or changed[0][1].get("bytes") != repin.get("topic_bytes")
            or repin.get("issue") != 173
            or repin.get("verified_on") != "2026-09-28"
            or repin.get("verification_method") != "user-chrome-browser-control"
        ):
            raise VerificationError("supplement repin identity differs")
    elif capture.get("identity_sha256") != identity:
        raise VerificationError("supplement capture identity digest differs")
    return topics, entries


def load_corpus_supplements(
    corpus: dict[str, Any], cache: Path
) -> tuple[dict[str, Topic], dict[str, dict[str, Any]]]:
    raw_topics = corpus.get("supplemental_topics", [])
    if not isinstance(raw_topics, list):
        raise VerificationError("corpus supplemental topic set is invalid")
    topics: dict[str, Topic] = {}
    entries: dict[str, dict[str, Any]] = {}
    for entry in raw_topics:
        if not isinstance(entry, dict):
            raise VerificationError("corpus supplemental topic is not an object")
        path = entry.get("topic_path")
        digest = entry.get("sha256")
        size = entry.get("bytes")
        product = entry.get("product_key")
        dimensions = entry.get("allowed_evidence_dimensions")
        boundary = entry.get("target_authority_boundary")
        if (
            not isinstance(path, str)
            or not isinstance(product, str)
            or not path.startswith(product + "/")
            or not isinstance(digest, str)
            or re.fullmatch(r"[0-9a-f]{64}", digest) is None
            or not isinstance(size, int)
            or isinstance(size, bool)
            or size <= 0
            or boundary != "cross-product-not-target-authority"
            or entry.get("target_product_authority") is not False
            or entry.get("semantic_authority") is not False
            or entry.get("coverage_credit") != 0
            or not isinstance(dimensions, list)
            or not set(dimensions)
            <= {
                "syntax",
                "options",
                "operand-directions",
                "conditions",
                "execution-context",
            }
            or path in topics
        ):
            raise VerificationError("corpus supplement authority boundary differs")
        canonical, _ = cache_names(path, digest)
        if entry.get("cache_key") != canonical:
            raise VerificationError(f"corpus supplement cache key differs: {path}")
        parsed = urlsplit(str(entry.get("content_url", "")))
        if parsed.scheme != "https" or parsed.netloc != "www.ibm.com":
            raise VerificationError(f"corpus supplement URL is not IBM HTML: {path}")
        body = cached_topic(cache, path, digest, size)
        topics[path] = Topic(
            path,
            digest,
            size,
            parse_fragment(body),
            product,
            boundary,
            frozenset(dimensions),
        )
        entries[path] = entry
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

    def choice_branches(item: SyntaxItem) -> set[tuple[Element, Element]]:
        result: set[tuple[Element, Element]] = set()
        for token in item.tokens:
            current = token.node.parent
            branch = token.node
            while current is not None and current is not diagram.node:
                if "groupchoice" in current.classes():
                    result.add((current, branch))
                branch = current
                current = current.parent
        return result

    forbidden_branches = {
        branch
        for item in items
        if {
            match.group(0)
            for token in item.tokens
            if token.kind == "keyword"
            for match in [re.match(r"^[A-Z][A-Z0-9-]*", token.value)]
            if match is not None
        }
        & sibling_only
        for _, branch in choice_branches(item)
    }
    narrowed_choices = {
        parent
        for item in items
        for parent, branch in choice_branches(item)
        if branch in forbidden_branches
    }
    result: list[SyntaxItem] = []
    for item in items:
        names = {
            match.group(0)
            for token in item.tokens
            if token.kind == "keyword"
            for match in [re.match(r"^[A-Z][A-Z0-9-]*", token.value)]
            if match is not None
        }
        branches = choice_branches(item)
        if names & sibling_only or any(
            branch in forbidden_branches for _, branch in branches
        ):
            continue
        relation = (
            "required"
            if item.relation == "alternative"
            and (
                bool(names & selected)
                or any(parent in narrowed_choices for parent, _ in branches)
            )
            else item.relation
        )
        result.append(SyntaxItem(relation, item.tokens, item.node))
    return result


def independent_syntax_option_name(
    item: SyntaxItem, documented_options: set[str]
) -> str | None:
    leading = option_name("".join(token.value for token in item.tokens))
    if leading in documented_options:
        return leading
    matches = [
        candidate
        for token in item.tokens
        if token.kind == "keyword"
        and (candidate := option_name(token.value)) in documented_options
    ]
    return matches[-1] if matches else None


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


INDEPENDENT_OPTION_LEGALITY_PATTERNS = tuple(
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\bif (?:you )?specif(?:y|ied)\b.{0,320}\b(?:must|cannot|may not|need not|required)\b",
        r"\b(?:must|cannot|may not|must not)\b.{0,240}\b(?:specif(?:y|ied)|used|combined)\b",
        r"\b(?:mutually exclusive|cannot be used together|not valid with|only valid with|required with)\b",
        r"\b(?:must|required|cannot|may not|must not|need not|only|unless|either|should)\b",
    )
)
INDEPENDENT_CONTEXT_PREDICATE_PATTERNS = tuple(
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\b(?:is|are) threadsafe (?:when|if|only)\b",
        r"\b(?:is|are) non-threadsafe (?:when|if)\b",
        r"\bvalid only\b",
        r"\b(?:for use|supported|available) only\b",
        r"\bnot available when\b",
        r"\bcannot be used (?:from|when|in)\b",
        r"\bonly\b.{0,160}\bwhen\b",
    )
)


def independent_option_legality_descriptions(definition: Definition) -> list[Element]:
    return [
        description
        for description in definition.descriptions
        if any(
            pattern.search(normalize_text(description.text()))
            for pattern in INDEPENDENT_OPTION_LEGALITY_PATTERNS
        )
    ]


def independent_language_profile(node: Element) -> str | None:
    text = normalize_text(node.text()).lower()
    if re.search(r"assembler-language and c programs only", text):
        return "assembler-and-c-only"
    if (
        "for use only in non-language environment" in text
        and "amode(64) assembler" in text
        and "application programs" in text
    ):
        return "non-le-amode64-assembler-only"
    if (
        "supported only in cobol" in text
        and "pl/i" in text
        and "assembler language applications" in text
        and "not amode(64) assembler" in text
    ):
        return "cobol-pli-non-amode64-assembler-only"
    return None


def independent_language_node(node: Element) -> bool:
    return independent_language_profile(node) is not None and (
        (node.tag == "p" and "shortdesc" in node.classes())
        or (
            node.tag == "div"
            and "cds--inline-notification__subtitle" in node.classes()
        )
    )


def independent_language_applicability(profile: str) -> dict[str, Any]:
    return {
        "cobol": (
            "allowed"
            if profile
            in {
                "exec-cics-all-supported-languages",
                "cobol-pli-non-amode64-assembler-only",
            }
            else "not-applicable"
        ),
        "language_restriction": {"profile": profile},
    }


def independently_is_context_predicate(node: Element) -> bool:
    if node.tag != "p" or section_title(node) not in {
        "Syntax",
        "Description",
        "Rules",
    }:
        return False
    text = normalize_text(node.text())
    return any(pattern.search(text) for pattern in INDEPENDENT_CONTEXT_PREDICATE_PATTERNS)


def independently_is_mapped_dpl_predicate(label: str, node: Element) -> bool:
    return (
        label == "GDS EXTRACT ATTRIBUTES"
        and node.tag == "td"
        and normalize_text(node.text()).casefold()
        == "invreq for a dpl server program."
    )


def independent_syntax_head(
    tokens: list[dict[str, Any]], documented_options: set[str]
) -> str:
    head_parts: list[str] = []
    for token in tokens:
        if token["kind"] != "keyword":
            break
        value = str(token["value"])
        prefix = value.split("(", 1)[0].strip()
        words = prefix.split()
        if "(" in value and words and words[-1] in documented_options:
            if words[:-1]:
                head_parts.extend(words[:-1])
            elif not head_parts:
                head_parts.append(prefix)
            break
        if prefix:
            head_parts.append(prefix)
        if "(" in value:
            break
    return normalize_text(" ".join(head_parts)).upper()


def independent_syntax_identity(
    label: str, syntax_head: str, tokens: list[dict[str, Any]]
) -> tuple[str, list[dict[str, str]]]:
    if label == "WAIT" and syntax_head == "GDS WAIT":
        return "documented-alias", []
    option_relations: dict[str, set[str]] = {}
    for token in tokens:
        prefix = str(token["value"]).split("(", 1)[0]
        if token["kind"] != "keyword" or not prefix:
            continue
        option_relations.setdefault(prefix.split()[-1], set()).add(
            str(token["relation"])
        )

    def present_in_every_retained_branch(name: str) -> bool:
        all_branches: dict[str, set[str]] = {}
        name_branches: dict[str, set[str]] = {}
        for token in tokens:
            if token["kind"] != "keyword":
                continue
            prefix = str(token["value"]).split("(", 1)[0]
            token_name = prefix.split()[-1] if prefix else ""
            parts = str(token.get("group_path", "")).split("/")
            for index, part in enumerate(parts[:-1]):
                if not part.startswith("groupchoice["):
                    continue
                choice = "/".join(parts[: index + 1])
                branch = "/".join(parts[: index + 2])
                all_branches.setdefault(choice, set()).add(branch)
                if token_name == name:
                    name_branches.setdefault(choice, set()).add(branch)
        return any(
            len(branches) > 1 and name_branches.get(choice) == branches
            for choice, branches in all_branches.items()
        )
    if syntax_head == label:
        relation = "exact"
    elif label.startswith(syntax_head + " ") or syntax_head.startswith(label + " "):
        relation = "catalog-qualified"
    else:
        return "continuation", []
    suffix = label[len(syntax_head) :].strip().split()
    discriminators = [
        {"name": value, "state": "present"}
        for value in suffix
        if option_relations.get(value) == {"required"}
        or present_in_every_retained_branch(value)
    ]
    if label == "ASKTIME":
        discriminators.append({"name": "ABSTIME", "state": "absent"})
    if label == "ASKTIME ABSTIME":
        if "ABSTIME" not in option_relations:
            raise VerificationError("ASKTIME ABSTIME discriminator is absent")
        discriminators = [{"name": "ABSTIME", "state": "present"}]
    return relation, discriminators


def syntax_value(
    label: str,
    diagram: Diagram,
    ordinal: int,
    panel_index: int,
    panel_total: int,
    documented_options: set[str],
    items: Iterable[SyntaxItem] | None = None,
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
    syntax_head = independent_syntax_head(tokens, documented_options)
    if not syntax_head:
        raise VerificationError("independent syntax command head is empty")
    identity_relation, identity_discriminators = independent_syntax_identity(
        label, syntax_head, tokens
    )
    return {
        "type": "syntax",
        "variant": f"diagram-{ordinal:04d}",
        "syntax_head": syntax_head,
        "panel": {
            "index": panel_index,
            "total": panel_total,
            "role": (
                "continuation"
                if identity_relation == "continuation"
                else "command-head"
            ),
        },
        "identity_relation": identity_relation,
        "identity_discriminators": identity_discriminators,
        "tokens": tokens,
        "structure_sha256": fragment_sha256(encoded),
    }


def independent_dynamic_condition_clause(
    definition: Definition, document: Element | None, arguments: list[str]
) -> dict[str, Any]:
    if option_name(definition.stack[0]) != CONDITION_NAME_OPTION:
        return {}
    if document is None:
        raise VerificationError("dynamic condition option lacks its source document")
    limits = [
        node
        for node in document.descendants()
        if node.tag == "p"
        and re.search(
            r"cannot include\s+more\s+than\s+(?:16|sixteen)\s+conditions",
            normalize_text(node.text()),
            re.IGNORECASE,
        )
    ]
    if len(limits) != 1:
        raise VerificationError("dynamic condition occurrence limit is not unique")
    if set(arguments) == {"label"}:
        label_operand = "optional"
    elif arguments == ["none"]:
        label_operand = "forbidden"
    else:
        raise VerificationError("dynamic condition label shape differs")
    return {
        "dynamic_name_profile": CONDITION_NAME_PROFILE,
        "minimum_occurrences": 1,
        "maximum_occurrences": 16,
        "label_operand": label_operand,
        "occurrence_limit_fragment_sha256": fragment_sha256(
            normalize_text(limits[0].text())
        ),
    }


def option_value(
    definition: Definition, document: Element | None = None
) -> dict[str, Any]:
    name = option_name(definition.stack[0])
    if name is None:
        raise VerificationError("option definition has no symbolic name")
    arguments = argument_markers(definition.term)
    return {
        "type": "option",
        "term": name,
        "arguments": arguments,
        "definition_count": len(definition.descriptions),
        "description_fragment_sha256s": [
            fragment_sha256(normalize_text(description.text()))
            for description in definition.descriptions
        ],
        "option_legality": (
            "bounded-prose"
            if independent_option_legality_descriptions(definition)
            else "structural"
        ),
        "depth": definition.depth,
        "stack": option_stack(definition.stack),
        **independent_dynamic_condition_clause(definition, document, arguments),
    }


def condition_value(
    definition: Definition, response_codes: dict[str, int] | None = None
) -> dict[str, Any]:
    return {
        "type": "condition",
        "condition_stack": [
            condition_symbol(value, depth, response_codes)
            for depth, value in enumerate(definition.stack)
        ],
        "definition_count": len(definition.descriptions),
        "trigger_fragment_sha256s": [
            fragment_sha256(normalize_text(description.text()))
            for description in definition.descriptions
        ],
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


def independently_load_response_codes(
    plan: dict[str, Any], topics: dict[str, Topic]
) -> dict[str, int]:
    selectors = [
        item
        for item in plan.get("manual_context_selectors", [])
        if item.get("id") == "global-response-codes"
    ]
    if not selectors:
        return {}
    if len(selectors) != 1:
        raise VerificationError("response-code selector is not unique")
    selector = selectors[0]
    topic = topics.get(str(selector.get("topic_path")))
    if topic is None:
        raise VerificationError("response-code topic is outside source closure")
    result: dict[str, int] = {}
    for fragment in selector.get("fragment_selectors", []):
        identity = fragment.get("id")
        tables = [
            node
            for node in topic.document.descendants()
            if node.tag == "table" and node.attrs.get("id") == identity
        ]
        if len(tables) != 1:
            raise VerificationError("response-code table locator is not unique")
        for row in tables[0].descendants():
            if row.tag != "tr":
                continue
            cells = verifier_cells(row)
            if len(cells) < 2:
                continue
            name = normalize_text(cells[0].text()).upper()
            value = normalize_text(cells[1].text())
            if re.fullmatch(r"[A-Z][A-Z0-9-]{0,31}", name) is None or not value.isdigit():
                continue
            code = int(value)
            if not 0 <= code <= 255 or (name in result and result[name] != code):
                raise VerificationError(f"response-code table conflicts for {name}")
            result[name] = code
    if not result:
        raise VerificationError("response-code table is empty")
    return result


def independently_condition_name_authority(
    topics: dict[str, Topic], response_codes: dict[str, int]
) -> dict[str, Any] | None:
    topic = topics.get(CONDITION_NAME_TOPIC)
    if topic is None:
        return None
    tables = [
        node
        for node in topic.document.descendants()
        if node.tag == "table" and node.attrs.get("id") == CONDITION_NAME_TABLE
    ]
    if len(tables) != 1:
        raise VerificationError("EIBRESP condition-name table locator is not unique")
    name_to_code: dict[str, int] = {}
    for row in tables[0].descendants():
        if row.tag != "tr":
            continue
        cells = verifier_cells(row)
        for offset in (0, 2):
            if len(cells) <= offset + 1:
                continue
            code_text = normalize_text(cells[offset].text())
            name = normalize_text(cells[offset + 1].text()).upper()
            if not code_text.isdigit() or re.fullmatch(
                r"[A-Z][A-Z0-9-]{0,31}", name
            ) is None:
                continue
            code = int(code_text)
            if not 0 <= code <= 255 or (
                name in name_to_code and name_to_code[name] != code
            ):
                raise VerificationError(
                    f"EIBRESP condition-name table conflicts for {name}"
                )
            name_to_code[name] = code
    if not name_to_code:
        raise VerificationError("EIBRESP condition-name table is empty")
    for name in set(name_to_code) & set(response_codes):
        if name_to_code[name] != response_codes[name]:
            raise VerificationError(
                f"EIBRESP and response-table codes conflict for {name}"
            )
    allowed_names = sorted(name_to_code)
    names_encoded = json.dumps(
        allowed_names, ensure_ascii=True, sort_keys=False, separators=(",", ":")
    ).encode("utf-8")
    condition_pairs = [[name, name_to_code[name]] for name in allowed_names]
    pairs_encoded = json.dumps(
        condition_pairs, ensure_ascii=True, sort_keys=False, separators=(",", ":")
    ).encode("utf-8")
    return {
        "profile": CONDITION_NAME_PROFILE,
        "topic_path": CONDITION_NAME_TOPIC,
        "topic_sha256": "sha256:" + topic.sha256,
        "table_id": CONDITION_NAME_TABLE,
        "allowed_names": allowed_names,
        "conditions": [
            {"name": name, "code": name_to_code[name]} for name in allowed_names
        ],
        "allowed_names_digest_definition": CONDITION_NAME_DIGEST_DEFINITION,
        "allowed_names_sha256": "sha256:"
        + hashlib.sha256(CONDITION_NAME_DOMAIN + names_encoded).hexdigest(),
        "conditions_digest_definition": CONDITION_PAIR_DIGEST_DEFINITION,
        "conditions_sha256": "sha256:"
        + hashlib.sha256(
            CONDITION_NAME_DOMAIN + b"pairs\0" + pairs_encoded
        ).hexdigest(),
    }


def build_source_snapshot(
    root: Path,
    cache: Path,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> SourceSnapshot:
    """Derive all expected structural facts before any projection is opened."""

    config = source_batch(batch)
    manifest = read_object(root / config.manifest_path)
    mapping = read_object(root / config.map_path)
    corpus = (
        read_object(root / config.corpus_path)
        if (root / config.corpus_path).exists()
        else {}
    )
    plan = read_object(root / config.plan_path)
    topic_rows = manifest.get("topics")
    if not isinstance(topic_rows, list) or not topic_rows:
        raise VerificationError("source manifest has no topics")
    if corpus:
        inputs = plan.get("inputs", {})
        manifest_file_sha256 = file_sha256(root / config.manifest_path)
        logical_manifest_sha256 = "sha256:" + manifest_digest(topic_rows)
        if (
            mapping.get("mapping_sha256") != inputs.get("mapping", {}).get("sha256")
            or corpus.get("corpus_sha256") != corpus_digest(corpus)
            or corpus.get("corpus_sha256") != inputs.get("corpus", {}).get("sha256")
            or manifest_file_sha256
            != inputs.get("topic_manifest", {}).get("file_sha256")
            or logical_manifest_sha256
            != inputs.get("topic_manifest", {}).get("topic_manifest_sha256")
            or manifest.get("topic_manifest_digest") != logical_manifest_sha256[7:]
            or manifest.get("topic_count") != len(topic_rows)
            or manifest.get("total_bytes")
            != sum(int(row.get("bytes", 0)) for row in topic_rows)
            or corpus.get("topic_manifest", {}).get("file_sha256")
            != manifest_file_sha256
            or corpus.get("mapping", {}).get("sha256")
            != mapping.get("mapping_sha256")
        ):
            raise VerificationError("source input hash or closure differs")
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

    if config.supplements_path is not None:
        supplement_topics, supplement_entries = load_supplements(root, cache, config)
    else:
        supplement_topics, supplement_entries = load_corpus_supplements(corpus, cache)
    if set(topics) & set(supplement_topics):
        raise VerificationError("supplement topic aliases the target corpus")
    topics.update(supplement_topics)
    response_codes = independently_load_response_codes(plan, topics)
    condition_name_authority = independently_condition_name_authority(
        topics, response_codes
    )

    rows = mapping.get("rows")
    if not isinstance(rows, list) or not rows:
        raise VerificationError("source map has no rows")
    if mapping.get("schema_version") is not None and (
        len(rows) != config.row_count
        or any(not config.contains_row(str(row.get("official_row", ""))) for row in rows)
    ):
        raise VerificationError("source map row range differs from selected batch")
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
            if entry is not None:
                if official_row not in entry.get("applies_to_rows", []):
                    raise VerificationError(
                        f"supplement escapes its declared row: {official_row}"
                    )
                sources.append({"topic_path": direct, "role": "supplement"})
            elif (
                str(direct) in topics
                and str(direct).startswith("SSJL4D_6.x/")
                and str(direct) in (resolution or {}).get("target_context_topics", [])
            ):
                sources.append({"topic_path": direct, "role": "resolved-target"})
            else:
                raise VerificationError(
                    f"resolved topic escapes its declared row: {official_row}"
                )
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
            options = indexed.get("Options")
            selected_definitions = (
                [
                    definition
                    for definition in definition_inventory(options)
                    if definition_applies(row, siblings, definition)
                ]
                if options and "options" in allowed
                else []
            )
            documented_options = {
                name
                for definition in selected_definitions
                if (name := option_name(definition.stack[0])) is not None
            }
            syntax = indexed.get("Syntax")
            if syntax and "syntax" in allowed:
                selected = [
                    (ordinal, diagram)
                    for ordinal, diagram in enumerate(diagrams(syntax), 1)
                    if diagram_applies(row, siblings, diagram)
                ]
                selected_items_by_ordinal = {
                    ordinal: diagram_items_for_row(row, siblings, diagram)
                    for ordinal, diagram in selected
                }
                if row.get("selection_kind") == "shared-page" and len(siblings) > 1:
                    selected_option_names = {
                        name
                        for ordinal, _ in selected
                        for item in selected_items_by_ordinal[ordinal]
                        if (
                            name := independent_syntax_option_name(
                                item, documented_options
                            )
                        )
                        is not None
                    }
                    all_syntax_option_names = {
                        name
                        for _, diagram in selected
                        for item in diagram_items(diagram)
                        if (
                            name := independent_syntax_option_name(
                                item, documented_options
                            )
                        )
                        is not None
                    }
                    selected_definitions = [
                        definition
                        for definition in selected_definitions
                        if (name := option_name(definition.stack[0])) is None
                        or name not in all_syntax_option_names
                        or name in selected_option_names
                    ]
                    documented_options = {
                        name
                        for definition in selected_definitions
                        if (name := option_name(definition.stack[0])) is not None
                    }
                for panel_index, (ordinal, diagram) in enumerate(selected, 1):
                    add_expected(
                        expected,
                        ExpectedFact(
                            official_row,
                            "syntax",
                            "source-syntax",
                            path,
                            element_path(diagram.node),
                            syntax_value(
                                str(row["label"]),
                                diagram,
                                ordinal,
                                panel_index,
                                len(selected),
                                documented_options,
                                selected_items_by_ordinal[ordinal],
                            ),
                        ),
                    )
            if options and "options" in allowed:
                for definition in selected_definitions:
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
                            option_value(definition, topic.document),
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
                            condition_value(definition, response_codes),
                        ),
                    )
    return SourceSnapshot(
        manifest,
        mapping,
        corpus,
        plan,
        topics,
        expected,
        top_options,
        option_definitions,
        supplement_entries,
        response_codes,
        condition_name_authority,
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
    def title_article(titles: list[Element]) -> Element | None:
        if len(titles) != 1:
            return None
        current = titles[0].parent
        while current is not None and current.tag != "article":
            current = current.parent
        return current

    def common_article_parent(articles: list[Element]) -> Element | None:
        if len(articles) < 2:
            return None
        chains: list[list[Element]] = []
        for article in articles:
            chain: list[Element] = []
            current = article.parent
            while current is not None:
                chain.append(current)
                current = current.parent
            chains.append(list(reversed(chain)))
        common: Element | None = None
        for values in zip(*chains):
            if any(value is not values[0] for value in values[1:]):
                break
            common = values[0]
        return (
            common
            if common is not None and common.tag != "__document__"
            else None
        )

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
        return title_article(titles) if not matches else None
    articles = [node for node in topic.document.descendants() if node.tag == "article"]
    if len(articles) == 1:
        return articles[0]
    titles = [
        node
        for node in topic.document.descendants()
        if node.tag == "h1" and "topictitle1" in node.classes()
    ]
    return title_article(titles) or common_article_parent(articles)


def independently_derive_compatibility_stubs(snapshot: SourceSnapshot) -> set[str]:
    result: set[str] = set()
    for row in snapshot.mapping.get("rows", []):
        official_row = str(row.get("official_row"))
        if row.get("state") != "mapped":
            continue
        if any(
            fact.official_row == official_row
            and fact.dimension in {"syntax", "options", "conditions"}
            for fact in snapshot.expected.values()
        ):
            continue
        matches = 0
        for source in row.get("topics", []):
            topic = snapshot.topics.get(str(source.get("topic_path")))
            if topic is None:
                continue
            text = topic.document.text().casefold()
            if "supported for compatibility" not in text or "superseded" not in text:
                continue
            successors = [
                node
                for node in topic.document.descendants()
                if node.tag == "a"
                and normalize_text(node.text()).casefold() != "cics command summary"
                and (target := link_target(node.attrs.get("href", ""), topic.path))
                is not None
                and target[0] in snapshot.topics
            ]
            if len(successors) != 1:
                raise VerificationError(
                    f"compatibility successor is not independently unique: {topic.path}"
                )
            matches += 1
        if matches == 1:
            result.add(official_row)
    return result


BOUNDED_SOURCE_ISSUE_CODES = frozenset(
    {
        "syntax-panel-composition-unresolved",
        "prose-option-legality-not-structured",
        "context-predicate-not-structured",
        "shared-condition-applicability-unresolved",
    }
)


def independently_derive_bounded_source_issues(
    snapshot: SourceSnapshot,
) -> set[tuple[str, str, str, str]]:
    expected: set[tuple[str, str, str, str]] = set()
    rows = snapshot.mapping.get("rows", [])
    rows_by_topic: dict[str, list[dict[str, Any]]] = {}
    resolutions = {
        str(item["official_row"]): item
        for item in snapshot.plan.get("source_resolutions", [])
    }
    for row in rows:
        for source in row.get("topics", []):
            rows_by_topic.setdefault(str(source["topic_path"]), []).append(row)

    for fact in snapshot.expected.values():
        if (
            fact.kind == "source-syntax"
            and fact.value.get("panel", {}).get("role") == "continuation"
        ):
            expected.add(
                (
                    fact.official_row,
                    "syntax-panel-composition-unresolved",
                    fact.topic_path,
                    fact.structural_path,
                )
            )

    for row in rows:
        official_row = str(row["official_row"])
        sources = [dict(source) for source in row.get("topics", [])]
        resolution = resolutions.get(official_row)
        direct = resolution.get("direct_supplement_topic") if resolution else None
        if direct:
            sources.append({"topic_path": direct, "role": "supplement"})
        for source in sources:
            path = str(source["topic_path"])
            topic = snapshot.topics.get(path)
            if topic is None:
                continue
            indexed = sections(topic.document)
            options = indexed.get("Options")
            if options is not None:
                siblings = rows_by_topic.get(path, [row])
                for definition in definition_inventory(options):
                    if not definition_applies(row, siblings, definition):
                        continue
                    if option_name(definition.stack[0]) is None:
                        continue
                    for description in independent_option_legality_descriptions(
                        definition
                    ):
                        expected.add(
                            (
                                official_row,
                                "prose-option-legality-not-structured",
                                path,
                                element_path(description),
                            )
                        )
            conditions = indexed.get("Conditions")
            if (
                conditions is not None
                and row.get("selection_kind") == "shared-page"
                and len(rows_by_topic.get(path, [row])) > 1
            ):
                discriminators = row_discriminators(rows_by_topic[path])
                sibling_only = (
                    set().union(*discriminators.values())
                    - discriminators[official_row]
                )
                for definition in definition_inventory(conditions):
                    if definition.depth == 0:
                        continue
                    matching_descriptions = [
                        description
                        for description in definition.descriptions
                        if any(
                            re.search(
                                rf"\b{re.escape(name)}\b",
                                normalize_text(description.text()).upper(),
                            )
                            for name in sibling_only
                        )
                    ]
                    if matching_descriptions:
                        expected.add(
                            (
                                official_row,
                                "shared-condition-applicability-unresolved",
                                path,
                                element_path(matching_descriptions[0]),
                            )
                        )
            for node in topic.document.descendants():
                if independently_is_mapped_dpl_predicate(
                    str(row["label"]), node
                ):
                    expected.add(
                        (
                            official_row,
                            "context-predicate-not-structured",
                            path,
                            element_path(node),
                        )
                    )
                if (
                    independently_is_context_predicate(node)
                    and independent_language_profile(node) is None
                ):
                    expected.add(
                        (
                            official_row,
                            "context-predicate-not-structured",
                            path,
                            element_path(node),
                        )
                    )
    return expected


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


VERIFIER_THREADSAFE_ALIASES = {
    "DELETE FILE": "DELETE",
    "ENDBROWSE FILE": "ENDBR",
    "READ FILE": "READ",
    "READNEXT FILE": "READNEXT",
    "READPREV FILE": "READPREV",
    "REQUEST ENCRYPTPTKT": "REQUEST ENCYRPTPTKT",
    "RESETBROWSE FILE": "RESETBR",
    "REWRITE FILE": "REWRITE",
    "STARTBROWSE FILE": "STARTBR",
    "UNLOCK FILE": "UNLOCK",
    "WRITE FILE": "WRITE",
}
VERIFIER_THREADSAFE_FORMS = {
    "WEB ENDBROWSE": (
        "WEB ENDBROWSE FORMFIELD",
        "WEB ENDBROWSE HTTPHEADER",
        "WEB ENDBROWSE QUERYPARM",
    ),
    "WEB READ": (
        "WEB READ FORMFIELD",
        "WEB READ HTTPHEADER",
        "WEB READ QUERYPARM",
    ),
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
}
VERIFIER_DPL_APPC_LABELS = frozenset(
    {
        "CONNECT PROCESS",
        "CONVERSE",
        "EXTRACT ATTRIBUTES",
        "EXTRACT PROCESS",
        "FREE",
        "ISSUE ABEND",
        "ISSUE CONFIRMATION",
        "ISSUE ERROR",
        "ISSUE PREPARE",
        "ISSUE SIGNAL",
        "RECEIVE",
        "SEND",
        "WAIT TERMINAL",
    }
)
VERIFIER_THREADSAFE_FILE_COMMANDS = frozenset(
    {
        "DELETE",
        "ENDBR",
        "READ",
        "READNEXT",
        "READPREV",
        "RESETBR",
        "REWRITE",
        "STARTBR",
        "UNLOCK",
        "WRITE FILE",
    }
)
VERIFIER_THREADSAFE_QUEUE_COMMANDS = frozenset(
    {"DELETEQ TD", "DELETEQ TS", "READQ TD", "READQ TS", "WRITEQ TD", "WRITEQ TS"}
)
VERIFIER_THREADSAFE_PREDICATES = {
    "program-link-locality": {
        "threadsafe_when": ["resource-local", "resource-remote-ipic"],
        "not_threadsafe_when": ["resource-remote-non-ipic"],
    },
    "file-control-storage-and-locality": {
        "threadsafe_when": [
            "resource-local",
            "resource-remote-ipic",
            "file-vsam-rls",
            "file-coupling-facility-data-table",
            "file-shared-data-table-read-or-browse",
        ],
        "not_threadsafe_when": [
            "resource-remote-non-ipic",
            "file-bdam",
            "file-shared-data-table-update",
            "file-nsr",
        ],
    },
    "queue-control-locality": {
        "threadsafe_when": ["resource-local", "resource-remote-ipic"],
        "not_threadsafe_when": ["resource-remote-non-ipic"],
    },
    "enq-locality": {
        "threadsafe_when": ["resource-definition-local"],
        "not_threadsafe_when": ["resource-definition-global"],
    },
    "deq-locality": {
        "threadsafe_when": ["resource-definition-local"],
        "not_threadsafe_when": ["resource-definition-global"],
    },
    "write-operator-reply-key9": {
        "threadsafe_when": ["not-reply-from-key9-tcb"],
        "not_threadsafe_when": ["reply-from-key9-tcb"],
    },
}
VERIFIER_LANGUAGE_PROFILE_BY_LABEL = {
    **{
        label: "assembler-and-c-only"
        for label in (
            "GDS ALLOCATE",
            "GDS ASSIGN",
            "GDS CONNECT PROCESS",
            "GDS EXTRACT ATTRIBUTES",
            "GDS EXTRACT PROCESS",
            "GDS FREE",
            "GDS ISSUE ABEND",
            "GDS ISSUE CONFIRMATION",
            "GDS ISSUE ERROR",
            "GDS ISSUE PREPARE",
            "GDS ISSUE SIGNAL",
            "GDS RECEIVE",
            "WAIT",
        )
    },
    **{
        label: "non-le-amode64-assembler-only"
        for label in ("FREEMAIN64", "GETMAIN64", "GET64 CONTAINER", "PUT64 CONTAINER")
    },
    **{
        label: "cobol-pli-non-amode64-assembler-only"
        for label in (
            "HANDLE AID",
            "HANDLE CONDITION",
            "IGNORE CONDITION",
            "POP HANDLE",
            "PUSH HANDLE",
        )
    },
}


def verifier_cells(row: Element) -> list[Element]:
    return [child for child in row.children if child.tag in {"td", "th"}]


def independently_derive_dpl(label: str, root: Element) -> dict[str, Any]:
    section_text = normalize_text(root.text()).upper()
    if root.tag == "section":
        required = (
            "ANY OF THE EXEC CICS WEB COMMANDS",
            "RESP2 VALUE OF 1",
            "EXTRACT TCPIP",
            "EXTRACT CERTIFICATE",
            "RESP2 VALUE OF 5",
            "APPC COMMANDS LISTED ARE PROHIBITED ONLY WHEN",
            "PRINCIPAL FACILITY",
        )
        if any(item not in section_text for item in required):
            raise VerificationError("independent DPL server prose contract differs")
    if label.startswith("WEB ") or label in {"EXTRACT TCPIP", "EXTRACT CERTIFICATE"}:
        resp2 = 5 if label in {
            "WEB EXTRACT",
            "EXTRACT TCPIP",
            "EXTRACT CERTIFICATE",
        } else 1
        return {
            "dpl_server": "restricted",
            "dpl_restriction": {
                "kind": "prohibited",
                "source_kind": "prose",
                "table_command": "WEB" if label.startswith("WEB ") else label,
                "prohibited_options": [],
                "requirement": "none",
                "restriction_predicates": [],
                "condition": "INVREQ",
                "resp2": resp2,
            },
        }
    rows: list[tuple[str, str]] = []
    for element in root.descendants():
        if element.tag != "tr":
            continue
        cells = verifier_cells(element)
        if len(cells) < 2:
            continue
        command = normalize_text(cells[0].text()).upper()
        options = normalize_text(cells[1].text()).upper()
        if command and command != "COMMAND" and (
            label == command
            or (command in {"ISSUE", "SEND"} and label.startswith(command + " "))
        ):
            rows.append((command, options))
    if not rows:
        return {"dpl_server": "allowed"}
    command, options = sorted(rows, key=lambda item: len(item[0]))[-1]
    suffix = label[len(command) :].strip()
    option_values = sorted(
        set(re.findall(r"[A-Z][A-Z0-9-]*(?:\([A-Z]+\))?", options))
    )
    if command in {"ISSUE", "SEND"}:
        roots = {value.split("(", 1)[0] for value in option_values}
        if suffix and suffix.split()[0] not in roots:
            return {"dpl_server": "allowed"}
        if suffix:
            kind = "prohibited"
            prohibited: list[str] = []
        else:
            kind = "option-restricted"
            prohibited = option_values
        requirement = "none"
    elif "SYNCONRETURN" in options:
        kind = "conditional"
        prohibited = []
        requirement = "synconreturn-required"
    elif options.startswith("TERMID"):
        kind = "conditional"
        prohibited = ["TERMID"]
        requirement = "termid-not-intersystem-session"
    elif options == "ALL":
        kind = "prohibited"
        prohibited = []
        requirement = "none"
    else:
        kind = "option-restricted"
        prohibited = option_values
        requirement = "none"
    predicates: list[str] = []
    if label in VERIFIER_DPL_APPC_LABELS:
        kind = "conditional"
        predicates.append("principal-facility")
        if label == "CONNECT PROCESS":
            predicates.append("connect-process-principal-facility-error-is-dpl")
    return {
        "dpl_server": "restricted",
        "dpl_restriction": {
            "kind": kind,
            "source_kind": "table",
            "table_command": command,
            "prohibited_options": prohibited,
            "requirement": requirement,
            "restriction_predicates": predicates,
            **(
                {"restriction_match": "any-of"}
                if prohibited and predicates
                else {}
            ),
            "condition": "INVREQ",
            "resp2": 200,
        },
    }


def independently_derive_threadsafe_condition(label: str) -> dict[str, Any]:
    source_label = VERIFIER_THREADSAFE_ALIASES.get(label, label)
    if source_label == "LINK":
        profile = "program-link-locality"
    elif source_label in VERIFIER_THREADSAFE_FILE_COMMANDS | {"WRITE"}:
        profile = "file-control-storage-and-locality"
    elif source_label in VERIFIER_THREADSAFE_QUEUE_COMMANDS:
        profile = "queue-control-locality"
    elif source_label == "ENQ":
        profile = "enq-locality"
    elif source_label == "DEQ":
        profile = "deq-locality"
    elif source_label == "WRITE OPERATOR":
        profile = "write-operator-reply-key9"
    else:
        raise VerificationError(
            f"independent conditional threadsafe profile is missing: {label}"
        )
    return {"profile": profile, **VERIFIER_THREADSAFE_PREDICATES[profile]}


def independently_derive_threadsafe(label: str, root: Element) -> dict[str, Any]:
    listed: dict[str, str] = {}
    for element in root.descendants():
        if element.tag != "li":
            continue
        raw = normalize_text(element.text(frozenset({"ul", "ol"}))).upper()
        conditional = "*" in raw or (
            "THREADSAFE IF" in raw and "NON-THREADSAFE IF" in raw
        )
        raw = re.sub(r"\s*\([^)]*\).*$", "", raw).replace("*", "").strip()
        if not raw or len(raw) > 80:
            continue
        alternatives = [part.strip() for part in raw.split(" AND ")]
        if all(re.fullmatch(r"[A-Z][A-Z0-9 ]*", item) for item in alternatives):
            for alternative in alternatives:
                listed[alternative] = "conditional" if conditional else "yes"
    wanted = VERIFIER_THREADSAFE_FORMS.get(
        label, (VERIFIER_THREADSAFE_ALIASES.get(label, label),)
    )
    statuses = {
        listed.get(re.sub(r"\s*\(CHANNEL\)$", "", item), "no")
        for item in wanted
    }
    status = next(iter(statuses)) if len(statuses) == 1 else "conditional"
    result: dict[str, Any] = {"threadsafe": status}
    if status == "conditional":
        result["threadsafe_condition"] = independently_derive_threadsafe_condition(
            label
        )
    return result


def independent_applicability(
    row: dict[str, Any],
    selector: str,
    snapshot: SourceSnapshot,
    source: Element,
    dimension: str,
) -> dict[str, Any] | None:
    resolution = next(
        (
            item
            for item in snapshot.plan.get("source_resolutions", [])
            if item.get("official_row") == row.get("official_row")
        ),
        None,
    )
    not_applicable = row.get("state") == "source-gap" and (
        resolution is None or resolution.get("resolution") == "target-internal-only"
    )
    label = str(row["label"])
    if selector == "global-command-format" and dimension == "execution-context" and any(
        child.tag == "h2" and child.attrs.get("id") == "dfhp4kt__title__2"
        for child in source.children
    ):
        result: dict[str, Any] = {
            "local_task": "not-applicable" if not_applicable else "allowed"
        }
        if not_applicable:
            result.update(
                {
                    "cobol": "not-applicable",
                    "language_restriction": {"profile": "internal-only"},
                }
            )
        elif label not in VERIFIER_LANGUAGE_PROFILE_BY_LABEL:
            result.update(
                independent_language_applicability(
                    "exec-cics-all-supported-languages"
                )
            )
        return result
    if selector == "dpl-server-restrictions" and source.tag == "section":
        if label == "GDS EXTRACT ATTRIBUTES":
            return None
        return (
            {"dpl_server": "not-applicable"}
            if not_applicable
            else independently_derive_dpl(label, source)
        )
    if selector == "threadsafe-command-list" and source.tag == "section":
        return (
            {"threadsafe": "not-applicable"}
            if not_applicable
            else independently_derive_threadsafe(label, source)
        )
    if selector == "mapped-dpl-applicability":
        return (
            {"dpl_server": "restricted"}
            if independently_is_mapped_dpl_predicate(label, source)
            else None
        )
    if selector == "mapped-language-applicability":
        profile = independent_language_profile(source)
        expected = VERIFIER_LANGUAGE_PROFILE_BY_LABEL.get(label)
        if not independent_language_node(source) or profile is None or profile != expected:
            return None
        return independent_language_applicability(profile)
    return None


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
        if kind == "source-condition" and any(
            str(item).startswith("SYMBOL-")
            for item in expected.value.get("condition_stack", [])
        ):
            return "product-ambiguity", "condition-response-code-not-pinned"
        if issue_code in {
            "syntax-panel-composition-unresolved",
            "prose-option-legality-not-structured",
            "shared-condition-applicability-unresolved",
        }:
            return "product-ambiguity", issue_code
        return "verified", "independent-structural-match"

    if kind == "source-context":
        value = candidate.get("candidate_value")
        selector = value.get("selector_id") if isinstance(value, dict) else None
        if selector not in CONTEXT_SELECTORS:
            return "mismatch", "unknown-context-selector"
        if selector in {
            "global-command-format",
            "dpl-server-restrictions",
            "threadsafe-command-list",
            "mapped-dpl-applicability",
            "mapped-language-applicability",
        }:
            topic = snapshot.topics[str(candidate["evidence"]["topic_path"])]
            source = resolve_path(
                topic.document, str(candidate["evidence"]["structural_path"])
            )
            expected_applicability = independent_applicability(
                row,
                str(selector),
                snapshot,
                source,
                str(dimension.get("name")),
            )
            if expected_applicability is None:
                if "applicability" in value:
                    return "mismatch", "unexpected-context-applicability"
                return "verified", "global-context-binding"
            if value.get("applicability") != expected_applicability:
                return "mismatch", "context-applicability-value-differs"
            if selector == "mapped-dpl-applicability" and (
                value.get("predicate_id")
                != fragment_sha256(normalize_text(source.text()))
                or value.get("predicate_state") != "bounded"
                or issue_code != "context-predicate-not-structured"
            ):
                return "mismatch", "mapped-dpl-predicate-value-differs"
            if selector == "mapped-dpl-applicability":
                return "product-ambiguity", "context-predicate-not-structured"
            return "verified", "independent-context-applicability"
        if selector == "mapped-context-predicate":
            direct = {
                source["topic_path"]
                for source in row.get("topics", [])
                if isinstance(source, dict)
            }
            if candidate["evidence"]["topic_path"] not in direct:
                return "requires-reprojection", "context-predicate-outside-row-topic"
            topic = snapshot.topics[candidate["evidence"]["topic_path"]]
            node = resolve_path(topic.document, candidate["evidence"]["structural_path"])
            expected_predicate = fragment_sha256(normalize_text(node.text()))
            if (
                not independently_is_context_predicate(node)
                or independent_language_profile(node) is not None
                or value.get("predicate_id") != expected_predicate
                or value.get("predicate_state") != "bounded"
            ):
                return "mismatch", "context-predicate-value-differs"
            return (
                ("product-ambiguity", "context-predicate-not-structured")
                if issue_code == "context-predicate-not-structured"
                else ("requires-reprojection", "context-predicate-issue-missing")
            )
        if selector in {"global-command-argument-values", "global-response-codes"}:
            if "applicability" in value:
                return "mismatch", "unexpected-context-applicability"
            return "verified", "global-context-binding"
        if (selector == "gds-send-response-contract"
            or selector.startswith("appc-basic-state-transitions")
            or selector.startswith("appc-mapped-state-transitions")):
            contract = next(
                (
                    item for item in snapshot.plan.get("manual_context_selectors", [])
                    if item.get("id") == selector
                ),
                None,
            )
            evidence = candidate["evidence"]
            topic = snapshot.topics[str(evidence["topic_path"])]
            node = resolve_path(topic.document, str(evidence["structural_path"]))
            fragments = (contract or {}).get("fragment_selectors", [])
            selected = any(
                node.tag == fragment["tag"]
                and (
                    node.attrs.get("id") == fragment.get("id")
                    if "id" in fragment else any(
                        child.tag == "h2"
                        and child.attrs.get("id") == fragment.get("heading_id")
                        for child in node.children
                    )
                )
                for fragment in fragments
            )
            if (
                contract is None
                or row["official_row"] not in contract["applies_to"]["official_rows"]
                or evidence["topic_path"] != contract["topic_path"]
                or value.get("association") != "row-specific-context"
                or "applicability" in value
                or not selected
            ):
                return "mismatch", "row-specific-context-escaped-boundary"
            return "verified", "independent-row-specific-context"
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
        if selector in {
            "gap-exec-interface-classification",
            "gap-eibfn-identity",
        }:
            selector_contract = next(
                (
                    item
                    for item in snapshot.plan.get("manual_context_selectors", [])
                    if item.get("id") == selector
                ),
                None,
            )
            scoped_rows = (selector_contract or {}).get("applies_to", {}).get(
                "official_rows", []
            )
            topic = snapshot.topics[str(candidate["evidence"]["topic_path"])]
            node = resolve_path(
                topic.document, str(candidate["evidence"]["structural_path"])
            )
            compact = re.sub(r"[^A-Z0-9]", "", node.text().upper())
            label = re.sub(r"[^A-Z0-9]", "", str(row["label"]).upper())
            eibfn = str(row["eibfn"]).upper()
            if (
                selector_contract is None
                or row.get("official_row") not in scoped_rows
            ):
                return "mismatch", "gap-identity-row-evidence-differs"
            if label not in compact or (
                selector == "gap-eibfn-identity" and eibfn not in compact
            ):
                row_matches = [
                    item
                    for item in topic.document.descendants()
                    if item.tag == "tr"
                    and label
                    in re.sub(r"[^A-Z0-9]", "", item.text().upper())
                    and eibfn
                    in re.sub(r"[^A-Z0-9]", "", item.text().upper())
                ]
                if len(row_matches) != 1:
                    return "product-ambiguity", "gap-row-outside-selected-fragment"
            return "verified", "independent-gap-identity-match"
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
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    cache = cache or Path("/ibm-docs/topic-cache")
    snapshot = build_source_snapshot(root, cache, config)
    compatibility_stub_rows = independently_derive_compatibility_stubs(snapshot)
    expected_bounded_issues = independently_derive_bounded_source_issues(snapshot)
    # Deliberately load generated output only after the source-derived model is complete.
    projection_file = projection_path or root / config.projection_path
    projection = read_object(projection_file)
    if (
        projection.get("work_package") not in {None, config.project_work_package}
        or snapshot.plan.get("work_package") not in {None, config.project_work_package}
        or snapshot.plan.get("inputs", {}).get("cache_scope")
        not in {None, config.cache_scope}
    ):
        raise VerificationError("selected batch identity differs")
    if (
        snapshot.condition_name_authority is not None
        and projection.get("condition_name_authority")
        != snapshot.condition_name_authority
    ):
        raise VerificationError("EIBRESP condition-name authority differs")

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
    issue_evidence_errors: dict[str, str] = {}
    issue_signatures: dict[str, tuple[str, str, str, str]] = {}
    issue_rows: list[tuple[str, str, str, str, tuple[str, ...]]] = []
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
                issue_identity = str(issue.get("issue_id"))
                evidence_rows = issue.get("evidence", [])
                if not isinstance(evidence_rows, list):
                    raise VerificationError("projected issue evidence is not a list")
                if code == "compatibility-successor-not-equivalent" and not evidence_rows:
                    issue_evidence_errors[issue_identity] = "missing-issue-evidence"
                if code in BOUNDED_SOURCE_ISSUE_CODES:
                    if len(evidence_rows) != 1:
                        issue_evidence_errors[issue_identity] = (
                            "bounded-source-issue-evidence-count-differs"
                        )
                    else:
                        issue_signatures[issue_identity] = (
                            official_row,
                            code,
                            str(evidence_rows[0].get("topic_path")),
                            str(evidence_rows[0].get("structural_path")),
                        )
                for evidence in evidence_rows:
                    error = validate_evidence({"evidence": evidence}, snapshot)
                    if error is not None:
                        issue_evidence_errors[issue_identity] = error
                affected = issue.get("affected_dimensions", [])
                if not isinstance(affected, list) or not all(
                    isinstance(item, str) for item in affected
                ):
                    raise VerificationError("projected issue dimensions are malformed")
                issue_rows.append(
                    (
                        official_row,
                        str(dimension.get("name")),
                        issue_identity,
                        code,
                        tuple(sorted(set(affected))),
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
    projected_compatibility_rows = {
        official_row
        for official_row, _, _, code, _ in issue_rows
        if code == "compatibility-successor-not-equivalent"
    }
    missing_compatibility_issues = sorted(
        compatibility_stub_rows - projected_compatibility_rows
    )
    extra_compatibility_issues = sorted(
        projected_compatibility_rows - compatibility_stub_rows
    )
    projected_bounded_issues = set(issue_signatures.values())
    missing_bounded_issues = sorted(
        expected_bounded_issues - projected_bounded_issues
    )

    selector_fields = {
        "global-command-format": ("local_task", "cobol"),
        "dpl-server-restrictions": ("dpl_server",),
        "threadsafe-command-list": ("threadsafe",),
    }
    applicability_fields = {"local_task", "dpl_server", "threadsafe", "cobol"}
    expected_applicability: set[tuple[str, str]] = set()
    for source_row in mapped_rows:
        official_row = str(source_row["official_row"])
        for selector in snapshot.plan.get("manual_context_selectors", []):
            selector_id = selector.get("id")
            fields = selector_fields.get(str(selector_id))
            applies = selector.get("applies_to", {})
            if fields is None or (
                applies.get("kind") == "specific-rows"
                and official_row not in applies.get("official_rows", [])
            ):
                continue
            expected_applicability.update((official_row, field) for field in fields)
    applicability_counts: dict[tuple[str, str], int] = {}
    for source_row, _, candidate in candidate_locations.values():
        value = candidate.get("candidate_value")
        applicability = value.get("applicability") if isinstance(value, dict) else None
        if not isinstance(applicability, dict):
            continue
        for field in set(applicability) & applicability_fields:
            key = (str(source_row["official_row"]), str(field))
            applicability_counts[key] = applicability_counts.get(key, 0) + 1
    projected_applicability = set(applicability_counts)
    missing_applicability = sorted(expected_applicability - projected_applicability)
    extra_applicability = sorted(
        key
        for key, count in applicability_counts.items()
        for _ in range(max(0, count - 1))
        if key in expected_applicability
    ) + sorted(projected_applicability - expected_applicability)

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
                    "official_row": str(row["official_row"]),
                    "dimension": str(dimension.get("name")),
                    "category": category,
                    "reason_code": reason,
                }
            )

    for key in missing:
        findings.append(
            {
                "fact_sha256": category_digest(list(key)),
                "official_row": key[0],
                "dimension": key[1],
                "category": "requires-reprojection",
                "reason_code": "missing-structural-fact",
            }
        )
    for official_row, field in missing_applicability:
        findings.append(
            {
                "fact_sha256": category_digest([official_row, field]),
                "official_row": official_row,
                "dimension": "execution-context",
                "category": "requires-reprojection",
                "reason_code": "missing-context-applicability",
            }
        )
    for official_row, field in extra_applicability:
        findings.append(
            {
                "fact_sha256": category_digest([official_row, field, "extra"]),
                "official_row": official_row,
                "dimension": "execution-context",
                "category": "requires-reprojection",
                "reason_code": "duplicate-or-extra-context-applicability",
            }
        )
    for official_row in missing_compatibility_issues:
        findings.append(
            {
                "fact_sha256": category_digest(
                    [official_row, "compatibility-successor-not-equivalent"]
                ),
                "official_row": official_row,
                "dimension": "syntax",
                "category": "requires-reprojection",
                "reason_code": "missing-compatibility-stub-issue",
            }
        )
    for official_row, code, topic_path, structural_path in missing_bounded_issues:
        findings.append(
            {
                "fact_sha256": category_digest(
                    [official_row, code, topic_path, structural_path]
                ),
                "official_row": official_row,
                "dimension": (
                    "syntax"
                    if code == "syntax-panel-composition-unresolved"
                    else "options"
                    if code == "prose-option-legality-not-structured"
                    else "conditions"
                    if code == "shared-condition-applicability-unresolved"
                    else "execution-context"
                ),
                "category": "requires-reprojection",
                "reason_code": "missing-bounded-source-issue",
            }
        )
    issue_categories = {name: [] for name in CATEGORIES}
    for official_row, dimension, identity, code, _ in sorted(issue_rows):
        if identity in issue_evidence_errors:
            category = "mismatch"
        elif code == "unmatched-row" and declared_absence(snapshot, official_row, dimension):
            category = "verified"
        elif code == "unmatched-row":
            category = "requires-reprojection"
        elif code == "compatibility-successor-not-equivalent":
            category = (
                "product-ambiguity"
                if official_row in compatibility_stub_rows
                else "mismatch"
            )
        elif code in BOUNDED_SOURCE_ISSUE_CODES:
            category = (
                "product-ambiguity"
                if issue_signatures.get(identity) in expected_bounded_issues
                else "mismatch"
            )
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
                    "dimension": dimension,
                    "category": category,
                    "reason_code": issue_evidence_errors.get(identity, code),
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
    ambiguity_by_row: dict[str, set[str]] = {}
    for identity in categories["product-ambiguity"]:
        row, dimension, _ = candidate_locations[identity]
        ambiguity_by_row.setdefault(str(row["official_row"]), set()).add(
            str(dimension["name"])
        )
    ambiguous_issue_ids = set(issue_categories["product-ambiguity"])
    for official_row, dimension, identity, _, affected in issue_rows:
        if identity in ambiguous_issue_ids:
            ambiguity_by_row.setdefault(official_row, set()).update(
                affected or (dimension,)
            )
    ambiguity_scope = [
        {"official_row": official_row, "dimensions": sorted(dimensions)}
        for official_row, dimensions in sorted(ambiguity_by_row.items())
    ]
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
        or missing_applicability
        or extra_applicability
        or missing_compatibility_issues
        or missing_bounded_issues
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
        "source_map_sha256": file_sha256(root / config.map_path),
        "topic_manifest_sha256": file_sha256(root / config.manifest_path),
        "extraction_plan_sha256": file_sha256(root / config.plan_path),
        "candidate_projection_sha256": file_sha256(projection_file),
    }
    if (root / config.corpus_path).exists():
        input_identities["source_corpus_sha256"] = file_sha256(
            root / config.corpus_path
        )
    if (
        config.supplements_path is not None
        and (root / config.supplements_path).exists()
    ):
        input_identities["source_supplements_sha256"] = file_sha256(
            root / config.supplements_path
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
        "applicability_coverage": {
            "expected": len(expected_applicability),
            "projected": len(projected_applicability),
            "missing": len(missing_applicability),
            "extra": len(extra_applicability),
        },
        "candidate_categories": categories_report,
        "issue_categories": issue_report,
        "ambiguity_scope": ambiguity_scope,
        "findings": sorted(findings, key=canonical_bytes),
    }
    report["report_sha256"] = report_digest(report)
    return report


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=REPOSITORY)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--batch", choices=("a", "b", "c"), default="a")
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
        report = verify(
            args.root.resolve(),
            args.cache.resolve(),
            args.projection,
            args.batch,
        )
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
