"""Bounded MCP stdio adapter for the published 2025 handshake protocol family."""
from __future__ import annotations

import json
import sys
import threading

VERSIONS = ("2025-11-25", "2025-06-18")
MAX_MESSAGE = 1024 * 1024


def serve(client):
    output_lock = threading.Lock()
    pending_lock = threading.Lock()
    slots = threading.BoundedSemaphore(4)
    pending = {}
    initialized = False
    ready = False

    def emit(value):
        with output_lock:
            try:
                sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
                sys.stdout.flush()
            except (BrokenPipeError, OSError):
                pass

    def error(identity, code, message):
        emit({"jsonrpc": "2.0", "id": identity, "error": {"code": code, "message": message}})

    def execute(message, cancellation):
        identity = message["id"]
        try:
            params = message.get("params", {})
            if message["method"] == "tools/list":
                if params.get("cursor"):
                    raise ValueError("invalid pagination cursor")
                result = client.request("/sandbox/v1/tools")
            elif message["method"] == "tools/call":
                if not isinstance(params.get("name"), str) or not isinstance(params.get("arguments", {}), dict):
                    raise ValueError("tool call requires a name and object arguments")
                available = client.request("/sandbox/v1/tools")["tools"]
                if params["name"] not in {tool["name"] for tool in available}:
                    raise ValueError("unknown or unavailable tool")
                try:
                    data = client.call(params["name"], params.get("arguments", {}))
                    result = {"content": [{"type": "text", "text": json.dumps(data)}], "structuredContent": data}
                except (ValueError, OSError) as problem:
                    result = {"isError": True, "content": [{"type": "text", "text": str(problem)}]}
            else:
                if not cancellation.is_set():
                    error(identity, -32601, "method not found")
                return
            if not cancellation.is_set():
                emit({"jsonrpc": "2.0", "id": identity, "result": result})
        except (ValueError, OSError, KeyError, TypeError) as problem:
            if not cancellation.is_set():
                error(identity, -32602, str(problem))
        finally:
            with pending_lock:
                pending.pop(identity, None)
            slots.release()

    try:
        while True:
            line = sys.stdin.buffer.readline(MAX_MESSAGE + 1)
            if not line:
                break
            if len(line) > MAX_MESSAGE or not line.endswith(b"\n"):
                error(None, -32700, "message exceeds framing or byte limits")
                break
            try:
                message = json.loads(line)
            except (ValueError, UnicodeError):
                error(None, -32700, "invalid JSON")
                continue
            if not isinstance(message, dict) or message.get("jsonrpc") != "2.0" or not isinstance(message.get("method"), str):
                error(None, -32600, "invalid JSON-RPC request")
                continue
            method = message["method"]
            params = message.get("params", {})
            if not isinstance(params, dict):
                if "id" in message:
                    error(message["id"], -32602, "params must be an object")
                continue
            if "id" not in message:
                if method == "notifications/initialized" and initialized:
                    ready = True
                elif method == "notifications/cancelled":
                    identity = params.get("requestId")
                    if isinstance(identity, (str, int)) and not isinstance(identity, bool):
                        with pending_lock:
                            event = pending.get(identity)
                            if event:
                                event.set()
                continue
            identity = message["id"]
            if not isinstance(identity, (str, int)) or isinstance(identity, bool):
                error(None, -32600, "invalid request identity")
            elif method == "initialize":
                if initialized:
                    error(identity, -32600, "already initialized")
                    continue
                if not isinstance(params.get("protocolVersion"), str) or not isinstance(params.get("capabilities"), dict) or not isinstance(params.get("clientInfo"), dict):
                    error(identity, -32602, "invalid initialization parameters")
                    continue
                initialized = True
                requested = params["protocolVersion"]
                emit({"jsonrpc": "2.0", "id": identity, "result": {
                    "protocolVersion": requested if requested in VERSIONS else VERSIONS[0],
                    "capabilities": {"tools": {"listChanged": False}},
                    "serverInfo": {"name": "mainframe-sandbox", "version": "source"},
                    "instructions": "Read sandbox_status first. Deployment starts fresh seed data; restart retains data."
                }})
            elif method == "ping":
                emit({"jsonrpc": "2.0", "id": identity, "result": {}})
            elif method not in ("tools/list", "tools/call"):
                error(identity, -32601, "method not found")
            elif not ready:
                error(identity, -32600, "initialize the server first")
            else:
                with pending_lock:
                    if identity in pending:
                        error(identity, -32600, "duplicate in-flight request identity")
                        continue
                    if not slots.acquire(blocking=False):
                        error(identity, -32000, "request concurrency limit reached")
                        continue
                    cancellation = threading.Event()
                    pending[identity] = cancellation
                threading.Thread(target=execute, args=(message, cancellation), daemon=True).start()
    finally:
        with pending_lock:
            for event in pending.values():
                event.set()

# MCP cancellation suppresses responses. Already admitted controller mutations can
# complete; cancellation and disconnect never pretend to undo their effects.
