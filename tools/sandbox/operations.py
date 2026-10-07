"""Typed agent operations over the existing compiler and authenticated gateway."""
from __future__ import annotations

import base64
import json
from pathlib import Path
import re
import struct
from urllib.parse import quote, urlencode

from .runtime import Runtime, request


def string(maximum=1024, **extra):
    return {"type": "string", "maxLength": maximum, **extra}


def tool(name, description, properties=None, required=(), read_only=False, destructive=False):
    return {"name": name, "description": description,
            "inputSchema": {"type": "object", "properties": properties or {},
                            "required": list(required), "additionalProperties": False},
            "annotations": {"readOnlyHint": read_only, "destructiveHint": destructive,
                            "idempotentHint": read_only, "openWorldHint": False}}


SOURCE = {"path": string(512, minLength=1), "format": string(enum=["fixed", "free"]),
          "libraries": {"type": "array", "items": string(512), "maxItems": 16}}
SESSION = {"session": string(128, minLength=1)}
JOB = {"jobname": string(8, pattern=r"[A-Za-z0-9@$#]{1,8}"),
       "jobid": string(32, pattern=r"[A-Za-z0-9_-]{1,32}")}
TOOLS = [
    tool("sandbox_status", "Read profile capabilities, readiness and active generation.", read_only=True),
    tool("workspace_read", "Read a UTF-8 file inside the editable workspace.", {"path": string(512)}, ["path"], True),
    tool("workspace_write", "Write a UTF-8 source file inside the editable workspace.",
         {"path": string(512), "content": string(1024 * 1024)}, ["path", "content"]),
    tool("cobol_inspect", "Analyze a workspace COBOL source and return structured diagnostics.", SOURCE, ["path"], True),
    tool("cobol_compile", "Compile a workspace source; CICS execution requires deployment.", SOURCE, ["path"], True),
    tool("cobol_run", "Compile and execute a standalone workspace source with the local coordinator.", SOURCE, ["path"]),
    tool("sandbox_generations", "List retained application generations.", read_only=True),
    tool("sandbox_deploy", "Deploy workspace code to fresh seed data; retain the previous generation."),
    tool("sandbox_reset", "Start fresh seed data with currently deployed code; retain the previous generation.", destructive=True),
    tool("sandbox_rollback", "Select a retained generation and its stored data; replaces current terminal sessions.",
         {"generation": string(32, pattern=r"[a-f0-9]{32}")}, ["generation"]),
    tool("terminal_open", "Open CardDemo sign-on with a regular or administration transport identity.",
         {"workspace": string(enum=["regular", "administration"])}),
    tool("terminal_read", "Read named fields, editable field names and labels; secret values are masked.", SESSION, ["session"], True),
    tool("terminal_send", "Submit fields and an AID, then resume CardDemo once. Accepted mutations are not retried.",
         {**SESSION, "aid": string(enum=["ENTER", "CLEAR"] + [f"PF{i}" for i in range(1, 13)]),
          "fields": {"type": "object", "maxProperties": 100, "additionalProperties": string(4096)}}, ["session"]),
    tool("terminal_close", "Disconnect a terminal owned by this sandbox controller.", SESSION, ["session"]),
    tool("jobs_submit", "Submit JCL through JES; application batch programs require a profile that installs them.",
         {"jcl": string(65536, minLength=1)}, ["jcl"]),
    tool("jobs_status", "Read an admitted job's status.", JOB, ["jobname", "jobid"], True),
    tool("jobs_spool", "List job spool files, or read one file's records.",
         {**JOB, "file_id": string(32, pattern=r"[A-Za-z0-9_-]{1,32}")}, ["jobname", "jobid"], True),
    tool("datasets_list", "List datasets using the regular CardDemo identity and its existing RACF permissions.",
         {"pattern": string(128)}, read_only=True),
    tool("datasets_read", "Read a dataset under existing RACF permissions; returns bytes as base64.",
         {"dsn": string(128, pattern=r"[A-Za-z0-9@$#.-]{1,128}")}, ["dsn"], True),
]
COMMON_TOOLS = {"sandbox_status", "workspace_read", "workspace_write", "cobol_inspect",
                "cobol_compile", "cobol_run", "sandbox_generations"}


