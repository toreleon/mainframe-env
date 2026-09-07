#!/usr/bin/env python3
"""Read IBM Documentation through its topic API.

Two endpoints carry everything the probes need. The table of contents states a
book's topic tree — every node's `href`, `topicId` and `label` — and the content
endpoint returns one topic's body:

    https://www.ibm.com/docs/api/v1/toc/<product>?lang=en
    https://www.ibm.com/docs/api/v1/content/<topic_path>?parsebody=true&lang=en

`parsebody=true` is load-bearing: without it the same MQ topic comes back 8,264
bytes instead of 10,245, so the parameter belongs to the pin and is recorded in
every manifest as `content_url_template`.

These bytes are stable, which is why they can be pinned at all. The rendered
page is not: it carries a fresh `lit$<random>$` nonce and an Adobe `eto_<hex>`
nonce on every load, so no rendered-DOM capture can ever be re-verified.

The edge rejects clients by User-Agent, and it is a browser string that fails —
`Mozilla/5.0` returns 403 while a plain `curl` request returns 200. Python's own
`Python-urllib/3.13` is refused too, so the User-Agent is sent explicitly. It is
declared here once rather than being rediscovered by each tool.

Nothing here writes into the repository. Callers that cache pass a directory
outside the tree; IBM publication bytes are never retained.
"""

from __future__ import annotations

import hashlib
import html as html_module
import json
import re
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Callable, Iterable

USER_AGENT = "curl/8.7.1"
TOC_URL = "https://www.ibm.com/docs/api/v1/toc/{product}?lang=en"
CONTENT_URL = "https://www.ibm.com/docs/api/v1/content/{topic_path}?parsebody=true&lang=en"

#: The manifest digest, stated identically in
#: `conformance/0.2/schemas/topic-manifest.schema.json` as a `const`.
DIGEST_DEFINITION = (
    'sha256 over the concatenation, sorted by topic_path, of one '
    '"<topic_path> <sha256>\\n" line per topic, each <sha256> written as bare '
    'lowercase hex'
)

HEADING = re.compile(
    r"<h1[^>]*\bclass=\"[^\"]*\btopictitle1\b[^\"]*\"[^>]*>(.*?)</h1>", re.S
)
LAST_MODIFIED = re.compile(
    r"<div[^>]*\bid=\"lastModifiedDate\"[^>]*>(.*?)</div>", re.S
)
TAG = re.compile(r"<[^>]+>")
CELL = re.compile(r"<t[dh]\b[^>]*>(.*?)</t[dh]>", re.S)
ROW = re.compile(r"<tr\b[^>]*>(.*?)</tr>", re.S)
DATA_CELL = re.compile(r"<td\b", re.I)
CHAPTER = re.compile(r"^Chapter [0-9]+\. ")
ISO_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class Unreachable(Exception):
    """The endpoint could not be reached, as distinct from answering 404.

    The distinction is the whole point: a row whose topic 404s has drifted off
    its publication, while a row we could not ask about has not been checked.
    Reporting the second as the first invents findings.
    """

    def __init__(self, url: str, reason: str) -> None:
        super().__init__(f"{url}: {reason}")
        self.url = url
        self.reason = reason


class NotFound(Exception):
    """The endpoint answered, and the topic is not there."""

    def __init__(self, url: str) -> None:
        super().__init__(f"{url}: 404")
        self.url = url


def fetch(url: str, timeout: float = 60.0) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        if error.code == 404:
            raise NotFound(url) from error
        raise Unreachable(url, f"http-{error.code}") from error
    except Exception as error:  # noqa: BLE001 - any transport failure is unreachable
        raise Unreachable(url, type(error).__name__) from error


def content_url(topic_path: str, template: str = CONTENT_URL) -> str:
    """The content URL for a topic path, percent-encoded the way IBM expects.

    The whole path is one encoded path segment — the slashes are escaped — and
    any `?pos=` navigation suffix is dropped first. `?pos=2` disambiguates a
    repeated node in the navigation tree rather than naming a second document,
    and the content endpoint 302-redirects the suffixed form while losing
    `parsebody=true` on the way.
    """
    clean = topic_path.split("?", 1)[0]
    return template.replace("{topic_path}", urllib.parse.quote(clean, safe=""))


