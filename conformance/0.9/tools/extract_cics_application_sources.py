#!/usr/bin/env python3
"""Project a pinned CIC-901 application-source batch into review candidates.

The extractor is deliberately offline.  It reads digest-pinned IBM topic bodies
from the external documentation cache and writes only structural locators,
fragment hashes, and bounded symbolic values.  Publication prose and HTML never
enter the generated candidate artifact.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import hashlib
import html
from html.parser import HTMLParser
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Iterable, Iterator
import urllib.parse


ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "conformance/tools"))
sys.path.insert(0, str(ROOT / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import docs_api  # noqa: E402
import ibm_docs  # noqa: E402
from cics_application_source_batches import (  # noqa: E402
    SourceBatch,
    source_batch,
    source_batch_for_row,
)
import fetch_cics_application_sources as source_corpus  # noqa: E402


DEFAULT_BATCH = source_batch("a")
# Backwards-compatible aliases for focused sources-a tests and callers.
MAP_PATH = DEFAULT_BATCH.map_path
CORPUS_PATH = DEFAULT_BATCH.corpus_path
MANIFEST_PATH = DEFAULT_BATCH.manifest_path
PLAN_PATH = DEFAULT_BATCH.plan_path
OUTPUT_PATH = DEFAULT_BATCH.projection_path
BROWSER_RECEIPT_PATH = DEFAULT_BATCH.browser_receipt_path
SUPPLEMENTS_PATH = DEFAULT_BATCH.supplements_path

TARGET_VERSION = "0.9.0"
WORK_PACKAGE = DEFAULT_BATCH.project_work_package
PLAN_SCHEMA = "mainframe-env.cics-source-extraction@1"
OUTPUT_SCHEMA = "mainframe-env.cics-source-candidates@1"
CACHE_SCOPE = DEFAULT_BATCH.cache_scope
PRODUCT = "SSJL4D_6.x"
TARGET_AUTHORITY = "target-product-authority"
SUPPLEMENT_AUTHORITY = {
    "SSNAQ8_11.1.0": frozenset({"cross-product-not-target-authority"}),
    "SSGMCP_5.5.0": frozenset(
        {
            "target-product-older-version-compatibility",
            "target-product-older-version-context",
        }
    ),
    "SSXJAJ_14.1.0": frozenset({"target-platform-example-only"}),
}
PLAN_DOMAIN = b"mainframe-env.cics-source-extraction@1\0"
OUTPUT_DOMAIN = b"mainframe-env.cics-source-candidates@1\0"
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
FRAGMENT_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-fragment@1, one NUL "
    "byte (0x00), then UTF-8 order-preserving whitespace-normalized visible "
    "text of the selected DOM subtree"
)

DIMENSIONS = (
    "syntax",
    "options",
    "operand-directions",
    "conditions",
    "execution-context",
)
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
TOKEN_CLASSES = {
    "syntaxkwd": "keyword",
    "syntaxvar": "variable",
    "syntaxdelim": "delimiter",
    "fragref": "fragment",
    "syntaxfragref": "fragment",
}
STRUCTURAL_TAGS = frozenset({"svg", "g", "a"})


class ProjectionError(ValueError):
    """The plan, pinned source, or deterministic projection is invalid."""


@dataclass(eq=False)
class Node:
    """A forgiving HTML node whose mixed text/child content preserves order."""

    tag: str
    attrs: dict[str, str]
    parent: "Node | None" = None
    children: list["Node"] = field(default_factory=list)
    parts: list["str | Node"] = field(default_factory=list)

    def text(self, *, exclude_tags: frozenset[str] = frozenset()) -> str:
        values: list[str] = []
        for part in self.parts:
            if isinstance(part, str):
                values.append(part)
            elif part.tag not in exclude_tags:
                values.append(part.text(exclude_tags=exclude_tags))
        return normalize(" ".join(values))


class OrderedHtmlParser(HTMLParser):
    """Build a small DOM while keeping mixed content and HTML void tags sound."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = Node("root", {})
        self.stack = [self.root]

    def _append(self, tag: str, attrs: list[tuple[str, str | None]]) -> Node:
        node = Node(
            tag.lower(),
            {key.lower(): value or "" for key, value in attrs},
            self.stack[-1],
        )
        self.stack[-1].children.append(node)
        self.stack[-1].parts.append(node)
        return node

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        node = self._append(tag, attrs)
        if node.tag not in VOID_TAGS:
            self.stack.append(node)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._append(tag, attrs)

    def handle_endtag(self, tag: str) -> None:
        wanted = tag.lower()
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == wanted:
                del self.stack[index:]
                return

    def handle_data(self, data: str) -> None:
        self.stack[-1].parts.append(data)


@dataclass(frozen=True)
class Section:
    title: str
    section_id: str
    node: Node


@dataclass(frozen=True)
class DefinitionGroup:
    term: Node
    descriptions: tuple[Node, ...]
    stack: tuple[str, ...]
    depth: int


@dataclass(frozen=True)
class SyntaxToken:
    kind: str
    value: str
    node: Node


@dataclass(frozen=True)
class SyntaxItem:
    relation: str
    tokens: tuple[SyntaxToken, ...]
    node: Node


@dataclass(frozen=True)
class SyntaxDiagram:
    title: str
    title_id: str
    node: Node
    pieces: tuple[Node, ...]


def normalize(value: str) -> str:
    return " ".join(html.unescape(value).replace("\xa0", " ").split())


def bounded_text(value: str, maximum: int, label: str) -> str:
    if not value or len(value) > maximum:
        raise ProjectionError(f"{label} text length is invalid")
    return value


def parse_html(body: bytes | str) -> Node:
    parser = OrderedHtmlParser()
    parser.feed(body.decode("utf-8", "replace") if isinstance(body, bytes) else body)
    parser.close()
    return parser.root


def walk(node: Node) -> Iterator[Node]:
    for child in node.children:
        yield child
        yield from walk(child)


def classes(node: Node) -> set[str]:
    return set(node.attrs.get("class", "").split())


def sibling_ordinal(node: Node) -> int:
    if node.parent is None:
        return 1
    ordinal = 1
    for sibling in node.parent.children:
        if sibling is node:
            break
        if sibling.tag == node.tag:
            ordinal += 1
    return ordinal


def structural_path(node: Node) -> str:
    parts: list[str] = []
    current: Node | None = node
    while current is not None and current.tag != "root":
        identity = current.attrs.get("id")
        parts.append(
            f"{current.tag}#{identity}"
            if identity
            else f"{current.tag}[{sibling_ordinal(current)}]"
        )
        current = current.parent
    return "/".join(reversed(parts))


def direct_sections(root: Node) -> dict[str, Section]:
    """Index sections by their own direct ``h2.sectiontitle`` heading."""

    found: dict[str, Section] = {}
    for node in walk(root):
        if node.tag != "section":
            continue
        headings = [
            child
            for child in node.children
            if child.tag == "h2" and "sectiontitle" in classes(child)
        ]
        if len(headings) > 1:
            raise ProjectionError("section has more than one direct sectiontitle")
        if not headings:
            continue
        heading = headings[0]
        title = heading.text()
        section_id = heading.attrs.get("id", "")
        if not title or not section_id:
            raise ProjectionError("section title or id is empty")
        if title in found:
            raise ProjectionError(f"duplicate section title: {title}")
        found[title] = Section(title, section_id, node)
    return found


def _outer_definition_lists(section: Section) -> list[Node]:
    return [
        node
        for node in walk(section.node)
        if node.tag == "dl"
        and not any(
            ancestor.tag == "dl"
            for ancestor in ancestors_until(node, section.node)
        )
    ]


def ancestors_until(node: Node, stop: Node | None = None) -> Iterator[Node]:
    current = node.parent
    while current is not None and current is not stop:
        yield current
        current = current.parent


def _definition_groups(
    definition_list: Node, stack: tuple[str, ...], depth: int
) -> Iterator[DefinitionGroup]:
    current: Node | None = None
    descriptions: list[Node] = []

    def emit() -> DefinitionGroup | None:
        if current is None:
            return None
        term = current.text(exclude_tags=frozenset({"dl", "ul", "ol"}))
        if not term:
            raise ProjectionError("definition term is empty")
        return DefinitionGroup(current, tuple(descriptions), stack + (term,), depth)

    groups: list[DefinitionGroup] = []
    header_pending = False
    for child in definition_list.children:
        child_classes = classes(child)
        if child.tag == "dt" and "dthd" in child_classes:
            if emit() is not None:
                raise ProjectionError("definition header follows a term")
            header_pending = True
            continue
        if child.tag == "dd" and "ddhd" in child_classes and header_pending:
            header_pending = False
            continue
        if child.tag == "dt":
            prior = emit()
            if prior is not None:
                groups.append(prior)
            current = child
            descriptions = []
        elif child.tag == "dd":
            if current is None:
                raise ProjectionError("definition description has no preceding term")
            descriptions.append(child)
    final = emit()
    if final is not None:
        groups.append(final)
    if header_pending:
        raise ProjectionError("definition header has no description header")

    for group in groups:
        yield group
        for description in group.descriptions:
            nested = [
                child
                for child in walk(description)
                if child.tag == "dl"
                and not any(ancestor.tag == "dl" for ancestor in ancestors_until(child, description))
            ]
            for child in nested:
                yield from _definition_groups(child, group.stack, depth + 1)


def definition_groups(section: Section) -> list[DefinitionGroup]:
    groups: list[DefinitionGroup] = []
    for definition_list in _outer_definition_lists(section):
        groups.extend(_definition_groups(definition_list, (), 0))
    return groups


def _semantic_tokens(node: Node) -> list[SyntaxToken]:
    found: list[SyntaxToken] = []
    for child in walk(node):
        if child.tag != "text":
            continue
        names = classes(child)
        kind = next((TOKEN_CLASSES[name] for name in TOKEN_CLASSES if name in names), None)
        value = child.text()
        if kind == "keyword" and value == "condition":
            # IBM renders the HANDLE/IGNORE CONDITION name placeholder with a
            # syntaxkwd class even though the option definition and prose make
            # it a metavariable.  Keep real names such as ERROR out of command
            # identity and let the condition-clause profile validate them.
            kind = "variable"
        elif kind == "keyword" and value == "today":
            # DEFINE TIMER uses this lowercase label for the default date when
            # ON is omitted; it is not an accepted TODAY option.
            kind = "fragment"
        if kind and value:
            found.append(SyntaxToken(kind, value, child))
    return found


def _structural_children(node: Node) -> list[Node]:
    return [child for child in node.children if child.tag in STRUCTURAL_TAGS]


def _speaks(node: Node) -> bool:
    return bool(_semantic_tokens(node))


def _syntax_items(node: Node, relation: str = "required") -> Iterator[SyntaxItem]:
    names = classes(node)
    # ``groupcomp`` is a repeat/group container, not a token leaf.  CICS can
    # place several option groups inside one groupcomp; stopping there merges
    # their operands and attributes every metavariable to the first keyword.
    # The nearest node carrying a DITA token class is the semantic leaf, even
    # when it contains separate keyword/variable/delimiter ``text`` elements.
    if names & TOKEN_CLASSES.keys():
        tokens = tuple(_semantic_tokens(node))
        if tokens:
            yield SyntaxItem(relation, tokens, node)
        return

    children = _structural_children(node)
    spoken = [child for child in children if _speaks(child)]
    if spoken and len(spoken) < len(children):
        for child in spoken:
            yield from _syntax_items(child, relation if relation != "required" else "optional")
        return
    if "groupchoice" in names:
        for index, child in enumerate(spoken):
            child_relation = relation
            if relation == "required" and index > 0:
                child_relation = "alternative"
            yield from _syntax_items(child, child_relation)
        return
    for child in spoken:
        yield from _syntax_items(child, relation)


def syntax_diagrams(section: Section) -> list[SyntaxDiagram]:
    """Read each outer ``div.syntaxdiagram`` and its one or more SVG pieces."""

    containers = [
        node
        for node in walk(section.node)
        if node.tag == "div"
        and "syntaxdiagram" in classes(node)
        and not any(
            ancestor.tag == "div" and "syntaxdiagram" in classes(ancestor)
            for ancestor in ancestors_until(node, section.node)
        )
    ]
    result: list[SyntaxDiagram] = []
    for container in containers:
        headings = [
            node
            for node in walk(container)
            if node.tag == "h3" and "syntaxdiagram-title" in classes(node)
        ]
        pieces = [
            node
            for node in walk(container)
            if node.tag == "svg" and "syntaxdiagram" in classes(node)
        ]
        if len(headings) > 1:
            raise ProjectionError("syntax diagram has multiple titles")
        fragment_names = [
            node
            for node in walk(container)
            if node.tag == "span" and "fragmentname" in classes(node)
        ]
        if headings:
            title = headings[0].text()
            title_id = headings[0].attrs.get("id", "")
        elif len(fragment_names) == 1:
            title = fragment_names[0].text()
            title_id = fragment_names[0].attrs.get("id") or structural_path(container)
        elif container.attrs.get("id"):
            title = container.attrs["id"]
            title_id = container.attrs["id"]
        else:
            raise ProjectionError("untitled syntax fragment lacks a stable identity")
        if not title or not title_id:
            raise ProjectionError("syntax diagram title or id is empty")
        if not pieces:
            raise ProjectionError("syntax diagram title has no SVG piece")
        result.append(SyntaxDiagram(title, title_id, container, tuple(pieces)))
    return result


def syntax_items(diagram: SyntaxDiagram) -> list[SyntaxItem]:
    found: list[SyntaxItem] = []
    for piece in diagram.pieces:
        roots = [
            node
            for node in piece.children
            if node.tag == "g" and "diagram" in classes(node)
        ]
        if len(roots) != 1:
            raise ProjectionError("syntax SVG does not contain exactly one diagram root")
        found.extend(_syntax_items(roots[0]))
    return found


def syntax_group_path(token: SyntaxToken, diagram: SyntaxDiagram) -> str:
    parts: list[str] = []
    current = token.node.parent
    while current is not None and current is not diagram.node:
        group = next(
            (
                name
                for name in ("groupseq", "groupchoice", "groupcomp")
                if name in classes(current)
            ),
            None,
        )
        if group is not None:
            parts.append(f"{group}[{sibling_ordinal(current)}]")
        current = current.parent
    return "/".join(reversed(parts)) or "__root__"


def syntax_piece_ordinal(token: SyntaxToken, diagram: SyntaxDiagram) -> int:
    current: Node | None = token.node
    while current is not None:
        for ordinal, piece in enumerate(diagram.pieces, 1):
            if current is piece:
                return ordinal
        current = current.parent
    raise ProjectionError("syntax token is not beneath its diagram SVG pieces")


