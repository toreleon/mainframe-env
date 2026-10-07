#!/usr/bin/env python3
"""Provision digest-pinned official-docs caches offline; no Git/LFS or network fetch.

The locator index has schema_version mainframe-env.ibm-docs-cache-snapshots@1,
repository (an HTTPS GitHub repository locator), revision (40 lowercase hex),
and snapshots (rows printed by pack). The default index is manager-owned.

pins_sha256 is SHA-256 of UTF-8 JSON without a trailing newline: a list sorted
by canonical CacheTarget.key, each object containing key, sha256 and size,
encoded with sort_keys=True, separators=(",", ":"), ensure_ascii=True.
Topic size is the shared declared integer; TOC size is null (not declared).
This transport fingerprint does not replace shared source-manifest authority.

Archives contain sorted canonical flat keys in deterministic GNU tar framing
(long-name records are necessary for content-addressed keys), regular files
only, mode 0444, uid/gid/mtime 0, empty uname/gname, wrapped in gzip with mtime 0
and no original filename. Import accepts this canonical framing, verifies the
entire artifact and archive before shared publication, and never extractall.
Malformed archives and pre-existing conflicts publish no new cache entries.
Concurrent filesystem conflicts/I/O failures during shared import can leave
some verified entries, but never authorize overwriting a conflicting file.
Cache presence has semantic_authority=false and coverage_credit=0.
"""

from __future__ import annotations

import argparse
from contextlib import ExitStack
from dataclasses import replace
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import tarfile
import tempfile
import time
from urllib.parse import urlsplit

import ibm_docs

SCHEMA = "mainframe-env.ibm-docs-cache-snapshots@1"
DEFAULT_INDEX = ibm_docs.docs_api.REPOSITORY / "conformance/ibm-docs-cache-snapshots.json"
MAX_INDEX = 4 * 1024 * 1024
MAX_TAR = ibm_docs.MAX_IMPORT + ibm_docs.MAX_ARCHIVE_ENTRIES * 2048 + 10240
MAX_ARCHIVE = MAX_TAR + 1024 * 1024
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,79}\Z")
ROW_FIELDS = {"id", "archive", "sha256", "bytes", "scopes", "pins_sha256",
              "topics", "tocs", "semantic_authority", "coverage_credit"}


def repository_roots() -> list[Path]:
    """Read local Git's linked-worktree inventory, bounded and fail closed."""
    data = bytearray()
    deadline = time.monotonic() + 10
    with subprocess.Popen(
        ["git", "-C", str(ibm_docs.docs_api.REPOSITORY), "worktree", "list",
         "--porcelain", "-z"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    ) as process:
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not ready.select(remaining):
                        raise ValueError("local worktree inventory timed out")
                    block = os.read(process.stdout.fileno(), min(65536, 1024 * 1024 - len(data) + 1))
                    if not block:
                        break
                    data.extend(block)
                    if len(data) > 1024 * 1024:
                        raise ValueError("local worktree inventory exceeds bounded output")
            if process.wait(timeout=max(0.001, deadline - time.monotonic())):
                raise ValueError("cannot read local repository worktree inventory")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=1)
    roots = [Path(os.fsdecode(row[9:])).resolve() for row in bytes(data).split(b"\0")
             if row.startswith(b"worktree ")]
    if not roots:
        raise ValueError("local repository worktree inventory is empty")
    return roots


def external_path(value: Path, roots: list[Path]) -> Path:
    """Reject aliases and every linked checkout; a separate private clone is allowed."""
    if not value.is_absolute():
        raise ValueError("cache/archive paths must be absolute and external")
    for part in [value, *value.parents]:
        if part.is_symlink():
            raise ValueError("symlink in cache/archive path refused")
    path = ibm_docs.docs_api.outside_repository(value)
    if path in {Path(path.anchor), Path.home().resolve()}:
        raise ValueError("root/home cache or output path refused")
    if any(path == root or root in path.parents for root in roots):
        raise ValueError("cache/archive path is inside a mainframe repository worktree")
    return path


