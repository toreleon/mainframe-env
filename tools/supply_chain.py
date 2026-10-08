#!/usr/bin/env python3
"""Validate immutable CI inputs and install the reviewed Jenkins closure."""
from __future__ import annotations

import argparse
import base64
import binascii
from contextlib import ExitStack, contextmanager
from concurrent.futures import ThreadPoolExecutor
import errno
import gzip
import hashlib
import json
import lzma
import os
from pathlib import Path, PurePosixPath
import platform
import posixpath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parent.parent
CI_LOCK_PATH = Path("tools/ci-inputs.lock.json")
JENKINS_LOCK_PATH = Path("tools/jenkins/controller-plugins.lock.json")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
SAFE_ID = re.compile(r"[a-z0-9][a-z0-9-]{0,127}\Z")
SAFE_VERSION = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,191}\Z")
ACTION = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}\Z")
IMAGE = re.compile(r"[^\s@]+@sha256:[0-9a-f]{64}\Z")
MAX_JSON_BYTES = 1024 * 1024
MAX_DOWNLOAD_BYTES = 256 * 1024 * 1024
MAX_TREE_FILES = 200_000
MAX_TREE_BYTES = 4 * 1024 * 1024 * 1024
CI_PATH_PREFIXES = (".github/workflows/", "tools/")
CI_PATHS = {"Jenkinsfile"}
INSTALL_COMMAND = re.compile(
    r"^\s*(?:sudo\s+)?(?:apt(?:-get)?\s+install|apk\s+add|brew\s+install|"
    r"dnf\s+install|yum\s+install|pip(?:3)?\s+install|npm\s+install\s+-g)\b"
)


class SupplyChainError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SupplyChainError(message)


def load_json(path: Path) -> dict:
    size = path.stat().st_size
    require(0 < size <= MAX_JSON_BYTES, f"{path} is empty or too large")
    try:
        value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_json_object)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise SupplyChainError(f"cannot read {path}: {error}") from error
    require(isinstance(value, dict), f"{path} is not a JSON object")
    return value


def exact_keys(value: dict, expected: set[str], context: str) -> None:
    require(isinstance(value, dict), f"{context} is not an object")
    actual = set(value)
    require(actual == expected, f"{context} fields differ: {sorted(actual ^ expected)}")


def safe_relative(value: str, context: str) -> None:
    candidate = PurePosixPath(value)
    require(
        bool(value)
        and not candidate.is_absolute()
        and ".." not in candidate.parts
        and "\\" not in value
        and not any(ord(character) < 32 for character in value),
        f"unsafe {context}: {value!r}",
    )


def validate_ci_lock(root: Path) -> dict:
    lock = load_json(root / CI_LOCK_PATH)
    require(lock.get("schema_version") in {"mainframe-env.ci-input-lock@1", "mainframe-env.ci-input-lock@2"}, "bad CI lock schema")
    exact_keys(
        lock,
        {
            "schema_version",
            "reviewed_on",
            "rust",
            "tools",
            "tracked_remote_inputs",
            "unsupported_local_inputs",
        } | ({"development_profiles"} if lock["schema_version"] == "mainframe-env.ci-input-lock@2" else set()),
        "CI input lock",
    )
    require(re.fullmatch(r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", lock["reviewed_on"] or ""), "bad CI lock review date")
    exact_keys(lock["rust"], {"workspace", "msrv", "fuzz"}, "Rust lock")
    for name, toolchain in lock["rust"].items():
        require(isinstance(toolchain, dict), f"Rust {name} lock is not an object")
        expected_rust_keys = {"version", "rustc_commit", "cargo_commit"}
        if name == "fuzz":
            expected_rust_keys.add("toolchain")
        exact_keys(toolchain, expected_rust_keys, f"Rust {name} lock")
        if name == "fuzz":
            require(
                re.fullmatch(r"nightly-20[0-9]{2}-[0-9]{2}-[0-9]{2}", toolchain["toolchain"] or "")
                is not None,
                "bad Rust fuzz toolchain",
            )
            require(
                re.fullmatch(r"[1-9][0-9]*\.[0-9]+\.[0-9]+-nightly", toolchain["version"] or "")
                is not None,
                "bad Rust fuzz version",
            )
        else:
            require(re.fullmatch(r"[1-9][0-9]*\.[0-9]+\.[0-9]+", toolchain["version"] or ""), f"bad Rust {name} version")
        require(re.fullmatch(r"[0-9a-f]{40}", toolchain["rustc_commit"] or ""), f"bad Rust {name} commit")
        require(re.fullmatch(r"[0-9a-f]{40}", toolchain["cargo_commit"] or ""), f"bad Cargo {name} commit")

    expected_tools = {
        "cargo-deny",
        "cargo-fuzz",
        "cargo-llvm-cov",
        "git",
        "github-cli",
        "java",
        "postgresql",
        "python",
    }
    require(set(lock["tools"]) == expected_tools, "CI tool lock set differs")
    for name, tool in lock["tools"].items():
        require(isinstance(tool, dict), f"tool {name} lock is not an object")
        require(SAFE_VERSION.fullmatch(tool.get("version", "")) is not None, f"bad {name} version")
    exact_keys(lock["tools"]["cargo-deny"], {"version", "install"}, "cargo-deny lock")
    require(
        lock["tools"]["cargo-deny"]["install"]
        == "cargo +1.98.0 install cargo-deny --version 0.20.2 --locked",
        "cargo-deny installation is not exact and locked",
    )
    for name in ("cargo-fuzz", "cargo-llvm-cov"):
        exact_keys(lock["tools"][name], {"version", "install"}, f"{name} lock")
        require(
            lock["tools"][name]["install"]
            == f"cargo +1.98.0 install {name} --version {lock['tools'][name]['version']} --locked",
            f"{name} installation is not exact and locked",
        )

    remote = lock["tracked_remote_inputs"]
    exact_keys(remote, {"github_actions", "container_images", "package_install_commands"}, "remote input lock")
    for name, values in remote.items():
        require(isinstance(values, list) and values == sorted(set(values)), f"{name} lock must be sorted and unique")
    require(all(ACTION.fullmatch(value) is not None for value in remote["github_actions"]), "action lock contains a floating coordinate")
    require(all(IMAGE.fullmatch(value) is not None for value in remote["container_images"]), "container lock contains a mutable coordinate")
    require(not remote["package_install_commands"], "tracked CI package installation is forbidden")

    unsupported = lock["unsupported_local_inputs"]
    require(isinstance(unsupported, list) and unsupported == sorted(set(unsupported)), "unsupported local inputs must be sorted and unique")
    for relative in unsupported:
        safe_relative(relative, "unsupported local input")
    if lock["schema_version"] == "mainframe-env.ci-input-lock@2":
        exact_keys(lock["development_profiles"], {DEVELOPMENT_PROFILE}, "development profiles")
        validate_development_profile(lock["development_profiles"][DEVELOPMENT_PROFILE])
    return lock


DEVELOPMENT_PROFILE = "public-client-linux-x86_64"
CLIENT_LIBRARIES = {"loader", "libdl", "libstdcxx", "libm", "libgcc", "libpthread", "libc", "libnss_files"}
CLIENT_ROLES = CLIENT_LIBRARIES | {"node-archive", "node", "zowe-archive", "bubblewrap"}
NODE_MEMBERS = 20_000
NODE_PAYLOAD = 512 * 1024 * 1024
NODE_MEMBER_BYTES = 128 * 1024 * 1024
ZOWE_FILES = 10_216
ZOWE_BYTES = 42_715_208
ZOWE_FILE_BYTES = 1_048_945
ZOWE_DEPTH = 12
ZOWE_DIRECTORIES = 1066


def unique_json_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for name, value in pairs:
        require(name not in result, f"duplicate JSON key: {name}")
        result[name] = value
    return result


def bounded_integer(value: object, minimum: int, maximum: int, context: str) -> None:
    require(type(value) is int and minimum <= value <= maximum, f"bad {context} bound")


def byte_pin(value: dict, maximum: int, context: str, extra: set[str] | None = None) -> None:
    exact_keys(value, {"bytes", "sha256"} | (extra or set()), context)
    bounded_integer(value["bytes"], 1, maximum, context)
    require(isinstance(value["sha256"], str) and SHA256.fullmatch(value["sha256"]) is not None, f"bad {context} digest")


def sri_digest(value: object) -> bytes:
    require(isinstance(value, str) and value.startswith("sha512-") and len(value) == 95, "bad SHA-512 SRI")
    try:
        decoded = base64.b64decode(value[7:], validate=True)
    except (ValueError, binascii.Error) as error:
        raise SupplyChainError("bad SHA-512 SRI") from error
    require(len(decoded) == 64 and base64.b64encode(decoded).decode("ascii") == value[7:], "noncanonical SHA-512 SRI")
    return decoded


def client_version(value: object, context: str) -> None:
    require(isinstance(value, str) and re.fullmatch(r"(?:0|[1-9][0-9]{0,2})(?:\.(?:0|[1-9][0-9]{0,2})){2}", value) is not None, f"bad {context} version")


def validate_development_profile(profile: dict) -> None:
    exact_keys(profile, {"platform", "purpose", "node", "zowe", "bubblewrap", "libraries"}, "development profile")
    require(profile["platform"] == "linux-x86_64" and profile["purpose"] == "optional-development-client", "bad development profile purpose/platform")
    node = profile["node"]
    exact_keys(node, {"version", "archive", "binary", "license"}, "Node profile")
    client_version(node["version"], "Node")
    byte_pin(node["archive"], 64 * 1024 * 1024, "Node archive", {"url"})
    require(node["archive"]["url"] == f"https://nodejs.org/dist/v{node['version']}/node-v{node['version']}-linux-x64.tar.xz", "bad Node archive URL")
    byte_pin(node["binary"], NODE_MEMBER_BYTES, "Node binary", {"mode"})
    require(type(node["binary"]["mode"]) is int and node["binary"]["mode"] == 0o755, "bad Node binary mode")
    byte_pin(node["license"], MAX_JSON_BYTES, "Node LICENSE")
    zowe = profile["zowe"]
    exact_keys(zowe, {"version", "archive", "tree"}, "Zowe profile")
    client_version(zowe["version"], "Zowe")
    byte_pin(zowe["archive"], 32 * 1024 * 1024, "Zowe archive", {"url", "integrity"})
    require(zowe["archive"]["url"] == f"https://registry.npmjs.org/@zowe/cli/-/cli-{zowe['version']}.tgz", "bad Zowe archive URL")
    sri_digest(zowe["archive"]["integrity"])
    tree = zowe["tree"]
    exact_keys(tree, {"sha256", "files", "bytes", "max_file_bytes", "max_depth", "modes"}, "Zowe tree")
    require(isinstance(tree["sha256"], str) and SHA256.fullmatch(tree["sha256"]) is not None, "bad Zowe tree digest")
    for name, minimum, maximum in [("files", 1, ZOWE_FILES), ("bytes", 0, ZOWE_BYTES), ("max_file_bytes", 1, ZOWE_FILE_BYTES), ("max_depth", 1, ZOWE_DEPTH)]:
        bounded_integer(tree[name], minimum, maximum, "Zowe " + name)
    modes = tree["modes"]
    require(
        isinstance(modes, list) and len(modes) == 2
        and all(type(mode) is int for mode in modes) and modes == [0o644, 0o755],
        "bad Zowe tree modes",
    )
    bwrap = profile["bubblewrap"]
    byte_pin(bwrap, MAX_JSON_BYTES, "bubblewrap", {"version", "mode"})
    client_version(bwrap["version"], "bubblewrap")
    require(type(bwrap["mode"]) is int and bwrap["mode"] == 0o755, "bad bubblewrap mode")
    exact_keys(profile["libraries"], CLIENT_LIBRARIES, "client libraries")
    for name, pin in profile["libraries"].items():
        byte_pin(pin, 16 * 1024 * 1024, "client " + name)


def canonical_input(path: Path) -> None:
    require(isinstance(path, Path) and path.is_absolute() and len(str(path)) <= 4096, "profile input must be an explicit absolute path")
    require(not any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in str(path)), "profile input contains control characters")
    try:
        require(path.resolve(strict=True) == path, f"profile input is not resolved: {path}")
    except (OSError, RuntimeError) as error:
        raise SupplyChainError(f"profile input is unavailable: {path}") from error


def safe_input_mode(metadata: os.stat_result, context: str) -> None:
    require(metadata.st_uid in {0, os.getuid()} and not stat.S_IMODE(metadata.st_mode) & 0o7022, f"privileged or untrusted writable profile input: {context}")


def file_state(metadata: os.stat_result) -> tuple:
    return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mode, metadata.st_uid, metadata.st_gid, metadata.st_mtime_ns, metadata.st_ctime_ns)


