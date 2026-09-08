#!/usr/bin/env python3
"""Reuse the bounded semantic-fragment workflow for the COBOL MOVE proof."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import extract_cics_pilot_rules as fragments  # noqa: E402


def compile_corpus(manifest: dict, config: dict, topics: Path) -> dict:
    entries = manifest["topics"]
    configured = {source["topic_path"] for source in config["sources"]}
    pinned = {entry["topic_path"] for entry in entries}
    if configured != pinned or not 0 < len(entries) <= fragments.MAX_TOPICS:
        raise ValueError("COBOL MOVE source set differs from its bounded manifest")
    inventory = []
    for entry in entries:
        body = (topics / Path(entry["topic_path"]).name).read_bytes()
        if len(body) != entry["bytes"] or fragments.digest(body) != entry["sha256"]:
            raise ValueError(f"topic digest mismatch: {entry['topic_path']}")
        inventory.extend(fragments.compile_topic(entry, body, config))
    candidates = [item for item in inventory if item["classification"] == "candidate"]
    return {
        "schema_version": "mainframe-env.cobol-move-semantic-candidates@1",
        "extractor_version": config["extractor_version"],
        "baseline_id": manifest["baseline_id"],
        "product": manifest["product"],
        "release": config["release"],
        "topic_manifest_digest": manifest["topic_manifest_digest"],
        "compile_rules_sha256": fragments.digest(
            json.dumps(config, sort_keys=True, separators=(",", ":"))
        ),
        "coverage_credit": 0,
        "retained_publication_bytes": False,
        "totals": {
            "topics": len(entries),
            "normative_fragments": len(inventory),
            "candidates": len(candidates),
            "unsupported": sum(item["classification"] == "unsupported" for item in inventory),
            "conflicting": sum(item["classification"] == "conflicting" for item in inventory),
            "outside_scope": sum(item["classification"] == "outside-scope" for item in inventory),
            "informative": sum(item["classification"] == "informative" for item in inventory),
        },
        "inventory": inventory,
        "candidates": candidates,
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--topics", type=fragments.docs_api.retrieval_path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--compile-rules", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(list(argv) if argv is not None else None)
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    config = json.loads(args.compile_rules.read_text(encoding="utf-8"))
    rendered = fragments.pretty(compile_corpus(manifest, config, args.topics))
    if args.check:
        if not args.output.is_file() or args.output.read_bytes() != rendered:
            print(f"stale COBOL MOVE candidate projection: {args.output}", file=sys.stderr)
            return 1
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(rendered)
    totals = json.loads(rendered)["totals"]
    print(" ".join(f"{key}={value}" for key, value in totals.items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
