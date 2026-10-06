#!/usr/bin/env python3
"""Re-verify every pinned IBM publication against the baseline that cites it.

`conformance/subsystems/coverage/catalogs/index.json` names, for each baseline, the table of
contents it was read from and the digest of the topic manifest extracted from
it. This tool walks that list, re-retrieves the table of contents and every
topic the manifest records, and reports whether the book still hashes to its
pin. `supporting_sources` are verified too — the RACROUTE router-interface topic
is a pinned source like any other, and until now no tool in this repository ever
re-read it.

A pin is a whole book, not one file: the recorded digest is
`topic_manifest_digest`, taken over every topic's own sha256. So a mismatch is
localised rather than total, and the report says which topics moved.

A mismatch is a REVIEW DECISION, not a failure to repair here, and the first
question about one is whether the origin even served us the book it publishes. A
fourth cause turned out to dominate the other three:

  the origin served an older build   the served `Last Updated` date moves BACK
  IBM edited the topic               the date moves FORWARD
  IBM changed the endpoint           the date holds and the bytes move
  we changed what we ask for         the URL template differs from the pinned one

Only the first is cheap and common, and it is the one a digest alone cannot tell
from the others. Db2 is the case: four full re-reads of its 832 topics called 1,
7, 1 and 6 topics changed and never named the same topic twice, every changed
body was smaller than its pin, and every one carried an EARLIER date than the
pin — 2026-01-07 or 2026-05-12 where 2026-09-03 is pinned. Publication moves that
date forward, so an older stamp arriving where a newer one is pinned is an older
build being served.

So a mismatch is read twice before it is recorded. A re-read that reproduces the
pin settles it outright; otherwise the dates decide, and `stale-read` is a
verdict of its own that does NOT fail the run. `republished` does, because it is
the case a reviewer must look at, and so does an equal date over different bytes,
which is the one nothing here explains. A book that reports a red nobody can act
on trains its readers to skip it, and this is the one baseline of the nine with
832 chances per run to do that.

Nothing may re-derive a pin: CI can only re-verify a recorded one, because a
tool that repins on mismatch cannot tell drift from republication. `stale-read`
is emphatically not permission to repin — it says the origin is unreliable, not
that the pin is.

An unreachable endpoint is reported as `skipped`, never as a mismatch.

Topic bodies land in a cache outside the repository; IBM publication bytes are
never written into the tree.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))

import docs_api

REPOSITORY = Path(__file__).resolve().parents[2]
INDEX = Path("conformance/subsystems/coverage/catalogs/index.json")
DEFAULT_CACHE = docs_api.default_cache()

STALE_READ = "stale-read"
REPUBLISHED = "republished"
SAME_DATE = "same-date-different-bytes"
UNDATED = "undated-difference"

#: The resolutions that a reviewer has to act on, and so the ones that fail a
#: run. Everything not named here is reported and does not set the exit status.
UNEXPLAINED = (REPUBLISHED, SAME_DATE, UNDATED)


def outside_repository(path: Path) -> Path:
    """Refuse to cache or report inside the tree.

    The one rule that outranks everything else here is that IBM publication
    bytes never enter the repository, and the 0.2 gate now fails on any `.pdf`,
    `.html` or `.htm` found anywhere beneath `conformance/subsystems/coverage`. Enforcing it at
    the tool as well means the gate is a backstop rather than the only guard.
    """
    resolved = path.resolve()
    if resolved == REPOSITORY or REPOSITORY in resolved.parents:
        raise ValueError(f"path is inside the repository: {resolved}")
    return resolved


def baselines(index: Path, wanted: set[str]) -> list[dict[str, Any]]:
    document = json.loads(index.read_text(encoding="utf-8"))
    known = {row["subsystem"] for row in document["baselines"]}
    unknown = wanted - known
    if unknown:
        raise ValueError(f"unknown subsystem: {', '.join(sorted(unknown))}")
    return [row for row in document["baselines"] if not wanted or row["subsystem"] in wanted]


def self_consistency(manifest: dict[str, Any]) -> dict[str, Any]:
    """Check the manifest against itself before asking IBM anything.

    Offline and free, and it catches the failure live retrieval cannot: a
    manifest whose recorded digest no longer follows from its own topic list.
    """
    recomputed = docs_api.manifest_digest(manifest["topics"])
    return {
        "recomputed_digest": recomputed,
        "agrees": (
            recomputed == manifest["topic_manifest_digest"]
            and len(manifest["topics"]) == manifest["topic_count"]
            and sum(t["bytes"] for t in manifest["topics"]) == manifest["total_bytes"]
        ),
    }


def resolve(
    record: dict[str, Any], again: bytes | Exception | None
) -> dict[str, Any]:
    """Decide what a per-topic mismatch means, given a second read of it.

    Reading twice is not belt and braces: it is the only evidence that separates
    a body the origin served once from a body the publication now carries. When
    the second read reproduces the pin, the first read was the anomaly and there
    is nothing about the pin to review.

    When it does not, the dates decide, and they are read from the SECOND read
    where there is one, because that is the more recent measurement. Both are
    recorded either way, so a reviewer can see whether the two reads agreed.
    """
    served = record["last_modified"]
    if isinstance(again, (bytes, bytearray)):
        text = bytes(again).decode("utf-8", "replace")
        record["reread"] = {
            "sha256": docs_api.digest(again),
            "bytes": len(again),
            "last_modified": docs_api.last_modified_of(text),
        }
        if (
            record["reread"]["sha256"] == record["pinned_sha256"]
            and record["reread"]["bytes"] == record["pinned_bytes"]
        ):
            record["resolution"] = STALE_READ
            record["resolved_by"] = "the re-read reproduces the pin"
            return record
        served = record["reread"]["last_modified"]
    elif again is not None:
        record["reread"] = {"reason": getattr(again, "reason", "http-404")}

    stands = docs_api.compare_dates(record["pinned_last_modified"], served)
    record["resolution"], record["resolved_by"] = {
        "older": (STALE_READ, "the served build is older than the pinned one"),
        "newer": (REPUBLISHED, "the topic has been republished since it was pinned"),
        "same": (SAME_DATE, "the same date over different bytes"),
        "undated": (UNDATED, "no pair of dates to compare"),
    }[stands]
    return record


def reread(path: str, template: str) -> bytes | Exception:
    """Read one topic again, past any cache, to test a mismatch before recording it.

    Deliberately uncached and deliberately uncapped: the cache is what the first
    read may have come from, and a run with many mismatches is exactly the run
    whose mismatches most need a second opinion. It costs one extra request per
    changed topic and none at all for a book that matches.
    """
    try:
        return docs_api.topic(path, template, None)
    except (docs_api.NotFound, docs_api.Unreachable) as error:
        return error


def reread_url(url: str) -> bytes | Exception:
    """The same second opinion for a supporting source, which is cited by URL."""
    try:
        return docs_api.fetch(url)
    except (docs_api.NotFound, docs_api.Unreachable) as error:
        return error


def verify_topics(
    manifest: dict[str, Any], cache: Path | None, sample: int | None, workers: int
) -> dict[str, Any]:
    """Re-retrieve the manifest's topics and compare each against its pin."""
    template = manifest["content_url_template"]
    entries = manifest["topics"]
    selected = entries if sample is None else entries[:sample]
    live: list[dict[str, Any]] = []
    changed: list[dict[str, Any]] = []
    unreachable: list[dict[str, Any]] = []

    retrieved = dict(
        docs_api.topics([e["topic_path"] for e in selected], template, cache, workers)
    )
    for entry in selected:
        path = entry["topic_path"]
        data = retrieved[path]
        if isinstance(data, docs_api.NotFound):
            unreachable.append({"topic_path": path, "reason": "http-404"})
            continue
        if isinstance(data, Exception):
            unreachable.append({"topic_path": path, "reason": getattr(data, "reason", "error")})
            continue
        served = docs_api.digest(data)
        # The first read is what goes into the digest, even when a re-read
        # reproduces the pin. `matches_pin` then says what this run actually
        # retrieved, and the resolution beside it says what that meant.
        live.append({"topic_path": path, "sha256": served})
        if served != entry["sha256"] or len(data) != entry["bytes"]:
            record = {
                "topic_path": path,
                "pinned_sha256": entry["sha256"],
                "sha256": served,
                "pinned_bytes": entry["bytes"],
                "bytes": len(data),
                "pinned_last_modified": entry.get("last_modified"),
                "last_modified": docs_api.last_modified_of(
                    data.decode("utf-8", "replace")
                ),
            }
            changed.append(resolve(record, reread(path, template)))

    resolutions: dict[str, int] = {}
    for record in changed:
        resolutions[record["resolution"]] = resolutions.get(record["resolution"], 0) + 1
    unexplained = sum(resolutions.get(name, 0) for name in UNEXPLAINED)

    state: dict[str, Any] = {
        "topics_pinned": len(entries),
        "topics_checked": len(live),
        "topics_changed": len(changed),
        "topics_stale_read": resolutions.get(STALE_READ, 0),
        "topics_unexplained": unexplained,
        "topics_unreachable": len(unreachable),
        "resolutions": resolutions,
        "changed": changed,
        "unreachable": unreachable,
    }
    if sample is None and not unreachable:
        state["manifest_digest"] = docs_api.manifest_digest(live)
        state["matches_pin"] = state["manifest_digest"] == manifest["topic_manifest_digest"]
        if state["matches_pin"]:
            state["status"] = "match"
        elif unexplained or not changed:
            # A digest that moved with no topic reporting a mismatch is the
            # manifest disagreeing with itself, not the origin being slow.
            state["status"] = "differs"
        else:
            state["status"] = STALE_READ
    elif unreachable and not changed:
        state["status"] = "skipped"
    elif changed:
        state["status"] = "differs" if unexplained else STALE_READ
    else:
        state["status"] = "sampled"
    return state