@contextmanager
def checked_input(path: Path, pin: dict):
    canonical_input(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_size == pin["bytes"], f"profile input size/type differs: {path}")
    safe_input_mode(before, str(path))
    if "mode" in pin:
        require(stat.S_IMODE(before.st_mode) == pin["mode"], f"profile input mode differs: {path}")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as source:
        require(file_state(os.fstat(source.fileno())) == file_state(before), f"profile input changed before read: {path}")
        try:
            capability = os.getxattr(source.fileno(), "security.capability")
        except OSError as error:
            require(error.errno in {errno.ENODATA, errno.ENOTSUP}, f"cannot inspect profile capabilities: {path}")
            capability = b""
        require(not capability, f"profile input has capabilities: {path}")
        yield source, before
        require(file_state(os.fstat(source.fileno())) == file_state(before) == file_state(path.lstat()), f"profile input changed during read: {path}")


def stream_hash(source, maximum: int) -> tuple[str, bytes, int]:
    digest = hashlib.sha256()
    integrity = hashlib.sha512()
    total = 0
    while chunk := source.read(min(1024 * 1024, maximum - total + 1)):
        total += len(chunk)
        require(total <= maximum, "profile payload exceeds byte bound")
        digest.update(chunk)
        integrity.update(chunk)
    return digest.hexdigest(), integrity.digest(), total


def verify_profile_file(path: Path, pin: dict) -> dict:
    with checked_input(path, pin) as (source, metadata):
        identity = _verify_profile_source(path, source, metadata, pin)
    return identity


def _verify_profile_source(path: Path, source, metadata: os.stat_result, pin: dict) -> dict:
    digest, integrity, size = stream_hash(source, pin["bytes"])
    require(digest == pin["sha256"] and size == pin["bytes"], f"profile input digest differs: {path}")
    if "integrity" in pin:
        require(integrity == sri_digest(pin["integrity"]), "Zowe archive SRI differs")
    return {"bytes": size, "sha256": digest, "mode": stat.S_IMODE(metadata.st_mode)}


class BoundedTarInfo(tarfile.TarInfo):
    def _bounded_metadata(self, archive, maximum, process):
        require(0 <= self.size <= maximum, "archive extended metadata is too large")
        depth = getattr(archive, "_profile_metadata_depth", 0)
        require(depth < 8, "archive extended metadata nesting exceeds bound")
        archive._profile_metadata_depth = depth + 1
        try:
            result = process(archive)
            require(result is not None and result.sparse is None, "sparse or incomplete profile archive entry is forbidden")
            return result
        finally:
            archive._profile_metadata_depth = depth

    def _proc_pax(self, archive):
        return self._bounded_metadata(archive, MAX_JSON_BYTES, super()._proc_pax)

    def _proc_gnulong(self, archive):
        return self._bounded_metadata(archive, 4096, super()._proc_gnulong)

    def _proc_sparse(self, archive):
        raise SupplyChainError("sparse profile archive entry is forbidden")