def validate(value, schema, label="arguments"):
    kind = schema.get("type")
    expected = {"object": dict, "string": str, "array": list}.get(kind)
    if expected and not isinstance(value, expected):
        raise ValueError(f"{label} must be {kind}")
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"invalid {label}")
    if isinstance(value, str):
        if len(value) > schema.get("maxLength", 1024 * 1024) or len(value) < schema.get("minLength", 0):
            raise ValueError(f"{label} exceeds its string bounds")
        if "pattern" in schema and not re.fullmatch(schema["pattern"], value):
            raise ValueError(f"invalid {label}")
    if isinstance(value, list):
        if len(value) > schema.get("maxItems", 100):
            raise ValueError(f"too many {label}")
        for item in value:
            validate(item, schema["items"], label)
    if isinstance(value, dict):
        if len(value) > schema.get("maxProperties", 100):
            raise ValueError(f"too many {label}")
        if set(schema.get("required", [])) - value.keys():
            raise ValueError(f"missing required {label}")
        properties = schema.get("properties", {})
        for key, item in value.items():
            if len(key) > 128:
                raise ValueError("object key exceeds byte limit")
            if key in properties:
                validate(item, properties[key], key)
            elif schema.get("additionalProperties") is False:
                raise ValueError(f"unknown argument: {key}")
            elif isinstance(schema.get("additionalProperties"), dict):
                validate(item, schema["additionalProperties"], key)


def decode_fields(encoded: str) -> dict:
    data = base64.b64decode(encoded, validate=True)
    cursor = 0
    fields = {}
    while cursor < len(data):
        parts = []
        for _ in range(2):
            if cursor + 4 > len(data):
                raise ValueError("truncated terminal field length")
            length = struct.unpack_from(">I", data, cursor)[0]
            cursor += 4
            if length > 65536 or cursor + length > len(data):
                raise ValueError("truncated or oversized terminal field")
            parts.append(data[cursor:cursor + length].decode("utf-8", "replace"))
            cursor += length
        if len(fields) >= 512 or parts[0] in fields:
            raise ValueError("invalid terminal fields")
        fields[parts[0]] = parts[1].rstrip("\x00 ")
    return fields


