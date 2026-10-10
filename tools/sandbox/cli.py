"""Executable lifecycle and agent interface for Mainframe Sandbox."""
from __future__ import annotations

import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from .client import Client
from .instance import Instance, read_json, SCHEMA
from .runtime import Runtime

BUNDLE = Path(__file__).resolve().parents[2]


def parser():
    value = argparse.ArgumentParser(prog="mainframe-sandbox", description="Run mainframe applications in independent agent workspaces")
    commands = value.add_subparsers(dest="command", required=True)
    setup = commands.add_parser("setup", help="Build a relocatable runtime bundle")
    setup.add_argument("--destination", type=Path, required=True)
    setup.add_argument("--corpus", type=Path)
    setup.add_argument("--no-build", action="store_true", help="Use binaries already built in target/release")
    setup.add_argument("--target-directory", type=Path,
                       help="Fresh build directory below this checkout's target/; removed after retention")
    setup.add_argument("--producer-evidence", type=Path,
                       help="External directory for actual setup logs, artifacts and setup-receipt.json; requires a fresh target")
    serve = commands.add_parser("serve", help="Run one instance in the foreground")
    serve.add_argument("--instance", type=Path, required=True)
    serve.add_argument("--profile", choices=["cobol", "carddemo-online"], default="carddemo-online")
    serve.add_argument("--bundle", type=Path, default=BUNDLE)
    serve.add_argument("--reference", type=Path)
    serve.add_argument("--listen", default="127.0.0.1:8080")
    serve.add_argument("--allow-network-listener", action="store_true")
    verify = commands.add_parser("verify", help="Exercise real CardDemo through independent instances")
    verify.add_argument("--bundle", type=Path, default=BUNDLE)
    verify.add_argument("--directory", type=Path, required=True)
    rekey = commands.add_parser("rekey", help="Rotate controller credentials after a VM clone or restore")
    rekey.add_argument("--instance", type=Path, required=True)
    rekey.add_argument("--token-file", type=Path, required=True, help="Host-provisioned random token; never printed")
    for name in ("status", "deploy", "reset", "generations", "stop", "destroy", "mcp", "agent-config"):
        item = commands.add_parser(name)
        item.add_argument("--instance", type=Path, required=True)
    rollback = commands.add_parser("rollback")
    rollback.add_argument("generation")
    rollback.add_argument("--instance", type=Path, required=True)
    call = commands.add_parser("call", help="Invoke an operation with JSON arguments")
    call.add_argument("operation")
    call.add_argument("--arguments", default="{}")
    call.add_argument("--instance", type=Path, required=True)
    return value


def run(args):
    if args.command == "verify":
        from .verify import verify
        return verify(args.bundle.resolve(), args.directory.resolve())
    if args.command == "setup":
        from .setup import setup
        return setup(args.destination, args.corpus, args.no_build, args.target_directory, args.producer_evidence)
    if args.command == "serve":
        from .server import serve
        bundle = args.bundle.resolve()
        reference = args.reference or bundle / "reference/carddemo"
        instance = Instance(args.instance, args.profile, reference)
        runtime = Runtime(instance, bundle / "bin/mainframe-env", bundle / "bin/mainframe-sandbox-runtime",
                          reference if args.profile == "carddemo-online" else None, bundle / "share/carddemo-corpus.json")
        serve(runtime, args.listen, args.allow_network_listener)
        return None
    if args.command == "destroy":
        path = args.instance.resolve()
        if path in (Path("/"), Path.home().resolve(), BUNDLE):
            raise ValueError("refusing to destroy a broad or source directory")
        if args.instance.is_symlink() or read_json(path / "instance.json").get("schema_version") != SCHEMA:
            raise ValueError("refusing to destroy an unowned instance")
        descriptor = os.open(path / ".controller.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            import uuid
            tombstone = path.with_name(f".{path.name}.destroy-{uuid.uuid4().hex}")
            path.rename(tombstone)
            shutil.rmtree(tombstone)
        except BlockingIOError as error:
            raise ValueError("stop the instance before destroying it") from error
        finally:
            os.close(descriptor)
        return {"destroyed": True}
    if args.command == "agent-config":
        return {"mcpServers": {"mainframeSandbox": {"command": str(BUNDLE / "bin/mainframe-sandbox"),
                "args": ["mcp", "--instance", str(args.instance.resolve())]}}}
    client = Client(args.instance.resolve())
    if args.command == "mcp":
        from .mcp import serve
        serve(client)
        return None
    if args.command == "call":
        return client.call(args.operation, json.loads(args.arguments))
    if args.command == "stop":
        return client.request("/sandbox/v1/stop", {})
    if args.command == "rekey":
        if args.token_file.stat().st_size > 128:
            raise ValueError("controller token file exceeds byte limit")
        return client.request("/sandbox/v1/rekey", {"token": args.token_file.read_text(encoding="ascii").strip()})
    if args.command == "rollback":
        return client.call("sandbox_rollback", {"generation": args.generation})
    return client.call("sandbox_" + args.command, {})


def main():
    args = parser().parse_args()
    try:
        result = run(args)
        if result is not None:
            print(json.dumps(result))
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        # MCP stdout is reserved for JSON-RPC, including startup failures.
        stream = sys.stderr if args.command in ("serve", "mcp") else sys.stdout
        print(json.dumps({"ok": False, "error": str(error)}), file=stream)
        sys.exit(1)