class DecompressedLimit:
    def __init__(self, source, maximum):
        self.source = source
        self.maximum = maximum
        self.total = 0

    def read(self, size):
        require(0 <= size <= MAX_JSON_BYTES, "unbounded archive read")
        block = self.source.read(min(size, self.maximum - self.total + 1))
        self.total += len(block)
        require(self.total <= self.maximum, "archive decompression exceeds bound")
        return block


@contextmanager
def profile_archive(path: Path, pin: dict, compression: str, maximum: int):
    try:
        with checked_input(path, pin) as (source, _):
            digest, integrity, size = stream_hash(source, pin["bytes"])
            require(digest == pin["sha256"] and size == pin["bytes"], "profile archive digest differs")
            if "integrity" in pin:
                require(integrity == sri_digest(pin["integrity"]), "profile archive SRI differs")
            source.seek(0)
            reader = lzma.LZMAFile(source) if compression == "xz" else gzip.GzipFile(fileobj=source)
            with reader, tarfile.open(fileobj=DecompressedLimit(reader, maximum), mode="r|", tarinfo=BoundedTarInfo) as archive:
                yield archive
    except (tarfile.TarError, EOFError, lzma.LZMAError, ValueError) as error:
        raise SupplyChainError(f"invalid profile archive: {path}: {error}") from error


def archive_name(value: str, root: str, directory: bool = False) -> str:
    name = value[:-1] if directory and value.endswith("/") else value
    candidate = PurePosixPath(name)
    require(bool(name) and len(name.encode("utf-8")) <= 4096 and candidate.as_posix() == name and not candidate.is_absolute()
            and ".." not in candidate.parts and "\\" not in name and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in name)
            and candidate.parts[0] == root, f"unsafe profile archive path: {value!r}")
    require(directory or len(candidate.parts) > 1, "archive root is not a file")
    return name


def member_hash(archive, member) -> str:
    source = archive.extractfile(member)
    require(source is not None, "archive regular payload is missing")
    with source:
        digest, _, size = stream_hash(source, member.size)
    require(size == member.size, "archive regular payload is truncated")
    return digest


def validate_node_archive(path: Path, node: dict) -> None:
    root = "node-v" + node["version"] + "-linux-x64"
    selected = {root + "/bin/node": node["binary"], root + "/LICENSE": node["license"]}
    seen = set()
    found = set()
    non_directories = set()
    payload = 0
    with profile_archive(path, node["archive"], "xz", NODE_PAYLOAD) as archive:
        for member in archive:
            name = archive_name(member.name, root, member.isdir())
            require(name not in seen and len(seen) < NODE_MEMBERS, "duplicate or excessive Node archive members")
            seen.add(name)
            if not member.isdir():
                non_directories.add(name)
            require(len(PurePosixPath(name).parts) <= 32 and not member.mode & 0o7000, "unsafe Node archive member")
            if name in selected:
                pin = selected[name]
                require(member.type in {tarfile.REGTYPE, tarfile.AREGTYPE} and member.size == pin["bytes"] and member.mode == pin.get("mode", 0o644), "Node selected member size/type/mode differs")
                require(member_hash(archive, member) == pin["sha256"], "Node selected member digest differs")
                found.add(name)
            elif member.issym() or member.islnk():
                target = member.linkname
                require(not target.startswith("/") and "\\" not in target and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in target), "unsafe Node link target")
                joined = posixpath.join(posixpath.dirname(name), target) if member.issym() else target
                archive_name(posixpath.normpath(joined), root)
            else:
                require(member.type in {tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE}, "unexpected Node archive entry type")
            if member.isreg():
                bounded_integer(member.size, 0, NODE_MEMBER_BYTES, "Node member")
                payload += member.size
                require(payload <= NODE_PAYLOAD, "Node archive payload exceeds bound")
    require(not any(str(parent) in non_directories for name in seen for parent in PurePosixPath(name).parents), "Node archive has non-directory ancestors")
    require(found == set(selected), "Node archive lacks selected binary/LICENSE")


def zowe_archive_rows(path: Path, zowe: dict) -> dict[str, dict]:
    limits = zowe["tree"]
    rows = {}
    directories = set()
    seen = set()
    total = 0
    with profile_archive(path, zowe["archive"], "gz", 64 * 1024 * 1024) as archive:
        for member in archive:
            name = archive_name(member.name, "package", member.isdir())
            require(name not in seen and len(seen) < limits["files"] + ZOWE_DIRECTORIES, "duplicate or excessive Zowe archive entries")
            seen.add(name)
            require(len(PurePosixPath(name).parts) <= limits["max_depth"], "Zowe archive depth exceeds bound")
            require(member.type in {tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE}, "unexpected Zowe archive entry type")
            require(member.mode in limits["modes"], "Zowe archive mode differs")
            if member.isdir():
                require(member.size == 0, "Zowe archive directory has payload")
                directories.add(name)
                require(len(directories) <= ZOWE_DIRECTORIES, "Zowe archive directories exceed bound")
                continue
            require(len(rows) < limits["files"], "Zowe archive file count exceeds bound")
            bounded_integer(member.size, 0, limits["max_file_bytes"], "Zowe member")
            total += member.size
            require(total <= limits["bytes"], "Zowe archive payload exceeds bound")
            rows[name] = {"path": name, "bytes": member.size, "sha256": member_hash(archive, member), "mode": member.mode}
    parents = {str(parent) for name in rows for parent in PurePosixPath(name).parents if str(parent) != "."}
    require(not set(rows) & parents and directories <= parents and len(parents) <= ZOWE_DIRECTORIES, "Zowe archive has file ancestors or unexpected directories")
    require(len(rows) == limits["files"] and total == limits["bytes"], "Zowe archive totals differ")
    return rows


def validate_zowe_tree(tree: Path, rows: dict[str, dict], zowe: dict, _command=False) -> dict:
    if _command:
        return _validate_zowe_tree_command(tree, rows, zowe)
    canonical_input(tree)
    require(stat.S_ISDIR(tree.lstat().st_mode), "Zowe tree root is not a directory")
    safe_input_mode(tree.lstat(), str(tree))
    parents = {str(parent) for name in rows for parent in PurePosixPath(name).parents if str(parent) != "."}
    pending = [tree]
    found = set()
    directories = set()
    total = 0
    while pending:
        directory = pending.pop()
        before = directory.lstat()
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                name = path.relative_to(tree).as_posix()
                archive_name(name, "package", entry.is_dir(follow_symlinks=False))
                metadata = entry.stat(follow_symlinks=False)
                safe_input_mode(metadata, name)
                if stat.S_ISDIR(metadata.st_mode):
                    require(name in parents and name not in directories and len(directories) < ZOWE_DIRECTORIES, "unexpected Zowe tree directory")
                    directories.add(name)
                    pending.append(path)
                else:
                    require(stat.S_ISREG(metadata.st_mode) and name in rows and name not in found and len(found) < zowe["tree"]["files"], "unexpected Zowe tree file")
                    identity = verify_profile_file(path, rows[name])
                    require(identity["mode"] == rows[name]["mode"], "Zowe tree mode differs")
                    total += identity["bytes"]
                    require(total <= zowe["tree"]["bytes"], "Zowe tree payload exceeds bound")
                    found.add(name)
        require(file_state(before) == file_state(directory.lstat()), "Zowe directory changed during read")
    require(found == set(rows) and directories == parents, "Zowe tree membership differs")
    # The retained producer sorted Path objects, whose component order differs from strings.
    canonical = json.dumps([rows[name] for name in sorted(rows, key=PurePosixPath)], sort_keys=True, separators=(",", ":")).encode("utf-8")
    digest = hashlib.sha256(canonical).hexdigest()
    require(digest == zowe["tree"]["sha256"], "Zowe retained comparison digest differs")
    manifest_path = tree / "package/package.json"
    require("package/package.json" in rows and "package/lib/main.js" in rows, "Zowe tree lacks manifest/fixed entry")
    require(rows["package/package.json"]["bytes"] <= MAX_JSON_BYTES, "Zowe package manifest exceeds bound")
    with checked_input(manifest_path, rows["package/package.json"]) as (source, _):
        data = source.read(MAX_JSON_BYTES + 1)
        require(hashlib.sha256(data).hexdigest() == rows["package/package.json"]["sha256"], "Zowe manifest changed")
        try:
            manifest_value = json.loads(data, object_pairs_hook=unique_json_object)
        except (UnicodeError, json.JSONDecodeError) as error:
            raise SupplyChainError("invalid Zowe package manifest") from error
    require(isinstance(manifest_value, dict) and manifest_value.get("name") == "@zowe/cli" and manifest_value.get("version") == zowe["version"], "Zowe package identity differs")
    return {"sha256": digest, "files": len(found), "bytes": total}


