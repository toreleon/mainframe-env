#!/usr/bin/env python3
"""Verify reviewed MQ status facts; optional bounded offline source reproduction."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import sys

import mq_status_catalog as authority


def project_pairs(lines: list[str], first: int, last: int) -> list[dict]:
    """Only Reason's explicit CompCode groups, never page-wide MQRC matches."""
    if lines[first-1] != "Reason" or not lines[last].startswith(("For detailed information", "For more information about these codes")):
        raise ValueError("MQ source return-table boundary differs")
    result = []
    seen = {}
    group = None
    group_line = None
    for index in range(first, last):
        text = lines[index]
        heading = re.fullmatch(r"(?:If )?CompCode is (MQCC_OK|MQCC_WARNING|MQCC_FAILED)(?::|, the reason code is (?:as follows|one of the following):)", text)
        if heading:
            group, group_line = heading[1], index+1
        if not text.startswith("MQRC_"):
            continue
        if group is None or index+1 >= last:
            raise ValueError("MQ reason outside explicit completion group")
        numeric = re.match(r"\(?\s*(\d+),?\s*\(?\s*X'([0-9A-Fa-f]+)", lines[index+1])
        if numeric:
            decimal, hexa = int(numeric[1]), numeric[2].upper()
            review = "admitted" if decimal == int(hexa, 16) else "pending-numeric-conflict"
        elif lines[index+1].startswith("(nnnn, X'xxx')"):
            decimal = hexa = None
            review = "pending-number"
        else:
            raise ValueError(f"unreviewed MQ numeric form at line {index+2}")
        if not re.fullmatch(r"MQRC_[A-Z0-9_]+", text):
            if review != "admitted" or not re.fullmatch(r"MQRC_[A-Z0-9_ ]+", text):
                raise ValueError("unreviewed malformed source symbol")
            review = "pending-symbol-spelling"
        location = {"completion_line": group_line, "reason_line": index+1, "number_line": index+2}
        key = (group, text)
        if key in seen:
            existing = seen[key]
            if (existing["declared_decimal"], existing["declared_hex"]) != (decimal, hexa):
                raise ValueError("duplicate source declaration conflicts")
            existing["source_locations"].append(location)
        else:
            item = {"completion_symbol": group, "reason_symbol": text,
                "declared_decimal": decimal, "declared_hex": hexa, "review": review,
                "source_locations": [location]}
            seen[key] = item
            result.append(item)
    if not result:
        raise ValueError("MQ return section empty")
    return result


def apply_symbol_conflicts(calls: list[dict]) -> None:
    symbols = {}
    for call in calls:
        for pair in call["pairs"]:
            if pair["review"] == "admitted":
                symbols.setdefault(pair["reason_symbol"], set()).add(pair["declared_decimal"])
    conflicts = {symbol for symbol, numbers in symbols.items() if len(numbers) > 1}
    for call in calls:
        for pair in call["pairs"]:
            if pair["reason_symbol"] in conflicts and pair["review"] == "admitted":
                pair["review"] = "pending-symbol-conflict"


def reproduce(root: Path, cache: Path, catalog: dict) -> None:
    sys.path.insert(0, str(root / "conformance/tools"))
    import ibm_docs
    pins, tocs = ibm_docs.select(*ibm_docs.load_pins(), None, "mq")
    by_topic = {pin.topic: pin for pin in pins}
    for toc in tocs:
        data = ibm_docs.read_bounded(cache / toc.key)
        if hashlib.sha256(data).hexdigest() != toc.sha256:
            raise ValueError("MQ source TOC hash mismatch")
    reproduced = []
    for call in catalog["calls"]:
        pin = by_topic[call["topic_path"]]
        data = ibm_docs.read_bounded(cache / pin.key)
        if hashlib.sha256(data).hexdigest() != call["topic_sha256"] or len(data) != pin.size:
            raise ValueError(f"MQ source hash/size mismatch: {call['label']}")
        lines = ibm_docs.plain_text(data)
        for note in call["context_notes"]:
            fragment = "\n".join(lines[note["first_line"]-1:note["last_line"]])
            if hashlib.sha256(fragment.encode()).hexdigest() != note["fragment_sha256"]:
                raise ValueError("MQ context fragment mismatch")
        section = call["reason_section"]
        if section is None:
            # Signature is callback input/context only. No CompCode/Reason
            # outputs; receiving a reason inside MQCBC is a different role.
            if "Reason" in lines or "CompCode" in lines or "No entry point called MQCB_FUNCTION" not in lines[3]:
                raise ValueError("MQ callback no-return source differs")
            pairs = []
        else:
            pairs = project_pairs(lines, section["first_line"], section["last_line"])
        reproduced.append({"pairs": pairs})
    apply_symbol_conflicts(reproduced)
    for call, source in zip(catalog["calls"], reproduced):
        if call["pairs"] != source["pairs"]:
            raise ValueError(f"MQ source-reviewed return membership differs: {call['label']}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source-cache", type=Path)
    args = parser.parse_args()
    catalog = authority.load()
    if args.source_cache:
        reproduce(authority.ROOT, args.source_cache, catalog)
    print(f"MQ status: calls=26 positions=27 admitted={catalog['admitted_pair_count']} pending={catalog['pending_pair_count']} callback-not-applicable=1 credit=0")


if __name__ == "__main__":
    main()