def _key(value: str) -> str:
    """A cache file name for a URL or a topic path."""
    return re.sub(r"[^A-Za-z0-9._-]", "_", value)


def topic(topic_path: str, template: str = CONTENT_URL, cache: Path | None = None) -> bytes:
    url = content_url(topic_path, template)
    return _cached(cache, _key(topic_path.split("?", 1)[0]), lambda: fetch(url))


def toc_bytes(url: str, cache: Path | None = None) -> bytes:
    """The table of contents exactly as served, so its digest can be compared."""
    return _cached(cache, "toc-" + _key(url) + ".json", lambda: fetch(url))


def fetch_cached(url: str, cache: Path | None = None) -> bytes:
    """Retrieve any documentation URL, reusing a cached body when there is one."""
    return _cached(cache, _key(url), lambda: fetch(url))


def _cached(cache: Path | None, key: str, produce: Callable[[], bytes]) -> bytes:
    if cache is None:
        return produce()
    cache.mkdir(parents=True, exist_ok=True)
    target = cache / key
    if target.exists():
        return target.read_bytes()
    data = produce()
    target.write_bytes(data)
    return data


def topics(
    paths: Iterable[str],
    template: str = CONTENT_URL,
    cache: Path | None = None,
    workers: int = 8,
) -> list[tuple[str, bytes | Exception]]:
    """Retrieve many topics, in the order given, a few at a time.

    A book is thousands of topics and each request costs about a second, so a
    sequential re-verification of all nine baselines runs for over an hour. The
    pool is deliberately small: this is an audit reading a public documentation
    site, not a crawl.

    Failures are returned rather than raised, so one unreachable topic degrades
    that one row instead of abandoning the run.
    """
    ordered = list(paths)

    def retrieve(path: str) -> bytes | Exception:
        try:
            return topic(path, template, cache)
        except (NotFound, Unreachable) as error:
            return error

    with ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
        return list(zip(ordered, pool.map(retrieve, ordered)))


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def manifest_digest(topics: Iterable[dict[str, Any]]) -> str:
    """Recompute a manifest's `topic_manifest_digest` from its own topic list."""
    lines = sorted(f"{entry['topic_path']} {entry['sha256']}\n" for entry in topics)
    return hashlib.sha256("".join(lines).encode("utf-8")).hexdigest()


def normalize(value: str) -> str:
    """Collapse whitespace so a hard-wrapped heading compares equal.

    RACF sets several command headings across a line break inside the `h1`, so
    the served title carries a newline the catalog label does not.
    """
    return " ".join(html_module.unescape(value).replace("\xa0", " ").split())


def strip_markup(value: str) -> str:
    return normalize(TAG.sub(" ", value))


def without_chapter_number(value: str) -> str:
    """A heading with a book chapter number removed.

    113 reviewed labels read `Chapter 4. ALLOCATE` where the topic tree reads
    `ALLOCATE`: the number belongs to the printed book's sequence, not to the
    command. The labels are frozen row identity and cannot be rewritten, so the
    comparison drops the prefix instead — and every tool that does so records it
    separately, because it is a relaxation and must be visible as one.

    Deliberately narrow. `Chapter N. ` is the only such prefix any reviewed
    label carries (195 of them across the catalogs and the COBOL language file),
    so `Appendix`, `Part` and a bare `Chapter heading` are left alone rather
    than admitted on the chance that some future label might need them.
    """
    return CHAPTER.sub("", normalize(value))


def heading_of(body: str) -> str | None:
    """The `topictitle1` heading of a topic body."""
    match = HEADING.search(body)
    return strip_markup(match.group(1)) if match else None