@contextmanager
def _tree_directory(path: Path, before: os.stat_result, parent=None, name=None):
    require(stat.S_ISDIR(before.st_mode), "Zowe tree directory type differs")
    safe_input_mode(before, str(path))
    descriptor = os.open(path if parent is None else name,
                         os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK,
                         **({} if parent is None else {"dir_fd": parent}))
    try:
        require(file_state(os.fstat(descriptor)) == file_state(before), "Zowe directory changed before open")
        yield descriptor
        if parent is None:
            canonical_input(path)
        relative = path.lstat() if parent is None else os.stat(name, dir_fd=parent, follow_symlinks=False)
        require(file_state(os.fstat(descriptor)) == file_state(before)
                == file_state(relative) == file_state(path.lstat()), "Zowe directory changed during read")
    finally:
        os.close(descriptor)


@contextmanager
def _tree_file(parent: int, name: str, path: Path, before: os.stat_result, pin: dict):
    # The held parent chain supplies namespace authority instead of resolving
    # every absolute ancestor again. Leaf admission/fences match checked_input.
    require(stat.S_ISREG(before.st_mode) and before.st_size == pin["bytes"],
            f"profile input size/type differs: {path}")
    safe_input_mode(before, str(path))
    if "mode" in pin:
        require(stat.S_IMODE(before.st_mode) == pin["mode"], f"profile input mode differs: {path}")
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    try:
        # Retain raw-fd ownership even if stream construction fails. Closing the
        # stream precedes the sole raw close; fdopen never implicitly owns it.
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            require(file_state(os.fstat(source.fileno())) == file_state(before), f"profile input changed before read: {path}")
            try:
                capability = os.getxattr(source.fileno(), "security.capability")
            except OSError as error:
                require(error.errno in {errno.ENODATA, errno.ENOTSUP}, f"cannot inspect profile capabilities: {path}")
                capability = b""
            require(not capability, f"profile input has capabilities: {path}")
            yield source, before
            require(file_state(os.fstat(source.fileno())) == file_state(before)
                    == file_state(os.stat(name, dir_fd=parent, follow_symlinks=False))
                    == file_state(path.lstat()), f"profile input changed during read: {path}")
    finally:
        os.close(descriptor)


def _validate_zowe_tree_command(tree: Path, rows: dict[str, dict], zowe: dict) -> dict:
    """Fresh command-only walk; no descriptor/admission survives this call."""
    canonical_input(tree)
    root_before = tree.lstat()
    parents = {str(parent) for name in rows for parent in PurePosixPath(name).parents if str(parent) != "."}
    found, directories = set(), set()
    directory_states = {}
    total = 0

    def entries(descriptor, path):
        children = []
        # Close scandir before descending, so only the depth-bounded authority
        # stack is held; an untrusted directory cannot grow an unbounded list.
        with os.scandir(descriptor) as iterator:
            for entry in iterator:
                require(len(children) < zowe["tree"]["files"] + ZOWE_DIRECTORIES,
                        "Zowe directory entries exceed bound")
                child = path / entry.name
                name = child.relative_to(tree).as_posix()
                metadata = entry.stat(follow_symlinks=False)
                archive_name(name, "package", stat.S_ISDIR(metadata.st_mode))
                require(len(PurePosixPath(name).parts) <= zowe["tree"]["max_depth"], "Zowe tree depth exceeds bound")
                safe_input_mode(metadata, name)
                children.append((entry.name, child, name, metadata))
        return children

    def visit(descriptor, path):
        nonlocal total
        for leaf, child, name, metadata in entries(descriptor, path):
            if stat.S_ISDIR(metadata.st_mode):
                require(name in parents and name not in directories and len(directories) < ZOWE_DIRECTORIES,
                        "unexpected Zowe tree directory")
                directories.add(name)
                directory_states[child] = file_state(metadata)
                with _tree_directory(child, metadata, descriptor, leaf) as child_descriptor:
                    visit(child_descriptor, child)
            else:
                require(stat.S_ISREG(metadata.st_mode) and name in rows and name not in found
                        and len(found) < zowe["tree"]["files"], "unexpected Zowe tree file")
                with _tree_file(descriptor, leaf, child, metadata, rows[name]) as (source, before):
                    identity = _verify_profile_source(child, source, before, rows[name])
                require(identity["mode"] == rows[name]["mode"], "Zowe tree mode differs")
                total += identity["bytes"]
                require(total <= zowe["tree"]["bytes"], "Zowe tree payload exceeds bound")
                found.add(name)

    with _tree_directory(tree, root_before) as root_descriptor:
        children = entries(root_descriptor, tree)
        require(len(children) == 1 and children[0][2] == "package" and stat.S_ISDIR(children[0][3].st_mode)
                and "package" in parents, "Zowe tree root membership differs")
        leaf, package, name, metadata = children[0]
        directories.add(name); directory_states[package] = file_state(metadata)
        require(len(directories) <= ZOWE_DIRECTORIES, "unexpected Zowe tree directory")
        # Root and package stay held across the fresh manifest and ALL final
        # namespace checks, including already processed descendant directories.
        with _tree_directory(package, metadata, root_descriptor, leaf) as package_descriptor:
            visit(package_descriptor, package)
            require(found == set(rows) and directories == parents, "Zowe tree membership differs")
            canonical = json.dumps([rows[key] for key in sorted(rows, key=PurePosixPath)],
                                   sort_keys=True, separators=(",", ":")).encode("utf-8")
            digest = hashlib.sha256(canonical).hexdigest()
            require(digest == zowe["tree"]["sha256"], "Zowe retained comparison digest differs")
            manifest_path = package / "package.json"
            require("package/package.json" in rows and "package/lib/main.js" in rows,
                    "Zowe tree lacks manifest/fixed entry")
            require(rows["package/package.json"]["bytes"] <= MAX_JSON_BYTES, "Zowe package manifest exceeds bound")
            with checked_input(manifest_path, rows["package/package.json"]) as (source, _):
                data = source.read(MAX_JSON_BYTES + 1)
                require(hashlib.sha256(data).hexdigest() == rows["package/package.json"]["sha256"], "Zowe manifest changed")
                try:
                    manifest_value = json.loads(data, object_pairs_hook=unique_json_object)
                except (UnicodeError, json.JSONDecodeError) as error:
                    raise SupplyChainError("invalid Zowe package manifest") from error
            require(isinstance(manifest_value, dict) and manifest_value.get("name") == "@zowe/cli"
                    and manifest_value.get("version") == zowe["version"], "Zowe package identity differs")
            for path, before in directory_states.items():
                require(file_state(path.lstat()) == before, "Zowe directory namespace changed after manifest")
    return {"sha256": digest, "files": len(found), "bytes": total}


