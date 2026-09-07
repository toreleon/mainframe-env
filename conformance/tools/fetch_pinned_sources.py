#!/usr/bin/env python3
"""Retrieve every pinned IBM publication and verify it against its baseline.

`conformance/0.2/catalogs/index.json` already names, for each baseline, the
publication URL and the sha256 the catalog was extracted from. This tool walks
that list, pulls each source through a real browser, and reports whether the
bytes still hash to the pinned digest.

A match means the probes can run on exactly the edition the catalog cites
instead of whatever older edition happens to be reachable. A mismatch means IBM
has republished the file at the same URL, and the baseline needs a review
decision rather than a silent re-pin.

Sources land in a destination directory outside the repository; IBM publication
bytes are never written into the tree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))

from browser_fetch import establish, fetch_binary, fetch_dom, open_tab

INDEX = Path("conformance/0.2/catalogs/index.json")


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def suffix(media_type: str) -> str:
    return ".pdf" if media_type == "application/pdf" else ".html"


def baselines(index: Path, wanted: set[str]) -> list[dict[str, Any]]:
    document = json.loads(index.read_text(encoding="utf-8"))
    rows = [
        row
        for row in document["baselines"]
        if not wanted or row["subsystem"] in wanted
    ]
    unknown = wanted - {row["subsystem"] for row in document["baselines"]}
    if unknown:
        raise ValueError(f"unknown subsystem: {', '.join(sorted(unknown))}")
    return rows


def retrieve(tab: Any, source: dict[str, Any], target: Path) -> tuple[str, bytes | None]:
    """Return the retrieval status and the bytes, reusing a cached download."""
    if target.exists():
        return "cached", target.read_bytes()
    if source["media_type"] == "application/pdf":
        status, data = fetch_binary(tab, source["url"])
        if data is None:
            return f"http-{status or 'error'}", None
        return "fetched", data
    return "fetched", fetch_dom(tab, source["url"]).encode("utf-8")


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=9222)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--index", type=Path, default=INDEX)
    parser.add_argument(
        "--subsystem",
        action="append",
        default=[],
        help="restrict to these baselines; repeatable",
    )
    parser.add_argument("--report", type=Path)
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    rows = baselines(args.index, set(args.subsystem))
    args.destination.mkdir(parents=True, exist_ok=True)

    tab = open_tab(args.port)
    results: list[dict[str, Any]] = []
    try:
        establish(tab)
        for row in rows:
            source = row["source"]
            target = args.destination / (row["subsystem"] + suffix(source["media_type"]))
            status, data = retrieve(tab, source, target)
            entry: dict[str, Any] = {
                "subsystem": row["subsystem"],
                "baseline_id": row["id"],
                "publication_identity": row["publication_identity"],
                "url": source["url"],
                "status": status,
                "pinned_sha256": source["sha256"],
            }
            if data is None:
                entry["matches_pin"] = False
            else:
                if status == "fetched":
                    target.write_bytes(data)
                entry["path"] = str(target)
                entry["bytes"] = len(data)
                entry["sha256"] = digest(data)
                entry["matches_pin"] = entry["sha256"] == source["sha256"]
            results.append(entry)
            mark = "match" if entry["matches_pin"] else "DIFFERS"
            print(
                f"{row['subsystem']:18} {status:9} "
                f"{entry.get('bytes', 0):>9} bytes  {mark}"
            )
    finally:
        tab.close()

    if args.report:
        args.report.write_text(
            json.dumps(
                {
                    "schema_version": "mainframe-env.pinned-source-retrieval@1",
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

    matched = sum(1 for entry in results if entry["matches_pin"])
    print(f"pins={len(results)} matched={matched}")
    return 0 if matched == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
