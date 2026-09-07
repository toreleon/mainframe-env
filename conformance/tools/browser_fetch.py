#!/usr/bin/env python3
"""Fetch IBM Documentation through a real Chrome over the DevTools protocol.

`www.ibm.com/docs` rejects scripted clients at the edge, so the publication
probes cannot reach the current editions with an HTTP client. A real browser
passes. This driver attaches to a Chrome already listening on a debugging port,
navigates once to establish the origin, and then issues same-origin `fetch`
calls from inside the page so downloads reuse the browser's own session.

Nothing here is written to the repository: callers pass an output path outside
the tree. Requires websocket-client.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import time
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Iterable

ORIGIN = "https://www.ibm.com/docs/en/"


class Tab:
    """One CDP target with a synchronous request/response loop."""

    def __init__(self, endpoint: str, timeout: float = 120.0) -> None:
        # Imported here rather than at module scope: this driver is the fallback
        # retrieval path now that the documentation API answers a plain HTTP
        # client, and the unit tests of the tools that import this module should
        # not need websocket-client installed to run.
        import websocket

        self._socket = websocket.create_connection(endpoint, timeout=timeout)
        self._id = 0

    def call(self, method: str, **params: Any) -> dict[str, Any]:
        self._id += 1
        self._socket.send(json.dumps({"id": self._id, "method": method, "params": params}))
        while True:
            message = json.loads(self._socket.recv())
            if message.get("id") == self._id:
                if "error" in message:
                    raise RuntimeError(f"{method}: {message['error']}")
                return message.get("result", {})

    def evaluate(self, expression: str, timeout: float = 120.0) -> Any:
        result = self.call(
            "Runtime.evaluate",
            expression=expression,
            awaitPromise=True,
            returnByValue=True,
            timeout=int(timeout * 1000),
        )
        if result.get("exceptionDetails"):
            raise RuntimeError(result["exceptionDetails"].get("text", "evaluation failed"))
        return result["result"].get("value")

    def close(self) -> None:
        self._socket.close()


def open_tab(port: int, url: str = ORIGIN) -> Tab:
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/json/new?{urllib.parse.quote(url, safe='')}",
        method="PUT",
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        target = json.loads(response.read())
    tab = Tab(target["webSocketDebuggerUrl"])
    tab.call("Page.enable")
    tab.call("Runtime.enable")
    return tab


def settle(tab: Tab, attempts: int = 40, delay: float = 0.5) -> None:
    for _ in range(attempts):
        if tab.evaluate("document.readyState") == "complete":
            return
        time.sleep(delay)


FETCH = """
(async () => {
  const response = await fetch(%s, {credentials: 'include'});
  if (!response.ok) return {status: response.status, body: null};
  const buffer = await response.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  let binary = '';
  const step = 0x8000;
  for (let index = 0; index < bytes.length; index += step) {
    binary += String.fromCharCode.apply(null, bytes.subarray(index, index + step));
  }
  return {status: response.status, body: btoa(binary)};
})()
"""


def establish(tab: Tab) -> None:
    """Load the documentation origin so later `fetch` calls are same-origin."""
    settle(tab)
    tab.call("Page.navigate", url=ORIGIN)
    settle(tab)
    time.sleep(2)


def fetch_binary(tab: Tab, url: str) -> tuple[int, bytes | None]:
    result = tab.evaluate(FETCH % json.dumps(url))
    if not result:
        return 0, None
    body = result.get("body")
    return int(result.get("status") or 0), base64.b64decode(body) if body else None


def fetch_dom(tab: Tab, url: str) -> str:
    tab.call("Page.navigate", url=url)
    settle(tab)
    time.sleep(3)
    return tab.evaluate("document.documentElement.outerHTML") or ""


def parse_args(argv: Iterable[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=9222)
    parser.add_argument("--url", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=("dom", "binary"), default="binary")
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    args = parse_args(argv)
    tab = open_tab(args.port)
    try:
        establish(tab)
        if args.mode == "dom":
            payload = fetch_dom(tab, args.url)
            args.output.write_text(payload, encoding="utf-8")
            print(f"status=200 bytes={len(payload)} {args.output}")
            return 0
        status, data = fetch_binary(tab, args.url)
        if data is None:
            print(f"status={status or 'none'} no body")
            return 1
        args.output.write_bytes(data)
        print(
            f"status={status} bytes={len(data)} "
            f"sha256:{hashlib.sha256(data).hexdigest()} {args.output}"
        )
        return 0
    finally:
        tab.close()


if __name__ == "__main__":
    raise SystemExit(main())