def profile_bindings(values: list[str]) -> dict[str, Path]:
    result = {}
    require(len(values) <= len(CLIENT_ROLES), "too many profile role bindings")
    for value in values:
        role, separator, name = value.partition("=")
        require(separator and role in CLIENT_ROLES and role not in result, "unknown or duplicate profile role binding")
        require(name.startswith("/") and str(Path(name)) == name, "profile binding path is not canonical absolute")
        canonical_input(Path(name))
        result[role] = Path(name)
    return result


def validate_development_inputs(lock: dict, profile_name: str, files: dict[str, Path], tree: Path) -> dict:
    return _development_input_parts(lock, profile_name, files, tree)[0]


def _development_input_parts(lock: dict, profile_name: str, files: dict[str, Path], tree: Path, _command=False) -> tuple:
    require(profile_name == DEVELOPMENT_PROFILE, "unknown development profile")
    exact_keys(lock.get("development_profiles"), {DEVELOPMENT_PROFILE}, "development profiles")
    profile = lock["development_profiles"][profile_name]
    validate_development_profile(profile)
    require(sys.platform == "linux" and platform.machine() == "x86_64", "selected development profile requires Linux x86_64")
    exact_keys(files, CLIENT_ROLES, "profile role bindings")
    pins = {"node-archive": profile["node"]["archive"], "node": profile["node"]["binary"], "zowe-archive": profile["zowe"]["archive"], "bubblewrap": profile["bubblewrap"], **profile["libraries"]}
    try:
        identities = {name: verify_profile_file(files[name], pins[name]) for name in sorted(CLIENT_ROLES)}
        validate_node_archive(files["node-archive"], profile["node"])
        rows = zowe_archive_rows(files["zowe-archive"], profile["zowe"])
        identities["tree"] = (validate_zowe_tree(tree, rows, profile["zowe"], True) if _command
                              else validate_zowe_tree(tree, rows, profile["zowe"]))
    except OSError as error:
        raise SupplyChainError(f"development input unavailable: {error}") from error
    return {"files": dict(files), "tree": tree, "identities": identities}, rows


@contextmanager
def _development_input_command(lock: dict, profile_name: str, files: dict[str, Path],
                               tree: Path, *, lock_bytes: bytes):
    """Privately retain one accepted PRE parse for this command's sole POST.

    No proof or row argument is accepted. Fresh bytes authorize POST; retained
    rows replace only repeated decompression. Descriptors are opened after the
    child wait and fence both archives through every remaining POST check.
    """
    require(type(lock_bytes) is bytes and 0 < len(lock_bytes) <= MAX_JSON_BYTES,
            "command input proof requires bounded lock bytes")
    try:
        decoded = json.loads(lock_bytes, object_pairs_hook=unique_json_object)
        lock_identity = json.dumps(lock, sort_keys=True, separators=(",", ":"))
        require(json.dumps(decoded, sort_keys=True, separators=(",", ":")) == lock_identity,
                "command input proof lock bytes differ")
    except (UnicodeError, ValueError, TypeError) as error:
        raise SupplyChainError("invalid command input proof lock") from error
    bindings = tuple(sorted(files.items()))
    validated, rows = _development_input_parts(lock, profile_name, files, tree, True)
    require(json.dumps(lock, sort_keys=True, separators=(",", ":")) == lock_identity
            and tuple(sorted(files.items())) == bindings,
            "command input PRE identities changed before proof mint")
    # Only immutable scalars survive PRE. None of these rows escapes in the
    # ordinary validated result, nor can a caller supply replacements at POST.
    frozen_rows = tuple((name, row["path"], row["bytes"], row["sha256"], row["mode"])
                        for name, row in rows.items())
    accepted_tree = tree
    accepted_profile = profile_name
    accepted_archive_identities = {role: tuple(sorted(validated["identities"][role].items()))
                                   for role in ("node-archive", "zowe-archive")}
    alive = True
    used = False

    def post(current_lock, current_profile, current_files, current_tree, *, lock_bytes):
        nonlocal used
        require(alive and not used, "command input proof expired or consumed")
        used = True
        try:
            def bound():
                require(current_profile == accepted_profile and current_tree == accepted_tree
                        and isinstance(current_files, dict) and tuple(sorted(current_files.items())) == bindings,
                        "command input proof bindings differ")
                require(type(lock_bytes) is bytes and lock_bytes == raw_lock
                        and json.dumps(current_lock, sort_keys=True, separators=(",", ":")) == lock_identity,
                        "command input proof lock differs")
            bound()
            profile = json.loads(lock_identity)["development_profiles"][accepted_profile]
            validate_development_profile(profile)
            require(sys.platform == "linux" and platform.machine() == "x86_64",
                    "selected development profile requires Linux x86_64")
            pins = {"node-archive": profile["node"]["archive"], "node": profile["node"]["binary"],
                    "zowe-archive": profile["zowe"]["archive"], "bubblewrap": profile["bubblewrap"],
                    **profile["libraries"]}
            identities = {}
            with ExitStack() as fences:
                # Open both before other roles/tree checks. A later mutation,
                # capability/mode change or same-byte replacement must fail at
                # checked_input's final descriptor/path metadata comparison.
                for role in ("node-archive", "zowe-archive"):
                    path = current_files[role]
                    source, metadata = fences.enter_context(checked_input(path, pins[role]))
                    identities[role] = _verify_profile_source(path, source, metadata, pins[role])
                    require(tuple(sorted(identities[role].items())) == accepted_archive_identities[role],
                            "command input proof archive identity differs")
                for role in sorted(CLIENT_ROLES - {"node-archive", "zowe-archive"}):
                    identities[role] = verify_profile_file(current_files[role], pins[role])
                current_rows = {name: {"path": path, "bytes": size, "sha256": digest, "mode": mode}
                                for name, path, size, digest, mode in frozen_rows}
                identities["tree"] = validate_zowe_tree(current_tree, current_rows, profile["zowe"], True)
                bound()
            return {"files": dict(current_files), "tree": current_tree, "identities": identities}
        except OSError as error:
            raise SupplyChainError(f"development input unavailable: {error}") from error

    raw_lock = lock_bytes
    try:
        yield validated, post
    finally:
        alive = False
        frozen_rows = ()


def validate_jenkins_lock(root: Path) -> dict:
    lock = load_json(root / JENKINS_LOCK_PATH)
    exact_keys(lock, {"schema_version", "reviewed_on", "controller", "required_plugins", "plugins"}, "Jenkins lock")
    require(lock["schema_version"] == "mainframe-env.jenkins-input-lock@1", "bad Jenkins lock schema")
    require(re.fullmatch(r"20[0-9]{2}-[0-9]{2}-[0-9]{2}", lock["reviewed_on"] or ""), "bad Jenkins review date")
    controller = lock["controller"]
    exact_keys(controller, {"version", "url", "sha256"}, "Jenkins controller lock")
    require(SAFE_VERSION.fullmatch(controller["version"] or "") is not None, "bad Jenkins version")
    expected_url = f"https://get.jenkins.io/war-stable/{controller['version']}/jenkins.war"
    require(controller["url"] == expected_url, "Jenkins controller URL is not version-pinned")
    require(SHA256.fullmatch(controller["sha256"] or "") is not None, "bad Jenkins digest")
    plugins = lock["plugins"]
    require(isinstance(plugins, list) and 1 <= len(plugins) <= 256, "bad Jenkins plugin count")
    names = []
    for plugin in plugins:
        require(isinstance(plugin, dict), "Jenkins plugin entry is not an object")
        exact_keys(plugin, {"id", "version", "sha256"}, "Jenkins plugin")
        require(SAFE_ID.fullmatch(plugin["id"] or "") is not None, "bad Jenkins plugin id")
        require(SAFE_VERSION.fullmatch(plugin["version"] or "") is not None, f"bad {plugin['id']} version")
        require(SHA256.fullmatch(plugin["sha256"] or "") is not None, f"bad {plugin['id']} digest")
        names.append(plugin["id"])
    require(names == sorted(set(names)), "Jenkins plugins must be sorted and unique")
    required = lock["required_plugins"]
    require(isinstance(required, list) and required == sorted(set(required)), "required plugins must be sorted and unique")
    require(set(required) <= set(names), "a required Jenkins plugin is not locked")
    return lock


