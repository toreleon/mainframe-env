"""Authenticated local controller client shared by the CLI and MCP adapter."""
from __future__ import annotations

import json
from pathlib import Path
import urllib.error
from urllib.parse import urlparse
import urllib.request

from .instance import read_json


class NoControllerRedirects(urllib.request.HTTPRedirectHandler):
    """Keep controller credentials on the validated loopback endpoint."""

    def redirect_request(self, request, response, code, message, headers, target):
        raise ValueError("controller redirects are forbidden")


class Client:
    def __init__(self, instance: Path):
        self.connection = read_json(instance / "connection.json")
        url = urlparse(self.connection.get("url", ""))
        if (url.scheme != "http" or url.hostname != "127.0.0.1" or url.path or not url.port
                or url.username or url.password or url.query or url.fragment):
            raise ValueError("connection metadata must identify a local controller")
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), NoControllerRedirects())

    def request(self, path, value=None):
        payload = json.dumps(value).encode() if value is not None else None
        request = urllib.request.Request(self.connection["url"] + path, payload,
                    {"Authorization": "Bearer " + self.connection["token"], "Content-Type": "application/json"})
        try:
            response = self.opener.open(request, timeout=90)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            data = response.read(2 * 1024 * 1024 + 1)
            if len(data) > 2 * 1024 * 1024:
                raise ValueError("controller response exceeds byte limit")
            result = json.loads(data)
        if not isinstance(result, dict):
            raise ValueError("controller response must be an object")
        if result.get("ok") is False:
            raise ValueError(result.get("error", "controller rejected operation"))
        return result

    def call(self, name, arguments):
        return self.request("/sandbox/v1/call", {"name": name, "arguments": arguments})["result"]