def selected(scopes: list[str]) -> tuple[list, list, list]:
    if not scopes or len(scopes) > ibm_docs.MAX_ARCHIVE_ENTRIES:
        raise ValueError("snapshot must select bounded explicit source scopes")
    pins, tocs = ibm_docs.load_pins()
    topics, tables = {}, {}
    for scope in sorted(set(scopes)):
        if not isinstance(scope, str) or not 0 < len(scope) <= 200:
            raise ValueError("invalid source scope")
        chosen, chosen_tocs = ibm_docs.select(pins, tocs, scope, None)
        for pin in chosen:
            topics[pin.key] = pin
        for toc in chosen_tocs:
            tables[toc.key] = toc
    targets = [*(ibm_docs.pin_target(pin) for pin in topics.values()),
               *(ibm_docs.toc_target(toc) for toc in tables.values())]
    targets.sort(key=lambda target: target.key)
    if (len({target.key for target in targets}) != len(targets)
            or len(targets) > ibm_docs.MAX_ARCHIVE_ENTRIES
            or sum(target.size or 0 for target in targets) > ibm_docs.MAX_IMPORT):
        raise ValueError("selected targets conflict or exceed shared import bounds")
    return list(topics.values()), list(tables.values()), targets


def fingerprint(targets: list) -> str:
    rows = [{"key": target.key, "sha256": target.sha256, "size": target.size}
            for target in sorted(targets, key=lambda target: target.key)]
    return hashlib.sha256(json.dumps(rows, sort_keys=True, separators=(",", ":"),
                                    ensure_ascii=True).encode("utf-8")).hexdigest()


def identifier(value: object) -> bool:
    return isinstance(value, str) and ID.fullmatch(value) is not None


def integer(value: object, maximum: int, minimum: int = 1) -> bool:
    return type(value) is int and minimum <= value <= maximum


def validate_row(row: object) -> dict:
    if not isinstance(row, dict) or set(row) != ROW_FIELDS:
        raise ValueError("invalid snapshot row fields")
    scopes = row["scopes"]
    if (not identifier(row["id"])
            or row["archive"] != f"snapshots/{row['id']}.tar.gz"
            or any(not isinstance(row[k], str) or ibm_docs.HEX.fullmatch(row[k]) is None
                   for k in ["sha256", "pins_sha256"])
            or not integer(row["bytes"], MAX_ARCHIVE)
            or not integer(row["topics"], ibm_docs.MAX_ARCHIVE_ENTRIES)
            or not integer(row["tocs"], ibm_docs.MAX_ARCHIVE_ENTRIES)
            or row["topics"] + row["tocs"] > ibm_docs.MAX_ARCHIVE_ENTRIES
            or not isinstance(scopes, list) or not scopes
            or len(scopes) > ibm_docs.MAX_ARCHIVE_ENTRIES
            or any(not isinstance(s, str) or not 0 < len(s) <= 200 for s in scopes)
            or scopes != sorted(set(scopes))
            or row["semantic_authority"] is not False
            or type(row["coverage_credit"]) is not int or row["coverage_credit"] != 0):
        raise ValueError("invalid snapshot identity, bounds or zero-credit fields")
    return row


def snapshot_row(index: Path, snapshot: str) -> dict:
    if not identifier(snapshot):
        raise ValueError("invalid snapshot ID")
    if any(part.is_symlink() for part in [index, *index.parents]):
        raise ValueError("snapshot index symlink refused")
    with index.open("rb") as stream:
        data = stream.read(MAX_INDEX + 1)
    if len(data) > MAX_INDEX:
        raise ValueError("snapshot index exceeds bounded size")
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate snapshot index JSON field")
            result[key] = value
        return result
    document = json.loads(data, object_pairs_hook=unique_pairs)
    if (not isinstance(document, dict)
            or set(document) != {"schema_version", "repository", "revision", "snapshots"}
            or document["schema_version"] != SCHEMA):
        raise ValueError("invalid snapshot locator schema")
    locator = document["repository"]
    if not isinstance(locator, str):
        raise ValueError("invalid HTTPS GitHub repository locator")
    url = urlsplit(locator)
    if (url.scheme != "https" or url.netloc != "github.com" or url.query or url.fragment
            or re.fullmatch(r"/[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+", url.path) is None
            or url.path.split("/")[-1] in {".", ".."}):
        raise ValueError("invalid HTTPS GitHub repository locator (no credentials)")
    if (not isinstance(document["revision"], str)
            or re.fullmatch(r"[0-9a-f]{40}", document["revision"]) is None):
        raise ValueError("snapshot repository revision must be immutable 40hex")
    rows = document["snapshots"]
    if not isinstance(rows, list) or not rows or len(rows) > ibm_docs.MAX_ARCHIVE_ENTRIES:
        raise ValueError("invalid snapshot list")
    identities = {}
    for raw in rows:
        row = validate_row(raw)
        if row["id"] in identities:
            raise ValueError("duplicate snapshot ID")
        identities[row["id"]] = row
    if snapshot not in identities:
        raise ValueError("snapshot ID not registered")
    return identities[snapshot]