def tracked_files(root: Path) -> list[str]:
    try:
        output = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
        values = [part.decode("utf-8") for part in output.split(b"\0") if part]
    except (OSError, UnicodeError, subprocess.CalledProcessError) as error:
        raise SupplyChainError(f"cannot enumerate tracked files: {error}") from error
    require(len(values) <= MAX_TREE_FILES, "tracked file count is unbounded")
    for value in values:
        safe_relative(value, "tracked file")
    return values


def scan_external_inputs(root: Path, tracked: list[str]) -> dict[str, list[str]]:
    actions: set[str] = set()
    images: set[str] = set()
    installers: set[str] = set()
    for relative in tracked:
        if relative not in CI_PATHS and not relative.startswith(CI_PATH_PREFIXES):
            continue
        path = root / relative
        if not path.is_file() or path.stat().st_size > MAX_JSON_BYTES:
            raise SupplyChainError(f"tracked CI file is missing or too large: {relative}")
        text = path.read_text(encoding="utf-8")
        is_workflow = relative.startswith(".github/workflows/") and relative.endswith((".yml", ".yaml"))
        stages: set[str] = set()
        for number, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if path.name.startswith("Dockerfile"):
                base = re.match(r"FROM\s+(?:--platform=\S+\s+)?(\S+)(?:\s+AS\s+(\S+))?", stripped, re.IGNORECASE)
                if base:
                    coordinate = base.group(1)
                    if coordinate != "scratch" and coordinate.lower() not in stages:
                        require(IMAGE.fullmatch(coordinate) is not None, f"mutable Docker base at {relative}:{number}: {coordinate}")
                        images.add(coordinate)
                    if base.group(2):
                        stages.add(base.group(2).lower())
            if is_workflow:
                use = re.match(r"-?\s*uses:\s*['\"]?([^'\"#\s]+)", stripped)
                if use:
                    coordinate = use.group(1)
                    require(ACTION.fullmatch(coordinate) is not None, f"floating action at {relative}:{number}: {coordinate}")
                    actions.add(coordinate)
                image = re.match(r"image:\s*['\"]?([^'\"#\s]+)", stripped)
                if image:
                    coordinate = image.group(1)
                    require(IMAGE.fullmatch(coordinate) is not None, f"mutable container at {relative}:{number}: {coordinate}")
                    images.add(coordinate)
                runner = re.match(r"runs-on:\s*(.+)", stripped)
                if runner:
                    require("self-hosted" in runner.group(1), f"mutable hosted runner at {relative}:{number}")
            if INSTALL_COMMAND.match(stripped):
                installers.add(f"{relative}:{number}:{stripped}")
    return {
        "github_actions": sorted(actions),
        "container_images": sorted(images),
        "package_install_commands": sorted(installers),
    }


def check_repository(root: Path = ROOT) -> tuple[dict, dict]:
    ci_lock = validate_ci_lock(root)
    jenkins_lock = validate_jenkins_lock(root)
    tracked = tracked_files(root)
    require(not (set(ci_lock["unsupported_local_inputs"]) & set(tracked)), "an explicitly unsupported local CI input became tracked")
    observed = scan_external_inputs(root, tracked)
    require(observed == ci_lock["tracked_remote_inputs"], "tracked remote CI inputs differ from their lock")

    toolchain = (root / "rust-toolchain.toml").read_text(encoding="utf-8")
    workspace = (root / "Cargo.toml").read_text(encoding="utf-8")
    channel = re.search(r'^channel\s*=\s*"([^"]+)"\s*$', toolchain, re.MULTILINE)
    components = re.search(r'^components\s*=\s*\[([^]]+)\]\s*$', toolchain, re.MULTILINE)
    rust_version = re.search(r'^rust-version\s*=\s*"([^"]+)"\s*$', workspace, re.MULTILINE)
    require(channel is not None and channel.group(1) == ci_lock["rust"]["workspace"]["version"], "workspace Rust pin drifted")
    require(
        components is not None
        and all(name in components.group(1) for name in ('"clippy"', '"llvm-tools-preview"', '"rustfmt"')),
        "workspace Rust component lock is incomplete",
    )
    declared_msrv = rust_version.group(1) if rust_version is not None else ""
    normalized_msrv = declared_msrv + ".0" if declared_msrv.count(".") == 1 else declared_msrv
    require(normalized_msrv == ci_lock["rust"]["msrv"]["version"], "workspace MSRV pin drifted")
    assurance = load_json(root / "tools/assurance-gates.json")
    require(
        assurance.get("fuzz", {}).get("toolchain")
        == ci_lock["rust"]["fuzz"]["toolchain"],
        "fuzz toolchain lock drifted",
    )

    jenkins = (root / "Jenkinsfile").read_text(encoding="utf-8")
    msrv = re.search(r"stage\('MSRV'\)(.*?)(?=\n\s*stage\('|\Z)", jenkins, re.DOTALL)
    require(msrv is not None, "Jenkins MSRV stage is missing")
    normalized_msrv = " ".join(msrv.group(1).split())
    expected_msrv = f"cargo +{ci_lock['rust']['msrv']['version']} check --workspace --all-targets --all-features --locked"
    require(expected_msrv in normalized_msrv and " -p " not in f" {normalized_msrv} ", "Jenkins MSRV scope is incomplete")
    require("--gate supply-chain" in jenkins and "tools/supply_chain.py check" in jenkins, "Jenkins supply-chain gate is missing")

    launcher = (root / "tools/jenkins/run-local.sh").read_text(encoding="utf-8")
    require("controller/jenkins.war" in launcher and "exec \"$MAINFRAME_ENV_JAVA\" -jar" in launcher, "Jenkins launcher does not use the locked controller")
    require("jenkins-lts" not in launcher, "Jenkins launcher still uses a floating package")
    return ci_lock, jenkins_lock


def command_output(arguments: list[str], env: dict[str, str] | None = None) -> str:
    try:
        return subprocess.check_output(arguments, text=True, stderr=subprocess.STDOUT, env=env).strip()
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "output", "")
        raise SupplyChainError(f"cannot identify {' '.join(arguments)}: {detail or error}") from error


def field(output: str, name: str) -> str:
    prefix = f"{name}: "
    value = next((line.removeprefix(prefix) for line in output.splitlines() if line.startswith(prefix)), None)
    require(value is not None, f"tool output omits {name}")
    return value


def verify_rust(name: str, locked: dict) -> None:
    version = locked["version"]
    toolchain = locked.get("toolchain", version)
    rustc = command_output(["rustup", "run", toolchain, "rustc", "-Vv"])
    cargo = command_output(["rustup", "run", toolchain, "cargo", "-Vv"])
    require(field(rustc, "release") == version, f"Rust {name} release drifted")
    require(field(rustc, "commit-hash") == locked["rustc_commit"], f"Rust {name} compiler commit drifted")
    require(field(cargo, "release") == version, f"Rust {name} Cargo release drifted")
    require(field(cargo, "commit-hash") == locked["cargo_commit"], f"Rust {name} Cargo commit drifted")