def verify_supporting(source: dict[str, Any], cache: Path | None) -> dict[str, Any]:
    """Re-read one pinned supporting topic.

    Verified here because nothing else ever has: the RACROUTE router-interface
    pin has been carried in the index since the baseline was written and no tool
    in this repository's history has re-read it.
    """
    entry: dict[str, Any] = {
        "url": source["url"],
        "topic_path": source.get("topic_path"),
        "pinned_sha256": source["sha256"],
        "pinned_bytes": source["bytes"],
    }
    try:
        data = docs_api.fetch_cached(source["url"], cache)
    except (docs_api.NotFound, docs_api.Unreachable) as error:
        reason = "http-404" if isinstance(error, docs_api.NotFound) else error.reason
        return {**entry, "status": "skipped", "reason": reason}
    entry["sha256"] = "sha256:" + docs_api.digest(data)
    entry["bytes"] = len(data)
    entry["last_modified"] = docs_api.last_modified_of(data.decode("utf-8", "replace"))
    if entry["sha256"] == source["sha256"] and entry["bytes"] == source["bytes"]:
        entry["status"] = "match"
        return entry
    # Read twice and classify exactly as a manifest topic is. A supporting
    # source is served by the same origin and can go stale the same way, and one
    # topic reading `differs` fails its whole baseline. The index records no
    # `last_modified` for this pin, only the date it was captured, so the date
    # arm cannot fire and anything a second read does not settle still reads
    # `undated-difference` and still fails — which is where it was before.
    record = resolve(
        {
            "pinned_sha256": source["sha256"].removeprefix("sha256:"),
            "sha256": entry["sha256"].removeprefix("sha256:"),
            "pinned_bytes": source["bytes"],
            "bytes": entry["bytes"],
            "pinned_last_modified": source.get("last_modified"),
            "last_modified": entry["last_modified"],
        },
        reread_url(source["url"]),
    )
    entry["resolution"] = record["resolution"]
    entry["resolved_by"] = record["resolved_by"]
    entry["reread"] = record.get("reread")
    entry["status"] = "differs" if record["resolution"] in UNEXPLAINED else STALE_READ
    return entry