class Operations:
    def __init__(self, runtime: Runtime):
        self.runtime = runtime

    def tools(self):
        return [t for t in TOOLS if self.runtime.instance.state["profile"] == "carddemo-online"
                or t["name"] in COMMON_TOOLS]

    def call(self, name: str, args: dict) -> dict:
        definition = next((t for t in self.tools() if t["name"] == name), None)
        if definition is None:
            raise ValueError("operation is unavailable in this profile")
        validate(args, definition["inputSchema"])
        if name.startswith("cobol_"):
            return self.runtime.compile(name.removeprefix("cobol_"), args)
        with self.runtime.lock:
            return self._call(name, args)

    def _call(self, name, args):
        instance = self.runtime.instance
        if name == "sandbox_status":
            return {"profile": instance.state["profile"], "ready": self.runtime.ready(),
                    "generation": instance.state["active"], "workspace": str(instance.workspace),
                    "isolation": "application state and child processes; OS isolation supplied by deployment",
                    "deployment": "fresh seed data with retained prior generations",
                    "tools": [t["name"] for t in self.tools()],
                    "application": {"programs": 18, "transactions": 17, "maps": 17} if instance.state["profile"] == "carddemo-online" else None}
        if name == "workspace_read":
            path = instance.source_path(args["path"])
            if path.stat().st_size > 1024 * 1024:
                raise ValueError("source read exceeds byte limit")
            return {"path": args["path"], "content": path.read_text(encoding="utf-8")}
        if name == "workspace_write":
            path = instance.source_path(args["path"])
            data = args["content"].encode("utf-8")
            if len(data) > 1024 * 1024:
                raise ValueError("source write exceeds byte limit")
            path.parent.mkdir(parents=True, exist_ok=True)
            # Replace the workspace file atomically; application snapshots remain immutable.
            import os
            import tempfile
            descriptor, temporary = tempfile.mkstemp(dir=path.parent)
            try:
                with os.fdopen(descriptor, "wb") as stream:
                    stream.write(data)
                os.replace(temporary, path)
            finally:
                Path(temporary).unlink(missing_ok=True)
            return {"path": args["path"], "bytes": len(data)}
        if name == "sandbox_generations":
            return {"generations": [{"generation": p.name, "active": p.name == instance.state["active"]}
                                    for p in sorted(instance.generations.iterdir()) if (p / "generation.json").is_file()]}
        if name in ("sandbox_deploy", "sandbox_reset"):
            return self.runtime.deploy(reset=name == "sandbox_reset")
        if name == "sandbox_rollback":
            return self.runtime.activate(args["generation"])
        if name == "terminal_open":
            if len(self.runtime.sessions) >= 128:
                raise ValueError("controller terminal limit reached; close unused terminals")
            administrator = args.get("workspace") == "administration"
            identity = "WEBADM:admin-transport-password" if administrator else "WEBUSER:transport-password"
            result = self.gateway("POST", "/mainframe-env/cics/v1/sessions",
                                  {"transaction": "CC00", "rows": 24, "columns": 80}, identity)
            session = result["session"]
            self.runtime.sessions[session] = (identity, result["csrf_token"])
            return self.terminal(session)
        if name in ("terminal_read", "terminal_send", "terminal_close"):
            session = args["session"]
            identity, csrf = self.session(session)
            path = "/mainframe-env/cics/v1/sessions/" + quote(session, safe="")
            if name == "terminal_send":
                current = self.terminal(session, mask=False)
                fields = {key: current["fields"].get(key, "") for key in current["editable"]}
                if set(args.get("fields", {})) - fields.keys():
                    raise ValueError("input names include an unknown or protected field")
                fields.update(args.get("fields", {}))
                aids = {"ENTER": 125, "CLEAR": 109, **{f"PF{i}": 240+i for i in range(1, 10)}, "PF10": 122, "PF11": 123, "PF12": 124}
                self.gateway("PUT", path + "/input", {"aid": aids[args.get("aid", "ENTER")], "fields": fields}, identity, csrf)
                self.gateway("POST", path + "/resume", None, identity, csrf)
            if name == "terminal_close":
                self.gateway("DELETE", path, None, identity, csrf)
                del self.runtime.sessions[session]
                return {"closed": True}
            return self.terminal(session)
        if name == "jobs_submit":
            return self.gateway("PUT", "/zosmf/restjobs/jobs", args["jcl"].encode(), "IBMUSER:TESTPASS")
        if name.startswith("jobs_"):
            path = "/zosmf/restjobs/jobs/" + quote(args["jobname"], safe="") + "/" + quote(args["jobid"], safe="")
            if name == "jobs_spool":
                path += "/files"
                if "file_id" in args:
                    path += "/" + quote(args["file_id"], safe="") + "/records"
            result = self.gateway("GET", path, identity="IBMUSER:TESTPASS")
            return result if isinstance(result, dict) else {"result": result}
        if name == "datasets_list":
            result = self.gateway("GET", "/zosmf/restfiles/ds?" + urlencode({"dslevel": args.get("pattern", "**")}), identity="WEBUSER:transport-password")
            return result if isinstance(result, dict) else {"result": result}
        if name == "datasets_read":
            payload = self.gateway("GET", "/zosmf/restfiles/ds/" + quote(args["dsn"], safe=""), identity="WEBUSER:transport-password", binary=True)
            return {"dsn": args["dsn"], "data_base64": base64.b64encode(payload).decode()}
        raise ValueError("unknown operation")

    def session(self, session):
        try:
            return self.runtime.sessions[session]
        except KeyError as error:
            raise ValueError("terminal is not owned by this controller; open a new terminal") from error

    def gateway(self, method, path, body=None, identity="WEBUSER:transport-password", csrf=None, binary=False):
        headers = {"Authorization": "Basic " + base64.b64encode(identity.encode()).decode(),
                   "X-CSRF-ZOSMF-HEADER": "true"}
        if csrf:
            headers["X-CSRF-TOKEN"] = csrf
        if isinstance(body, dict):
            body = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        status, response_headers, payload = request(self.runtime.port, method, path, body, headers, timeout=10)
        if status >= 400:
            raise ValueError(f"gateway rejected operation ({status}): {payload[:2048].decode('utf-8', 'replace')}")
        if binary:
            return payload
        if not payload:
            return {}
        try:
            return json.loads(payload)
        except ValueError:
            return {"text": payload.decode("utf-8", "replace")}

    def terminal(self, session, mask=True):
        identity, csrf = self.session(session)
        path = "/mainframe-env/cics/v1/sessions/" + quote(session, safe="")
        terminal = self.gateway("GET", path, identity=identity)
        fields = decode_fields(terminal["screen_base64"])
        layouts = self.gateway("GET", "/carddemo/layouts", identity=identity)
        layout = layouts.get(terminal.get("mapset"), {})
        record = self.gateway("GET", path + "/tn3270", identity=identity, binary=True)
        protection = {}
        for at in range(max(0, len(record) - 4)):
            if record[at] == 0x11 and record[at + 3] == 0x1d:
                protection[((record[at + 1] & 0x3f) << 8) | record[at + 2]] = bool(record[at + 4] & 0x20)
        editable, labels = [], []
        for field in layout.get("fields", []):
            if not field.get("position") or not field.get("length"):
                continue
            row, column = field["position"]
            address = (row - 1) * terminal["columns"] + column - 1
            name = field.get("name")
            if name and not protection.get(address, field["protected"]):
                editable.append(name)
            if name and field.get("secret") and mask and name in fields:
                fields[name] = "[masked]" if fields[name] else ""
            if not name and field.get("initial"):
                labels.append(field["initial"])
        return {"session": session, "mapset": terminal.get("mapset"), "map": terminal.get("map"),
                "connected": terminal["connected"], "fields": fields, "editable": editable, "labels": labels}