def normalized_fragment(node: Node, *, exclude_lists: bool = False) -> str:
    excluded = frozenset({"dl", "ul", "ol"}) if exclude_lists else frozenset()
    return node.text(exclude_tags=excluded)


def fragment_sha256(value: str) -> str:
    return hashlib.sha256(FRAGMENT_DOMAIN + value.encode("utf-8")).hexdigest()


def canonical_digest(value: object, domain: bytes, omitted: str) -> str:
    if not isinstance(value, dict):
        raise ProjectionError("digest input must be an object")
    core = {key: item for key, item in value.items() if key != omitted}
    encoded = json.dumps(
        core, ensure_ascii=True, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(domain + encoded).hexdigest()


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ProjectionError(f"{path}: {error}") from error
    if not isinstance(value, dict):
        raise ProjectionError(f"{path} must contain an object")
    return value


def pretty(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def canonical_topic_path(value: str) -> str | None:
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
    ):
        return None
    return clean


def linked_reference(href: str, source: str) -> tuple[str, str] | None:
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
        fragment = parsed.fragment
    elif href.startswith("/docs/en/"):
        joined = urllib.parse.urlsplit(href)
        candidate = joined.path[len("/docs/en/") :]
        fragment = joined.fragment
    elif href.startswith("/"):
        return None
    else:
        joined = urllib.parse.urlsplit(urllib.parse.urljoin(source, href))
        candidate = joined.path
        fragment = joined.fragment
    topic = canonical_topic_path(candidate)
    fragment = urllib.parse.unquote(fragment)
    if topic is None or len(fragment) > 256 or any(character.isspace() for character in fragment):
        return None
    return topic, fragment


def linked_topic(href: str, source: str) -> str | None:
    reference = linked_reference(href, source)
    return reference[0] if reference is not None else None


def one_hop_anchors(root: Node, source: str) -> list[str]:
    return sorted(
        {
            target
            for node in walk(root)
            if node.tag == "a"
            for target in [linked_topic(node.attrs.get("href", ""), source)]
            if target is not None
        }
    )


def compatibility_successor_anchor(
    root: Node, source: str, available_topics: set[str]
) -> Node | None:
    text = root.text().casefold()
    if "supported for compatibility" not in text or "superseded" not in text:
        return None
    matches = [
        node
        for node in walk(root)
        if node.tag == "a"
        and normalize(node.text()).casefold() != "cics command summary"
        and (reference := linked_reference(node.attrs.get("href", ""), source))
        is not None
        and reference[0] in available_topics
    ]
    if len(matches) != 1:
        raise ProjectionError(f"compatibility successor is not unique: {source}")
    return matches[0]


def exact_target_fragment(root: Node, fragment: str, topic_path: str) -> Node:
    def title_article(titles: list[Node]) -> Node | None:
        if len(titles) != 1:
            return None
        current = titles[0].parent
        while current is not None and current.tag != "article":
            current = current.parent
        return current

    def common_article_parent(articles: list[Node]) -> Node | None:
        if len(articles) < 2:
            return None
        chains: list[list[Node]] = []
        for article in articles:
            chain: list[Node] = []
            current = article.parent
            while current is not None:
                chain.append(current)
                current = current.parent
            chains.append(list(reversed(chain)))
        common: Node | None = None
        for values in zip(*chains):
            if any(value is not values[0] for value in values[1:]):
                break
            common = values[0]
        return common if common is not None and common.tag != "root" else None

    if fragment:
        matches = [node for node in walk(root) if node.attrs.get("id") == fragment]
        if len(matches) == 1:
            return matches[0]
        # DITA links sometimes use the source topic ID (for example
        # ``#dfhp4kr``) even though the rendered HTML exposes only derived
        # heading IDs such as ``dfhp4kr__title__1``.  A unique top-level title
        # with that prefix identifies the topic root without guessing a
        # section.  Any other missing or ambiguous anchor remains an error.
        topic_titles = [
            node
            for node in walk(root)
            if node.tag == "h1"
            and "topictitle1" in classes(node)
            and node.attrs.get("id", "").startswith(fragment + "__")
        ]
        article = title_article(topic_titles)
        if not matches and article is not None:
            return article
        else:
            raise ProjectionError(
                f"one-hop target anchor is not unique: {topic_path}#{fragment}"
            )
    articles = [node for node in walk(root) if node.tag == "article"]
    if len(articles) == 1:
        return articles[0]
    titles = [
        node
        for node in walk(root)
        if node.tag == "h1" and "topictitle1" in classes(node)
    ]
    article = title_article(titles)
    if article is not None:
        return article
    container = common_article_parent(articles)
    if container is not None:
        return container
    raise ProjectionError(f"one-hop target topic root is not unique: {topic_path}")


PLAN_KEYS = (
    "schema_version",
    "target_version",
    "work_package",
    "status",
    "semantic_authority",
    "automatic_registration",
    "coverage_credit",
    "differential_credit",
    "inputs",
    "dimensions",
    "bounds",
    "expected_shape",
    "selector_profiles",
    "section_exceptions",
    "manual_context_selectors",
    "conflict_expectations",
    "source_resolutions",
    "extraction_digest_definition",
    "extraction_sha256",
)
OUTPUT_KEYS = (
    "schema_version",
    "target_version",
    "work_package",
    "status",
    "review_status",
    "semantic_authority",
    "automatic_registration",
    "coverage_credit",
    "differential_credit",
    "inputs",
    "extractor",
    "condition_name_authority",
    "counts",
    "rows",
    "blocking_issues",
    "projection_digest_definition",
    "projection_sha256",
)
FORBIDDEN_OUTPUT_KEYS = frozenset(
    {"text", "html", "quote", "excerpt", "description", "interpretation"}
)
PROJECTION_DIGEST_DEFINITION = (
    "SHA-256 over ASCII domain mainframe-env.cics-source-candidates@1, one NUL "
    "byte (0x00), then UTF-8 JSON with ensure_ascii=true, sort_keys=true and "
    "separators=(',', ':') of the complete candidate projection with "
    "projection_sha256 omitted"
)
DIRECT_SOURCE_ROLES = {
    "primary": "primary-command",
    "variant": "variant-command",
    "shared": "shared-command",
    "combined": "combined-command",
    "alias": "alias-command",
}
MANUAL_SOURCE_ROLES = {
    "global-command-format": "common-command-format",
    "global-command-argument-values": "argument-value-context",
    "dpl-server-restrictions": "manual-context",
    "threadsafe-command-list": "manual-context",
    "gap-exec-interface-classification": "gap-classification",
    "gap-eibfn-identity": "gap-identity",
    "traceid-monitor-compatibility": "compatibility-context",
    "traceid-dfhcmp-compatibility": "compatibility-context",
    "global-response-codes": "response-code-context",
    "gds-send-response-contract": "manual-context",
    "appc-basic-state-transitions": "manual-context",
    "appc-basic-state-transitions-sl0": "manual-context",
    "appc-basic-state-transitions-sl1": "manual-context",
    "appc-basic-state-transitions-sl2": "manual-context",
}
SUPPLEMENT_SOURCE_ROLES = {
    "cross-product-explicit-compatibility": "cross-product-compatibility",
    "cross-product-command-reference": "cross-product-compatibility",
    "target-product-version-compatibility": "version-compatibility-context",
    "target-platform-interface-example": "target-platform-example",
}
MARKERS = (
    "data-value",
    "data-area",
    "data-area64",
    "cvda",
    "ptr-value",
    "ptr-value64",
    "ptr-ref",
    "ptr-ref64",
    "name",
    "filename",
    "systemname",
    "label",
    "hhmmss",
)


def file_sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def authority_boundary(source_product: str, explicit: str | None = None) -> str:
    if source_product == PRODUCT:
        if explicit not in {None, TARGET_AUTHORITY}:
            raise ProjectionError("target topic has a non-target authority boundary")
        return TARGET_AUTHORITY
    allowed = SUPPLEMENT_AUTHORITY.get(source_product)
    if allowed is None or (explicit is not None and explicit not in allowed):
        raise ProjectionError("supplement authority boundary differs from its product")
    if explicit is not None:
        return explicit
    if len(allowed) != 1:
        raise ProjectionError("supplement requires an explicit authority boundary")
    return next(iter(allowed))


def validate_plan(
    plan: dict[str, Any], batch: str | SourceBatch = DEFAULT_BATCH
) -> None:
    config = source_batch(batch)
    if tuple(plan) != PLAN_KEYS:
        raise ProjectionError("extraction plan fields or ordering differ")
    inputs = plan.get("inputs", {})
    if (
        plan.get("schema_version") != PLAN_SCHEMA
        or plan.get("target_version") != TARGET_VERSION
        or plan.get("work_package") != config.project_work_package
        or plan.get("status") != "candidate-plan"
        or plan.get("semantic_authority") is not False
        or plan.get("automatic_registration") is not False
        or plan.get("coverage_credit") != 0
        or plan.get("differential_credit") != 0
        or tuple(plan.get("dimensions", [])) != DIMENSIONS
        or inputs.get("mapping", {}).get("path") != config.map_path.as_posix()
        or inputs.get("corpus", {}).get("path") != config.corpus_path.as_posix()
        or inputs.get("topic_manifest", {}).get("path")
        != config.manifest_path.as_posix()
        or inputs.get("cache_scope") != config.cache_scope
        or plan.get("extraction_sha256")
        != canonical_digest(plan, PLAN_DOMAIN, "extraction_sha256")
    ):
        raise ProjectionError("extraction plan identity or digest differs")
    if config.supplements_path is None:
        if "supplements" in inputs:
            raise ProjectionError("batch unexpectedly declares source supplements")
    elif inputs.get("supplements", {}).get("path") != config.supplements_path.as_posix():
        raise ProjectionError("source supplement binding differs")
    if config.browser_receipt_path is None:
        if "browser_verification" in inputs:
            raise ProjectionError("batch unexpectedly declares a browser receipt")
    elif (
        inputs.get("browser_verification", {}).get("path")
        != config.browser_receipt_path.as_posix()
    ):
        raise ProjectionError("browser receipt binding differs")
    bounds = plan.get("bounds")
    expected = plan.get("expected_shape")
    if not isinstance(bounds, dict) or not isinstance(expected, dict):
        raise ProjectionError("extraction plan bounds or expected shape is invalid")
    for name in (
        "max_topics",
        "max_topic_bytes",
        "max_dom_depth",
        "max_locator_bytes",
        "max_normalized_fragment_bytes",
        "max_fragments",
        "max_fragments_per_row",
        "max_link_depth",
        "max_candidate_values_per_key",
    ):
        if not isinstance(bounds.get(name), int) or isinstance(bounds.get(name), bool):
            raise ProjectionError(f"invalid extraction bound: {name}")
    if bounds["max_link_depth"] != 1 or expected.get("dimensions_per_row") != 5:
        raise ProjectionError("extraction depth or row-dimension shape differs")
    resolutions = plan.get("source_resolutions")
    if not isinstance(resolutions, list) or any(
        not isinstance(row, dict)
        or not config.contains_row(str(row.get("official_row", "")))
        for row in resolutions
    ):
        raise ProjectionError("source-resolution rows differ")
    if config.name != "a":
        if any(
            row.get("direct_supplement_topic")
            and row.get("direct_supplement_topic")
            not in (
                row.get("supplement_topics", [])
                + row.get("target_context_topics", [])
            )
            for row in resolutions
        ):
            raise ProjectionError("direct resolved topic is outside its row closure")
    else:
        expected_rows = {
            "ibm-cics-ts-6x-2026-08-31:api-commands:0027",
            "ibm-cics-ts-6x-2026-08-31:api-commands:0056",
            "ibm-cics-ts-6x-2026-08-31:api-commands:0065",
        }
        if {row.get("official_row") for row in resolutions} != expected_rows:
            raise ProjectionError("sources-a resolution rows differ")
        direct = {
            row["official_row"]: row.get("direct_supplement_topic")
            for row in resolutions
            if row.get("resolution") == "authority-bounded-projection"
        }
        if direct != {
            "ibm-cics-ts-6x-2026-08-31:api-commands:0056": (
                "SSNAQ8_11.1.0/reference-api/r_dump.html"
            ),
            "ibm-cics-ts-6x-2026-08-31:api-commands:0065": (
                "SSNAQ8_11.1.0/reference-api/r_enter.html"
            ),
        }:
            raise ProjectionError("supplement direct-product bindings differ")
    if any(
        row.get("resolution")
        in {"authority-bounded-projection", "target-product-projection"}
        and row.get("no_one_hop") is not True
        for row in resolutions
    ):
        raise ProjectionError("supplement link expansion must remain disabled")
    for resolution in resolutions:
        target_topics = resolution.get("target_context_topics", [])
        supplement_topics = resolution.get("supplement_topics", [])
        if any(topic.split("/", 1)[0] != PRODUCT for topic in target_topics):
            raise ProjectionError("source resolution assigns a foreign target topic")
        if set(target_topics) & set(supplement_topics):
            raise ProjectionError("source resolution aliases target and supplement topics")
        if resolution["resolution"] in {
            "authority-bounded-projection",
            "target-product-projection",
        } and (
            resolution["direct_supplement_topic"] not in supplement_topics
            and resolution["direct_supplement_topic"] not in target_topics
            or not {
                item["topic_path"]
                for item in resolution.get("context_fragments", [])
            }
            <= (set(supplement_topics) | set(target_topics))
        ):
            raise ProjectionError("supplement projection scope differs")


def _input_files(
    root: Path,
    plan: dict[str, Any],
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> tuple[
    dict[str, Any],
    dict[str, Any],
    dict[str, Any],
    dict[str, Any],
    dict[str, Any],
]:
    config = source_batch(batch)
    inputs = plan["inputs"]
    mapping_path = root / inputs["mapping"]["path"]
    corpus_path = root / inputs["corpus"]["path"]
    manifest_path = root / inputs["topic_manifest"]["path"]
    mapping = read_json(mapping_path)
    corpus = read_json(corpus_path)
    manifest = read_json(manifest_path)
    if (
        mapping.get("mapping_sha256") != inputs["mapping"]["sha256"]
        or corpus.get("corpus_sha256") != inputs["corpus"]["sha256"]
        or file_sha256(manifest_path) != inputs["topic_manifest"]["file_sha256"]
        or "sha256:" + str(manifest.get("topic_manifest_digest"))
        != inputs["topic_manifest"]["topic_manifest_sha256"]
    ):
        raise ProjectionError("an extraction-plan input identity differs")
    rows = mapping.get("rows", [])
    if (
        not isinstance(rows, list)
        or len(rows) != config.row_count
        or any(not config.contains_row(str(row.get("official_row", ""))) for row in rows)
    ):
        raise ProjectionError("source-map row range differs from the selected batch")
    receipt: dict[str, Any] = {}
    if config.browser_receipt_path is not None:
        receipt_path = root / inputs["browser_verification"]["path"]
        receipt = read_json(receipt_path)
        if (
            file_sha256(receipt_path)
            != inputs["browser_verification"]["file_sha256"]
            or receipt.get("observation", {}).get("identity_sha256")
            != inputs["browser_verification"]["identity_sha256"]
        ):
            raise ProjectionError("an extraction-plan browser identity differs")
    supplements: dict[str, Any] = {"topics": [], "source_resolutions": []}
    if config.supplements_path is not None:
        supplements_path = root / inputs["supplements"]["path"]
        supplements = read_json(supplements_path)
        if (
            file_sha256(supplements_path)
            != inputs["supplements"]["file_sha256"]
            or supplements.get("supplements_sha256")
            != inputs["supplements"]["supplements_sha256"]
        ):
            raise ProjectionError("an extraction-plan input identity differs: supplement")
    planned_resolutions = {
        row["official_row"]: row for row in plan["source_resolutions"]
    }
    resolution_contract = supplements if config.supplements_path is not None else corpus
    received_resolutions = {
        row["official_row"]: row
        for row in resolution_contract.get("source_resolutions", [])
    }
    expected_resolution_rows = (
        set(planned_resolutions)
        if config.name == "a"
        else {
            official_row
            for official_row, planned in planned_resolutions.items()
            if planned.get("supplement_topics")
            or planned.get("direct_supplement_topic")
        }
    )
    if set(received_resolutions) != expected_resolution_rows:
        raise ProjectionError("source resolution contract differs from the extraction plan")
    for official_row in sorted(expected_resolution_rows):
        planned = planned_resolutions[official_row]
        received = received_resolutions[official_row]
        if (
            (planned["label"], planned["eibfn"])
            != (received.get("label"), received.get("eibfn"))
            or planned.get("supplement_topics", [])
            != received.get("supplemental_topic_sources")
        ):
            raise ProjectionError(
                f"supplement resolution binding differs: {official_row}"
            )
        received_targets = received.get("target_topic_sources")
        if (
            received_targets is not None
            and planned["target_context_topics"] != received_targets
        ):
            raise ProjectionError(
                f"supplement target binding differs: {official_row}"
            )
    return mapping, corpus, manifest, receipt, supplements


def _max_depth(root: Node) -> int:
    maximum = 0
    pending = [(root, 0)]
    while pending:
        node, depth = pending.pop()
        maximum = max(maximum, depth)
        pending.extend((child, depth + 1) for child in node.children)
    return maximum


def _validate_unique_structural_paths(root: Node, topic_path: str) -> None:
    seen: dict[str, Node] = {}
    for node in walk(root):
        path = structural_path(node)
        if path in seen and seen[path] is not node:
            raise ProjectionError(
                f"non-unique structural locator in pinned topic: {topic_path}:{path}"
            )
        seen[path] = node


def load_cached_documents(
    cache: Path,
    manifest: dict[str, Any],
    plan: dict[str, Any],
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> tuple[dict[str, bytes], dict[str, Node], dict[str, dict[str, Any]]]:
    config = source_batch(batch)
    bounds = plan["bounds"]
    entries = manifest.get("topics")
    if (
        not isinstance(entries, list)
        or len(entries) != plan["expected_shape"]["manifest_topics"]
        or len(entries) > bounds["max_topics"]
    ):
        raise ProjectionError("topic manifest count differs from the extraction plan")
    pins, tocs = ibm_docs.select(*ibm_docs.load_pins(), config.cache_scope, None)
    by_topic = {pin.topic: pin for pin in pins}
    expected_paths = {entry["topic_path"] for entry in entries}
    if set(by_topic) != expected_paths:
        raise ProjectionError("cache scope differs from the topic manifest")
    for toc in tocs:
        ibm_docs.cached_toc(cache, toc)
    bodies: dict[str, bytes] = {}
    roots: dict[str, Node] = {}
    metadata: dict[str, dict[str, Any]] = {}
    for entry in entries:
        path = entry["topic_path"]
        body = ibm_docs.cached_body(cache, by_topic[path])
        if (
            len(body) != entry["bytes"]
            or len(body) > bounds["max_topic_bytes"]
            or docs_api.digest(body) != entry["sha256"]
        ):
            raise ProjectionError(f"cached topic differs from its pin: {path}")
        root = parse_html(body)
        if _max_depth(root) > bounds["max_dom_depth"]:
            raise ProjectionError(f"topic DOM depth exceeds its bound: {path}")
        _validate_unique_structural_paths(root, path)
        bodies[path] = body
        roots[path] = root
        metadata[path] = entry
    return bodies, roots, metadata


def load_supplement_documents(
    cache: Path, supplements: dict[str, Any], plan: dict[str, Any]
) -> tuple[dict[str, bytes], dict[str, Node], dict[str, dict[str, Any]]]:
    """Load only the three explicitly verified supplement bodies.

    Supplement topics are a closed set.  They never participate in the target
    corpus one-hop closure, and their cross-product authority stays attached to
    every projected evidence record.
    """

    declared = {
        topic
        for resolution in plan["source_resolutions"]
        for topic in resolution.get("supplement_topics", [])
    }
    entries = supplements.get("topics")
    if not isinstance(entries, list) or {entry.get("topic_path") for entry in entries} != declared:
        raise ProjectionError("supplement topic set differs from the extraction plan")
    import cache_cics_application_source_supplements as supplement_sources

    by_topic = {
        pin.topic: pin for pin in supplement_sources.pins_from_receipt(supplements)
    }
    bodies: dict[str, bytes] = {}
    roots: dict[str, Node] = {}
    metadata: dict[str, dict[str, Any]] = {}
    for entry in entries:
        path = entry["topic_path"]
        if path not in by_topic:
            raise ProjectionError(f"supplement pin is missing: {path}")
        body = ibm_docs.cached_body(cache, by_topic[path])
        if (
            len(body) != entry["bytes"]
            or len(body) > plan["bounds"]["max_topic_bytes"]
            or docs_api.digest(body) != entry["sha256"]
        ):
            raise ProjectionError(f"cached supplement differs from its receipt: {path}")
        product_id = entry.get("product_key")
        boundary = entry.get("target_authority_boundary")
        role = SUPPLEMENT_SOURCE_ROLES.get(entry.get("source_role"))
        if (
            role is None
            or path.split("/", 1)[0] != product_id
        ):
            raise ProjectionError(f"unsupported supplement product or role: {path}")
        authority_boundary(product_id, boundary)
        root = parse_html(body)
        if _max_depth(root) > plan["bounds"]["max_dom_depth"]:
            raise ProjectionError(f"supplement DOM depth exceeds its bound: {path}")
        _validate_unique_structural_paths(root, path)
        bodies[path] = body
        roots[path] = root
        metadata[path] = {
            **entry,
            "source_product": product_id,
            "target_authority_boundary": boundary,
            "projected_source_role": role,
        }
    return bodies, roots, metadata


def load_corpus_supplement_documents(
    cache: Path,
    corpus: dict[str, Any],
    plan: dict[str, Any],
) -> tuple[dict[str, bytes], dict[str, Node], dict[str, dict[str, Any]]]:
    """Load checker-bound direct topics carried by the selected batch corpus."""

    declared = {
        topic
        for resolution in plan["source_resolutions"]
        for topic in resolution.get("supplement_topics", [])
        if topic.split("/", 1)[0] != PRODUCT
    }
    entries = corpus.get("supplemental_topics", [])
    if not isinstance(entries, list) or {
        entry.get("topic_path") for entry in entries
    } != declared:
        raise ProjectionError("corpus supplement topic set differs from the plan")
    bodies: dict[str, bytes] = {}
    roots: dict[str, Node] = {}
    metadata: dict[str, dict[str, Any]] = {}
    for entry in entries:
        path = entry["topic_path"]
        product_id = entry.get("product_key")
        boundary = entry.get("target_authority_boundary")
        role = SUPPLEMENT_SOURCE_ROLES.get(entry.get("source_role"))
        if (
            not isinstance(product_id, str)
            or not path.startswith(product_id + "/")
            or role is None
            or entry.get("target_product_authority") is not False
            or entry.get("semantic_authority") is not False
            or entry.get("coverage_credit") != 0
        ):
            raise ProjectionError(f"corpus supplement boundary differs: {path}")
        authority_boundary(product_id, boundary)
        pin = ibm_docs.Pin(
            path,
            entry["content_url"],
            entry["sha256"],
            entry["bytes"],
            (),
        )
        if entry.get("cache_key") != pin.key:
            raise ProjectionError(f"corpus supplement cache key differs: {path}")
        body = ibm_docs.cached_body(cache, pin)
        root = parse_html(body)
        if _max_depth(root) > plan["bounds"]["max_dom_depth"]:
            raise ProjectionError(f"supplement DOM depth exceeds its bound: {path}")
        _validate_unique_structural_paths(root, path)
        bodies[path] = body
        roots[path] = root
        metadata[path] = {
            **entry,
            "source_product": product_id,
            "target_authority_boundary": boundary,
            "projected_source_role": role,
        }
    return bodies, roots, metadata


def make_evidence(
    topic_path: str,
    topic_sha256: str,
    source_role: str,
    section_id: str,
    node: Node,
    bounds: dict[str, int],
    *,
    source_product: str | None = None,
    target_authority_boundary: str | None = None,
) -> dict[str, Any]:
    material = normalized_fragment(node)
    encoded = material.encode("utf-8")
    path = structural_path(node)
    if not material or len(encoded) > bounds["max_normalized_fragment_bytes"]:
        raise ProjectionError(f"normalized fragment is empty or too large: {topic_path}")
    if len(path.encode("utf-8")) > bounds["max_locator_bytes"]:
        raise ProjectionError(f"structural locator is too large: {topic_path}")
    product_id = source_product or topic_path.split("/", 1)[0]
    authority = authority_boundary(product_id, target_authority_boundary)
    return {
        "topic_path": topic_path,
        "topic_sha256": "sha256:" + topic_sha256,
        "source_role": source_role,
        "source_product": product_id,
        "target_authority_boundary": authority,
        "section_id": section_id,
        "tag": node.tag,
        "structural_path": path,
        "fragment_sha256": "sha256:" + fragment_sha256(material),
    }


def make_candidate(
    official_row: str,
    dimension: str,
    kind: str,
    key: str,
    candidate_value: dict[str, Any],
    evidence: dict[str, Any],
) -> dict[str, Any]:
    if not 0 < len(key) <= 160:
        raise ProjectionError(
            f"candidate key length is invalid: {official_row}:{dimension}"
        )
    number = official_row.rsplit(":", 1)[-1]
    core = {
        "official_row": official_row,
        "dimension": dimension,
        "kind": kind,
        "key": key,
        "candidate_value": candidate_value,
        "evidence": evidence,
    }
    suffix = hashlib.sha256(
        json.dumps(core, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()[:12]
    prefix = source_batch_for_row(official_row).candidate_id_prefix
    return {
        "candidate_id": f"{prefix}-{number}-{dimension}-{suffix}",
        "kind": kind,
        "key": key,
        "candidate_value": candidate_value,
        "evidence": evidence,
    }


def make_issue(
    official_row: str,
    dimension: str,
    code: str,
    candidates: Iterable[dict[str, Any]] = (),
    evidence: Iterable[dict[str, Any]] = (),
    *,
    affected_dimensions: Iterable[str] | None = None,
) -> dict[str, Any]:
    number = official_row.rsplit(":", 1)[-1]
    candidate_ids = sorted({item["candidate_id"] for item in candidates})
    source_evidence = sorted(
        {json.dumps(item, sort_keys=True): item for item in evidence}.values(),
        key=lambda item: (item["topic_path"], item["structural_path"]),
    )
    if len(candidate_ids) > 64 or len(source_evidence) > 64:
        raise ProjectionError(
            f"blocking issue evidence bound exceeded: {official_row}:{dimension}:{code}"
        )
    affected = list(affected_dimensions or (dimension,))
    if (
        not affected
        or len(affected) != len(set(affected))
        or any(item not in DIMENSIONS for item in affected)
    ):
        raise ProjectionError(
            f"blocking issue affected dimensions differ: {official_row}:{dimension}:{code}"
        )
    core = {
        "official_row": official_row,
        "dimension": dimension,
        "code": code,
        "candidate_ids": candidate_ids,
        "evidence": source_evidence,
        "affected_dimensions": affected,
    }
    suffix = hashlib.sha256(
        json.dumps(core, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()[:12]
    return {
        "issue_id": (
            f"{source_batch_for_row(official_row).candidate_id_prefix}-issue-"
            f"{number}-{code}-{suffix}"
        ),
        "code": code,
        "blocking": True,
        "official_row": official_row,
        "dimension": dimension,
        "candidate_ids": candidate_ids,
        "evidence": source_evidence,
        "affected_dimensions": core["affected_dimensions"],
    }


def _coalesce_by_identity(
    values: Iterable[dict[str, Any]], identity: str, label: str
) -> list[dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for value in values:
        key = value[identity]
        prior = result.get(key)
        if prior is not None and prior != value:
            raise ProjectionError(f"{label} identity collision: {key}")
        result[key] = value
    return [result[key] for key in sorted(result)]


def _option_name(term: str) -> str | None:
    if re.match(r"^condition(?:\s*\(|$)", normalize(term), re.IGNORECASE):
        return CONDITION_NAME_OPTION
    match = re.match(r"^([A-Z][A-Z0-9-]{0,31})\b", term)
    return match.group(1) if match else None


def _symbolic_option_stack(values: Iterable[str]) -> list[str]:
    return [
        _option_name(value)
        or f"FRAGMENT-{fragment_sha256(value)[:12].upper()}"
        for value in values
    ]


def _symbolic_condition_term(
    value: str, depth: int, response_codes: dict[str, int] | None = None
) -> str:
    normalized = normalize(value)
    response = re.match(r"^([0-9]{1,5})\s+([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if response:
        return f"RESP-{response.group(1)}-{response.group(2)}"
    response2 = re.match(r"^([0-9]{1,5})\b", normalized)
    if depth > 0 and response2:
        return f"RESP2-{response2.group(1)}"
    symbol = re.match(r"^([A-Z][A-Z0-9-]{0,31})\b", normalized)
    if symbol:
        name = symbol.group(1)
        code = (response_codes or {}).get(name)
        return f"RESP-{code}-{name}" if code is not None else f"SYMBOL-{name}"
    return f"FRAGMENT-{fragment_sha256(normalized)[:12].upper()}"


def _markers_in_text(value: str) -> list[str]:
    text = value.casefold().replace("_", "-").replace("pointer-ref", "ptr-ref")
    found = [
        marker
        for marker in MARKERS
        if re.search(rf"(?<![a-z0-9-]){re.escape(marker)}(?![a-z0-9-])", text)
    ]
    if not found and re.fullmatch(r"[a-z][a-z0-9-]*name", text):
        return ["name"]
    return found


def _non_operand_annotation(value: str) -> bool:
    normalized = normalize(value)
    text = normalized.casefold()
    return bool(
        re.fullmatch(r"[0-9]+", text)
        or re.fullmatch(r"[A-Z][A-Z0-9-]*", normalized)
        or re.search(r"\bonly\b", text)
    )


def _argument_markers(node: Node) -> list[str]:
    values: list[str] = []
    for child in walk(node):
        if "var" not in classes(child):
            continue
        raw = child.text()
        if _non_operand_annotation(raw):
            continue
        markers = _markers_in_text(raw)
        markers = markers or ["unknown"]
        for marker in markers:
            if marker not in values:
                values.append(marker)
    if not values:
        raw = normalize(node.text())
        operand = re.match(r"^[A-Z][A-Z0-9-]{0,31}\s*\(([^)]*)\)", raw)
        marker_source = operand.group(1) if operand is not None else raw
        markers = (
            _markers_in_text(marker_source)
            if operand is not None or not _non_operand_annotation(raw)
            else []
        )
        for marker in markers:
            if marker not in values:
                values.append(marker)
    return values or ["none"]


def _direction(marker: str, definitions: Iterable[Node] = ()) -> str:
    if marker in {"data-value", "ptr-value", "ptr-value64", "name", "filename", "systemname", "label", "hhmmss"}:
        return "input"
    if marker in {"ptr-ref", "ptr-ref64"}:
        return "output"
    if marker == "none":
        return "none"
    if marker not in {"data-area", "data-area64", "cvda"}:
        return "unknown"
    text = " ".join(node.text(exclude_tags=frozenset({"dl", "ul", "ol"})) for node in definitions).casefold()
    sends = bool(re.search(r"\b(specifies|contains|supplies|passes|input)\b", text))
    receives = bool(re.search(r"\b(returns|receives|output|set by cics|set to)\b", text))
    if sends and receives:
        return "input-output"
    if sends:
        return "input"
    if receives:
        return "output"
    return "unknown"


def _row_discriminators(rows: list[dict[str, Any]]) -> dict[str, set[str]]:
    words = [row["label"].split() for row in rows]
    common = 0
    while all(len(value) > common for value in words) and len(
        {value[common] for value in words}
    ) == 1:
        common += 1
    return {
        row["official_row"]: set(value[common:])
        for row, value in zip(rows, words)
    }


def _group_applies(
    row: dict[str, Any], siblings: list[dict[str, Any]], group: DefinitionGroup
) -> bool:
    if len(siblings) == 1 or row.get("selection_kind") not in {"shared-page", "combined-page"}:
        return True
    name = _option_name(group.stack[0])
    discriminators = _row_discriminators(siblings)
    all_discriminators = set().union(*discriminators.values())
    return name not in all_discriminators or name in discriminators[row["official_row"]]


def _diagram_applies(
    row: dict[str, Any], siblings: list[dict[str, Any]], diagram: SyntaxDiagram
) -> bool:
    if row.get("selection_kind") == "combined-page" and len(siblings) > 1:
        return normalize(diagram.title).casefold() == normalize(row["label"]).casefold()
    if row.get("selection_kind") != "shared-page" or len(siblings) == 1:
        return True
    discriminators = _row_discriminators(siblings)[row["official_row"]]
    values = {
        re.match(r"^[A-Z][A-Z0-9-]*", token.value).group(0)
        for item in syntax_items(diagram)
        for token in item.tokens
        if token.kind == "keyword" and re.match(r"^[A-Z][A-Z0-9-]*", token.value)
    }
    return not discriminators or discriminators <= values


def _syntax_items_for_row(
    row: dict[str, Any], siblings: list[dict[str, Any]], diagram: SyntaxDiagram
) -> list[SyntaxItem]:
    """Split shared-page syntax without leaking sibling-only command branches."""

    items = syntax_items(diagram)
    if row.get("selection_kind") != "shared-page" or len(siblings) == 1:
        return items
    discriminators = _row_discriminators(siblings)
    selected = discriminators[row["official_row"]]
    sibling_only = set().union(*discriminators.values()) - selected

    def choice_branches(item: SyntaxItem) -> set[tuple[Node, Node]]:
        result: set[tuple[Node, Node]] = set()
        for token in item.tokens:
            current = token.node.parent
            branch = token.node
            while current is not None and current is not diagram.node:
                if "groupchoice" in classes(current):
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


def _syntax_option_name(
    item: SyntaxItem, documented_options: set[str]
) -> str | None:
    """Bind an operand to the documented option, never the command prefix."""

    leading = _option_name("".join(token.value for token in item.tokens))
    if leading in documented_options:
        return leading
    matches = [
        option
        for token in item.tokens
        if token.kind == "keyword"
        and (option := _option_name(token.value)) in documented_options
    ]
    return matches[-1] if matches else None


def _reject_supplement_alias(
    official_row: str,
    topic_path: str,
    candidates: Iterable[dict[str, Any]],
    forbidden_alias_tokens: Iterable[str],
) -> None:
    forbidden = set(forbidden_alias_tokens)
    projected_tokens = {
        token["value"].rstrip("(")
        for candidate in candidates
        if candidate["evidence"]["topic_path"] == topic_path
        and candidate["kind"] == "source-syntax"
        for token in candidate["candidate_value"]["tokens"]
        if token["kind"] == "keyword"
    }
    if projected_tokens & forbidden:
        raise ProjectionError(
            f"supplement aliases a distinct target command: {official_row}"
        )


def _detected_argument_conflicts(
    candidates: Iterable[dict[str, Any]], roots: dict[str, Node]
) -> dict[tuple[str, str, str, str], list[dict[str, Any]]]:
    grouped: dict[
        tuple[str, str], dict[str, tuple[set[str], list[dict[str, Any]]]]
    ] = {}
    for candidate in candidates:
        if candidate["kind"] != "source-operand-direction":
            continue
        marker = candidate["candidate_value"]["marker"]
        if marker in {"none", "unknown"}:
            continue
        topic = candidate["evidence"]["topic_path"]
        sections = direct_sections(roots[topic])
        section_id = candidate["evidence"]["section_id"]
        source = (
            "syntax"
            if sections.get("Syntax") is not None
            and section_id == sections["Syntax"].section_id
            else "options"
            if sections.get("Options") is not None
            and section_id == sections["Options"].section_id
            else None
        )
        if source is None:
            continue
        values, evidence = grouped.setdefault((topic, candidate["key"]), {}).setdefault(
            source, (set(), [])
        )
        values.add(marker)
        evidence.append(candidate)

    conflicts: dict[tuple[str, str, str, str], list[dict[str, Any]]] = {}
    for (topic, key), sources in grouped.items():
        if set(sources) != {"syntax", "options"}:
            continue
        syntax_values, syntax_candidates = sources["syntax"]
        option_values, option_candidates = sources["options"]
        if (
            len(syntax_values) == 1
            and len(option_values) == 1
            and syntax_values != option_values
        ):
            signature = (
                topic,
                key,
                next(iter(syntax_values)),
                next(iter(option_values)),
            )
            conflicts[signature] = syntax_candidates + option_candidates
    return conflicts


def _section_id_for(node: Node) -> str:
    current: Node | None = node
    while current is not None:
        if current.tag == "section":
            headings = [
                child
                for child in current.children
                if child.tag == "h2" and "sectiontitle" in classes(child)
            ]
            if len(headings) == 1 and headings[0].attrs.get("id"):
                return headings[0].attrs["id"]
        current = current.parent
    return "__topic__"


def _manual_nodes(selector: dict[str, Any], root: Node) -> list[tuple[dict[str, Any], Node]]:
    found: list[tuple[dict[str, Any], Node]] = []
    for fragment in selector["fragment_selectors"]:
        tag = fragment["tag"]
        if tag == "section":
            heading_id = fragment["heading_id"]
            matches = [
                node
                for node in walk(root)
                if node.tag == "section"
                and any(child.tag == "h2" and child.attrs.get("id") == heading_id for child in node.children)
            ]
        elif "id" in fragment:
            matches = [
                node
                for node in walk(root)
                if node.tag == tag and node.attrs.get("id") == fragment["id"]
            ]
        elif tag in {"ul", "ol"} and "within_heading_id" in fragment:
            sections = [
                node
                for node in walk(root)
                if node.tag == "section"
                and any(
                    child.tag == "h2"
                    and child.attrs.get("id") == fragment["within_heading_id"]
                    for child in node.children
                )
            ]
            matches = [node for section in sections for node in walk(section) if node.tag == tag]
        else:
            raise ProjectionError(f"unsupported manual selector: {selector['id']}")
        if len(matches) != fragment["cardinality"]:
            raise ProjectionError(
                f"manual selector cardinality differs: {selector['id']}:{tag}"
            )
        found.extend((fragment, node) for node in matches)
    return found


def _response_code_map(
    plan: dict[str, Any], roots: dict[str, Node]
) -> dict[str, int]:
    selectors = [
        selector
        for selector in plan["manual_context_selectors"]
        if selector["id"] == "global-response-codes"
    ]
    if not selectors:
        return {}
    if len(selectors) != 1:
        raise ProjectionError("response-code selector is not unique")
    selector = selectors[0]
    result: dict[str, int] = {}
    for _, table in _manual_nodes(selector, roots[selector["topic_path"]]):
        for row in (node for node in walk(table) if node.tag == "tr"):
            cells = _cells(row)
            if len(cells) < 2:
                continue
            name = normalize(cells[0].text()).upper()
            code_text = normalize(cells[1].text())
            if re.fullmatch(r"[A-Z][A-Z0-9-]{0,31}", name) is None or not code_text.isdigit():
                continue
            code = int(code_text)
            if not 0 <= code <= 255 or (name in result and result[name] != code):
                raise ProjectionError(f"response-code table conflicts for {name}")
            result[name] = code
    if not result:
        raise ProjectionError("response-code table is empty")
    return result


def _condition_name_authority(
    roots: dict[str, Node],
    metadata: dict[str, dict[str, Any]],
    response_codes: dict[str, int],
) -> dict[str, Any]:
    root = roots.get(CONDITION_NAME_TOPIC)
    if root is None or CONDITION_NAME_TOPIC not in metadata:
        raise ProjectionError("EIBRESP condition-name authority is outside source closure")
    tables = [
        node
        for node in walk(root)
        if node.tag == "table" and node.attrs.get("id") == CONDITION_NAME_TABLE
    ]
    if len(tables) != 1:
        raise ProjectionError("EIBRESP condition-name table locator is not unique")
    name_to_code: dict[str, int] = {}
    for row in (node for node in walk(tables[0]) if node.tag == "tr"):
        cells = _cells(row)
        for offset in (0, 2):
            if len(cells) <= offset + 1:
                continue
            code_text = normalize(cells[offset].text())
            name = normalize(cells[offset + 1].text()).upper()
            if not code_text.isdigit() or not re.fullmatch(
                r"[A-Z][A-Z0-9-]{0,31}", name
            ):
                continue
            code = int(code_text)
            if not 0 <= code <= 255 or (
                name in name_to_code and name_to_code[name] != code
            ):
                raise ProjectionError(f"EIBRESP condition-name table conflicts for {name}")
            name_to_code[name] = code
    if not name_to_code:
        raise ProjectionError("EIBRESP condition-name table is empty")
    for name in set(name_to_code) & set(response_codes):
        if name_to_code[name] != response_codes[name]:
            raise ProjectionError(
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
        "topic_sha256": "sha256:" + metadata[CONDITION_NAME_TOPIC]["sha256"],
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


def _dynamic_condition_clause(
    group: DefinitionGroup, root: Node, arguments: list[str]
) -> dict[str, Any]:
    if _option_name(group.stack[0]) != CONDITION_NAME_OPTION:
        return {}
    limits = [
        node
        for node in walk(root)
        if node.tag == "p"
        and re.search(
            r"cannot include\s+more\s+than\s+(?:16|sixteen)\s+conditions",
            normalize(node.text()),
            re.IGNORECASE,
        )
    ]
    if len(limits) != 1:
        raise ProjectionError("dynamic condition occurrence limit is not unique")
    if set(arguments) == {"label"}:
        label_operand = "optional"
    elif arguments == ["none"]:
        label_operand = "forbidden"
    else:
        raise ProjectionError("dynamic condition label shape differs")
    return {
        "dynamic_name_profile": CONDITION_NAME_PROFILE,
        "minimum_occurrences": 1,
        "maximum_occurrences": 16,
        "label_operand": label_operand,
        "occurrence_limit_fragment_sha256": "sha256:"
        + fragment_sha256(normalized_fragment(limits[0])),
    }


def _source_resolution_context_node(root: Node, selector: str) -> Node:
    if selector == "unique-article-root":
        matches = [node for node in walk(root) if node.tag == "article"]
    elif selector == "unique-h1":
        matches = [node for node in walk(root) if node.tag == "h1"]
    else:
        def matches_selector(node: Node) -> bool:
            value = node.text().casefold()
            if selector == "dump-applicability-item":
                return (
                    node.tag == "li"
                    and "exec cics dump" in value
                    and "always" in value
                )
            if selector == "traceid-compatibility-note":
                return (
                    "enter tracenum" in value
                    and "enter traceid" in value
                    and "compatibility" in value
                )
            raise ProjectionError(f"unsupported source-resolution selector: {selector}")

        candidates = [node for node in walk(root) if matches_selector(node)]
        matches = [
            node
            for node in candidates
            if not any(matches_selector(child) for child in node.children)
        ]
    if len(matches) != 1:
        raise ProjectionError(
            f"source-resolution selector cardinality differs: {selector}"
        )
    return matches[0]


THREADSAFE_LABEL_ALIASES = {
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
THREADSAFE_FORM_ALIASES = {
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

# The DPL page qualifies only the APPC forms in its table by principal
# facility.  These catalog rows are the APPC forms/variant unions evidenced by
# the pinned command pages; the other ISSUE/SEND forms in the table remain
# ordinary table restrictions.
DPL_APPC_PRINCIPAL_FACILITY_LABELS = frozenset(
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

THREADSAFE_FILE_COMMANDS = frozenset(
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
THREADSAFE_QUEUE_COMMANDS = frozenset(
    {"DELETEQ TD", "DELETEQ TS", "READQ TD", "READQ TS", "WRITEQ TD", "WRITEQ TS"}
)
THREADSAFE_PROFILE_PREDICATES = {
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

LANGUAGE_PROFILE_BY_LABEL = {
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

OPTION_LEGALITY_PATTERNS = tuple(
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\bif (?:you )?specif(?:y|ied)\b.{0,320}\b(?:must|cannot|may not|need not|required)\b",
        r"\b(?:must|cannot|may not|must not)\b.{0,240}\b(?:specif(?:y|ied)|used|combined)\b",
        r"\b(?:mutually exclusive|cannot be used together|not valid with|only valid with|required with)\b",
        r"\b(?:must|required|cannot|may not|must not|need not|only|unless|either|should)\b",
    )
)

CONTEXT_PREDICATE_PATTERNS = tuple(
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


def _cells(row: Node) -> list[Node]:
    return [child for child in row.children if child.tag in {"td", "th"}]


def _dpl_server_applicability(label: str, node: Node) -> dict[str, Any]:
    """Return the row outcome from the complete pinned DPL server section."""

    section_text = normalize(node.text()).upper()
    if node.tag == "section":
        required_prose = (
            "ANY OF THE EXEC CICS WEB COMMANDS",
            "RESP2 VALUE OF 1",
            "EXTRACT TCPIP",
            "EXTRACT CERTIFICATE",
            "RESP2 VALUE OF 5",
            "APPC COMMANDS LISTED ARE PROHIBITED ONLY WHEN",
            "PRINCIPAL FACILITY",
        )
        if any(value not in section_text for value in required_prose):
            raise ProjectionError("DPL server prose contract differs")

    if label.startswith("WEB ") or label in {"EXTRACT TCPIP", "EXTRACT CERTIFICATE"}:
        response2 = 5 if label in {
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
                "resp2": response2,
            },
        }

    matches: list[tuple[str, str]] = []
    for row in (item for item in walk(node) if item.tag == "tr"):
        cells = _cells(row)
        if len(cells) < 2:
            continue
        command = normalize(cells[0].text()).upper()
        options = normalize(cells[1].text()).upper()
        if not command or command == "COMMAND":
            continue
        if label == command or (
            command in {"ISSUE", "SEND"} and label.startswith(command + " ")
        ):
            matches.append((command, options))
    if not matches:
        return {"dpl_server": "allowed"}
    command, options = max(matches, key=lambda item: len(item[0]))
    suffix = label[len(command) :].strip()
    option_values = sorted(
        set(re.findall(r"[A-Z][A-Z0-9-]*(?:\([A-Z]+\))?", options))
    )
    if command in {"ISSUE", "SEND"} and suffix:
        if suffix.split()[0] not in {value.split("(", 1)[0] for value in option_values}:
            return {"dpl_server": "allowed"}
        kind = "prohibited"
        prohibited: list[str] = []
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

    restriction_predicates: list[str] = []
    if label in DPL_APPC_PRINCIPAL_FACILITY_LABELS:
        kind = "conditional"
        restriction_predicates.append("principal-facility")
        if label == "CONNECT PROCESS":
            restriction_predicates.append(
                "connect-process-principal-facility-error-is-dpl"
            )
    return {
        "dpl_server": "restricted",
        "dpl_restriction": {
            "kind": kind,
            "source_kind": "table",
            "table_command": command,
            "prohibited_options": prohibited,
            "requirement": requirement,
            "restriction_predicates": restriction_predicates,
            **(
                {"restriction_match": "any-of"}
                if prohibited and restriction_predicates
                else {}
            ),
            "condition": "INVREQ",
            "resp2": 200,
        },
    }


def _threadsafe_names(node: Node) -> dict[str, str]:
    names: dict[str, str] = {}
    for item in walk(node):
        if item.tag != "li":
            continue
        value = normalize(item.text(exclude_tags=frozenset({"ul", "ol"}))).upper()
        conditional = "*" in value or (
            "THREADSAFE IF" in value and "NON-THREADSAFE IF" in value
        )
        value = re.sub(r"\s*\([^)]*\).*$", "", value)
        value = value.replace("*", "").strip()
        if not value or len(value) > 80:
            continue
        parts = [part.strip() for part in value.split(" AND ")]
        if all(re.fullmatch(r"[A-Z][A-Z0-9 ]*", part) for part in parts):
            for part in parts:
                names[part] = "conditional" if conditional else "yes"
    return names


def _threadsafe_condition(label: str) -> dict[str, Any]:
    source_label = THREADSAFE_LABEL_ALIASES.get(label, label)
    if source_label == "LINK":
        profile = "program-link-locality"
    elif source_label in THREADSAFE_FILE_COMMANDS | {"WRITE"}:
        profile = "file-control-storage-and-locality"
    elif source_label in THREADSAFE_QUEUE_COMMANDS:
        profile = "queue-control-locality"
    elif source_label == "ENQ":
        profile = "enq-locality"
    elif source_label == "DEQ":
        profile = "deq-locality"
    elif source_label == "WRITE OPERATOR":
        profile = "write-operator-reply-key9"
    else:
        raise ProjectionError(f"conditional threadsafe command lacks a profile: {label}")
    return {"profile": profile, **THREADSAFE_PROFILE_PREDICATES[profile]}


def _threadsafe_applicability(
    label: str, node: Node, row_state: str
) -> dict[str, Any]:
    if row_state == "source-gap":
        return {"threadsafe": "not-applicable"}
    names = _threadsafe_names(node)
    wanted = THREADSAFE_FORM_ALIASES.get(
        label, (THREADSAFE_LABEL_ALIASES.get(label, label),)
    )
    statuses = {
        names.get(re.sub(r"\s*\(CHANNEL\)$", "", item), "no")
        for item in wanted
    }
    status = next(iter(statuses)) if len(statuses) == 1 else "conditional"
    result: dict[str, Any] = {"threadsafe": status}
    if status == "conditional":
        result["threadsafe_condition"] = _threadsafe_condition(label)
    return result


def _language_profile(node: Node) -> str | None:
    text = normalize(node.text()).lower()
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


def _language_nodes(root: Node) -> list[Node]:
    return [
        node
        for node in walk(root)
        if _language_profile(node) is not None
        and (
            (node.tag == "p" and "shortdesc" in classes(node))
            or (
                node.tag == "div"
                and "cds--inline-notification__subtitle" in classes(node)
            )
        )
    ]


def _language_applicability(profile: str) -> dict[str, Any]:
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


def _option_legality_descriptions(group: DefinitionGroup) -> list[Node]:
    return [
        description
        for description in group.descriptions
        if any(
            pattern.search(normalized_fragment(description))
            for pattern in OPTION_LEGALITY_PATTERNS
        )
    ]


def _section_title(node: Node) -> str | None:
    current = node.parent
    while current is not None:
        if current.tag == "section":
            heading = next(
                (
                    child
                    for child in current.children
                    if child.tag == "h2" and "sectiontitle" in classes(child)
                ),
                None,
            )
            return normalize(heading.text()) if heading is not None else None
        current = current.parent
    return None


def _is_context_predicate(node: Node) -> bool:
    if node.tag != "p" or _section_title(node) not in {
        "Syntax",
        "Description",
        "Rules",
    }:
        return False
    text = normalized_fragment(node)
    return any(pattern.search(text) for pattern in CONTEXT_PREDICATE_PATTERNS)


def _mapped_dpl_predicate_nodes(label: str, root: Node) -> list[Node]:
    if label != "GDS EXTRACT ATTRIBUTES":
        return []
    return [
        node
        for node in walk(root)
        if node.tag == "td"
        and normalize(node.text()).casefold() == "invreq for a dpl server program."
    ]


def _syntax_head(
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
    return normalize(" ".join(head_parts)).upper()


def _syntax_identity(
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
            raise ProjectionError("ASKTIME ABSTIME discriminator is absent")
        discriminators = [{"name": "ABSTIME", "state": "present"}]
    return relation, discriminators


def _manual_context_value(
    selector_id: str,
    association: str,
    row: dict[str, Any],
    node: Node,
    resolution: dict[str, Any] | None,
    dimension: str,
    fragment: dict[str, Any],
) -> dict[str, Any]:
    value: dict[str, Any] = {
        "type": "context",
        "selector_id": selector_id,
        "association": association,
    }
    label = str(row["label"])
    not_applicable = row["state"] == "source-gap" and (
        resolution is None or resolution.get("resolution") == "target-internal-only"
    )
    if (
        selector_id == "global-command-format"
        and dimension == "execution-context"
        and fragment.get("heading_id") == "dfhp4kt__title__2"
    ):
        applicability = {
            "local_task": "not-applicable" if not_applicable else "allowed"
        }
        if not_applicable:
            applicability.update(
                {
                    "cobol": "not-applicable",
                    "language_restriction": {"profile": "internal-only"},
                }
            )
        elif label not in LANGUAGE_PROFILE_BY_LABEL:
            applicability.update(
                _language_applicability("exec-cics-all-supported-languages")
            )
        value["applicability"] = applicability
    elif (
        selector_id == "dpl-server-restrictions"
        and dimension == "execution-context"
        and node.tag == "section"
    ):
        if label != "GDS EXTRACT ATTRIBUTES":
            value["applicability"] = (
                {"dpl_server": "not-applicable"}
                if not_applicable
                else _dpl_server_applicability(label, node)
            )
    elif (
        selector_id == "threadsafe-command-list"
        and dimension == "execution-context"
        and node.tag == "section"
    ):
        value["applicability"] = _threadsafe_applicability(
            label, node, "source-gap" if not_applicable else "mapped"
        )
    return value


def _inputs_for_projection(
    root: Path,
    plan: dict[str, Any],
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    mapping = root / config.map_path
    corpus = root / config.corpus_path
    manifest = root / config.manifest_path
    extraction = root / config.plan_path
    result = {
        "source_map": {
            "path": str(config.map_path),
            "file_sha256": file_sha256(mapping),
            "logical_sha256": plan["inputs"]["mapping"]["sha256"],
        },
        "source_corpus": {
            "path": str(config.corpus_path),
            "file_sha256": file_sha256(corpus),
            "logical_sha256": plan["inputs"]["corpus"]["sha256"],
        },
        "topic_manifest": {
            "path": str(config.manifest_path),
            "file_sha256": file_sha256(manifest),
            "logical_sha256": plan["inputs"]["topic_manifest"]["topic_manifest_sha256"],
        },
        "extraction_plan": {
            "path": str(config.plan_path),
            "file_sha256": file_sha256(extraction),
            "logical_sha256": plan["extraction_sha256"],
        },
    }
    if config.supplements_path is not None:
        supplements = root / config.supplements_path
        result["source_supplements"] = {
            "path": str(config.supplements_path),
            "file_sha256": file_sha256(supplements),
            "logical_sha256": plan["inputs"]["supplements"]["supplements_sha256"],
        }
    return result


def _assert_expected_shape(
    mapping: dict[str, Any],
    corpus: dict[str, Any],
    manifest: dict[str, Any],
    roots: dict[str, Node],
    plan: dict[str, Any],
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, int]:
    config = source_batch(batch)
    expected = plan["expected_shape"]
    rows = mapping.get("rows", [])
    mapped = [row for row in rows if row.get("state") == "mapped"]
    gaps = [row for row in rows if row.get("state") == "source-gap"]
    paths = sorted({topic["topic_path"] for row in mapped for topic in row["topics"]})
    syntax_sections = 0
    syntax_diagram_count = 0
    syntax_parts = 0
    multiple = 0
    option_sections = 0
    option_terms = 0
    condition_sections = 0
    condition_terms = 0
    for path in paths:
        sections = direct_sections(roots[path])
        syntax = sections.get("Syntax")
        if syntax is not None:
            syntax_sections += 1
            diagrams = syntax_diagrams(syntax)
            syntax_diagram_count += len(diagrams)
            syntax_parts += sum(len(diagram.pieces) for diagram in diagrams)
            multiple += len(diagrams) > 1
        options = sections.get("Options")
        if options is not None:
            option_sections += 1
            option_terms += sum(group.depth == 0 for group in definition_groups(options))
        conditions = sections.get("Conditions")
        if conditions is not None:
            condition_sections += 1
            condition_terms += sum(group.depth == 0 for group in definition_groups(conditions))
    actual = {
        "mapping_rows": len(rows),
        "mapped_rows": len(mapped),
        "source_gap_rows": len(gaps),
        "mapping_edges": sum(len(row.get("topics", [])) for row in rows),
        "mapped_command_topics": len(paths),
        "linked_context_topics": len(corpus.get("linked_context_topics", [])),
        "manual_topics": len(corpus.get("manual_topics", [])),
        "manifest_topics": len(manifest.get("topics", [])),
        "browser_verified_topics": len(manifest.get("topics", [])),
        "dimensions_per_row": len(DIMENSIONS),
        "syntax_section_topics": syntax_sections,
        "syntax_diagrams": syntax_diagram_count,
        "multiple_syntax_diagram_topics": multiple,
        "options_section_topics": option_sections,
        "conditions_section_topics": condition_sections,
        "supplement_topics": len(
            {
                topic
                for resolution in plan["source_resolutions"]
                for topic in resolution.get("supplement_topics", [])
            }
        ),
        "supplement_direct_topics": len(
            {
                resolution["direct_supplement_topic"]
                for resolution in plan["source_resolutions"]
                if resolution["resolution"]
                in {"authority-bounded-projection", "target-product-projection"}
            }
        ),
        "supplement_context_topics": len(
            {
                context["topic_path"]
                for resolution in plan["source_resolutions"]
                for context in resolution.get("context_fragments", [])
                if context["topic_path"].split("/", 1)[0] != PRODUCT
            }
        ),
    }
    if actual["mapping_rows"] != config.row_count:
        raise ProjectionError("pinned source row count differs from the selected batch")
    if actual != expected:
        raise ProjectionError(f"pinned source shape differs: expected={expected} actual={actual}")
    return {
        "syntax_diagrams": syntax_diagram_count,
        "syntax_svg_parts": syntax_parts,
        "top_level_option_terms": option_terms,
        "top_level_condition_terms": condition_terms,
    }


def build_projection(
    root: Path,
    plan: dict[str, Any],
    mapping: dict[str, Any],
    corpus: dict[str, Any],
    manifest: dict[str, Any],
    roots: dict[str, Node],
    metadata: dict[str, dict[str, Any]],
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    validate_plan(plan, config)
    shape_counts = _assert_expected_shape(
        mapping, corpus, manifest, roots, plan, config
    )
    bounds = plan["bounds"]
    exceptions = {
        (row.get("official_row"), row["topic_path"], row["dimension"]): row
        for row in plan["section_exceptions"]
    }
    resolution_plan = {
        row["official_row"]: row for row in plan["source_resolutions"]
    }
    response_codes = _response_code_map(plan, roots)
    condition_name_authority = _condition_name_authority(
        roots, metadata, response_codes
    )
    mapped_rows = [row for row in mapping["rows"] if row["state"] == "mapped"]
    rows_by_topic: dict[str, list[dict[str, Any]]] = {}
    for row in mapped_rows:
        for topic in row["topics"]:
            rows_by_topic.setdefault(topic["topic_path"], []).append(row)
    linked_paths = {row["topic_path"] for row in corpus["linked_context_topics"]}
    manual_by_path = {row["topic_path"]: row for row in corpus["manual_topics"]}

    projected_rows: list[dict[str, Any]] = []
    all_issues: list[dict[str, Any]] = []
    all_candidates: list[dict[str, Any]] = []
    matched_conflicts: set[str] = set()
    conflict_contract_drift: list[tuple[str, str, tuple[str, str, str, str]]] = []

    for row in mapping["rows"]:
        official_row = row["official_row"]
        resolution = resolution_plan.get(official_row)
        dimensions: dict[str, dict[str, Any]] = {
            name: {"name": name, "state": "unmatched", "candidates": [], "issues": []}
            for name in DIMENSIONS
        }
        primary = {name: False for name in DIMENSIONS}
        topic_roles: dict[str, str] = {}
        language_candidate_count = 0

        direct_topics = list(row.get("topics", []))
        if (
            resolution is not None
            and resolution["resolution"]
            in {"authority-bounded-projection", "target-product-projection"}
        ):
            direct_topics.append(
                {
                    "topic_path": resolution["direct_supplement_topic"],
                    "role": "supplement",
                }
            )
        for topic in direct_topics:
            path = topic["topic_path"]
            role = (
                metadata[path]["projected_source_role"]
                if topic["role"] == "supplement"
                else DIRECT_SOURCE_ROLES[topic["role"]]
            )
            topic_roles.setdefault(path, role)
            for language_node in _language_nodes(roots[path]):
                profile = _language_profile(language_node)
                if profile is None:
                    continue
                expected_profile = LANGUAGE_PROFILE_BY_LABEL.get(str(row["label"]))
                if profile != expected_profile:
                    raise ProjectionError(
                        f"language applicability inventory differs: {official_row}:{profile}"
                    )
                language_evidence = make_evidence(
                    path,
                    metadata[path]["sha256"],
                    role,
                    _section_id_for(language_node),
                    language_node,
                    bounds,
                )
                language_candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    "mapped-language-applicability",
                    {
                        "type": "context",
                        "selector_id": "mapped-language-applicability",
                        "association": "mapping-topic-edge",
                        "applicability": _language_applicability(profile),
                    },
                    language_evidence,
                )
                dimensions["execution-context"]["candidates"].append(
                    language_candidate
                )
                primary["execution-context"] = True
                language_candidate_count += 1

            for dpl_node in _mapped_dpl_predicate_nodes(
                str(row["label"]), roots[path]
            ):
                dpl_evidence = make_evidence(
                    path,
                    metadata[path]["sha256"],
                    role,
                    _section_id_for(dpl_node),
                    dpl_node,
                    bounds,
                )
                dpl_candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    "mapped-dpl-applicability",
                    {
                        "type": "context",
                        "selector_id": "mapped-dpl-applicability",
                        "association": "mapping-topic-edge",
                        "applicability": {"dpl_server": "restricted"},
                        "predicate_id": dpl_evidence["fragment_sha256"],
                        "predicate_state": "bounded",
                    },
                    dpl_evidence,
                )
                dimensions["execution-context"]["candidates"].append(dpl_candidate)
                dimensions["execution-context"]["issues"].append(
                    make_issue(
                        official_row,
                        "execution-context",
                        "context-predicate-not-structured",
                        [dpl_candidate],
                        [dpl_evidence],
                    )
                )
                primary["execution-context"] = True

            for predicate_node in (
                node for node in walk(roots[path]) if _is_context_predicate(node)
            ):
                if _language_profile(predicate_node) is not None:
                    continue
                predicate_evidence = make_evidence(
                    path,
                    metadata[path]["sha256"],
                    role,
                    _section_id_for(predicate_node),
                    predicate_node,
                    bounds,
                )
                predicate_candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    "mapped-context-predicate",
                    {
                        "type": "context",
                        "selector_id": "mapped-context-predicate",
                        "association": "mapping-topic-edge",
                        "predicate_id": predicate_evidence["fragment_sha256"],
                        "predicate_state": "bounded",
                    },
                    predicate_evidence,
                )
                dimensions["execution-context"]["candidates"].append(
                    predicate_candidate
                )
                dimensions["execution-context"]["issues"].append(
                    make_issue(
                        official_row,
                        "execution-context",
                        "context-predicate-not-structured",
                        [predicate_candidate],
                        [predicate_evidence],
                    )
                )
                primary["execution-context"] = True

            sections = direct_sections(roots[path])
            siblings = rows_by_topic.get(path, [row])
            condition_sibling_only = (
                set().union(*_row_discriminators(siblings).values())
                - _row_discriminators(siblings)[official_row]
                if row.get("selection_kind") == "shared-page" and len(siblings) > 1
                else set()
            )
            option_section = sections.get("Options")
            option_groups = (
                [
                    group
                    for group in definition_groups(option_section)
                    if _group_applies(row, siblings, group)
                ]
                if option_section is not None
                else []
            )
            documented_options = {
                name
                for group in option_groups
                if (name := _option_name(group.stack[0])) is not None
            }

            syntax = sections.get("Syntax")
            if syntax is not None:
                chosen = [
                    (ordinal, diagram)
                    for ordinal, diagram in enumerate(syntax_diagrams(syntax), 1)
                    if _diagram_applies(row, siblings, diagram)
                ]
                if row.get("selection_kind") == "combined-page" and len(chosen) != 1:
                    issue = make_issue(official_row, "syntax", "unresolved-syntax-diagram")
                    dimensions["syntax"]["issues"].append(issue)
                selected_items_by_ordinal = {
                    ordinal: _syntax_items_for_row(row, siblings, diagram)
                    for ordinal, diagram in chosen
                }
                if row.get("selection_kind") == "shared-page" and len(siblings) > 1:
                    selected_option_names = {
                        name
                        for ordinal, _ in chosen
                        for item in selected_items_by_ordinal[ordinal]
                        if (name := _syntax_option_name(item, documented_options))
                        is not None
                    }
                    all_syntax_option_names = {
                        name
                        for _, diagram in chosen
                        for item in syntax_items(diagram)
                        if (name := _syntax_option_name(item, documented_options))
                        is not None
                    }
                    option_groups = [
                        group
                        for group in option_groups
                        if (name := _option_name(group.stack[0])) is None
                        or name not in all_syntax_option_names
                        or name in selected_option_names
                    ]
                    documented_options = {
                        name
                        for group in option_groups
                        if (name := _option_name(group.stack[0])) is not None
                    }
                for panel_index, (diagram_ordinal, diagram) in enumerate(chosen, 1):
                    variant = f"diagram-{diagram_ordinal:04d}"
                    selected_items = selected_items_by_ordinal[diagram_ordinal]
                    tokens = [
                        {
                            "kind": token.kind,
                            "value": bounded_text(token.value, 128, "syntax token"),
                            "relation": item.relation,
                            "piece": syntax_piece_ordinal(token, diagram),
                            "group_path": syntax_group_path(token, diagram),
                        }
                        for item in selected_items
                        for token in item.tokens
                    ]
                    if not 0 < len(tokens) <= 512:
                        raise ProjectionError(
                            f"syntax token bound exceeded: {path}:{variant}"
                        )
                    structure = json.dumps(
                        tokens, ensure_ascii=True, sort_keys=True, separators=(",", ":")
                    )
                    syntax_head = _syntax_head(tokens, documented_options)
                    if not syntax_head:
                        raise ProjectionError(
                            f"syntax command head is empty: {path}:{variant}"
                        )
                    identity_relation, identity_discriminators = _syntax_identity(
                        str(row["label"]), syntax_head, tokens
                    )
                    panel_role = (
                        "continuation"
                        if identity_relation == "continuation"
                        else "command-head"
                    )
                    evidence = make_evidence(
                        path,
                        metadata[path]["sha256"],
                        role,
                        syntax.section_id,
                        diagram.node,
                        bounds,
                    )
                    candidate = make_candidate(
                        official_row,
                        "syntax",
                        "source-syntax",
                        variant,
                        {
                            "type": "syntax",
                            "variant": variant,
                            "syntax_head": syntax_head,
                            "panel": {
                                "index": panel_index,
                                "total": len(chosen),
                                "role": panel_role,
                            },
                            "identity_relation": identity_relation,
                            "identity_discriminators": identity_discriminators,
                            "tokens": tokens,
                            "structure_sha256": "sha256:" + fragment_sha256(structure),
                        },
                        evidence,
                    )
                    dimensions["syntax"]["candidates"].append(candidate)
                    primary["syntax"] = True
                    if panel_role == "continuation":
                        dimensions["syntax"]["issues"].append(
                            make_issue(
                                official_row,
                                "syntax",
                                "syntax-panel-composition-unresolved",
                                [candidate],
                                [evidence],
                            )
                        )

                    for item in selected_items:
                        name = _syntax_option_name(item, documented_options)
                        if name == CONDITION_NAME_OPTION:
                            # The condition symbol selects a control-flow rule;
                            # it is not a data-area operand direction.
                            continue
                        markers = [
                            marker
                            for token in item.tokens
                            if token.kind == "variable"
                            for marker in (
                                []
                                if _non_operand_annotation(token.value)
                                else (_markers_in_text(token.value) or ["unknown"])
                            )
                        ]
                        if name is None or not markers:
                            continue
                        for marker in markers:
                            direction_evidence = make_evidence(
                                path,
                                metadata[path]["sha256"],
                                role,
                                syntax.section_id,
                                item.node,
                                bounds,
                            )
                            direction = make_candidate(
                                official_row,
                                "operand-directions",
                                "source-operand-direction",
                                name,
                                {
                                    "type": "operand-direction",
                                    "option": name,
                                    "marker": marker,
                                    "direction": _direction(marker),
                                },
                                direction_evidence,
                            )
                            dimensions["operand-directions"]["candidates"].append(direction)
                            primary["operand-directions"] = True

            options = option_section
            if options is not None:
                for group in option_groups:
                    name = _option_name(group.stack[0])
                    if name is None:
                        continue
                    arguments = _argument_markers(group.term)
                    stack = _symbolic_option_stack(group.stack)
                    legality_descriptions = _option_legality_descriptions(group)
                    if (
                        len(arguments) > 16
                        or len(group.descriptions) > 64
                        or group.depth > 64
                        or len(stack) > 64
                    ):
                        raise ProjectionError(f"option candidate bound exceeded: {path}:{name}")
                    evidence = make_evidence(
                        path,
                        metadata[path]["sha256"],
                        role,
                        options.section_id,
                        group.term,
                        bounds,
                    )
                    candidate_value = {
                        "type": "option",
                        "term": name,
                        "arguments": arguments,
                        "definition_count": len(group.descriptions),
                        "description_fragment_sha256s": [
                            "sha256:"
                            + fragment_sha256(normalized_fragment(description))
                            for description in group.descriptions
                        ],
                        "option_legality": (
                            "bounded-prose"
                            if legality_descriptions
                            else "structural"
                        ),
                        "depth": group.depth,
                        "stack": stack,
                        **_dynamic_condition_clause(group, roots[path], arguments),
                    }
                    candidate = make_candidate(
                        official_row,
                        "options",
                        "source-option",
                        name,
                        candidate_value,
                        evidence,
                    )
                    dimensions["options"]["candidates"].append(candidate)
                    primary["options"] = True
                    for description in legality_descriptions:
                        legality_evidence = make_evidence(
                            path,
                            metadata[path]["sha256"],
                            role,
                            options.section_id,
                            description,
                            bounds,
                        )
                        dimensions["options"]["issues"].append(
                            make_issue(
                                official_row,
                                "options",
                                "prose-option-legality-not-structured",
                                [candidate],
                                [legality_evidence],
                            )
                        )
                    for marker in (
                        [] if name == CONDITION_NAME_OPTION else arguments
                    ):
                        direction = make_candidate(
                            official_row,
                            "operand-directions",
                            "source-operand-direction",
                            name,
                            {
                                "type": "operand-direction",
                                "option": name,
                                "marker": marker,
                                "direction": _direction(marker, group.descriptions),
                            },
                            evidence,
                        )
                        dimensions["operand-directions"]["candidates"].append(direction)
                        primary["operand-directions"] = True

            conditions = sections.get("Conditions")
            if conditions is not None:
                for group in definition_groups(conditions):
                    stack = [
                        _symbolic_condition_term(value, depth, response_codes)
                        for depth, value in enumerate(group.stack)
                    ]
                    name = stack[-1]
                    if (
                        len(group.descriptions) > 64
                        or group.depth > 64
                        or len(stack) > 64
                    ):
                        raise ProjectionError(
                            f"condition candidate bound exceeded: {path}:{name}"
                        )
                    evidence = make_evidence(
                        path,
                        metadata[path]["sha256"],
                        role,
                        conditions.section_id,
                        group.term,
                        bounds,
                    )
                    candidate = make_candidate(
                        official_row,
                        "conditions",
                        "source-condition",
                        name,
                        {
                            "type": "condition",
                            "condition_stack": stack,
                            "definition_count": len(group.descriptions),
                            "trigger_fragment_sha256s": [
                                "sha256:"
                                + fragment_sha256(normalized_fragment(description))
                                for description in group.descriptions
                            ],
                            "depth": group.depth,
                        },
                        evidence,
                    )
                    dimensions["conditions"]["candidates"].append(candidate)
                    primary["conditions"] = True
                    if group.depth > 0 and condition_sibling_only:
                        matching_descriptions = [
                            description
                            for description in group.descriptions
                            if any(
                                re.search(
                                    rf"\b{re.escape(name)}\b",
                                    normalized_fragment(description).upper(),
                                )
                                for name in condition_sibling_only
                            )
                        ]
                        if matching_descriptions:
                            condition_evidence = make_evidence(
                                path,
                                metadata[path]["sha256"],
                                role,
                                conditions.section_id,
                                matching_descriptions[0],
                                bounds,
                            )
                            dimensions["conditions"]["issues"].append(
                                make_issue(
                                    official_row,
                                    "conditions",
                                    "shared-condition-applicability-unresolved",
                                    [candidate],
                                    [condition_evidence],
                                )
                            )

            if syntax is not None:
                for child in syntax.node.children:
                    if child.tag != "p":
                        continue
                    if not normalized_fragment(child):
                        continue
                    evidence = make_evidence(
                        path,
                        metadata[path]["sha256"],
                        role,
                        syntax.section_id,
                        child,
                        bounds,
                    )
                    candidate = make_candidate(
                        official_row,
                        "execution-context",
                        "source-context",
                        "mapped-inline-execution-context",
                        {
                            "type": "context",
                            "selector_id": "mapped-inline-execution-context",
                            "association": "mapping-topic-edge",
                        },
                        evidence,
                    )
                    dimensions["execution-context"]["candidates"].append(candidate)
                    primary["execution-context"] = True

            allowed_sections = {"Syntax", "Description", "Rules", "Options", "Conditions"}
            anchors: list[tuple[Node, str, str, str]] = []
            if path.split("/", 1)[0] == PRODUCT and not (
                resolution is not None
                and resolution.get("no_one_hop") is True
                and path == resolution.get("direct_supplement_topic")
            ):
                for title, section in sections.items():
                    if title not in allowed_sections:
                        continue
                    for anchor in walk(section.node):
                        if anchor.tag != "a":
                            continue
                        reference = linked_reference(anchor.attrs.get("href", ""), path)
                        if reference is not None and reference[0] in linked_paths:
                            anchors.append(
                                (anchor, reference[0], reference[1], section.section_id)
                            )
            seen_links: set[tuple[str, str, str]] = set()
            for anchor, target, fragment, section_id in anchors:
                anchor_path = structural_path(anchor)
                link_identity = (anchor_path, target, fragment)
                if link_identity in seen_links:
                    continue
                seen_links.add(link_identity)
                pair = hashlib.sha256(
                    (
                        path
                        + "\0"
                        + anchor_path
                        + "\0"
                        + target
                        + "\0"
                        + fragment
                    ).encode("utf-8")
                ).hexdigest()[:16]
                evidence = make_evidence(
                    path,
                    metadata[path]["sha256"],
                    role,
                    section_id,
                    anchor,
                    bounds,
                )
                candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    f"one-hop-source-{pair}",
                    {
                        "type": "context",
                        "selector_id": "mapped-one-hop-context",
                        "association": "mapping-edge-source-fragment-and-target-fragment",
                    },
                    evidence,
                )
                dimensions["execution-context"]["candidates"].append(candidate)
                target_node = exact_target_fragment(roots[target], fragment, target)
                target_evidence = make_evidence(
                    target,
                    metadata[target]["sha256"],
                    "one-hop-context",
                    _section_id_for(target_node),
                    target_node,
                    bounds,
                )
                target_candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    f"one-hop-target-{pair}",
                    {
                        "type": "context",
                        "selector_id": "mapped-one-hop-context",
                        "association": "mapping-edge-source-fragment-and-target-fragment",
                    },
                    target_evidence,
                )
                dimensions["execution-context"]["candidates"].append(target_candidate)
                topic_roles.setdefault(target, "one-hop-context")
                primary["execution-context"] = True

            if topic["role"] == "supplement":
                allowed_dimensions = set(metadata[path]["allowed_evidence_dimensions"])
                for dimension in DIMENSIONS:
                    if dimension in allowed_dimensions:
                        continue
                    dimensions[dimension]["candidates"] = [
                        candidate
                        for candidate in dimensions[dimension]["candidates"]
                        if candidate["evidence"]["topic_path"] != path
                    ]
                    primary[dimension] = any(
                        candidate["kind"] != "source-context"
                        for candidate in dimensions[dimension]["candidates"]
                    )
                forbidden_aliases = resolution.get("forbidden_alias_tokens", [])
                if forbidden_aliases:
                    _reject_supplement_alias(
                        official_row,
                        path,
                        dimensions["syntax"]["candidates"],
                        forbidden_aliases,
                    )

        expected_language_profile = LANGUAGE_PROFILE_BY_LABEL.get(str(row["label"]))
        if expected_language_profile is not None and language_candidate_count != 1:
            raise ProjectionError(
                f"language applicability evidence count differs: "
                f"{official_row}:{language_candidate_count}"
            )

        for selector in plan["manual_context_selectors"]:
            applies = selector["applies_to"]
            if applies["kind"] == "specific-rows" and official_row not in applies["official_rows"]:
                continue
            path = selector["topic_path"]
            role = MANUAL_SOURCE_ROLES[selector["id"]]
            for fragment, node in _manual_nodes(selector, roots[path]):
                evidence = make_evidence(
                    path,
                    metadata[path]["sha256"],
                    role,
                    _section_id_for(node),
                    node,
                    bounds,
                )
                for dimension in selector["dimensions"]:
                    if row["state"] == "mapped" and dimension != "execution-context" and not primary[dimension]:
                        continue
                    candidate = make_candidate(
                        official_row,
                        dimension,
                        "source-context",
                        selector["id"],
                        _manual_context_value(
                            selector["id"],
                            selector["association"],
                            row,
                            node,
                            resolution,
                            dimension,
                            fragment,
                        ),
                        evidence,
                    )
                    dimensions[dimension]["candidates"].append(candidate)
                topic_roles[path] = role

        if resolution is not None:
            for context in resolution.get("context_fragments", []):
                path = context["topic_path"]
                node = _source_resolution_context_node(
                    roots[path], context["selector"]
                )
                source_metadata = metadata[path]
                role = source_metadata.get(
                    "projected_source_role", "compatibility-context"
                )
                evidence = make_evidence(
                    path,
                    source_metadata["sha256"],
                    role,
                    _section_id_for(node),
                    node,
                    bounds,
                    source_product=source_metadata.get("source_product"),
                    target_authority_boundary=source_metadata.get(
                        "target_authority_boundary"
                    ),
                )
                candidate = make_candidate(
                    official_row,
                    "execution-context",
                    "source-context",
                    "source-resolution-context",
                    {
                        "type": "context",
                        "selector_id": "source-resolution-context",
                        "association": "exact-topic-root-no-link-expansion",
                    },
                    evidence,
                )
                dimensions["execution-context"]["candidates"].append(candidate)
                topic_roles[path] = role

        semantic_dimensions = (
            "syntax",
            "options",
            "operand-directions",
            "conditions",
        )
        if row["state"] == "mapped" and not any(
            primary[dimension] for dimension in semantic_dimensions
        ):
            successor_evidence = []
            for topic in direct_topics:
                path = topic["topic_path"]
                anchor = compatibility_successor_anchor(
                    roots[path], path, set(roots)
                )
                if anchor is None:
                    continue
                successor_evidence.append(
                    make_evidence(
                        path,
                        metadata[path]["sha256"],
                        DIRECT_SOURCE_ROLES[topic["role"]],
                        _section_id_for(anchor),
                        anchor,
                        bounds,
                    )
                )
            if successor_evidence:
                dimensions["syntax"]["issues"].append(
                    make_issue(
                        official_row,
                        "syntax",
                        "compatibility-successor-not-equivalent",
                        evidence=successor_evidence,
                        affected_dimensions=semantic_dimensions,
                    )
                )

        if row["state"] == "source-gap":
            if resolution is None:
                for dimension in DIMENSIONS:
                    issue = make_issue(official_row, dimension, "source-gap")
                    dimensions[dimension] = {
                        "name": dimension,
                        "state": "source-gap",
                        "candidates": [],
                        "issues": [issue],
                    }
            elif (resolution["label"], resolution["eibfn"]) != (
                row["label"],
                row["eibfn"],
            ):
                raise ProjectionError(f"source resolution differs: {official_row}")
            if resolution is not None:
                for path in resolution["target_context_topics"]:
                    topic_roles.setdefault(path, "gap-identity")
                for path in resolution.get("supplement_topics", []):
                    topic_roles.setdefault(path, metadata[path]["projected_source_role"])
            if resolution is None:
                pass
            elif resolution["resolution"] == "target-internal-only":
                for dimension in resolution["non_applicable_dimensions"]:
                    dimensions[dimension] = {
                        "name": dimension,
                        "state": "source-backed-not-applicable",
                        "candidates": [],
                        "issues": [],
                    }
                if not dimensions["execution-context"]["candidates"]:
                    raise ProjectionError(
                        f"internal-only row lacks target execution context: {official_row}"
                    )
                dimensions["execution-context"]["state"] = "projected"
            elif resolution["resolution"] in {
                "authority-bounded-projection",
                "target-product-projection",
            }:
                affected = resolution["authority_bounded_dimensions"]
                for dimension in DIMENSIONS:
                    dimensions[dimension]["state"] = (
                        "projected"
                        if resolution["resolution"] == "authority-bounded-projection"
                        or dimensions[dimension]["candidates"]
                        else "declared-absent"
                    )
                if resolution["resolution"] == "authority-bounded-projection":
                    issue_dimension = resolution["ambiguity_issue_dimension"]
                    evidence = [
                        candidate["evidence"]
                        for dimension in affected
                        for candidate in dimensions[dimension]["candidates"]
                    ]
                    issue = make_issue(
                        official_row,
                        issue_dimension,
                        "target-equivalence-ambiguity",
                        dimensions[issue_dimension]["candidates"],
                        evidence,
                        affected_dimensions=affected,
                    )
                    dimensions[issue_dimension]["issues"].append(issue)
            else:
                raise ProjectionError(
                    f"unsupported source resolution: {official_row}"
                )
        else:
            for dimension in DIMENSIONS:
                candidates = dimensions[dimension]["candidates"]
                direct_paths = [topic["topic_path"] for topic in direct_topics]
                declared = direct_paths and all(
                    (official_row, path, dimension) in exceptions
                    or (None, path, dimension) in exceptions
                    for path in direct_paths
                )
                if dimension == "operand-directions":
                    unknown_candidates = [
                        candidate
                        for candidate in candidates
                        if candidate["kind"] == "source-operand-direction"
                        and candidate["candidate_value"]["marker"] == "unknown"
                    ]
                    if unknown_candidates:
                        dimensions[dimension]["issues"].append(
                            make_issue(
                                official_row,
                                dimension,
                                "ambiguous-direction",
                                unknown_candidates,
                                [
                                    candidate["evidence"]
                                    for candidate in unknown_candidates
                                ],
                            )
                        )
                    expected_conflicts = [
                        conflict
                        for conflict in plan["conflict_expectations"]
                        if conflict["official_row"] == official_row
                        and conflict["dimension"] == dimension
                    ]
                    expected_by_signature = {
                        (
                            conflict["topic_path"],
                            conflict["key"],
                            next(
                                value["marker"]
                                for value in conflict["evidence_values"]
                                if value["section"] == "syntax"
                            ),
                            next(
                                value["marker"]
                                for value in conflict["evidence_values"]
                                if value["section"] == "options"
                            ),
                        ): conflict
                        for conflict in expected_conflicts
                    }
                    detected_conflicts = _detected_argument_conflicts(
                        candidates, roots
                    )
                    for signature in sorted(
                        set(expected_by_signature) - set(detected_conflicts)
                    ):
                        conflict_contract_drift.append(
                            (official_row, "missing-declared", signature)
                        )
                    if detected_conflicts:
                        for signature in sorted(detected_conflicts):
                            conflict = expected_by_signature.get(signature)
                            selected = detected_conflicts[signature]
                            if (
                                conflict is not None
                                and conflict.get("resolution")
                                != "options-definition-over-syntax-metavariable"
                            ):
                                raise ProjectionError(
                                    f"argument conflict lacks deterministic resolution: "
                                    f"{official_row}:{signature}"
                                )
                            option_section = direct_sections(roots[signature[0]]).get(
                                "Options"
                            )
                            if option_section is None:
                                raise ProjectionError(
                                    f"argument conflict options authority is missing: "
                                    f"{official_row}:{signature}"
                                )
                            winners = [
                                candidate
                                for candidate in selected
                                if candidate["evidence"]["section_id"]
                                == option_section.section_id
                            ]
                            losers = [
                                candidate
                                for candidate in selected
                                if candidate not in winners
                            ]
                            if not winners or not losers:
                                raise ProjectionError(
                                    f"argument conflict cannot select options authority: "
                                    f"{official_row}:{signature}"
                                )
                            loser_ids = {candidate["candidate_id"] for candidate in losers}
                            candidates[:] = [
                                candidate
                                for candidate in candidates
                                if candidate["candidate_id"] not in loser_ids
                            ]
                            if conflict is not None:
                                matched_conflicts.add(conflict["id"])
                    if unknown_candidates:
                        dimensions[dimension]["state"] = "unmatched"
                        continue
                automatically_absent = (
                    config.name != "a"
                    and dimension != "execution-context"
                    and not primary[dimension]
                )
                if primary[dimension]:
                    dimensions[dimension]["state"] = "projected"
                elif declared or automatically_absent:
                    dimensions[dimension]["state"] = "declared-absent"
                    dimensions[dimension]["candidates"] = []
                elif candidates and dimension == "execution-context":
                    dimensions[dimension]["state"] = "projected"
                else:
                    issue = make_issue(
                        official_row,
                        dimension,
                        "unmatched-row",
                        candidates,
                        [candidate["evidence"] for candidate in candidates],
                    )
                    dimensions[dimension]["state"] = "unmatched"
                    dimensions[dimension]["issues"].append(issue)

        ordered_dimensions = [dimensions[name] for name in DIMENSIONS]
        for dimension in ordered_dimensions:
            dimension["candidates"] = _coalesce_by_identity(
                dimension["candidates"], "candidate_id", "candidate"
            )
            values_by_key: dict[str, set[str]] = {}
            for candidate in dimension["candidates"]:
                value = json.dumps(
                    candidate["candidate_value"],
                    ensure_ascii=True,
                    sort_keys=True,
                    separators=(",", ":"),
                )
                values_by_key.setdefault(candidate["key"], set()).add(value)
            if any(
                len(values) > bounds["max_candidate_values_per_key"]
                for values in values_by_key.values()
            ):
                raise ProjectionError(
                    f"candidate values-per-key bound exceeded: "
                    f"{official_row}:{dimension['name']}"
                )
            dimension["issues"] = _coalesce_by_identity(
                dimension["issues"], "issue_id", "blocking issue"
            )
            if len(dimension["candidates"]) > 512:
                raise ProjectionError(
                    f"dimension candidate bound exceeded: "
                    f"{official_row}:{dimension['name']}"
                )
            if (
                dimension["state"] == "projected"
                and not dimension["candidates"]
                and not (
                    resolution is not None
                    and resolution["resolution"]
                    in {"authority-bounded-projection", "target-product-projection"}
                    and dimension["name"]
                    in resolution["authority_bounded_dimensions"]
                )
            ):
                raise ProjectionError(
                    f"projected dimension lacks candidates: "
                    f"{official_row}:{dimension['name']}"
                )
            all_candidates.extend(dimension["candidates"])
            all_issues.extend(dimension["issues"])
        if (
            sum(len(dimension["candidates"]) for dimension in ordered_dimensions)
            > bounds["max_fragments_per_row"]
        ):
            raise ProjectionError(f"row candidate bound exceeded: {official_row}")
        states = {dimension["state"] for dimension in ordered_dimensions}
        row_state = (
            "conflicting"
            if "conflicting" in states
            else "unmatched"
            if "unmatched" in states
            else "projected"
        )
        bindings = [
            {
                "topic_path": path,
                "topic_sha256": metadata[path]["sha256"],
                "source_role": role,
            }
            for path, role in sorted(topic_roles.items())
        ]
        if (not bindings and row["state"] != "source-gap") or len(bindings) > 128:
            raise ProjectionError(f"row topic binding count is invalid: {official_row}")
        projected_rows.append(
            {
                "official_row": official_row,
                "label": row["label"],
                "eibfn": row["eibfn"],
                "mapping_state": row["state"],
                "row_state": row_state,
                "topics": bindings,
                "dimensions": ordered_dimensions,
            }
        )

    expected_conflict_ids = {
        conflict["id"] for conflict in plan["conflict_expectations"]
    }
    if conflict_contract_drift:
        raise ProjectionError(
            f"declared source conflict set differs: {conflict_contract_drift}"
        )
    if matched_conflicts != expected_conflict_ids:
        raise ProjectionError("not every declared source conflict was projected")

    _coalesce_by_identity(all_candidates, "candidate_id", "global candidate")
    blocking_issues = _coalesce_by_identity(
        all_issues, "issue_id", "global blocking issue"
    )
    states = [dimension["state"] for row in projected_rows for dimension in row["dimensions"]]
    candidate_count = sum(
        len(dimension["candidates"])
        for row in projected_rows
        for dimension in row["dimensions"]
    )
    if candidate_count > bounds["max_fragments"]:
        raise ProjectionError("candidate projection exceeds its global fragment bound")
    counts = {
        "rows": len(projected_rows),
        "dimension_records": len(projected_rows) * len(DIMENSIONS),
        "candidates": candidate_count,
        "blocking_issues": len(blocking_issues),
        "projected_dimensions": states.count("projected"),
        "declared_absent_dimensions": states.count("declared-absent"),
        "source_gap_dimensions": states.count("source-gap"),
        "unmatched_dimensions": states.count("unmatched"),
        "conflicting_dimensions": states.count("conflicting"),
        "source_backed_not_applicable_dimensions": states.count(
            "source-backed-not-applicable"
        ),
        **shape_counts,
    }
    result: dict[str, Any] = {
        "schema_version": OUTPUT_SCHEMA,
        "target_version": TARGET_VERSION,
        "work_package": config.project_work_package,
        "status": "candidate",
        "review_status": "unreviewed",
        "semantic_authority": False,
        "automatic_registration": False,
        "coverage_credit": 0,
        "differential_credit": 0,
        "inputs": _inputs_for_projection(root, plan, config),
        "extractor": {
            "name": "cics-dita-source-projector",
            "version": "cics-dita-source-projector@1",
            "implementation_path": "conformance/0.9/tools/extract_cics_application_sources.py",
            "implementation_sha256": file_sha256(Path(__file__)),
            "fragment_digest_definition": FRAGMENT_DIGEST_DEFINITION,
        },
        "condition_name_authority": condition_name_authority,
        "counts": counts,
        "rows": projected_rows,
        "blocking_issues": blocking_issues,
        "projection_digest_definition": PROJECTION_DIGEST_DEFINITION,
        "projection_sha256": "",
    }
    result["projection_sha256"] = canonical_digest(
        result, OUTPUT_DOMAIN, "projection_sha256"
    )
    return result


def project_from_cache(
    root: Path,
    cache: Path,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    config = source_batch(batch)
    # Recompute the upstream map/corpus contracts before trusting their
    # embedded logical digests.  This remains offline: the corpus checker reads
    # only committed identities and the selected external cache scope.
    source_corpus.check(root, cache, batch=config.name)
    plan = read_json(root / config.plan_path)
    validate_plan(plan, config)
    mapping, corpus, manifest, _, bound_supplements = _input_files(
        root, plan, config
    )
    _, roots, metadata = load_cached_documents(cache, manifest, plan, config)
    if config.supplements_path is not None:
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        import cache_cics_application_source_supplements as supplement_sources

        supplements = supplement_sources.check(root, cache)
        if supplements != bound_supplements:
            raise ProjectionError("dedicated supplement check returned different receipt")
        _, supplement_roots, supplement_metadata = load_supplement_documents(
            cache, supplements, plan
        )
        if set(roots) & set(supplement_roots):
            raise ProjectionError("supplement topic aliases the target corpus")
        roots.update(supplement_roots)
        metadata.update(supplement_metadata)
    else:
        _, supplement_roots, supplement_metadata = load_corpus_supplement_documents(
            cache, corpus, plan
        )
        if set(roots) & set(supplement_roots):
            raise ProjectionError("corpus supplement topic aliases the target corpus")
        roots.update(supplement_roots)
        metadata.update(supplement_metadata)
    for resolution in plan["source_resolutions"]:
        path = resolution.get("direct_supplement_topic")
        if not path or path not in metadata or path.split("/", 1)[0] != PRODUCT:
            continue
        metadata[path].update(
            {
                "source_product": PRODUCT,
                "target_authority_boundary": TARGET_AUTHORITY,
                "projected_source_role": "primary-command",
                "allowed_evidence_dimensions": resolution[
                    "authority_bounded_dimensions"
                ],
            }
        )
    return build_projection(
        root, plan, mapping, corpus, manifest, roots, metadata, config
    )


def _reject_publication_fields(value: Any) -> None:
    if isinstance(value, dict):
        forbidden = FORBIDDEN_OUTPUT_KEYS & value.keys()
        if forbidden:
            raise ProjectionError(
                f"candidate projection contains publication fields: {sorted(forbidden)}"
            )
        for child in value.values():
            _reject_publication_fields(child)
    elif isinstance(value, list):
        for child in value:
            _reject_publication_fields(child)


def check_committed(
    root: Path = ROOT,
    batch: str | SourceBatch = DEFAULT_BATCH,
) -> dict[str, Any]:
    """Validate committed identities and structure without requiring source bytes."""

    config = source_batch(batch)
    source_corpus.check(root, batch=config.name)
    plan = read_json(root / config.plan_path)
    validate_plan(plan, config)
    mapping, corpus, manifest, _, supplements = _input_files(root, plan, config)
    output_path = root / config.projection_path
    output = read_json(output_path)
    if tuple(output) != OUTPUT_KEYS:
        raise ProjectionError("candidate projection fields or ordering differ")
    if (
        output.get("schema_version") != OUTPUT_SCHEMA
        or output.get("target_version") != TARGET_VERSION
        or output.get("work_package") != config.project_work_package
        or output.get("status") != "candidate"
        or output.get("review_status") != "unreviewed"
        or output.get("semantic_authority") is not False
        or output.get("automatic_registration") is not False
        or output.get("coverage_credit") != 0
        or output.get("differential_credit") != 0
        or output.get("inputs") != _inputs_for_projection(root, plan, config)
        or output.get("projection_digest_definition")
        != PROJECTION_DIGEST_DEFINITION
        or output.get("projection_sha256")
        != canonical_digest(output, OUTPUT_DOMAIN, "projection_sha256")
        or output_path.read_text(encoding="utf-8") != pretty(output)
    ):
        raise ProjectionError("candidate projection identity, digest, or encoding differs")
    expected_extractor = {
        "name": "cics-dita-source-projector",
        "version": "cics-dita-source-projector@1",
        "implementation_path": "conformance/0.9/tools/extract_cics_application_sources.py",
        "implementation_sha256": file_sha256(Path(__file__)),
        "fragment_digest_definition": FRAGMENT_DIGEST_DEFINITION,
    }
    if output.get("extractor") != expected_extractor:
        raise ProjectionError("candidate projector implementation identity differs")
    _reject_publication_fields(output)

    rows = output.get("rows")
    if not isinstance(rows, list) or len(rows) != len(mapping["rows"]):
        raise ProjectionError("candidate row count differs from the source map")
    pins = {item["topic_path"]: item["sha256"] for item in manifest["topics"]}
    supplement_entries: dict[str, dict[str, Any]] = {}
    for item in [
        *supplements["topics"],
        *corpus.get("supplemental_topics", []),
    ]:
        path = item["topic_path"]
        if path in pins:
            raise ProjectionError(f"supplement aliases a target corpus topic: {path}")
        pins[path] = item["sha256"]
        supplement_entries[path] = item
    candidate_identities: dict[str, dict[str, Any]] = {}
    local_issues: dict[str, dict[str, Any]] = {}
    states: list[str] = []
    candidate_count = 0
    for projected, mapped in zip(rows, mapping["rows"]):
        if (
            (projected.get("official_row"), projected.get("label"), projected.get("eibfn"))
            != (mapped["official_row"], mapped["label"], mapped["eibfn"])
            or projected.get("mapping_state") != mapped["state"]
        ):
            raise ProjectionError("candidate row identity or order differs from the source map")
        topics = projected.get("topics")
        if not isinstance(topics, list) or topics != sorted(
            topics, key=lambda item: item["topic_path"]
        ):
            raise ProjectionError(f"candidate row topics are not ordered: {mapped['official_row']}")
        row_topics: dict[str, str] = {}
        for topic in topics:
            path = topic.get("topic_path")
            sha256 = topic.get("topic_sha256")
            if path in row_topics or pins.get(path) != sha256:
                raise ProjectionError(f"candidate topic binding differs: {mapped['official_row']}")
            row_topics[path] = sha256
        dimensions = projected.get("dimensions")
        if (
            not isinstance(dimensions, list)
            or [dimension.get("name") for dimension in dimensions] != list(DIMENSIONS)
        ):
            raise ProjectionError(f"candidate dimensions differ: {mapped['official_row']}")
        row_states: set[str] = set()
        for dimension in dimensions:
            state = dimension.get("state")
            states.append(state)
            row_states.add(state)
            candidates = dimension.get("candidates")
            issues = dimension.get("issues")
            if (
                not isinstance(candidates, list)
                or not isinstance(issues, list)
                or candidates != sorted(candidates, key=lambda item: item["candidate_id"])
                or issues != sorted(issues, key=lambda item: item["issue_id"])
            ):
                raise ProjectionError(
                    f"candidate dimension ordering differs: {mapped['official_row']}:"
                    f"{dimension.get('name')}"
                )
            local_candidate_ids: set[str] = set()
            for candidate in candidates:
                candidate_count += 1
                identity = candidate.get("candidate_id")
                prior = candidate_identities.get(identity)
                if prior is not None and prior != candidate:
                    raise ProjectionError(f"candidate identity collision: {identity}")
                if identity in local_candidate_ids:
                    raise ProjectionError(f"duplicate candidate identity: {identity}")
                candidate_identities[identity] = candidate
                local_candidate_ids.add(identity)
                evidence = candidate.get("evidence", {})
                path = evidence.get("topic_path")
                if (
                    path not in row_topics
                    or evidence.get("topic_sha256") != "sha256:" + pins[path]
                ):
                    raise ProjectionError(f"candidate evidence is not pinned: {identity}")
                source_product = path.split("/", 1)[0]
                supplement = supplement_entries.get(path)
                authority = authority_boundary(
                    source_product,
                    supplement.get("target_authority_boundary")
                    if supplement is not None
                    else None,
                )
                if (
                    evidence.get("source_product") != source_product
                    or evidence.get("target_authority_boundary") != authority
                ):
                    raise ProjectionError(
                        f"candidate evidence authority boundary differs: {identity}"
                    )
                if supplement is not None and (
                    mapped["official_row"] not in supplement["applies_to_rows"]
                    or candidate.get("key", "").startswith("one-hop-")
                ):
                    raise ProjectionError(
                        f"supplement evidence escaped its row or link boundary: {identity}"
                    )
            for issue in issues:
                identity = issue.get("issue_id")
                prior = local_issues.get(identity)
                if prior is not None and prior != issue:
                    raise ProjectionError(f"blocking issue identity collision: {identity}")
                local_issues[identity] = issue
                if not set(issue.get("candidate_ids", [])) <= local_candidate_ids:
                    raise ProjectionError(f"blocking issue cites a foreign candidate: {identity}")
        expected_row_state = (
            "conflicting"
            if "conflicting" in row_states
            else "unmatched"
            if "unmatched" in row_states
            else "projected"
        )
        if projected.get("row_state") != expected_row_state:
            raise ProjectionError(f"candidate row state differs: {mapped['official_row']}")

    blocking = output.get("blocking_issues")
    if (
        not isinstance(blocking, list)
        or blocking != sorted(blocking, key=lambda item: item["issue_id"])
        or {item["issue_id"]: item for item in blocking} != local_issues
    ):
        raise ProjectionError("top-level blocking issues differ from row dimensions")
    counts = output.get("counts", {})
    expected_counts = {
        "rows": len(rows),
        "dimension_records": len(states),
        "candidates": candidate_count,
        "blocking_issues": len(blocking),
        "projected_dimensions": states.count("projected"),
        "declared_absent_dimensions": states.count("declared-absent"),
        "source_gap_dimensions": states.count("source-gap"),
        "unmatched_dimensions": states.count("unmatched"),
        "conflicting_dimensions": states.count("conflicting"),
        "source_backed_not_applicable_dimensions": states.count(
            "source-backed-not-applicable"
        ),
    }
    if any(counts.get(key) != value for key, value in expected_counts.items()):
        raise ProjectionError("candidate projection counts differ from row content")
    return output


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch", choices=("a", "b", "c"), default="a")
    parser.add_argument("--cache", type=docs_api.retrieval_path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(list(argv) if argv is not None else None)
    try:
        if args.cache is None:
            if not args.check:
                parser.error("--cache is required when generating the projection")
            result = check_committed(ROOT, args.batch)
            print(
                f"cics-sources-{args.batch}-project: committed pass "
                f"rows={result['counts']['rows']} "
                f"candidates={result['counts']['candidates']}"
            )
            return 0
        config = source_batch(args.batch)
        result = project_from_cache(ROOT, args.cache, config)
        rendered = pretty(result)
        output = ROOT / config.projection_path
        if args.check:
            if not output.is_file() or output.read_text(encoding="utf-8") != rendered:
                raise ProjectionError(
                    f"stale CICS sources-{args.batch} candidate projection: {output}"
                )
            print(
                f"cics-sources-{args.batch}-project: pass rows={result['counts']['rows']} "
                f"candidates={result['counts']['candidates']} "
                f"blocking={result['counts']['blocking_issues']}"
            )
        else:
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(rendered, encoding="utf-8")
            print(
                f"cics-sources-{args.batch}-project: generated "
                f"{config.projection_path} "
                f"candidates={result['counts']['candidates']}"
            )
        return 0
    except (OSError, ProjectionError, ValueError) as error:
        parser.exit(1, f"cics-sources-{args.batch}-project: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
