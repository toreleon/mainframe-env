#!/usr/bin/env python3
"""Three-way merge a generated documentation manifest by document identity."""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
from typing import Any


class MergeConflict(ValueError):
    """The two candidates changed the same semantic value differently."""


MISSING = object()


def conflict(path: tuple[str, ...]) -> MergeConflict:
    location = ".".join(path) if path else "manifest"
    return MergeConflict(f"conflicting generated documentation value at {location}")


def merge_value(base: Any, ours: Any, theirs: Any, path: tuple[str, ...] = ()) -> Any:
    if ours == theirs:
        return ours
    if ours == base:
        return theirs
    if theirs == base:
        return ours
    if path == ("counts",):
        return dict(ours)
    if path == ("documents",):
        return merge_keyed(base, ours, theirs, "path", path)
    if path == ("package_topology", "packages"):
        return merge_keyed(base, ours, theirs, "name", path)
    if all(isinstance(value, dict) for value in (base, ours, theirs)):
        output: dict[str, Any] = {}
        for key in sorted(set(base) | set(ours) | set(theirs)):
            value = merge_optional(
                base.get(key, MISSING),
                ours.get(key, MISSING),
                theirs.get(key, MISSING),
                (*path, key),
            )
            if value is not MISSING:
                output[key] = value
        return output
    raise conflict(path)


def merge_optional(base: Any, ours: Any, theirs: Any, path: tuple[str, ...]) -> Any:
    if ours is MISSING and theirs is MISSING:
        return MISSING
    if base is MISSING:
        if ours is MISSING:
            return theirs
        if theirs is MISSING or ours == theirs:
            return ours
        raise conflict(path)
    if ours is MISSING:
        if theirs == base:
            return MISSING
        raise conflict(path)
    if theirs is MISSING:
        if ours == base:
            return MISSING
        raise conflict(path)
    return merge_value(base, ours, theirs, path)


def indexed(values: Any, key: str, path: tuple[str, ...]) -> dict[str, dict[str, Any]]:
    if not isinstance(values, list):
        raise conflict(path)
    output: dict[str, dict[str, Any]] = {}
    for value in values:
        if not isinstance(value, dict) or not isinstance(value.get(key), str):
            raise conflict(path)
        identity = value[key]
        if identity in output:
            raise conflict((*path, identity))
        output[identity] = value
    return output


def merge_keyed(
    base: Any,
    ours: Any,
    theirs: Any,
    key: str,
    path: tuple[str, ...],
) -> list[dict[str, Any]]:
    base_by_key = indexed(base, key, path)
    ours_by_key = indexed(ours, key, path)
    theirs_by_key = indexed(theirs, key, path)
    output = []
    for identity in sorted(set(base_by_key) | set(ours_by_key) | set(theirs_by_key)):
        value = merge_optional(
            base_by_key.get(identity, MISSING),
            ours_by_key.get(identity, MISSING),
            theirs_by_key.get(identity, MISSING),
            (*path, identity),
        )
        if value is not MISSING:
            output.append(value)
    return output


def merge_manifest(base: dict[str, Any], ours: dict[str, Any], theirs: dict[str, Any]) -> dict[str, Any]:
    expected = "mainframe-env.documentation-manifest@1"
    if any(value.get("schema_version") != expected for value in (base, ours, theirs)):
        raise MergeConflict("unsupported documentation manifest schema")
    merged = merge_value(base, ours, theirs)
    documents = merged.get("documents")
    navigation = merged.get("navigation")
    if not isinstance(documents, list) or not isinstance(navigation, list):
        raise MergeConflict("merged documentation manifest omits documents or navigation")
    merged["counts"] = {
        "markdown_documents": len(documents),
        "navigation_groups": len(navigation),
        "normative_documents": sum(row.get("normative") is True for row in documents),
        "xtask_commands": sum(row.get("xtask_commands", 0) for row in documents),
    }
    return merged


def load(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise MergeConflict(f"{path} is not a JSON object")
    return value


def write_atomic(path: Path, value: dict[str, Any]) -> None:
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            json.dump(value, output, indent=2, sort_keys=True, ensure_ascii=False)
            output.write("\n")
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        raise MergeConflict("usage: merge_documentation_manifest.py BASE OURS THEIRS")
    base_path, ours_path, theirs_path = map(Path, argv[1:])
    merged = merge_manifest(load(base_path), load(ours_path), load(theirs_path))
    write_atomic(ours_path, merged)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv))
    except (MergeConflict, OSError, json.JSONDecodeError) as error:
        print(f"documentation manifest merge: {error}", file=sys.stderr)
        raise SystemExit(1)