def verify_active_rust(locked: dict) -> None:
    rustc = command_output(["rustc", "-Vv"])
    cargo = command_output(["cargo", "-Vv"])
    require(field(rustc, "release") == locked["version"], "active Rust release drifted")
    require(field(rustc, "commit-hash") == locked["rustc_commit"], "active compiler commit drifted")
    require(field(cargo, "release") == locked["version"], "active Cargo release drifted")
    require(field(cargo, "commit-hash") == locked["cargo_commit"], "active Cargo commit drifted")


def java_executable() -> str:
    configured = os.environ.get("MAINFRAME_ENV_JAVA")
    candidate = configured or shutil.which("java")
    require(bool(candidate), "Java is missing; set MAINFRAME_ENV_JAVA")
    return str(candidate)


def verify_runtime(ci_lock: dict, scope: str) -> None:
    tools = ci_lock["tools"]
    if scope in {"ci", "controller", "offline", "all"}:
        expected_python = tuple(map(int, tools["python"]["version"].split(".")))
        require(sys.version_info[:3] == expected_python, f"Python must be exactly {tools['python']['version']}")
        git = command_output(["git", "--version"])
        match = re.search(r"git version ([0-9]+\.[0-9]+\.[0-9]+)", git)
        require(match is not None and match.group(1) == tools["git"]["version"], f"Git must be exactly {tools['git']['version']}")
    if scope in {"ci", "offline", "all"}:
        verify_rust("workspace", ci_lock["rust"]["workspace"])
        verify_active_rust(ci_lock["rust"]["workspace"])
    if scope in {"ci", "all"}:
        verify_rust("MSRV", ci_lock["rust"]["msrv"])
        verify_rust("fuzz", ci_lock["rust"]["fuzz"])
        deny = command_output(["cargo", "deny", "--version"])
        require(deny == f"cargo-deny {tools['cargo-deny']['version']}", f"cargo-deny must be exactly {tools['cargo-deny']['version']}")
        fuzz = command_output(["cargo", "fuzz", "--version"])
        require(fuzz == f"cargo-fuzz {tools['cargo-fuzz']['version']}", f"cargo-fuzz must be exactly {tools['cargo-fuzz']['version']}")
        coverage = command_output(["cargo", "llvm-cov", "--version"])
        require(coverage == f"cargo-llvm-cov {tools['cargo-llvm-cov']['version']}", f"cargo-llvm-cov must be exactly {tools['cargo-llvm-cov']['version']}")
    if scope in {"controller", "all"}:
        java = command_output([java_executable(), "-version"])
        match = re.search(r'version "([0-9.]+)', java)
        require(match is not None and match.group(1) == tools["java"]["version"], f"Java must be exactly {tools['java']['version']}")
    if scope in {"postgres", "all"}:
        postgres = command_output(["postgres", "--version"])
        match = re.search(r"PostgreSQL\)?\s+([0-9]+\.[0-9]+)", postgres)
        require(match is not None and match.group(1) == tools["postgresql"]["version"], f"PostgreSQL must be exactly {tools['postgresql']['version']}")
    if scope in {"release", "all"}:
        github = command_output(["gh", "--version"])
        match = re.search(r"gh version ([0-9]+\.[0-9]+\.[0-9]+)", github)
        require(match is not None and match.group(1) == tools["github-cli"]["version"], f"GitHub CLI must be exactly {tools['github-cli']['version']}")


def sha256_file(path: Path, limit: int = MAX_DOWNLOAD_BYTES) -> str:
    metadata = path.stat()
    require(stat.S_ISREG(metadata.st_mode) and 0 < metadata.st_size <= limit, f"{path} is not a bounded regular file")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def manifest(path: Path) -> dict[str, str]:
    try:
        with zipfile.ZipFile(path) as archive:
            info = archive.getinfo("META-INF/MANIFEST.MF")
            require(info.file_size <= MAX_JSON_BYTES, f"{path} manifest is too large")
            text = archive.read(info).decode("utf-8")
    except (OSError, KeyError, UnicodeError, zipfile.BadZipFile) as error:
        raise SupplyChainError(f"cannot inspect {path}: {error}") from error
    lines: list[str] = []
    for line in text.replace("\r\n", "\n").split("\n"):
        if line.startswith(" ") and lines:
            lines[-1] += line[1:]
        else:
            lines.append(line)
    return dict(line.split(": ", 1) for line in lines if ": " in line)


def plugin_dependencies(fields: dict[str, str]) -> set[str]:
    dependencies = set()
    for entry in fields.get("Plugin-Dependencies", "").split(","):
        if not entry:
            continue
        parts = entry.split(";")
        if "resolution:=optional" not in parts[1:]:
            dependency = parts[0].split(":", 1)[0]
            require(SAFE_ID.fullmatch(dependency) is not None, f"bad plugin dependency: {dependency}")
            dependencies.add(dependency)
    return dependencies


def verify_jenkins_artifacts(jenkins_lock: dict, controller: Path, plugins: Path) -> None:
    require(sha256_file(controller) == jenkins_lock["controller"]["sha256"], "Jenkins controller digest drifted")
    require(manifest(controller).get("Jenkins-Version") == jenkins_lock["controller"]["version"], "Jenkins controller version drifted")
    locked = {plugin["id"]: plugin for plugin in jenkins_lock["plugins"]}
    plugin_paths = list(plugins.glob("*.jpi")) + list(plugins.glob("*.hpi"))
    actual = {path.stem: path for path in plugin_paths}
    require(len(actual) == len(plugin_paths), "duplicate Jenkins plugin archives are active")
    require(set(actual) == set(locked), f"active Jenkins plugin set differs: {sorted(set(actual) ^ set(locked))}")
    disabled = sorted(path.name for path in plugins.glob("*.disabled"))
    require(not disabled, f"locked Jenkins plugins are disabled: {disabled}")
    dependencies: dict[str, set[str]] = {}
    for name, plugin in locked.items():
        path = actual[name]
        require(sha256_file(path) == plugin["sha256"], f"Jenkins plugin digest drifted: {name}")
        fields = manifest(path)
        require(fields.get("Short-Name") == name, f"Jenkins plugin identity drifted: {name}")
        require(fields.get("Plugin-Version") == plugin["version"], f"Jenkins plugin version drifted: {name}")
        dependencies[name] = plugin_dependencies(fields)
    missing = {f"{name}->{dependency}" for name, values in dependencies.items() for dependency in values if dependency not in locked}
    require(not missing, f"Jenkins plugin dependency closure is incomplete: {sorted(missing)}")


def verify_jenkins_home(jenkins_lock: dict, home: Path) -> None:
    require(home.is_absolute() and len(home.parts) >= 3, "Jenkins home must be a specific absolute path")
    verify_jenkins_artifacts(jenkins_lock, home / "controller/jenkins.war", home / "plugins")
    locked = {plugin["id"] for plugin in jenkins_lock["plugins"]}
    pinned = {
        path.name.removesuffix(".jpi.pinned")
        for path in (home / "plugins").glob("*.jpi.pinned")
    }
    require(pinned == locked, f"Jenkins plugin pin markers differ: {sorted(pinned ^ locked)}")