def tar_info(key: str, size: int) -> tarfile.TarInfo:
    info = tarfile.TarInfo(key)
    info.size = size
    info.mode = 0o444
    info.uid = info.gid = info.mtime = 0
    info.uname = info.gname = ""
    return info


def file_digest(stream) -> tuple[str, int]:
    digest, total = hashlib.sha256(), 0
    while block := stream.read(1024 * 1024):
        total += len(block)
        if total > MAX_ARCHIVE:
            raise ValueError("compressed artifact exceeds bounded size")
        digest.update(block)
    return digest.hexdigest(), total


def pack(snapshot: str, scopes: list[str], cache: Path, archive: Path) -> dict:
    if not identifier(snapshot):
        raise ValueError("invalid snapshot ID")
    roots = repository_roots()
    cache, archive = external_path(cache, roots), external_path(archive, roots)
    external_path(archive.parent, roots)
    if cache.exists() and not cache.is_dir():
        raise ValueError("cache must be an external directory")
    if not str(archive).endswith(".tar.gz") or not archive.parent.is_dir():
        raise ValueError("archive requires .tar.gz and an existing external parent directory")
    if archive.exists():
        raise ValueError("archive already exists; no overwrite")
    pins, tocs, targets = selected(scopes)
    total = 0
    with tempfile.NamedTemporaryFile(prefix=".snapshot-", dir=archive.parent) as staged:
        with gzip.GzipFile(fileobj=staged, mode="wb", filename="", mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w|", format=tarfile.GNU_FORMAT) as tar:
                for target in targets:
                    # Shared verifier accepts addressed or verified legacy bytes.
                    for key in {target.key, target.legacy_key}:
                        if (cache / key).is_symlink():
                            raise ValueError("selected cache symlink refused")
                    body = ibm_docs.cached_bytes(cache, target)
                    total += len(body)
                    if total > ibm_docs.MAX_IMPORT:
                        raise ValueError("selected bytes exceed shared import bound")
                    tar.addfile(tar_info(target.key, len(body)), io.BytesIO(body))
        staged.flush()
        os.fsync(staged.fileno())
        staged.seek(0)
        digest, size = file_digest(staged)
        row = validate_row(dict(id=snapshot, archive=f"snapshots/{snapshot}.tar.gz",
                                sha256=digest, bytes=size, scopes=sorted(set(scopes)),
                                pins_sha256=fingerprint(targets), topics=len(pins),
                                tocs=len(tocs), semantic_authority=False, coverage_credit=0))
        # Atomic exclusive publication; an existing file or link wins unchanged.
        external_path(archive, repository_roots())
        os.link(staged.name, archive, follow_symlinks=False)
    return row


def preflight(tar, targets: list) -> None:
    """Validate canonical GNU framing directly, including bounded long-name records."""
    expected = {target.key: target for target in targets}
    seen, total, entries = set(), 0, 0
    while True:
        block = tar.read(512)
        if len(block) != 512:
            raise ValueError("truncated tar header")
        if block == bytes(512):
            # Require two terminator blocks and only bounded zero padding after them.
            if tar.read(512) != bytes(512):
                raise ValueError("invalid tar terminator")
            while tail := tar.read(1024 * 1024):
                if any(tail):
                    raise ValueError("unregistered trailing archive data")
            break
        info = tarfile.TarInfo.frombuf(block, encoding="utf-8", errors="strict")
        prefix = b""
        key = info.name
        if info.type == tarfile.GNUTYPE_LONGNAME:
            if not 1 < info.size <= 256:
                raise ValueError("oversized tar long-name record")
            payload = tar.read((info.size + 511) // 512 * 512)
            if len(payload) != 512 or payload[info.size - 1] != 0:
                raise ValueError("truncated tar long-name record")
            key = payload[:info.size - 1].decode("utf-8")
            prefix = block + payload
            block = tar.read(512)
            if len(block) != 512:
                raise ValueError("truncated regular tar header")
            info = tarfile.TarInfo.frombuf(block, encoding="utf-8", errors="strict")
        entries += 1
        total += info.size
        target = expected.get(key)
        if (entries > ibm_docs.MAX_ARCHIVE_ENTRIES or info.size < 0
                or info.size > ibm_docs.MAX_FILE or total > ibm_docs.MAX_IMPORT):
            raise ValueError("archive file/count/byte limit exceeded")
        if (target is None or key in seen or "/" in key or "\\" in key
                or info.type != tarfile.REGTYPE
                or (target.size is not None and info.size != target.size)):
            raise ValueError("unsafe, duplicate, unregistered or nonregular archive member")
        if prefix + block != tar_info(key, info.size).tobuf(format=tarfile.GNU_FORMAT):
            raise ValueError("noncanonical tar framing or metadata")
        seen.add(key)
        body = tar.read(info.size)
        padding = tar.read((-info.size) % 512)
        if (len(body) != info.size or len(padding) != (-info.size) % 512
                or any(padding) or hashlib.sha256(body).hexdigest() != target.sha256):
            raise ValueError("truncated or mismatched pinned archive body")
    if seen != set(expected):
        raise ValueError("incomplete selected snapshot archive")


def import_snapshot(snapshot: str, index: Path, archive: Path, cache: Path) -> dict:
    row = snapshot_row(index, snapshot)
    pins, tocs, targets = selected(row["scopes"])
    if (row["pins_sha256"] != fingerprint(targets)
            or row["topics"] != len(pins) or row["tocs"] != len(tocs)):
        raise ValueError("snapshot does not match current shared pins/counts")
    roots = repository_roots()
    archive, cache = external_path(archive, roots), external_path(cache, roots)
    external_path(archive.parent, roots)
    if cache.exists() and not cache.is_dir():
        raise ValueError("cache must be an external directory")
    if not archive.is_file() or archive.stat().st_size != row["bytes"]:
        raise ValueError("compressed artifact size mismatch")
    # Copy and hash once so decompression reads the exact verified artifact bytes.
    with ExitStack() as stack:
        compressed = stack.enter_context(tempfile.TemporaryFile(dir=archive.parent))
        with archive.open("rb") as source:
            digest, size = hashlib.sha256(), 0
            while block := source.read(1024 * 1024):
                size += len(block)
                if size > row["bytes"] or size > MAX_ARCHIVE:
                    raise ValueError("excess compressed artifact bytes")
                digest.update(block)
                compressed.write(block)
        if size != row["bytes"] or digest.hexdigest() != row["sha256"]:
            raise ValueError("compressed artifact SHA-256/size mismatch")
        compressed.seek(0)
        decoded = stack.enter_context(tempfile.TemporaryFile(dir=archive.parent))
        with gzip.GzipFile(fileobj=compressed, mode="rb") as zipped:
            total = 0
            while block := zipped.read(min(1024 * 1024, MAX_TAR - total + 1)):
                total += len(block)
                if total > MAX_TAR:
                    raise ValueError("decompressed archive exceeds bounded size")
                decoded.write(block)
        decoded.seek(0)
        preflight(decoded, targets)
        # Known destination conflicts are also preflighted before any publication.
        for target in targets:
            destination = cache / target.key
            if destination.exists() or destination.is_symlink():
                ibm_docs.cached_bytes(cache, replace(target, legacy_key=target.key))
        external_path(cache, repository_roots())
        decoded.seek(0)
        counts = ibm_docs.import_cache(decoded, cache, pins, tocs)
    if any(counts[k] for k in ["rejected_mismatch", "rejected_conflict", "missing_expected",
                              "mismatch_expected", "skipped_unrecognized"]):
        raise ValueError("shared cache import failed verification or encountered conflict")
    return {"snapshot": snapshot, "semantic_authority": False, "coverage_credit": 0,
            **dict(counts)}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    packer = sub.add_parser("pack")
    packer.add_argument("--id", required=True)
    packer.add_argument("--scope", required=True, action="append")
    importer = sub.add_parser("import")
    importer.add_argument("--snapshot", required=True)
    importer.add_argument("--index", type=Path, default=DEFAULT_INDEX)
    for command in [packer, importer]:
        command.add_argument("--archive", required=True, type=Path)
        command.add_argument("--cache", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "pack":
            result = pack(args.id, args.scope, args.cache, args.archive)
        else:
            result = import_snapshot(args.snapshot, args.index, args.archive, args.cache)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (OSError, ValueError, tarfile.TarError, EOFError, subprocess.SubprocessError):
        # Never echo a credential-bearing locator or arbitrary publication bytes.
        print("IBM docs snapshot: verification failed; no conflicting file replaced",
              file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