def last_modified_of(body: str) -> str | None:
    """The topic's own `Last Updated` date, if it publishes one.

    A manifest mismatch is a review decision, and this is what separates its
    causes: IBM editing a paragraph moves this date, our selector changing does
    not.
    """
    match = LAST_MODIFIED.search(body)
    if not match:
        return None
    text = strip_markup(match.group(1))
    date = re.search(r"\d{4}-\d{2}-\d{2}", text)
    return date.group(0) if date else text or None


def _table(body: str, table_id: str) -> str | None:
    """The markup of the named table, or None when the topic has no such table."""
    opening = re.search(rf"<table\b[^>]*\bid=\"{re.escape(table_id)}\"", body)
    if not opening:
        return None
    end = body.find("</table>", opening.start())
    return None if end < 0 else body[opening.start() : end]


def table_cells(body: str, table_id: str) -> list[str] | None:
    """Every cell of the named table, or None when the table is absent."""
    segment = _table(body, table_id)
    return None if segment is None else [strip_markup(cell) for cell in CELL.findall(segment)]


def table_rows(body: str, table_id: str) -> list[list[str]] | None:
    """The named table's body rows, in order, each as its own list of cells.

    Header rows are dropped, so row 1 is the first row of data — the ordinal a
    locator's `row:` component counts in. A header row is recognised by carrying
    no `td` at all rather than by sitting inside a `thead`, because the same
    tables are also served with the header written as a plain `tr` of `th`.

    None, not an empty list, when the table is absent: a table that is gone is
    drift, and a table that is there and empty is a different finding.
    """
    segment = _table(body, table_id)
    if segment is None:
        return None
    return [
        [strip_markup(cell) for cell in CELL.findall(markup)]
        for markup in ROW.findall(segment)
        if DATA_CELL.search(markup)
    ]


def heading_in_cells(heading: str, cells: Iterable[str]) -> bool:
    """Whether any of these cells names this heading.

    A cell may carry the statement as the book prints it — `// SCHEDULE` for
    SCHEDULE — so a whitespace-separated token counts as well as the whole cell.
    """
    wanted = normalize(heading).casefold()
    return any(
        cell.casefold() == wanted or wanted in cell.casefold().split() for cell in cells
    )


def compare_dates(pinned: str | None, served: str | None) -> str:
    """How a served `Last Updated` date stands to the pinned one.

    `older`, `newer`, `same` or `undated`. The direction is the load-bearing
    part: republication moves this date forward, so an older stamp arriving
    where a newer one is pinned is an older build being served rather than a
    new one being published.
    """
    if not (pinned and served and ISO_DATE.match(pinned) and ISO_DATE.match(served)):
        return "undated"
    if served < pinned:
        return "older"
    return "newer" if served > pinned else "same"


def toc_index(document: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    """Every table-of-contents node keyed by topic path.

    Keyed by href and never by label: 72 of the reviewed headings repeat inside
    their own book, so a label-keyed map hands back an arbitrary one of them.

    A path maps to a *list* because the tree files one topic under several
    branches. Db2 lists `SET CURRENT ACCELERATOR` under both the statements and
    the accelerators branch, and the two nodes publish different `topicId`
    slugs; keeping only the first seen would report four correct rows as having
    the wrong identifier.
    """
    found: dict[str, list[dict[str, Any]]] = {}

    def walk(node: dict[str, Any]) -> None:
        href = node.get("href")
        if href:
            found.setdefault(href.split("?", 1)[0], []).append(node)
        for child in node.get("topics") or []:
            walk(child)

    walk(document["toc"])
    return found


def without_product(topic_path: str) -> str:
    """A topic path with its leading product key removed.

    `SSLTBW_3.2.0/com.ibm.zos.v3r2.icha400/addgroup.htm` becomes
    `com.ibm.zos.v3r2.icha400/addgroup.htm`. A z/OS 3.3 baseline republishes the
    same book under a new product key, and comparing the tail lets that read as
    573 rows that moved rather than 573 rows that vanished.
    """
    _, _, tail = topic_path.split("?", 1)[0].partition("/")
    return tail or topic_path