def download(url: str, destination: Path, expected: str) -> None:
    require(url.startswith("https://"), "Jenkins artifact URL is not HTTPS")
    request = urllib.request.Request(url, headers={"User-Agent": "mainframe-env-supply-chain/1"})
    digest = hashlib.sha256()
    total = 0
    try:
        with urllib.request.urlopen(request, timeout=60) as response, destination.open("wb") as output:
            require(response.geturl().startswith("https://"), "Jenkins artifact redirected away from HTTPS")
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                require(total <= MAX_DOWNLOAD_BYTES, "Jenkins artifact exceeds its bound")
                digest.update(chunk)
                output.write(chunk)
    except (OSError, urllib.error.URLError) as error:
        raise SupplyChainError(f"cannot download {url}: {error}") from error
    require(total > 0 and digest.hexdigest() == expected, f"Jenkins artifact digest mismatch: {url}")


def install_jenkins(jenkins_lock: dict, home: Path) -> None:
    require(home.is_absolute() and len(home.parts) >= 3, "Jenkins home must be a specific absolute path")
    home.mkdir(parents=True, exist_ok=True)
    plugin_directory = home / "plugins"
    existing = {path.stem for suffix in ("*.jpi", "*.hpi") for path in plugin_directory.glob(suffix)} if plugin_directory.is_dir() else set()
    locked = {plugin["id"] for plugin in jenkins_lock["plugins"]}
    require(not (existing - locked), f"unreviewed Jenkins plugins are installed: {sorted(existing - locked)}")
    with tempfile.TemporaryDirectory(prefix="jenkins-inputs-", dir=home) as temporary:
        staging = Path(temporary)
        staged_plugins = staging / "plugins"
        staged_plugins.mkdir()
        controller = staging / "jenkins.war"

        def fetch_plugin(plugin: dict) -> None:
            quoted_name = urllib.parse.quote(plugin["id"], safe="-")
            quoted_version = urllib.parse.quote(plugin["version"], safe="._-")
            url = f"https://updates.jenkins.io/download/plugins/{quoted_name}/{quoted_version}/{quoted_name}.hpi"
            download(url, staged_plugins / f"{plugin['id']}.jpi", plugin["sha256"])

        with ThreadPoolExecutor(max_workers=4) as executor:
            controller_download = executor.submit(
                download,
                jenkins_lock["controller"]["url"],
                controller,
                jenkins_lock["controller"]["sha256"],
            )
            plugin_downloads = [executor.submit(fetch_plugin, plugin) for plugin in jenkins_lock["plugins"]]
            controller_download.result()
            for plugin_download in plugin_downloads:
                plugin_download.result()
        verify_jenkins_artifacts(jenkins_lock, controller, staged_plugins)
        controller_directory = home / "controller"
        controller_directory.mkdir(exist_ok=True)
        plugin_directory.mkdir(exist_ok=True)
        for source, destination in [(controller, controller_directory / "jenkins.war")]:
            temporary_destination = destination.with_suffix(destination.suffix + ".new")
            shutil.copyfile(source, temporary_destination)
            os.chmod(temporary_destination, 0o644)
            os.replace(temporary_destination, destination)
        for plugin in jenkins_lock["plugins"]:
            destination = plugin_directory / f"{plugin['id']}.jpi"
            temporary_destination = destination.with_suffix(".jpi.new")
            shutil.copyfile(staged_plugins / destination.name, temporary_destination)
            os.chmod(temporary_destination, 0o644)
            os.replace(temporary_destination, destination)
            (plugin_directory / f"{plugin['id']}.jpi.pinned").touch(mode=0o644)
    verify_jenkins_home(jenkins_lock, home)


def length_prefixed(digest: hashlib._Hash, value: bytes) -> None:
    digest.update(len(value).to_bytes(8, "big"))
    digest.update(value)


def tree_identity(directory: Path) -> dict[str, int | str]:
    require(directory.is_dir() and not directory.is_symlink(), f"tree is missing or unsafe: {directory}")
    files = sorted(path for path in directory.rglob("*") if path.is_file() or path.is_symlink())
    require(len(files) <= MAX_TREE_FILES, "offline vendor tree has too many files")
    digest = hashlib.sha256(b"mainframe-env.offline-tree@1\0")
    total = 0
    for path in files:
        metadata = path.lstat()
        require(stat.S_ISREG(metadata.st_mode), f"offline vendor tree contains a non-regular file: {path}")
        total += metadata.st_size
        require(total <= MAX_TREE_BYTES, "offline vendor tree is too large")
        relative = path.relative_to(directory).as_posix()
        safe_relative(relative, "offline vendor path")
        length_prefixed(digest, relative.encode("utf-8"))
        digest.update(stat.S_IMODE(metadata.st_mode).to_bytes(4, "big"))
        digest.update(metadata.st_size.to_bytes(8, "big"))
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
    return {"schema_version": "mainframe-env.offline-tree@1", "sha256": digest.hexdigest(), "files": len(files), "bytes": total}


def executable_identity(name: str, arguments: list[str], executable: str | None = None) -> dict[str, str]:
    resolved = Path(executable or shutil.which(arguments[0]) or "").resolve()
    require(resolved.is_file(), f"offline input tool is missing: {name}")
    output = command_output(arguments)
    require(bool(output), f"offline input tool has no version: {name}")
    return {"version": output.splitlines()[0], "sha256": sha256_file(resolved)}


def atomic_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    with tempfile.NamedTemporaryFile(prefix=path.name + ".", dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        output.write(data)
    try:
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    subcommands = value.add_subparsers(dest="command", required=True)
    check = subcommands.add_parser("check")
    check.add_argument("--runtime", choices=["ci", "controller", "offline", "postgres", "release", "all"])
    check.add_argument("--jenkins-home", type=Path)
    check.add_argument("--development-profile")
    check.add_argument("--profile-file", action="append", default=[])
    check.add_argument("--profile-tree", type=Path)
    install = subcommands.add_parser("install-jenkins")
    install.add_argument("--home", required=True, type=Path)
    verify = subcommands.add_parser("verify-jenkins")
    verify.add_argument("--home", required=True, type=Path)
    return value


def main(arguments: list[str] | None = None) -> int:
    args = parser().parse_args(arguments)
    try:
        if args.command == "check":
            require(args.development_profile or not (args.profile_file or args.profile_tree), "profile bindings require explicit development profile selection")
            ci_lock, jenkins_lock = check_repository(ROOT)
            if args.development_profile:
                bindings = profile_bindings(args.profile_file)
                validated = validate_development_inputs(ci_lock, args.development_profile, bindings, args.profile_tree)
                print(f"development-profile: {args.development_profile} lock-sha256={sha256_file(ROOT / CI_LOCK_PATH, MAX_JSON_BYTES)}")
                for role in sorted(validated["files"]):
                    print(f"profile-file: {role}={validated['files'][role]} {json.dumps(validated['identities'][role], sort_keys=True)}")
                print(f"profile-tree: {validated['tree']} {json.dumps(validated['identities']['tree'], sort_keys=True)}")
                print("input admission only; Node release signatures unverified; bubblewrap host bootstrap is qualified, not hermetic/source attestation")
            if args.runtime:
                verify_runtime(ci_lock, args.runtime)
            if args.jenkins_home:
                verify_jenkins_home(jenkins_lock, args.jenkins_home.resolve())
            print(f"supply-chain: pass plugins={len(jenkins_lock['plugins'])} runtime={args.runtime or 'not-requested'}")
        elif args.command == "install-jenkins":
            jenkins_lock = validate_jenkins_lock(ROOT)
            install_jenkins(jenkins_lock, args.home.resolve())
            print(f"installed locked Jenkins controller and {len(jenkins_lock['plugins'])} plugins")
        elif args.command == "verify-jenkins":
            jenkins_lock = validate_jenkins_lock(ROOT)
            verify_jenkins_home(jenkins_lock, args.home.resolve())
            print(f"jenkins-inputs: pass plugins={len(jenkins_lock['plugins'])}")
    except (OSError, KeyError, TypeError, SupplyChainError) as error:
        print(f"supply-chain: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
