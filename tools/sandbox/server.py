"""Loopback-first HTTP composition and authenticated controller operations."""
from __future__ import annotations

import fcntl
import hmac
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import secrets
import signal
import threading

from .instance import write_json
from .operations import Operations
from .runtime import request

MAX_BODY = 1024 * 1024
HOP_HEADERS = {"connection", "transfer-encoding", "keep-alive", "proxy-authenticate",
               "proxy-authorization", "te", "trailer", "upgrade", "content-length", "host"}


class Server(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, address, runtime, token):
        self.runtime = runtime
        self.operations = Operations(runtime)
        self.token = token
        self.slots = threading.BoundedSemaphore(16)
        super().__init__(address, Handler)

    def process_request(self, request_socket, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request_socket)
            return
        request_socket.settimeout(15)
        try:
            super().process_request(request_socket, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request_socket, client_address):
        try:
            super().process_request_thread(request_socket, client_address)
        finally:
            self.slots.release()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        # Request bodies, tokens and mainframe credentials are never logged.
        pass

    def do_GET(self):
        self.handle_request()

    do_POST = do_GET
    do_PUT = do_GET
    do_PATCH = do_GET
    do_DELETE = do_GET
    do_HEAD = do_GET

    def respond(self, status, payload, headers=None):
        self.send_response(status)
        for key, value in (headers or {}).items():
            if key.lower() not in HOP_HEADERS:
                self.send_header(key, value)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)

    def json_response(self, status, value):
        self.respond(status, json.dumps(value).encode(), {"Content-Type": "application/json", "Cache-Control": "no-store"})

    def read_body(self):
        if self.headers.get("Transfer-Encoding"):
            raise ValueError("chunked request bodies are unsupported")
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 <= length <= MAX_BODY:
            raise ValueError("request body exceeds byte limit")
        payload = self.rfile.read(length)
        if len(payload) != length:
            raise ValueError("incomplete request body")
        return payload

    def handle_request(self):
        try:
            if self.server.server_address[0] == "127.0.0.1":
                host = self.headers.get("Host", "").split(":", 1)[0]
                if host not in ("localhost", "127.0.0.1"):
                    self.json_response(403, {"error": "invalid loopback Host header"})
                    return
            if self.path == "/readyz" and self.command in ("GET", "HEAD"):
                ready = self.server.runtime.ready()
                self.json_response(200 if ready else 503, {"ready": ready, "profile": self.server.runtime.instance.state["profile"]})
                return
            if self.path.startswith("/sandbox/"):
                authorization = self.headers.get("Authorization", "")
                if not hmac.compare_digest(authorization, "Bearer " + self.server.token):
                    self.json_response(401, {"ok": False, "error": "sandbox controller authentication required"})
                    return
                if self.headers.get("Origin"):
                    self.json_response(403, {"ok": False, "error": "browser origins cannot invoke controller operations"})
                    return
                if self.path == "/sandbox/v1/tools" and self.command == "GET":
                    self.json_response(200, {"tools": self.server.operations.tools()})
                elif self.path == "/sandbox/v1/call" and self.command == "POST":
                    value = json.loads(self.read_body())
                    if not isinstance(value, dict) or set(value) != {"name", "arguments"}:
                        raise ValueError("call requires name and arguments")
                    if not isinstance(value["name"], str):
                        raise ValueError("operation name must be a string")
                    result = self.server.operations.call(value["name"], value["arguments"])
                    self.json_response(200, {"ok": True, "result": result})
                elif self.path == "/sandbox/v1/stop" and self.command == "POST":
                    self.json_response(200, {"ok": True, "stopping": True})
                    threading.Thread(target=self.server.shutdown, daemon=True).start()
                elif self.path == "/sandbox/v1/rekey" and self.command == "POST":
                    value = json.loads(self.read_body())
                    token = value.get("token") if isinstance(value, dict) else None
                    import re
                    if not isinstance(token, str) or not re.fullmatch(r"[A-Za-z0-9_-]{32,128}", token):
                        raise ValueError("invalid controller token")
                    with self.server.runtime.lock:
                        from .instance import read_json
                        path = self.server.runtime.instance.path / "connection.json"
                        connection = read_json(path)
                        write_json(path, {**connection, "token": token})
                        self.server.token = token
                    self.json_response(200, {"rekeyed": True})
                else:
                    self.json_response(404, {"ok": False, "error": "unknown controller route"})
                return
            if self.server.runtime.instance.state["profile"] == "cobol":
                self.json_response(404, {"error": "this profile has no browser application"})
                return
            body = self.read_body() if self.command in ("POST", "PUT", "PATCH", "DELETE") else None
            headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP_HEADERS}
            with self.server.runtime.lock:
                status, response_headers, payload = request(self.server.runtime.port, self.command, self.path, body, headers, timeout=10)
            self.respond(status, payload, response_headers)
        except (ValueError, OSError, KeyError, TypeError, http.client.HTTPException) as error:
            self.json_response(400, {"ok": False, "error": str(error)})


def serve(runtime, listen: str, allow_network: bool = False):
    host, port = listen.rsplit(":", 1)
    if host not in ("127.0.0.1", "localhost") and not allow_network:
        raise ValueError("network listener requires --allow-network-listener and protected deployment ingress")
    lock_path = runtime.instance.path / ".controller.lock"
    descriptor = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    connection = runtime.instance.path / "connection.json"
    server = None
    acquired = False
    handlers = {}
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            acquired = True
        except BlockingIOError as error:
            raise ValueError("an instance controller is already running") from error
        def stop(signum, frame):
            runtime.closing.set()
            if server is not None:
                threading.Thread(target=server.shutdown, daemon=True).start()
        for signum in (signal.SIGINT, signal.SIGTERM):
            handlers[signum] = signal.signal(signum, stop)
        runtime.start()
        if runtime.closing.is_set():
            return
        token = secrets.token_urlsafe(32)
        server = Server((host, int(port)), runtime, token)
        port = server.server_address[1]
        url = f"http://127.0.0.1:{port}"
        write_json(connection, {"url": url, "token": token, "pid": os.getpid()})
        import sys
        print(json.dumps({"url": url, "instance": str(runtime.instance.path), "profile": runtime.instance.state["profile"]}), file=sys.stderr, flush=True)
        server.serve_forever(poll_interval=0.1)
    finally:
        for signum, handler in handlers.items():
            signal.signal(signum, handler)
        if server is not None:
            server.server_close()
        runtime.close()
        if acquired:
            connection.unlink(missing_ok=True)
        os.close(descriptor)