def verify_baseline(
    row: dict[str, Any],
    root: Path,
    cache: Path | None,
    sample: int | None,
    workers: int,
) -> dict[str, Any]:
    source = row["source"]
    manifest = json.loads((root / source["manifest"]).read_text(encoding="utf-8"))
    entry: dict[str, Any] = {
        "subsystem": row["subsystem"],
        "baseline_id": row["id"],
        "publication_identity": row["publication_identity"],
        "manifest": source["manifest"],
        "toc_url": source["url"],
        "pinned_manifest_digest": source["sha256"],
        "self_consistency": self_consistency(manifest),
    }
    entry["index_agrees_with_manifest"] = (
        source["sha256"] == "sha256:" + manifest["topic_manifest_digest"]
        and source["bytes"] == manifest["total_bytes"]
        and source["topic_count"] == manifest["topic_count"]
        and source["toc_sha256"] == "sha256:" + manifest["toc_sha256"]
    )

    try:
        raw = docs_api.toc_bytes(source["url"], cache)
        entry["toc_sha256"] = "sha256:" + docs_api.digest(raw)
        entry["toc_matches_pin"] = entry["toc_sha256"] == source["toc_sha256"]
    except (docs_api.NotFound, docs_api.Unreachable) as error:
        entry["toc_matches_pin"] = None
        entry["toc_reason"] = getattr(error, "reason", "http-404")

    entry["topics"] = verify_topics(manifest, cache, sample, workers)
    supporting = [verify_supporting(s, cache) for s in row.get("supporting_sources", [])]
    if supporting:
        entry["supporting_sources"] = supporting

    disagrees = (
        not entry["self_consistency"]["agrees"]
        or not entry["index_agrees_with_manifest"]
        or any(s["status"] == "differs" for s in supporting)
    )
    if disagrees:
        entry["status"] = "differs"
    elif entry["topics"]["status"] == "match" and any(
        s["status"] == STALE_READ for s in supporting
    ):
        entry["status"] = STALE_READ
    else:
        entry["status"] = entry["topics"]["status"]
    return entry


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--index", type=Path, default=INDEX)
    parser.add_argument(
        "--subsystem", action="append", default=[],
        help="restrict to these baselines; repeatable",
    )
    parser.add_argument(
        "--cache", type=Path, default=DEFAULT_CACHE,
        help="reuse retrieved topic bodies from this directory, outside the tree",
    )
    parser.add_argument("--no-cache", action="store_true")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument(
        "--sample", type=int,
        help="re-read only the first N topics of each book; reports `sampled`, "
             "never `match`, because a partial read cannot reproduce the digest",
    )
    parser.add_argument("--report", type=Path)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    rows = baselines(args.index, set(args.subsystem))
    cache = None if args.no_cache else outside_repository(args.cache)
    report_path = outside_repository(
        args.report or DEFAULT_CACHE.parent / "pinned-source-retrieval.json"
    )

    results: list[dict[str, Any]] = []
    for row in rows:
        entry = verify_baseline(row, REPOSITORY, cache, args.sample, args.workers)
        results.append(entry)
        topics = entry["topics"]
        # Printed as each book finishes rather than at the end: re-reading all
        # 4,488 topics takes the better part of an hour, and a run that says
        # nothing until it is over cannot be watched.
        print(
            f"{entry['subsystem']:18} {entry['status']:10} "
            f"topics={topics['topics_checked']}/{topics['topics_pinned']:>5} "
            f"changed={topics['topics_changed']:>4} "
            f"stale={topics['topics_stale_read']:>4} "
            f"unexplained={topics['topics_unexplained']:>4} "
            f"unreachable={topics['topics_unreachable']:>4} "
            f"toc={entry.get('toc_matches_pin')}",
            flush=True,
        )

    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(
            {
                "schema_version": "mainframe-env.pinned-source-retrieval@3",
                "coverage_credit": 0,
                "retained_in_repository": False,
                "sources": results,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    counts = {
        status: 0 for status in ("match", "differs", STALE_READ, "skipped", "sampled")
    }
    for entry in results:
        counts[entry["status"]] += 1
    print(
        f"pins={len(results)} matched={counts['match']} differs={counts['differs']} "
        f"stale-read={counts[STALE_READ]} skipped={counts['skipped']} "
        f"sampled={counts['sampled']} report={report_path}"
    )
    # Only `differs` fails. A stale read is a fact about the origin, and a run
    # that exits non-zero on it is a run whose red says nothing.
    return 1 if counts["differs"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
