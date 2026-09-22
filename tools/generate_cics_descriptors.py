#!/usr/bin/env python3
"""Generate CICS identities, source-bound contracts, and registry descriptors."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CATALOG_PATH = Path("conformance/0.9/cics/command-descriptors.json")
LEGACY_EXECUTION_CATALOG_PATH = Path(
    "conformance/0.9/cics/legacy-execution-options.json"
)
TYPED_EXECUTION_REGISTRATIONS_PATH = Path(
    "conformance/0.9/cics/typed-execution-registrations.json"
)
OUTPUT_PATH = Path("crates/providers/mainframe-env-cics/src/generated/command_descriptors.rs")
HOST_OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-host-api/src/generated/cics_application_commands.rs"
)
IR_REGISTRY_OUTPUT_PATH = Path(
    "crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs"
)
CONTRACT_OUTPUT_PATH = Path(
    "conformance/0.9/generated/cics-application-command-contracts.json"
)
COMPILER_SPI_COMPAT_OUTPUT_PATH = Path(
    "crates/kernel/mainframe-env-compiler/src/hir/typed/"
    "generated_cics_spi_compatibility.rs"
)
SCHEMA_VERSION = "mainframe-env.cics-command-descriptors@2"
CONTRACT_SCHEMA_VERSION = "mainframe-env.cics-application-command-contracts@2"
OFFICIAL_SCHEMA_VERSION = "mainframe-env.official-catalog@1"
OFFICIAL_BASELINE = "ibm-cics-ts-6x-2026-08-31"
OFFICIAL_CATALOG_DIGEST = (
    "sha256:fccd2a8e5cc24dd08aeb32754daf14ed80e9f1b20b5d9e762a1b0cfe429ceeba"
)
APPLICATION_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-identities@2\0"
CONTRACT_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-contracts@2\0"
ROW_CONTRACT_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-row@1\0"
REGISTRY_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-registry@1\0"
HANDLER_DIGEST_DOMAIN = b"mainframe-env.cics-application-command-handler@1\0"
SOURCE_REVIEW_DIGEST_DOMAIN = b"mainframe-env.cics-source-review@2\0"
CONDITION_NAME_DOMAIN = b"mainframe-env.cics-condition-name-authority@1\0"
CONDITION_NAME_PROFILE = "cics-eibresp-condition-name@1"
AID_NAMES = tuple(
    sorted(
        ["ANYKEY", "CLEAR", "CLRPARTN", "ENTER", "LIGHTPEN", "OPERID", "TRIGGER"]
        + [f"PA{number}" for number in range(1, 4)]
        + [f"PF{number}" for number in range(1, 25)]
    )
)
CONDITION_NAME_OPTION = "CONDITION-NAME"
IDENTIFIER = re.compile(r"^[A-Z][A-Za-z0-9]*$")
OPTION_IDENTIFIER = re.compile(r"^[A-Z][A-Z0-9-]*$")
FAMILY_ID = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
EIBFN = re.compile(r"^[0-9A-F]{4}$")
EXPECTED_UNITS = {
    "api-commands": (263, "dfha8mf__eibfn_table_cmds_api", "API"),
    "spi-commands-unique": (269, "dfha8mf__eibfn_table_cmds_spi", "SPI"),
    "fepi-commands": (39, "dfha8mf__eibfn_table_cmds_fepi", "FEPI"),
}
API_LOCATOR_FAMILY_EXCEPTIONS = {
    # The pinned application table itself labels this one locator as SPI. The
    # unit membership remains the denominator authority; preserve the source
    # bytes rather than silently rewriting the publication anomaly.
    f"{OFFICIAL_BASELINE}:api-commands:0193": "SPI",
}
OFFICIAL_EIBFN_SOURCE_EXCEPTIONS = {
    # The pinned SPI table prints this code with an embedded space. It is not
    # part of the application projection, but full unit validation must retain
    # and explicitly normalize the exact reviewed source form.
    f"{OFFICIAL_BASELINE}:spi-commands-unique:0192": "70 32",
}
EXPECTED_FAMILIES = {
    "task-control": "TaskControl",
    "time": "Time",
    "program-control": "ProgramControl",
    "terminal-control": "TerminalControl",
    "file-control": "FileControl",
    "queue-control": "QueueControl",
    "recovery": "Recovery",
    "interval-control": "IntervalControl",
    "storage-control": "StorageControl",
}
EXPECTED_RUNTIME_OPERATIONS = [
    ("Abend", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0001"),
    ("Address", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0005"),
    ("AddressSet", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0006"),
    ("AsktimeEib", "api", "time", False, f"{OFFICIAL_BASELINE}:api-commands:0009"),
    ("Asktime", "api", "time", False, f"{OFFICIAL_BASELINE}:api-commands:0010"),
    ("Assign", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0011"),
    ("Cancel", "api", "interval-control", True, f"{OFFICIAL_BASELINE}:api-commands:0016"),
    ("ChangeTask", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0022"),
    ("Delay", "api", "interval-control", True, f"{OFFICIAL_BASELINE}:api-commands:0039"),
    ("Delete", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0040"),
    (
        "DeleteTransientData",
        "api",
        "queue-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0048",
    ),
    (
        "DeleteTemporaryStorage",
        "api",
        "queue-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0049",
    ),
    ("Deq", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0050"),
    ("EndBrowse", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0058"),
    ("Enq", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0064"),
    ("FormatTime", "api", "time", False, f"{OFFICIAL_BASELINE}:api-commands:0080"),
    ("Freemain", "api", "storage-control", True, f"{OFFICIAL_BASELINE}:api-commands:0084"),
    ("Getmain", "api", "storage-control", True, f"{OFFICIAL_BASELINE}:api-commands:0094"),
    ("HandleAbend", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0097"),
    ("HandleAid", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0098"),
    ("HandleCondition", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0099"),
    ("IgnoreCondition", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0100"),
    (
        "Inquire",
        "spi-compatibility",
        "program-control",
        False,
        f"{OFFICIAL_BASELINE}:spi-commands-unique:0155",
    ),
    ("Link", "api", "program-control", True, f"{OFFICIAL_BASELINE}:api-commands:0138"),
    ("PopHandle", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0146"),
    (
        "PurgeMessage",
        "api",
        "terminal-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0148",
    ),
    ("PushHandle", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0149"),
    ("Read", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0156"),
    ("ReadNext", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0157"),
    ("ReadPrev", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0158"),
    ("ReceiveMap", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0163"),
    ("Retrieve", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0175"),
    ("Return", "api", "task-control", True, f"{OFFICIAL_BASELINE}:api-commands:0178"),
    ("Rewrite", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0181"),
    ("SendMap", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0189"),
    ("SendText", "api", "terminal-control", True, f"{OFFICIAL_BASELINE}:api-commands:0192"),
    (
        "SetAssociationUserCorrData",
        "api",
        "task-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0193",
    ),
    (
        "SetFileStatus",
        "spi-compatibility",
        "file-control",
        True,
        f"{OFFICIAL_BASELINE}:spi-commands-unique:0224",
    ),
    ("Start", "api", "interval-control", True, f"{OFFICIAL_BASELINE}:api-commands:0205"),
    ("StartBrowse", "api", "file-control", False, f"{OFFICIAL_BASELINE}:api-commands:0208"),
    ("Suspend", "api", "task-control", False, f"{OFFICIAL_BASELINE}:api-commands:0214"),
    ("Syncpoint", "api", "recovery", True, f"{OFFICIAL_BASELINE}:api-commands:0218"),
    ("Write", "api", "file-control", True, f"{OFFICIAL_BASELINE}:api-commands:0253"),
    (
        "WriteTransientData",
        "api",
        "queue-control",
        True,
        f"{OFFICIAL_BASELINE}:api-commands:0257",
    ),
    ("Xctl", "api", "program-control", True, f"{OFFICIAL_BASELINE}:api-commands:0263"),
]

CONTRACT_BATCHES = (
    (
        "sources-a",
        1,
        88,
        Path("conformance/0.9/generated/cics-application-api-sources-a-candidates.json"),
        Path("conformance/0.9/cics/application-api-sources-a-review.json"),
    ),
    (
        "sources-b",
        89,
        176,
        Path("conformance/0.9/generated/cics-application-api-sources-b-candidates.json"),
        Path("conformance/0.9/cics/application-api-sources-b-review.json"),
    ),
    (
        "sources-c",
        177,
        263,
        Path("conformance/0.9/generated/cics-application-api-sources-c-candidates.json"),
        Path("conformance/0.9/cics/application-api-sources-c-review.json"),
    ),
)
SOURCE_DIMENSIONS = (
    ("syntax", "grammar", "source-syntax"),
    ("options", "option-legality", "source-option"),
    ("operand-directions", "operand-direction", "source-operand-direction"),
    ("conditions", "conditions", "source-condition"),
    ("execution-context", "context-applicability", "source-context"),
)
CONTRACT_DIMENSIONS = (
    "grammar",
    "option-legality",
    "operand-direction",
    "option-bounds",
    "resource-key",
    "capability-intent",
    "eib-response",
    "conditions",
    "context-applicability",
    "effect-class",
    "cancellation",
    "audit",
    "recovery",
)
ACCEPTED_SOURCE_REVIEWS = {
    "auto-accepted",
    "auto-accepted-with-bounded-ambiguities",
}

# CICS assigns the high EIBFN byte by command processor.  Keep this small,
# reviewed processor-to-owner map here and derive all 263 row assignments from
# the pinned EIBFN values.  Existing runtime rows retain their already-reviewed
# handler family below, so this table never silently moves shipped behavior.
EIBFN_FAMILY_BY_PREFIX = {
    "02": "task-control",
    "04": "conversation-control",
    "06": "file-control",
    "08": "queue-control",
    "0A": "queue-control",
    "0C": "storage-control",
    "0E": "program-control",
    "10": "interval-control",
    "12": "task-control",
    "14": "journal-control",
    "16": "recovery",
    "18": "terminal-control",
    "1A": "diagnostics",
    "1C": "diagnostics",
    "1E": "terminal-control",
    "20": "counter-control",
    "24": "conversation-control",
    "26": "transform-control",
    "28": "event-control",
    "34": "bts-control",
    "36": "event-control",
    "38": "web-control",
    "3C": "document-control",
    "3E": "network-control",
    "48": "diagnostics",
    "4A": "time",
    "56": "spool-control",
    "5E": "task-control",
    "6A": "security-control",
    "6C": "operator-control",
    "74": "security-control",
    "7E": "diagnostics",
    "96": "bts-control",
    "C0": "web-service-control",
    "C4": "task-control",
}

# Some processor-code ranges contain more than one semantic family. Keep the
# reviewed command-specific owners explicit rather than flattening by high byte.
EIBFN_FAMILY_BY_CODE = {
    # The bare ASKTIME form shares the time family with ASKTIME ABSTIME even
    # though its older EIBFN is allocated from the 0x10 processor range.
    "1002": "time",
    "2002": "builtin-function-control",
    "2020": "builtin-function-control",
}

# A resource option is meaningful only within its command-family namespace.
# This avoids the former global-name heuristic, which treated ASSIGN output
# fields and WEB PARSE URL output fields as authorization selectors.  The
# direction check in `_resource_contract` is still authoritative: output-only
# candidates can never bind a resource even when their name appears here.
RESOURCE_SELECTOR_OPTIONS_BY_FAMILY = {
    "bts-control": frozenset(
        {
            "ACQACTIVITY",
            "ACQPROCESS",
            "ACTIVITY",
            "ACTIVITYID",
            "BROWSETOKEN",
            "CHANNEL",
            "CHILD",
            "CONTAINER",
            "EVENT",
            "PROCESS",
            "PROCESSID",
            "PROCESSTYPE",
            "TIMER",
        }
    ),
    "conversation-control": frozenset(
        {"CONVID", "PARTNER", "PARTNSET", "SESSION"}
    ),
    "counter-control": frozenset({"COUNTER", "DCOUNTER", "POOL"}),
    "document-control": frozenset({"DOCTOKEN", "DOCUMENT"}),
    "event-control": frozenset({"EVENT", "SUBEVENT", "TIMER"}),
    "file-control": frozenset({"DATASET", "FILE"}),
    "interval-control": frozenset({"REQID", "TIMER"}),
    "journal-control": frozenset({"JOURNALNAME", "JOURNALNUM"}),
    "program-control": frozenset({"PROGRAM"}),
    "queue-control": frozenset({"QNAME", "QUEUE", "TDQUEUE", "TSQUEUE"}),
    "security-control": frozenset(
        {"RESCLASS", "RESID", "RESTYPE", "TOKEN", "USERID"}
    ),
    "spool-control": frozenset({"TOKEN"}),
    "terminal-control": frozenset({"MAP", "MAPSET", "TERMID", "TERMINAL"}),
    "transform-control": frozenset(
        {"CHANNEL", "INCONTAINER", "OUTCONTAINER", "TRANSFORMER"}
    ),
    "web-control": frozenset({"HOST", "SESSTOKEN", "TCPIPSERVICE", "URIMAP"}),
    "web-service-control": frozenset(
        {"CHANNEL", "CONTAINER", "URIMAP", "WEBSERVICE"}
    ),
}

RESOURCE_SELECTOR_OPTIONS_BY_LABEL = {
    "DEQ": frozenset({"RESOURCE"}),
    "ENQ": frozenset({"RESOURCE"}),
    "INVOKE APPLICATION": frozenset(
        {"APPLICATION", "MAJORVERSION", "MINORVERSION", "OPERATION", "PLATFORM"}
    ),
    "INVOKE SERVICE": frozenset({"OPERATION", "SERVICE", "URI"}),
}

HOST_LIMITS = {
    "profile": "mainframe-env.host-cics-bounds@1",
    "max_argument_count": 512,
    "max_argument_name_bytes": 128,
    "max_argument_value_bytes": 1048576,
    "max_request_bytes": 4194304,
}

COMMON_COMMAND_OPTIONS = {
    "NOHANDLE": ({"none"}, {"none"}),
    "RESP": ({"data-area"}, {"output"}),
    "RESP2": ({"data-area"}, {"output"}),
}

POLICY_BINDINGS = {
    "authorization": {
        "contract": "docs/architecture/PLUGIN-AND-SECURITY.md",
        "rule": "typed-saf-resource-before-dispatch",
    },
    "audit": {
        "contract": "docs/architecture/PLUGIN-AND-SECURITY.md",
        "rule": "deny-failure-and-effect-audit",
    },
    "canonical_effect": {
        "contract": "docs/contracts/EFFECT-CANONICAL-V1.md",
        "schema": "mainframe-env.effect-canonical@1",
    },
    "execution": {
        "contract": "docs/architecture/EXECUTION-AND-DURABILITY.md",
        "rule": "durable-coordinator-selected-route",
    },
    "persistence": {
        "contract": "docs/contracts/PROVIDER-ROW-PERSISTENCE-V1.md",
        "rule": "versioned-provider-row-cas",
    },
    "storage": {
        "contract": "docs/contracts/DURABLE-STORAGE-PROFILE.md",
        "rule": "memory-sqlite-postgresql-profile",
    },
}

TYPED_RUNTIME_OPERATIONS = frozenset(
    {
        "Abend",
        "Address",
        "Cancel",
        "ChangeTask",
        "Delay",
        "AddressSet",
        "Asktime",
        "AsktimeEib",
        "Deq",
        "Enq",
        "FormatTime",
        "Freemain",
        "Getmain",
        "HandleAbend",
        "HandleAid",
        "HandleCondition",
        "IgnoreCondition",
        "Link",
        "Xctl",
        "Return",
        "StartBrowse",
        "ReadNext",
        "ReadPrev",
        "EndBrowse",
        "Delete",
        "Write",
        "WriteTransientData",
        "DeleteTransientData",
        "DeleteTemporaryStorage",
        "ReceiveMap",
        "SendMap",
        "SendText",
        "Assign",
        "PurgeMessage",
        "PopHandle",
        "PushHandle",
        "Read",
        "Rewrite",
        "SetAssociationUserCorrData",
        "Suspend",
        "Syncpoint",
        "Start",
        "Retrieve",
    }
)
ENQUEUE_COMMAND_ROWS = frozenset(
    {
        f"{OFFICIAL_BASELINE}:api-commands:0050",
        f"{OFFICIAL_BASELINE}:api-commands:0064",
    }
)
# Each profile is a reviewed compiler-only route to a pre-existing runtime
# operation. It does not change application-registry readiness or counts.
COMPILER_SPI_COMPATIBILITY = {
    "operation": "Inquire",
    "official_row": f"{OFFICIAL_BASELINE}:spi-commands-unique:0155",
    "runtime_official_row": f"{OFFICIAL_BASELINE}:spi-commands-unique:0155",
    "interface": "spi-compatibility",
    "family": "program-control",
    "mutating": False,
    "label": "INQUIRE PROGRAM",
    "recognition_head": ["INQUIRE"],
    "required_value_options": ["PROGRAM"],
    "optional_value_options": ["RESP", "RESP2"],
    "optional_flag_options": ["NOHANDLE"],
    "resp2_requires_resp": True,
    "expected_discriminators": ["ACTIVITYID", "CONTAINER", "EVENT", "PROCESS", "TIMER"],
    "reject_unknown_options": True,
}
COMPILER_SEND_COMPATIBILITY = {
    "operation": "SendText",
    "official_row": f"{OFFICIAL_BASELINE}:api-commands:0187",
    "runtime_official_row": f"{OFFICIAL_BASELINE}:api-commands:0192",
    "interface": "api",
    "family": "terminal-control",
    "mutating": True,
    "label": "SEND",
    "recognition_head": ["SEND"],
    "required_value_options": ["FROM"],
    "optional_value_options": ["LENGTH", "RESP", "RESP2"],
    "optional_flag_options": ["ERASE", "NOHANDLE"],
    "resp2_requires_resp": True,
    "expected_discriminators": ["CONTROL", "MAP", "PAGE", "PARTNSET", "TEXT"],
    "reject_unknown_options": False,
}
COMPILER_LEGACY_COMPATIBILITY = (COMPILER_SPI_COMPATIBILITY, COMPILER_SEND_COMPATIBILITY)
TYPED_RUNTIME_IR_EFFECTS = {
    "Abend": frozenset(
        {
            "memory-read",
            "memory-write",
            "program-control",
            "condition",
            "transaction",
        }
    ),
    "AddressSet": frozenset({"memory-read", "memory-write", "condition"}),
    "Address": frozenset({"memory-read", "memory-write", "condition"}),
    "Asktime": frozenset({"memory-write", "clock", "condition"}),
    "AsktimeEib": frozenset({"memory-write", "clock", "condition"}),
    "ChangeTask": frozenset(
        {"memory-read", "memory-write", "suspension", "condition"}
    ),
    "Deq": frozenset({"memory-read", "memory-write", "condition", "transaction"}),
    "Enq": frozenset(
        {"memory-read", "memory-write", "suspension", "condition", "transaction"}
    ),
    "FormatTime": frozenset({"memory-read", "memory-write", "condition"}),
    "HandleAbend": frozenset({"memory-read", "memory-write", "condition"}),
    "HandleAid": frozenset({"memory-read", "memory-write", "condition"}),
    "HandleCondition": frozenset({"memory-read", "memory-write", "condition"}),
    "IgnoreCondition": frozenset({"memory-read", "memory-write", "condition"}),
    "Link": frozenset(
        {
            "memory-read",
            "memory-write",
            "program-control",
            "condition",
            "transaction",
        }
    ),
    "Xctl": frozenset(
        {
            "memory-read",
            "memory-write",
            "program-control",
            "condition",
            "transaction",
        }
    ),
    "Return": frozenset(
        {
            "memory-read",
            "memory-write",
            "program-control",
            "condition",
            "transaction",
        }
    ),
    "StartBrowse": frozenset(
        {"dataset-read", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "ReadNext": frozenset(
        {"dataset-read", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "ReadPrev": frozenset(
        {"dataset-read", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "EndBrowse": frozenset(
        {"dataset-read", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "Delete": frozenset(
        {"dataset-write", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "Write": frozenset(
        {"dataset-write", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "WriteTransientData": frozenset(
        {"memory-read", "memory-write", "condition", "transaction"}
    ),
    "DeleteTransientData": frozenset(
        {"memory-read", "memory-write", "condition", "transaction"}
    ),
    "DeleteTemporaryStorage": frozenset(
        {"memory-read", "memory-write", "condition", "transaction"}
    ),
    "Getmain": frozenset(
        {"memory-read", "memory-write", "condition", "transaction"}
    ),
    "Freemain": frozenset(
        {"memory-read", "memory-write", "condition", "transaction"}
    ),
    "ReceiveMap": frozenset(
        {
            "memory-read",
            "memory-write",
            "terminal-read",
            "suspension",
            "condition",
            "transaction",
        }
    ),
    "SendMap": frozenset(
        {"memory-read", "memory-write", "terminal-write", "condition", "transaction"}
    ),
    "SendText": frozenset(
        {"memory-read", "memory-write", "terminal-write", "condition", "transaction"}
    ),
    "Assign": frozenset({"memory-write", "condition", "transaction"}),
    "PurgeMessage": frozenset({"memory-write", "condition", "transaction"}),
    "Cancel": frozenset({"memory-read", "memory-write", "condition", "transaction"}),
    "Delay": frozenset({"memory-read", "memory-write", "condition", "transaction"}),
    "PopHandle": frozenset({"memory-write", "condition"}),
    "PushHandle": frozenset({"memory-write", "condition"}),
    "Read": frozenset(
        {"dataset-read", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "Rewrite": frozenset(
        {"dataset-write", "memory-read", "memory-write", "condition", "transaction"}
    ),
    "Syncpoint": frozenset({"memory-write", "condition", "transaction"}),
    "SetAssociationUserCorrData": frozenset(
        {"memory-read", "memory-write", "condition"}
    ),
    "Suspend": frozenset({"memory-write", "suspension", "condition"}),
    "Start": frozenset(
        {"memory-read", "memory-write", "clock", "condition", "transaction"}
    ),
    "Retrieve": frozenset({"memory-write", "condition", "transaction"}),
}


class DescriptorError(ValueError):
    """The descriptor authority is malformed or its generated source is stale."""


def _object(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise DescriptorError(f"{label} must be an object")
    return value


def _array(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise DescriptorError(f"{label} must be an array")
    return value


def _text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise DescriptorError(f"{label} must be non-empty text")
    return value


def _integer(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise DescriptorError(f"{label} must be an integer")
    return value


def _read_json(path: Path) -> dict[str, Any]:
    try:
        return _object(json.loads(path.read_text()), str(path))
    except (OSError, json.JSONDecodeError) as error:
        raise DescriptorError(f"{path}: {error}") from error


def _load_legacy_execution_options(
    root: Path,
    commands: list[dict[str, Any]],
    operations: list[dict[str, Any]],
) -> dict[str, list[str]]:
    path = root / LEGACY_EXECUTION_CATALOG_PATH
    catalog = _read_json(path)
    expected_fields = {
        "schema_version",
        "target_version",
        "source_baseline",
        "application_identity_set_sha256",
        "routes",
    }
    if set(catalog) != expected_fields:
        raise DescriptorError(f"{path} fields differ")
    if (
        catalog["schema_version"] != "mainframe-env.cics-legacy-execution-options@1"
        or catalog["target_version"] != "0.9.0"
        or catalog["source_baseline"] != OFFICIAL_BASELINE
        or catalog["application_identity_set_sha256"]
        != application_identity_digest(commands)
    ):
        raise DescriptorError(f"{path} identity binding differs")

    expected_routes = [
        (operation["official_row"], operation["operation"])
        for operation in operations
        if operation["interface"] == "api"
        and operation["operation"] not in TYPED_RUNTIME_OPERATIONS
    ]
    routes = _array(catalog["routes"], "legacy execution routes")
    normalized = []
    for index, raw_route in enumerate(routes):
        route = _object(raw_route, f"legacy execution routes[{index}]")
        if set(route) != {"official_row", "runtime_operation", "execution_options"}:
            raise DescriptorError(f"legacy execution routes[{index}] fields differ")
        official_row = _text(
            route["official_row"], f"legacy execution routes[{index}].official_row"
        )
        operation = _text(
            route["runtime_operation"],
            f"legacy execution routes[{index}].runtime_operation",
        )
        execution_options = [
            _text(value, f"legacy execution routes[{index}].execution_options")
            for value in _array(
                route["execution_options"],
                f"legacy execution routes[{index}].execution_options",
            )
        ]
        if (
            not execution_options
            or execution_options != sorted(set(execution_options))
            or any(
                OPTION_IDENTIFIER.fullmatch(value) is None for value in execution_options
            )
        ):
            raise DescriptorError(f"legacy execution route {official_row} options differ")
        normalized.append((official_row, operation, execution_options))
    if [(row, operation) for row, operation, _ in normalized] != expected_routes:
        raise DescriptorError(f"{path} route identities or order differ")
    return {row: options for row, _, options in normalized}


def _load_typed_execution_registrations(
    root: Path,
    commands: list[dict[str, Any]],
    families: dict[str, str],
    existing_operations: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    path = root / TYPED_EXECUTION_REGISTRATIONS_PATH
    catalog = _read_json(path)
    if set(catalog) != {
        "schema_version",
        "target_version",
        "application_identity_set_sha256",
        "registrations",
    } or (
        catalog["schema_version"]
        != "mainframe-env.cics-typed-execution-registrations@1"
        or catalog["target_version"] != "0.9.0"
        or catalog["application_identity_set_sha256"]
        != application_identity_digest(commands)
    ):
        raise DescriptorError(f"{path} identity or fields differ")
    command_by_row = {command["official_row"]: command for command in commands}
    existing_names = {operation["operation"] for operation in existing_operations}
    existing_rows = {operation["official_row"] for operation in existing_operations}
    expected_by_name = {
        operation: (interface, family, mutating, official_row)
        for operation, interface, family, mutating, official_row in EXPECTED_RUNTIME_OPERATIONS
    }
    normalized = []
    for index, raw_registration in enumerate(
        _array(catalog["registrations"], "typed execution registrations")
    ):
        registration = _object(
            raw_registration, f"typed execution registrations[{index}]"
        )
        if set(registration) != {
            "operation",
            "interface",
            "family",
            "mutating",
            "official_row",
        }:
            raise DescriptorError(f"typed execution registrations[{index}] fields differ")
        name = _text(
            registration["operation"],
            f"typed execution registrations[{index}].operation",
        )
        family = _text(
            registration["family"], f"typed execution registrations[{index}].family"
        )
        official_row = _text(
            registration["official_row"],
            f"typed execution registrations[{index}].official_row",
        )
        command = command_by_row.get(official_row)
        expected = expected_by_name.get(name)
        if (
            expected is None
            or (
                registration["interface"],
                family,
                registration["mutating"],
                official_row,
            )
            != expected
            or IDENTIFIER.fullmatch(name) is None
            or family not in families
            or command is None
            or name in existing_names
            or official_row in existing_rows
        ):
            raise DescriptorError(f"invalid typed execution registration {name}")
        existing_names.add(name)
        existing_rows.add(official_row)
        normalized.append(
            {
                "operation": name,
                "interface": registration["interface"],
                "family": family,
                "mutating": registration["mutating"],
                "official_row": official_row,
                "label": command["label"],
            }
        )
    if [row["operation"] for row in normalized] != [
        "Address",
        "AddressSet",
        "AsktimeEib",
        "Cancel",
        "ChangeTask",
        "Delay",
        "DeleteTransientData",
        "DeleteTemporaryStorage",
        "Deq",
        "Enq",
        "Freemain",
        "Getmain",
        "HandleAid",
        "IgnoreCondition",
        "PopHandle",
        "PurgeMessage",
        "PushHandle",
        "SetAssociationUserCorrData",
        "Start",
        "Suspend",
    ]:
        raise DescriptorError(f"{path} registration identities or order differ")
    return normalized


def _official_units(
    root: Path, relative: Path, expected_digest: str
) -> tuple[dict[str, list[dict[str, Any]]], dict[str, dict[str, Any]]]:
    path = root / relative
    try:
        source = path.read_bytes()
    except OSError as error:
        raise DescriptorError(f"{path}: {error}") from error
    digest = f"sha256:{hashlib.sha256(source).hexdigest()}"
    if digest != expected_digest or digest != OFFICIAL_CATALOG_DIGEST:
        raise DescriptorError("official CICS catalog digest drifted")
    try:
        official = _object(json.loads(source), str(path))
    except json.JSONDecodeError as error:
        raise DescriptorError(f"{path}: {error}") from error
    if (
        official.get("schema_version") != OFFICIAL_SCHEMA_VERSION
        or official.get("baseline_id") != OFFICIAL_BASELINE
        or official.get("subsystem") != "cics"
        or official.get("mandatory_rows") != sum(row[0] for row in EXPECTED_UNITS.values())
    ):
        raise DescriptorError("official CICS catalog identity or denominator drifted")

    units: dict[str, list[dict[str, Any]]] = {}
    rows_by_id: dict[str, dict[str, Any]] = {}
    raw_units = _array(official.get("units"), "official units")
    if len(raw_units) != len(EXPECTED_UNITS):
        raise DescriptorError("official CICS catalog unit set drifted")
    for raw_unit in raw_units:
        unit = _object(raw_unit, "official unit")
        unit_id = _text(unit.get("id"), "official unit id")
        if unit_id in units or unit_id not in EXPECTED_UNITS:
            raise DescriptorError(f"unknown or duplicate official CICS unit {unit_id}")
        denominator, table, family = EXPECTED_UNITS[unit_id]
        if unit.get("denominator") != denominator:
            raise DescriptorError(f"official CICS unit {unit_id} denominator drifted")
        rows = _array(unit.get("rows"), f"official {unit_id} rows")
        if len(rows) != denominator:
            raise DescriptorError(f"official CICS unit {unit_id} row count drifted")
        normalized = []
        labels: set[str] = set()
        for ordinal, raw_row in enumerate(rows, 1):
            row = _object(raw_row, f"official {unit_id} row {ordinal}")
            expected_id = f"{OFFICIAL_BASELINE}:{unit_id}:{ordinal:04d}"
            row_id = _text(row.get("id"), f"official {unit_id} row id")
            label = _text(row.get("label"), f"official {unit_id} label")
            locator = _text(row.get("source_locator"), f"official {unit_id} locator")
            match = re.fullmatch(
                rf"html-table:{re.escape(table)};eibfn:([^;]+);family:([A-Z]+)",
                locator,
            )
            expected_family = API_LOCATOR_FAMILY_EXCEPTIONS.get(row_id, family)
            raw_eibfn = match.group(1) if match is not None else ""
            exceptional_eibfn = OFFICIAL_EIBFN_SOURCE_EXCEPTIONS.get(row_id)
            canonical_eibfn = raw_eibfn.replace(" ", "") if exceptional_eibfn else raw_eibfn
            if (
                row_id != expected_id
                or row.get("mandatory") is not True
                or match is None
                or match.group(2) != expected_family
                or (
                    exceptional_eibfn is not None
                    and raw_eibfn != exceptional_eibfn
                )
                or (
                    exceptional_eibfn is None
                    and EIBFN.fullmatch(raw_eibfn) is None
                )
                or EIBFN.fullmatch(canonical_eibfn) is None
                or row_id in rows_by_id
                or label in labels
            ):
                raise DescriptorError(f"official CICS row {expected_id} is not exact")
            normalized_row = {
                "official_row": row_id,
                "label": label,
                "eibfn": canonical_eibfn,
                "unit": unit_id,
            }
            normalized.append(normalized_row)
            rows_by_id[row_id] = normalized_row
            labels.add(label)
        units[unit_id] = normalized
    if set(units) != set(EXPECTED_UNITS):
        raise DescriptorError("official CICS catalog unit set is incomplete")
    return units, rows_by_id


def load_catalog(
    root: Path = ROOT, *, include_runtime_admission: bool = True
) -> dict[str, Any]:
    path = root / CATALOG_PATH
    catalog = _read_json(path)
    expected = {
        "schema_version",
        "target_version",
        "official_catalog",
        "official_catalog_sha256",
        "application_catalog",
        "runtime",
    }
    if set(catalog) != expected:
        raise DescriptorError(
            f"{path} fields differ: missing={sorted(expected - set(catalog))} "
            f"unknown={sorted(set(catalog) - expected)}"
        )
    if catalog["schema_version"] != SCHEMA_VERSION or catalog["target_version"] != "0.9.0":
        raise DescriptorError(f"{path} has an unsupported schema or target version")

    official_relative = Path(_text(catalog["official_catalog"], "official_catalog"))
    if official_relative.is_absolute() or ".." in official_relative.parts:
        raise DescriptorError("official_catalog must be repository-relative")
    official_digest = _text(catalog["official_catalog_sha256"], "official_catalog_sha256")
    units, official_rows = _official_units(root, official_relative, official_digest)

    application = _object(catalog["application_catalog"], "application_catalog")
    application_fields = {
        "unit",
        "command_count",
        "automatic_registration",
        "generated_coverage_credit",
        "commands",
    }
    if set(application) != application_fields:
        raise DescriptorError("application_catalog fields differ from the contract")
    commands = _array(application["commands"], "application commands")
    official_api = units["api-commands"]
    if (
        application["unit"] != "api-commands"
        or _integer(application["command_count"], "application command_count") != 263
        or len(commands) != 263
        or application["automatic_registration"] is not False
        or _integer(application["generated_coverage_credit"], "generated coverage credit") != 0
    ):
        raise DescriptorError(
            "application catalog must remain 263 identities, unregistered, and zero-credit"
        )
    normalized_commands = []
    for ordinal, raw_command in enumerate(commands, 1):
        command = _object(raw_command, f"application commands[{ordinal - 1}]")
        if set(command) != {"official_row", "label", "eibfn"}:
            raise DescriptorError(f"application commands[{ordinal - 1}] fields differ")
        normalized = {
            "official_row": _text(command["official_row"], "application official_row"),
            "label": _text(command["label"], "application label"),
            "eibfn": _text(command["eibfn"], "application eibfn"),
        }
        expected_row = {
            key: official_api[ordinal - 1][key]
            for key in ("official_row", "label", "eibfn")
        }
        if normalized != expected_row or EIBFN.fullmatch(normalized["eibfn"]) is None:
            raise DescriptorError(
                f"application command ordinal {ordinal:04d} differs from its official row"
            )
        normalized_commands.append(normalized)

    runtime = _object(catalog["runtime"], "runtime")
    runtime_fields = {
        "operation_count",
        "api_operation_count",
        "spi_compatibility_operation_count",
        "families",
        "operations",
    }
    if set(runtime) != runtime_fields:
        raise DescriptorError("runtime fields differ from the contract")
    families: dict[str, str] = {}
    family_rows = _array(runtime["families"], "runtime families")
    for index, raw_family in enumerate(family_rows):
        family = _object(raw_family, f"runtime families[{index}]")
        if set(family) != {"id", "rust_variant", "responsibility"}:
            raise DescriptorError(f"runtime families[{index}] fields differ")
        family_id = _text(family["id"], f"runtime families[{index}].id")
        variant = _text(family["rust_variant"], f"runtime families[{index}].rust_variant")
        _text(family["responsibility"], f"runtime families[{index}].responsibility")
        if (
            FAMILY_ID.fullmatch(family_id) is None
            or IDENTIFIER.fullmatch(variant) is None
            or family_id in families
            or variant in families.values()
        ):
            raise DescriptorError(f"duplicate or invalid CICS runtime family {family_id}/{variant}")
        families[family_id] = variant
    if families != EXPECTED_FAMILIES:
        raise DescriptorError(f"CICS runtime families differ from the frozen layout: {families}")

    operations = _array(runtime["operations"], "runtime operations")
    if (
        _integer(runtime["operation_count"], "runtime operation_count") != 25
        or len(operations) != 25
        or _integer(runtime["api_operation_count"], "runtime api_operation_count") != 23
        or _integer(
            runtime["spi_compatibility_operation_count"],
            "runtime spi_compatibility_operation_count",
        )
        != 2
    ):
        raise DescriptorError("CICS runtime operation counts must remain exactly 25/23/2")
    operation_names: set[str] = set()
    runtime_rows: set[str] = set()
    interface_counts = {"api": 0, "spi-compatibility": 0}
    normalized_operations = []
    for index, raw_operation in enumerate(operations):
        operation = _object(raw_operation, f"runtime operations[{index}]")
        if set(operation) != {
            "operation",
            "interface",
            "family",
            "mutating",
            "official_row",
        }:
            raise DescriptorError(f"runtime operations[{index}] fields differ")
        name = _text(operation["operation"], f"runtime operations[{index}].operation")
        interface = _text(operation["interface"], f"runtime operations[{index}].interface")
        family = _text(operation["family"], f"runtime operations[{index}].family")
        official_row = _text(
            operation["official_row"], f"runtime operations[{index}].official_row"
        )
        official = official_rows.get(official_row)
        expected_unit = {
            "api": "api-commands",
            "spi-compatibility": "spi-commands-unique",
        }.get(interface)
        if (
            IDENTIFIER.fullmatch(name) is None
            or name in operation_names
            or official_row in runtime_rows
            or family not in families
            or not isinstance(operation["mutating"], bool)
            or official is None
            or official["unit"] != expected_unit
        ):
            raise DescriptorError(f"invalid CICS runtime operation {name}")
        operation_names.add(name)
        runtime_rows.add(official_row)
        interface_counts[interface] += 1
        normalized_operations.append(
            {
                "operation": name,
                "interface": interface,
                "family": family,
                "mutating": operation["mutating"],
                "official_row": official_row,
                "label": official["label"],
            }
        )
    if interface_counts != {"api": 23, "spi-compatibility": 2}:
        raise DescriptorError(f"CICS runtime interface split drifted: {interface_counts}")
    if include_runtime_admission:
        normalized_operations.extend(
            _load_typed_execution_registrations(
                root, normalized_commands, families, normalized_operations
            )
        )
    expected_order = {
        (operation, official_row): index
        for index, (operation, _, _, _, official_row) in enumerate(
            EXPECTED_RUNTIME_OPERATIONS
        )
    }
    normalized_operations.sort(
        key=lambda row: expected_order.get(
            (row["operation"], row["official_row"]), len(expected_order)
        )
    )
    observed_runtime = [
        (
            row["operation"],
            row["interface"],
            row["family"],
            row["mutating"],
            row["official_row"],
        )
        for row in normalized_operations
    ]
    expected_runtime = (
        EXPECTED_RUNTIME_OPERATIONS
        if include_runtime_admission
        else [
            row
            for row in EXPECTED_RUNTIME_OPERATIONS
            if row[0]
            not in {
                "ChangeTask",
                "Address",
                "AddressSet",
                "AsktimeEib",
                "Cancel",
                "Delay",
                "Deq",
                "DeleteTransientData",
                "DeleteTemporaryStorage",
                "Enq",
                "Freemain",
                "Getmain",
                "HandleAid",
                "IgnoreCondition",
                "PopHandle",
                "PurgeMessage",
                "PushHandle",
                "SetAssociationUserCorrData",
                "Start",
                "Suspend",
            }
        ]
    )
    if observed_runtime != expected_runtime:
        raise DescriptorError("CICS runtime operation compatibility set drifted")
    legacy_execution_options = (
        _load_legacy_execution_options(root, normalized_commands, normalized_operations)
        if include_runtime_admission
        else {}
    )
    for operation in normalized_operations:
        operation["legacy_execution_options"] = legacy_execution_options.get(
            operation["official_row"], []
        )

    result = dict(catalog)
    result["_families"] = families
    result["_application_commands"] = normalized_commands
    result["_runtime_operations"] = normalized_operations
    return result


def _digest_field(hasher: Any, value: bytes) -> None:
    hasher.update(len(value).to_bytes(8, "big"))
    hasher.update(value)


def application_identity_digest(commands: list[dict[str, Any]]) -> str:
    """Return the logical, formatting-independent application identity digest."""
    hasher = hashlib.sha256()
    hasher.update(APPLICATION_DIGEST_DOMAIN)
    ordered = sorted(commands, key=lambda row: row["official_row"])
    hasher.update(len(ordered).to_bytes(8, "big"))
    for row in ordered:
        _digest_field(hasher, row["official_row"].encode())
        _digest_field(hasher, row["label"].encode())
        _digest_field(hasher, bytes.fromhex(row["eibfn"]))
    return f"sha256:{hasher.hexdigest()}"


def _file_sha256(path: Path) -> str:
    try:
        source = path.read_bytes()
    except OSError as error:
        raise DescriptorError(f"{path}: {error}") from error
    return f"sha256:{hashlib.sha256(source).hexdigest()}"


def _canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode()


def contract_digest(contract: dict[str, Any]) -> str:
    """Return the logical digest of a generated application-command contract batch."""
    payload = {key: value for key, value in contract.items() if key != "contract_sha256"}
    hasher = hashlib.sha256()
    hasher.update(CONTRACT_DIGEST_DOMAIN)
    hasher.update(_canonical_json(payload))
    return f"sha256:{hasher.hexdigest()}"


def _source_review_digest(review: dict[str, Any]) -> str:
    payload = {key: value for key, value in review.items() if key != "review_sha256"}
    canonical = json.dumps(
        payload,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
    ).encode()
    return f"sha256:{hashlib.sha256(SOURCE_REVIEW_DIGEST_DOMAIN + canonical).hexdigest()}"


def _source_review_is_current(
    root: Path,
    review: dict[str, Any],
    projection_sha256: str,
) -> bool:
    try:
        if review.get("review_sha256") != _source_review_digest(review):
            return False
        if _object(review.get("inputs"), "source review inputs").get(
            "candidate_projection_sha256"
        ) != projection_sha256:
            return False
        if any(
            review.get(field) is not False
            for field in ("semantic_authority", "execution_authority", "automatic_registration")
        ):
            return False
        if any(
            review.get(field) != 0
            for field in ("coverage_credit", "semantic_credit", "differential_credit")
        ):
            return False
        authority = _object(review.get("review_authority"), "source review authority")
        if authority.get("automatic_acceptance") is not True or authority.get(
            "manual_approval"
        ) is not False:
            return False
        for binding_name in ("schema", "checker", "independent_verifier"):
            binding = _object(
                _object(review.get("review_contract"), "source review contract").get(
                    binding_name
                ),
                f"source review {binding_name}",
            )
            relative = Path(_text(binding.get("path"), f"source review {binding_name} path"))
            if relative.is_absolute() or ".." in relative.parts:
                return False
            if binding.get("file_sha256") != _file_sha256(root / relative):
                return False
        status = review.get("review_status")
        blocking = _object(review.get("counts"), "source review counts").get(
            "blocking_findings"
        )
        if status in ACCEPTED_SOURCE_REVIEWS and blocking != 0:
            return False
        if status == "blocked" and (not isinstance(blocking, int) or blocking < 1):
            return False
    except (DescriptorError, KeyError, TypeError):
        return False
    return True


def _source_verification_status(
    review_status: str,
    projection_state: str,
    bounded_ambiguity: bool,
) -> str:
    if projection_state == "not-projected":
        return "not-projected"
    if review_status == "auto-accepted-with-bounded-ambiguities" and bounded_ambiguity:
        if projection_state in {
            "projected",
            "declared-absent",
            "source-backed-not-applicable",
        }:
            return "verified-with-bounded-ambiguity"
    accepted = review_status in ACCEPTED_SOURCE_REVIEWS
    if projection_state == "projected":
        return "verified" if accepted else "projected-unverified"
    if projection_state == "declared-absent":
        return "verified-absent" if accepted else "projected-absent-unverified"
    if projection_state == "source-backed-not-applicable":
        return "not-applicable" if accepted else "projected-not-applicable-unverified"
    if projection_state == "conflicting":
        return "ambiguous"
    if projection_state in {"source-gap", "unmatched"}:
        return projection_state
    raise DescriptorError(f"unknown CICS source projection state {projection_state}")


def _source_fact_groups(candidates: list[Any], expected_kind: str, label: str) -> list[dict[str, Any]]:
    groups: dict[bytes, dict[str, Any]] = {}
    seen_ids: set[str] = set()
    for index, raw_candidate in enumerate(candidates):
        candidate = _object(raw_candidate, f"{label}.candidates[{index}]")
        kind = _text(candidate.get("kind"), f"{label}.candidates[{index}].kind")
        candidate_id = _text(
            candidate.get("candidate_id"), f"{label}.candidates[{index}].candidate_id"
        )
        if candidate_id in seen_ids:
            raise DescriptorError(f"duplicate source candidate {candidate_id}")
        seen_ids.add(candidate_id)
        if kind != expected_kind:
            # Global context candidates are intentionally projected into several
            # source dimensions. Only execution-context consumes them as facts.
            if kind == "source-context":
                continue
            raise DescriptorError(f"unexpected {kind} candidate in {label}")
        value = _object(
            candidate.get("candidate_value"), f"{label}.candidates[{index}].candidate_value"
        )
        key = _canonical_json(value)
        group = groups.setdefault(key, {"value": value, "candidate_ids": []})
        group["candidate_ids"].append(candidate_id)
    facts = []
    for key in sorted(groups):
        group = groups[key]
        group["candidate_ids"].sort()
        facts.append(group)
    return facts


def _validated_condition_name_authority(value: Any) -> dict[str, Any]:
    authority = _object(value, "condition name authority")
    expected_fields = {
        "profile",
        "topic_path",
        "topic_sha256",
        "table_id",
        "allowed_names",
        "conditions",
        "allowed_names_digest_definition",
        "allowed_names_sha256",
        "conditions_digest_definition",
        "conditions_sha256",
    }
    if set(authority) != expected_fields or authority.get("profile") != CONDITION_NAME_PROFILE:
        raise DescriptorError("condition name authority fields differ")
    names = [
        _text(name, "condition name")
        for name in _array(authority.get("allowed_names"), "condition names")
    ]
    if (
        len(names) != len(set(names))
        or names != sorted(names)
        or any(re.fullmatch(r"[A-Z][A-Z0-9-]{0,31}", name) is None for name in names)
    ):
        raise DescriptorError("condition name authority names differ")
    raw_conditions = _array(authority.get("conditions"), "condition name codes")
    pairs = []
    for raw_condition in raw_conditions:
        condition = _object(raw_condition, "condition name code")
        if set(condition) != {"name", "code"}:
            raise DescriptorError("condition name code fields differ")
        name = _text(condition.get("name"), "condition code name")
        code = _integer(condition.get("code"), f"{name} condition code")
        if not 0 <= code <= 255:
            raise DescriptorError(f"{name} condition code is out of range")
        pairs.append([name, code])
    if [pair[0] for pair in pairs] != names:
        raise DescriptorError("condition name and code ordering differs")
    names_encoded = json.dumps(
        names, ensure_ascii=True, sort_keys=False, separators=(",", ":")
    ).encode()
    pairs_encoded = json.dumps(
        pairs, ensure_ascii=True, sort_keys=False, separators=(",", ":")
    ).encode()
    if authority.get("allowed_names_sha256") != "sha256:" + hashlib.sha256(
        CONDITION_NAME_DOMAIN + names_encoded
    ).hexdigest() or authority.get("conditions_sha256") != "sha256:" + hashlib.sha256(
        CONDITION_NAME_DOMAIN + b"pairs\0" + pairs_encoded
    ).hexdigest():
        raise DescriptorError("condition name authority digest differs")
    return authority


def _load_source_batch(
    root: Path,
    batch_id: str,
    start: int,
    end: int,
    projection_relative: Path,
    review_relative: Path,
    commands: list[dict[str, Any]],
) -> tuple[dict[str, Any], dict[str, dict[str, Any]], dict[str, Any]]:
    projection_path = root / projection_relative
    review_path = root / review_relative
    if not projection_path.is_file():
        if review_path.exists():
            raise DescriptorError(f"{review_relative} exists without {projection_relative}")
        return {"projection": None, "review": None}, {}, {
            "status": "not-projected",
            "accepted_for_freeze": False,
            "ambiguity_scope": {},
            "ambiguity_issue_ids": frozenset(),
            "verified_candidates": 0,
            "ambiguous_candidates": 0,
            "condition_name_authority": None,
        }

    projection = _read_json(projection_path)
    condition_name_authority = _validated_condition_name_authority(
        projection.get("condition_name_authority")
    )
    rows = _array(projection.get("rows"), f"{batch_id} source rows")
    expected_commands = commands[start - 1 : end]
    if len(rows) != len(expected_commands):
        raise DescriptorError(
            f"{batch_id} source projection must cover exactly rows {start:04d}-{end:04d}"
        )
    rows_by_id: dict[str, dict[str, Any]] = {}
    for offset, (raw_row, command) in enumerate(zip(rows, expected_commands, strict=True), start):
        row = _object(raw_row, f"{batch_id} source row {offset:04d}")
        identity = {
            "official_row": row.get("official_row"),
            "label": row.get("label"),
            "eibfn": row.get("eibfn"),
        }
        if identity != command:
            raise DescriptorError(f"{batch_id} source row {offset:04d} identity differs")
        dimensions = _array(row.get("dimensions"), f"{batch_id} source row dimensions")
        if [dimension.get("name") for dimension in dimensions if isinstance(dimension, dict)] != [
            source_name for source_name, _, _ in SOURCE_DIMENSIONS
        ]:
            raise DescriptorError(f"{batch_id} source row {offset:04d} dimensions differ")
        rows_by_id[command["official_row"]] = row

    projection_sha256 = _file_sha256(projection_path)
    review_binding: dict[str, Any] | None = None
    review_status = "not-reviewed"
    accepted_for_freeze = False
    ambiguity_scope: dict[str, frozenset[str]] = {}
    ambiguity_issue_ids: frozenset[str] = frozenset()
    verified_candidates = 0
    ambiguous_candidates = 0
    if review_path.is_file():
        review = _read_json(review_path)
        review_status = _text(review.get("review_status"), f"{batch_id} review_status")
        if review_status not in ACCEPTED_SOURCE_REVIEWS | {"blocked"}:
            raise DescriptorError(f"{batch_id} review status is unsupported")
        if not _source_review_is_current(root, review, projection_sha256):
            review_status = "stale"
        else:
            candidate_dispositions = _object(
                review.get("candidate_dispositions"), f"{batch_id} candidate dispositions"
            )
            verified_candidates = _integer(
                _object(
                    candidate_dispositions.get("verified"),
                    f"{batch_id} verified candidates",
                ).get("count"),
                f"{batch_id} verified candidate count",
            )
            product_candidates = _object(
                candidate_dispositions.get("product-ambiguity"),
                f"{batch_id} product candidate ambiguities",
            )
            ambiguous_candidates = _integer(
                product_candidates.get("count"),
                f"{batch_id} product candidate ambiguity count",
            )
            if review_status == "auto-accepted-with-bounded-ambiguities":
                ambiguity_issue_ids = frozenset(
                    _text(issue_id, f"{batch_id} ambiguity issue id")
                    for issue_id in _array(
                        review.get("blocker_ids"), f"{batch_id} blocker ids"
                    )
                )
                raw_scope = review.get("ambiguity_scope")
                if isinstance(raw_scope, list):
                    source_names = {name for name, _, _ in SOURCE_DIMENSIONS}
                    for index, raw_entry in enumerate(raw_scope):
                        entry = _object(raw_entry, f"{batch_id} ambiguity_scope[{index}]")
                        if set(entry) != {"official_row", "dimensions"}:
                            raise DescriptorError(
                                f"{batch_id} ambiguity_scope[{index}] fields differ"
                            )
                        official_row = _text(
                            entry.get("official_row"),
                            f"{batch_id} ambiguity_scope[{index}].official_row",
                        )
                        if official_row not in rows_by_id or official_row in ambiguity_scope:
                            raise DescriptorError(
                                f"{batch_id} ambiguity scope row is foreign or duplicated"
                            )
                        dimensions = frozenset(
                            _text(value, f"{batch_id} ambiguity dimension")
                            for value in _array(
                                entry.get("dimensions"),
                                f"{batch_id} ambiguity_scope[{index}].dimensions",
                            )
                        )
                        if not dimensions or not dimensions <= source_names:
                            raise DescriptorError(
                                f"{batch_id} ambiguity scope dimensions differ"
                            )
                        ambiguity_scope[official_row] = dimensions
                if ambiguous_candidates > 0 and not ambiguity_scope:
                    # A compact row/dimension scope is required before the
                    # command contract can freeze.  Do not smear one source
                    # ambiguity across every row as the scaffold did.
                    accepted_for_freeze = False
                else:
                    accepted_for_freeze = True
            else:
                accepted_for_freeze = review_status == "auto-accepted"
        review_binding = {
            "path": review_relative.as_posix(),
            "file_sha256": _file_sha256(review_path),
            "review_status": review_status,
        }
    return (
        {
            "projection": {
                "path": projection_relative.as_posix(),
                "file_sha256": projection_sha256,
            },
            "review": review_binding,
        },
        rows_by_id,
        {
            "status": review_status,
            "accepted_for_freeze": accepted_for_freeze,
            "ambiguity_scope": ambiguity_scope,
            "ambiguity_issue_ids": ambiguity_issue_ids,
            "verified_candidates": verified_candidates,
            "ambiguous_candidates": ambiguous_candidates,
            "condition_name_authority": condition_name_authority,
        },
    )


def _row_contract_digest(command: dict[str, Any]) -> str:
    payload = {key: value for key, value in command.items() if key != "row_contract_sha256"}
    return "sha256:" + hashlib.sha256(
        ROW_CONTRACT_DIGEST_DOMAIN + _canonical_json(payload)
    ).hexdigest()


def _registry_digest(registry_rows: list[dict[str, Any]]) -> str:
    return "sha256:" + hashlib.sha256(
        REGISTRY_DIGEST_DOMAIN + _canonical_json(registry_rows)
    ).hexdigest()


def _handler_digest(handler: dict[str, Any]) -> str:
    payload = {
        key: handler[key]
        for key in (
            "family",
            "handler_id",
            "readiness",
            "advertised",
            "runtime_operation",
            "legacy_execution_options",
            "unready_result",
        )
    }
    return "sha256:" + hashlib.sha256(
        HANDLER_DIGEST_DOMAIN + _canonical_json(payload)
    ).hexdigest()


def _command_slug(label: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", label.casefold()).strip("-")
    if not slug:
        raise DescriptorError(f"CICS command label has no stable slug: {label!r}")
    return slug


def _contract_family(
    command: dict[str, Any], runtime_operation: dict[str, Any] | None
) -> str:
    if runtime_operation is not None:
        return runtime_operation["family"]
    explicit_family = EIBFN_FAMILY_BY_CODE.get(command["eibfn"])
    if explicit_family is not None:
        return explicit_family
    prefix = command["eibfn"][:2]
    try:
        return EIBFN_FAMILY_BY_PREFIX[prefix]
    except KeyError as error:
        raise DescriptorError(
            f"{command['official_row']} has unmapped EIBFN processor {prefix}"
        ) from error


def _source_dimension(
    dimensions: list[dict[str, Any]], name: str
) -> dict[str, Any]:
    matches = [dimension for dimension in dimensions if dimension["name"] == name]
    if len(matches) != 1:
        raise DescriptorError(f"source dimension {name} is missing or duplicated")
    return matches[0]


def _source_values(dimension: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        _object(fact.get("value"), f"{dimension['name']} source fact")
        for fact in _array(dimension.get("facts"), f"{dimension['name']} source facts")
    ]


def _source_contract_status(dimension: dict[str, Any]) -> str:
    status = dimension["verification_status"]
    if status in {"verified", "verified-absent"}:
        return "resolved"
    if status == "not-applicable":
        return "not-applicable"
    if status == "verified-with-bounded-ambiguity":
        return "bounded-ambiguity"
    return "pending"


def _top_level_source_option_names(
    command: dict[str, Any], dimensions: list[dict[str, Any]]
) -> list[str]:
    dimension = _source_dimension(dimensions, "option-legality")
    names = {
        _text(value.get("term"), "top-level option term")
        for value in _source_values(dimension)
        if value.get("type") == "option"
        and value.get("depth") == 0
        and value.get("dynamic_name_profile") is None
    }
    if dimension["source_projection_state"] != "source-backed-not-applicable":
        names.update(COMMON_COMMAND_OPTIONS)
    if (
        command["official_row"] in ENQUEUE_COMMAND_ROWS
        and dimension["source_projection_state"] == "projected"
    ):
        names.update({"TASK", "UOW"})
    return sorted(names)


def _host_option_value_limit(markers: set[str]) -> int:
    valued = markers - {"none"}
    if not valued:
        return 0
    if valued <= {"name", "filename", "systemname"}:
        return HOST_LIMITS["max_argument_name_bytes"]
    if valued <= {"cvda", "hhmmss", "ptr-ref", "ptr-ref64", "ptr-value", "ptr-value64"}:
        return 16
    return HOST_LIMITS["max_argument_value_bytes"]


def _grammar_contract(dimensions: list[dict[str, Any]]) -> dict[str, Any]:
    dimension = _source_dimension(dimensions, "grammar")
    variants = _source_values(dimension)
    status = _source_contract_status(dimension)
    if status == "resolved" and not variants:
        # A declared-empty compatibility stub is not a grammar.  Keep it
        # recognized, but require a later successor-specific projection before
        # any parser can claim the command shape is exact.
        status = "bounded-ambiguity"
    return {"status": status, "variants": variants}


def _option_value_shape(markers: set[str]) -> str:
    if markers == {"none"}:
        return "flag"
    if markers and "none" not in markers:
        return "value"
    return "bounded-ambiguity"


def _detachable_operand_options(variants: list[dict[str, Any]]) -> set[str]:
    """Return options whose syntax makes the parenthesized value optional."""
    detachable: set[str] = set()
    for variant in variants:
        if variant.get("type") != "syntax":
            continue
        tokens = _array(variant.get("tokens"), "grammar variant tokens")
        for index, raw_token in enumerate(tokens[:-1]):
            token = _object(raw_token, "grammar token")
            if token.get("kind") != "keyword":
                continue
            name = _text(token.get("value"), "grammar keyword value")
            if not name or name.endswith("("):
                continue
            keyword_path = _text(token.get("group_path"), "grammar keyword group path")
            next_token = _object(tokens[index + 1], "grammar token")
            if next_token.get("kind") != "delimiter" or next_token.get("value") != "(":
                continue
            next_path = _text(next_token.get("group_path"), "grammar delimiter group path")
            if next_path != keyword_path and next_path.startswith(f"{keyword_path}/"):
                detachable.add(name)
    return detachable


def _source_option_bound(value: dict[str, Any], name: str) -> int | None:
    # Bounds are accepted only when they are carried by a pinned source fact.
    # Host marker-based ceilings are deliberately handled separately and can
    # never promote the IBM option-bounds dimension to resolved.
    candidates = [
        value.get("ibm_max_value_bytes"),
        value.get("source_max_value_bytes"),
    ]
    candidates = [candidate for candidate in candidates if candidate is not None]
    if not candidates:
        return None
    if len(set(candidates)) != 1:
        raise DescriptorError(f"{name} has conflicting IBM source bounds")
    bound = candidates[0]
    if isinstance(bound, bool) or not isinstance(bound, int) or bound < 1:
        raise DescriptorError(f"{name} has malformed IBM source bound")
    return bound


def _choice_root_and_branch(group_path: str, option: str) -> tuple[str, str] | None:
    segments = group_path.split("/")
    choices = [
        index
        for index, segment in enumerate(segments)
        if re.fullmatch(r"groupchoice\[[0-9]+\]", segment)
    ]
    if not choices:
        return None
    choice = choices[-1]
    root = "/".join(segments[: choice + 1])
    suffix = segments[choice + 1 :]
    # Direct keyword children share the choice path, so their option identity
    # is the branch. Composite/sequence children have an explicit branch node.
    branch = suffix[0] if suffix else f"option:{option}"
    return root, branch


def _option_constraints(
    grammar: dict[str, Any],
    option_names: set[str],
    applicable: bool,
    legality_status: str,
) -> dict[str, Any]:
    if not applicable:
        return {
            "status": "not-applicable",
            "source_scope": "not-applicable",
            "required": [],
            "alternatives": [],
            "dependencies": [],
            "mutual_exclusions": [],
        }

    variants = grammar["variants"]
    dependencies = (
        [{"option": "RESP2", "requires": ["RESP"]}]
        if {"RESP", "RESP2"} <= option_names
        else []
    )
    structural_complete = bool(variants) and all(
        isinstance(variant.get("panel"), dict)
        and variant["panel"].get("total") == 1
        and variant["panel"].get("role") == "command-head"
        for variant in variants
    )
    if not structural_complete:
        return {
            "status": "pending" if grammar["status"] == "pending" else "bounded-ambiguity",
            "source_scope": "syntax-diagram-only",
            "required": [],
            "alternatives": [],
            "dependencies": dependencies,
            "mutual_exclusions": [],
        }

    required_by_variant: list[set[str]] = []
    choice_observations: dict[
        tuple[str, ...], list[tuple[int, bool]]
    ] = {}
    skipped_complex_choice = False
    for variant_index, variant in enumerate(variants):
        tokens = _array(variant.get("tokens"), "grammar variant tokens")
        required: set[str] = set()
        choice_required: list[tuple[str, str]] = []
        choices: dict[str, dict[str, Any]] = {}
        for raw_token in tokens:
            token = _object(raw_token, "grammar token")
            if token.get("kind") != "keyword":
                continue
            option = str(token.get("value", "")).removesuffix("(")
            if option not in option_names:
                continue
            relation = _text(token.get("relation"), f"{option} grammar relation")
            group_path = _text(token.get("group_path"), f"{option} grammar group path")
            choice = _choice_root_and_branch(group_path, option)
            if relation == "required":
                if choice is None:
                    required.add(option)
                else:
                    choice_required.append((option, choice[0]))
            if choice is None:
                continue
            root, branch = choice
            group = choices.setdefault(root, {"branches": {}, "required": False})
            group["branches"].setdefault(branch, set()).add(option)
            if relation in {"required", "alternative"}:
                group["required"] = True
        actual_choices = {
            root
            for root, group in choices.items()
            if len(group["branches"]) >= 2
        }
        required.update(
            option for option, root in choice_required if root not in actual_choices
        )
        required_by_variant.append(required)
        for group in choices.values():
            branch_options = list(group["branches"].values())
            if (
                len(branch_options) < 2
                or any(len(options) != 1 for options in branch_options)
            ):
                if len(branch_options) >= 2:
                    skipped_complex_choice = True
                continue
            members = tuple(sorted(next(iter(options)) for options in branch_options))
            if len(set(members)) != len(members):
                skipped_complex_choice = True
                continue
            choice_observations.setdefault(members, []).append(
                (variant_index, bool(group["required"]))
            )

    required = (
        set.intersection(*required_by_variant)
        if required_by_variant
        else set()
    )
    alternatives = []
    for members, observations in sorted(choice_observations.items()):
        observed_variants = {variant for variant, _ in observations}
        group_required = (
            len(observed_variants) == len(variants)
            and all(required_value for _, required_value in observations)
        )
        alternatives.append({"members": list(members), "required": group_required})

    source_status = grammar["status"]
    if source_status == "pending":
        status = "pending"
    elif (
        source_status == "bounded-ambiguity"
        or legality_status == "bounded-ambiguity"
        or skipped_complex_choice
    ):
        status = "bounded-ambiguity"
    else:
        status = "resolved"
    if status == "bounded-ambiguity":
        required = set()
        alternatives = []
    return {
        "status": status,
        "source_scope": "syntax-diagram-only",
        "required": sorted(required),
        "alternatives": alternatives,
        "dependencies": dependencies,
        "mutual_exclusions": [group["members"] for group in alternatives],
    }


def _option_contract(
    command: dict[str, Any], dimensions: list[dict[str, Any]]
) -> dict[str, Any]:
    option_dimension = _source_dimension(dimensions, "option-legality")
    direction_dimension = _source_dimension(dimensions, "operand-direction")
    applicable = (
        option_dimension["source_projection_state"]
        != "source-backed-not-applicable"
    )
    enqueue_source_projected = (
        command["official_row"] in ENQUEUE_COMMAND_ROWS
        and option_dimension["source_projection_state"] == "projected"
    )
    options: dict[str, dict[str, Any]] = {}
    condition_clause_values: list[dict[str, Any]] = []
    for value in _source_values(option_dimension):
        if value.get("type") != "option":
            continue
        name = _text(value.get("term"), "source option term")
        dynamic_profile = value.get("dynamic_name_profile")
        if dynamic_profile is not None:
            if (
                name != CONDITION_NAME_OPTION
                or dynamic_profile != CONDITION_NAME_PROFILE
                or value.get("minimum_occurrences") != 1
                or value.get("maximum_occurrences") != 16
                or value.get("label_operand") not in {"optional", "forbidden"}
                or not isinstance(value.get("occurrence_limit_fragment_sha256"), str)
            ):
                raise DescriptorError("dynamic condition clause source shape differs")
            condition_clause_values.append(
                {
                    "name_authority": CONDITION_NAME_PROFILE,
                    "minimum_occurrences": 1,
                    "maximum_occurrences": 16,
                    "label_operand": value["label_operand"],
                    "occurrence_limit_fragment_sha256": value[
                        "occurrence_limit_fragment_sha256"
                    ],
                }
            )
            continue
        stack = tuple(
            _text(item, f"{name} option stack item")
            for item in _array(value.get("stack"), f"{name} option stack")
        )
        markers = [
            _text(marker, f"{name} argument marker")
            for marker in _array(value.get("arguments"), f"{name} arguments")
        ]
        if (
            enqueue_source_projected
            and name == "MAXLIFETIME"
            and len(stack) == 2
            and stack[1] in {"TASK", "UOW"}
        ):
            name = stack[1]
            stack = (name,)
            markers = ["none"]
        entry = options.setdefault(
            name,
            {
                "markers": set(),
                "directions": set(),
                "stacks": set(),
                "authorities": set(),
                "source_bounds": set(),
                "legalities": set(),
            },
        )
        entry["authorities"].add("command-source")
        entry["markers"].update(markers)
        if stack:
            entry["stacks"].add(stack)
        source_bound = _source_option_bound(value, name)
        if source_bound is not None:
            entry["source_bounds"].add(source_bound)
        legality = value.get("option_legality")
        if legality is not None:
            if legality not in {"structural", "bounded-prose"}:
                raise DescriptorError(f"{name} has malformed option legality")
            entry["legalities"].add(legality)
    for value in _source_values(direction_dimension):
        if value.get("type") != "operand-direction":
            continue
        name = _text(value.get("option"), "source operand option")
        marker = _text(value.get("marker"), f"{name} direction marker")
        direction = _text(value.get("direction"), f"{name} direction")
        if enqueue_source_projected:
            if name == "MAXLIFETIME" and marker == "none":
                continue
            if name in {"MAXLIFETIME", "RESOURCE"} and direction == "unknown":
                direction = "input"
        entry = options.setdefault(
            name,
            {
                "markers": set(),
                "directions": set(),
                "stacks": set(),
                "authorities": set(),
                "source_bounds": set(),
                "legalities": set(),
            },
        )
        entry["authorities"].add("command-source")
        entry["markers"].add(marker)
        entry["directions"].add(direction)

    if option_dimension["source_projection_state"] != "source-backed-not-applicable":
        for name, (markers, directions) in COMMON_COMMAND_OPTIONS.items():
            entry = options.setdefault(
                name,
                {
                    "markers": set(),
                    "directions": set(),
                    "stacks": set(),
                    "authorities": set(),
                    "source_bounds": set(),
                    "legalities": set(),
                },
            )
            entry["markers"].update(markers)
            entry["directions"].update(directions)
            entry["authorities"].add("global-command-format")

    grammar = _grammar_contract(dimensions)
    detachable_operand_options = _detachable_operand_options(grammar["variants"])

    entries = []
    for name in sorted(options):
        raw = options[name]
        markers = set(raw["markers"])
        directions = set(raw["directions"])
        if not directions and markers == {"none"}:
            directions.add("none")
        value_shape = _option_value_shape(markers)
        if value_shape == "value" and name in detachable_operand_options:
            value_shape = "optional-value"
        direction_status = (
            "resolved"
            if len(directions) == 1 and "unknown" not in directions
            else "bounded-ambiguity"
        )
        source_bounds = set(raw["source_bounds"])
        if value_shape == "flag":
            bound_status = "not-applicable"
            source_max_value_bytes = None
        elif value_shape in ("value", "optional-value") and len(source_bounds) == 1:
            bound_status = "resolved"
            source_max_value_bytes = next(iter(source_bounds))
        else:
            bound_status = "bounded-ambiguity"
            source_max_value_bytes = None
        entries.append(
            {
                "name": name,
                "authorities": sorted(raw["authorities"]),
                "argument_markers": sorted(markers),
                "value_shape": value_shape,
                "directions": sorted(directions),
                "direction_status": direction_status,
                "source_stacks": [list(stack) for stack in sorted(raw["stacks"])],
                "bound_status": bound_status,
                "source_max_value_bytes": source_max_value_bytes,
                "host_max_value_bytes": _host_option_value_limit(markers),
            }
        )
    source_direction_status = _source_contract_status(direction_dimension)
    if not applicable:
        direction_status = "not-applicable"
        bounds_status = "not-applicable"
    elif source_direction_status == "pending":
        direction_status = "pending"
        bounds_status = "pending"
    else:
        direction_status = (
            "bounded-ambiguity"
            if source_direction_status == "bounded-ambiguity"
            or any(entry["direction_status"] == "bounded-ambiguity" for entry in entries)
            else source_direction_status
        )
        entry_bound_statuses = {entry["bound_status"] for entry in entries}
        bounds_status = (
            "bounded-ambiguity"
            if "bounded-ambiguity" in entry_bound_statuses
            else "resolved"
        )
    top_level_options = set(_top_level_source_option_names(command, dimensions))
    option_status = _source_contract_status(option_dimension)
    source_entries = [
        entry for entry in options.values() if "command-source" in entry["authorities"]
    ]
    legality_values = {
        legality for entry in source_entries for legality in entry["legalities"]
    }
    missing_legality = any(not entry["legalities"] for entry in source_entries)
    if (
        applicable
        and option_status == "resolved"
        and (
            "bounded-prose" in legality_values
            or missing_legality
            or (
                option_dimension["source_projection_state"] == "projected"
                and not source_entries
            )
        )
    ):
        option_status = "bounded-ambiguity"
    condition_clause_material = {
        _canonical_json(value): value for value in condition_clause_values
    }
    if len(condition_clause_material) > 1:
        raise DescriptorError("dynamic condition clause facts conflict")
    condition_clauses = next(iter(condition_clause_material.values()), None)
    constraints = _option_constraints(grammar, top_level_options, applicable, option_status)
    if enqueue_source_projected:
        constraints["required"] = ["RESOURCE"]
        lifetime = {"members": ["MAXLIFETIME", "TASK", "UOW"], "required": False}
        constraints["alternatives"] = [
            *[group for group in constraints["alternatives"] if group != lifetime],
            lifetime,
        ]
        constraints["mutual_exclusions"] = [
            *[
                group
                for group in constraints["mutual_exclusions"]
                if group != lifetime["members"]
            ],
            lifetime["members"],
        ]
    return {
        "status": option_status,
        "direction_status": direction_status,
        "bounds_status": bounds_status,
        "entries": entries,
        "condition_clauses": condition_clauses,
        "constraints": constraints,
        "unknown_option": "reject",
        "duplicate_option": "reject",
        "max_argument_count": HOST_LIMITS["max_argument_count"],
        "max_aggregate_value_bytes": HOST_LIMITS["max_request_bytes"],
    }


def _condition_code_index(
    loaded_batches: list[tuple[Any, ...]],
) -> dict[str, int]:
    observed: dict[str, set[int]] = {"NORMAL": {0}}
    for _, _, _, _, source_rows, _ in loaded_batches:
        for row in source_rows.values():
            dimensions = _array(row.get("dimensions"), "source row dimensions")
            condition = next(
                (
                    dimension
                    for dimension in dimensions
                    if isinstance(dimension, dict) and dimension.get("name") == "conditions"
                ),
                None,
            )
            if condition is None:
                continue
            for raw_candidate in _array(condition.get("candidates"), "condition candidates"):
                candidate = _object(raw_candidate, "condition candidate")
                if candidate.get("kind") != "source-condition":
                    continue
                value = _object(candidate.get("candidate_value"), "condition candidate value")
                stack = _array(value.get("condition_stack"), "condition stack")
                head = re.fullmatch(r"RESP-([0-9]+)-(.+)", str(stack[0])) if stack else None
                if head is not None:
                    observed.setdefault(head.group(2), set()).add(int(head.group(1)))
    conflicts = {name: codes for name, codes in observed.items() if len(codes) != 1}
    if conflicts:
        raise DescriptorError(f"CICS condition names have conflicting RESP codes: {conflicts}")
    return {name: next(iter(codes)) for name, codes in observed.items()}


def _eib_response_contract(
    dimensions: list[dict[str, Any]], eibfn: str, condition_codes: dict[str, int]
) -> dict[str, Any]:
    condition_dimension = _source_dimension(dimensions, "conditions")
    primary: dict[tuple[int, str], list[dict[str, Any]]] = {}
    symbolic: set[str] = set()
    missing_resp2_triggers = False
    for fact in _array(condition_dimension.get("facts"), "condition source facts"):
        fact = _object(fact, "condition source fact")
        value = _object(fact.get("value"), "condition source fact value")
        if value.get("type") != "condition":
            continue
        stack = [
            _text(item, "condition stack item")
            for item in _array(value.get("condition_stack"), "condition stack")
        ]
        head = re.fullmatch(r"RESP-([0-9]+)-(.+)", stack[0]) if stack else None
        symbol = re.fullmatch(r"SYMBOL-(.+)", stack[0]) if stack else None
        if head is not None:
            key = (int(head.group(1)), head.group(2))
        elif symbol is not None and symbol.group(1) in condition_codes:
            key = (condition_codes[symbol.group(1)], symbol.group(1))
        else:
            symbolic.update(stack)
            continue
        response2 = []
        for item in stack[1:]:
            nested = re.fullmatch(r"RESP2-([0-9]+)", item)
            if nested is None:
                symbolic.add(item)
            else:
                response2.append(int(nested.group(1)))
        trigger_hashes = [
            _text(item, "condition trigger fragment sha256")
            for item in _array(
                value.get("trigger_fragment_sha256s"),
                "condition trigger fragment sha256s",
            )
        ] if "trigger_fragment_sha256s" in value else []
        if any(not re.fullmatch(r"sha256:[0-9a-f]{64}", value) for value in trigger_hashes):
            raise DescriptorError("condition trigger fragment digest is malformed")
        trigger_predicate = value.get("trigger_predicate")
        if trigger_predicate is not None and not isinstance(trigger_predicate, dict):
            raise DescriptorError("condition trigger predicate is malformed")
        candidate_ids = [
            _text(item, "condition candidate id")
            for item in _array(fact.get("candidate_ids"), "condition candidate ids")
        ]
        if response2 and (not trigger_hashes or trigger_predicate is None):
            missing_resp2_triggers = True
        for response2_value in response2 or [None]:
            primary.setdefault(key, []).append(
                {
                    "resp2": response2_value,
                    "condition_stack": stack,
                    "trigger_fragment_sha256s": trigger_hashes,
                    "trigger_predicate": trigger_predicate,
                    "trigger_status": (
                        "resolved"
                        if response2_value is not None and trigger_predicate is not None
                        else "bounded-ambiguity"
                        if response2_value is not None
                        else "not-applicable"
                    ),
                    "candidate_ids": candidate_ids,
                }
            )
    conditions = [
        {
            "resp": code,
            "condition": name,
            "outcomes": sorted(
                outcomes,
                key=lambda outcome: (
                    -1 if outcome["resp2"] is None else outcome["resp2"],
                    outcome["condition_stack"],
                    outcome["trigger_fragment_sha256s"],
                    _canonical_json(outcome["trigger_predicate"])
                    if outcome["trigger_predicate"] is not None
                    else b"",
                    outcome["candidate_ids"],
                ),
            ),
        }
        for (code, name), outcomes in sorted(primary.items())
    ]
    source_status = _source_contract_status(condition_dimension)
    source_applicable = (
        condition_dimension["source_projection_state"]
        != "source-backed-not-applicable"
    )
    status = (
        "bounded-ambiguity"
        if source_status == "bounded-ambiguity"
        or symbolic
        or missing_resp2_triggers
        else source_status
    )
    return {
        "status": status,
        "response_authority": "SSJL4D_6.x/reference-diagnostics/eib/dfha8me.html",
        "eibfn": eibfn,
        "eibfn_bytes": [int(eibfn[:2], 16), int(eibfn[2:], 16)],
        "normal_return": (
            {"condition": "NORMAL", "resp": 0, "resp2": 0}
            if source_applicable
            else None
        ),
        "conditions": conditions,
        "symbolic_fragments": sorted(symbolic),
        "condition_policy": {
            "authority": "global-command-format",
            "default": True,
            "nohandle": source_applicable,
            "resp": source_applicable,
            "resp2": source_applicable,
            "resp2_requires_resp": True,
        },
    }


def _dpl_restriction(
    dpl_state: str, restrictions: list[dict[str, Any]]
) -> tuple[dict[str, Any], bool]:
    empty = {
        "kind": dpl_state,
        "source_kind": None,
        "table_command": None,
        "prohibited_options": [],
        "requirement": "none",
        "restriction_predicates": [],
        "restriction_match": None,
        "condition": None,
        "resp2": None,
    }
    if dpl_state in {"allowed", "not-applicable"}:
        return (empty, not restrictions)
    if dpl_state != "restricted" or len(restrictions) != 1:
        return (
            {
                "kind": "bounded-ambiguity",
                "source_kind": None,
                "table_command": None,
                "prohibited_options": [],
                "requirement": "none",
                "restriction_predicates": [],
                "restriction_match": None,
                "condition": None,
                "resp2": None,
            },
            False,
        )
    restriction = restrictions[0]
    expected = {
        "kind",
        "source_kind",
        "table_command",
        "prohibited_options",
        "requirement",
        "restriction_predicates",
        "condition",
        "resp2",
    }
    restriction_fields = set(restriction)
    if not expected <= restriction_fields or not restriction_fields <= expected | {
        "restriction_match"
    }:
        return ({**empty, "kind": "bounded-ambiguity"}, False)
    kind = restriction.get("kind")
    source_kind = restriction.get("source_kind")
    table_command = restriction.get("table_command")
    options = restriction.get("prohibited_options")
    requirement = restriction.get("requirement")
    restriction_predicates = restriction.get("restriction_predicates")
    restriction_match = restriction.get("restriction_match")
    condition = restriction.get("condition")
    resp2 = restriction.get("resp2")
    if (
        kind not in {"prohibited", "option-restricted", "conditional"}
        or source_kind not in {"table", "prose"}
        or not isinstance(table_command, str)
        or not table_command
        or not isinstance(options, list)
        or any(not isinstance(option, str) or not option for option in options)
        or len(set(options)) != len(options)
        or requirement
        not in {"none", "synconreturn-required", "termid-not-intersystem-session"}
        or not isinstance(restriction_predicates, list)
        or any(
            predicate
            not in {
                "principal-facility",
                "connect-process-principal-facility-error-is-dpl",
            }
            for predicate in restriction_predicates
        )
        or len(set(restriction_predicates)) != len(restriction_predicates)
        or (
            restriction_match != "any-of"
            if options and restriction_predicates
            else restriction_match is not None
        )
        or condition not in {None, "INVREQ"}
        or (resp2 is not None and resp2 not in {1, 5, 200})
    ):
        return ({**empty, "kind": "bounded-ambiguity"}, False)
    return (
        {
            "kind": kind,
            "source_kind": source_kind,
            "table_command": table_command,
            "prohibited_options": sorted(options),
            "requirement": requirement,
            "restriction_predicates": restriction_predicates,
            "restriction_match": restriction_match,
            "condition": condition,
            "resp2": resp2,
        },
        True,
    )


def _applicability_contract(dimensions: list[dict[str, Any]]) -> dict[str, Any]:
    dimension = _source_dimension(dimensions, "context-applicability")
    observed: dict[str, set[str]] = {
        "local_task": set(),
        "dpl_server": set(),
        "threadsafe": set(),
        "cobol": set(),
    }
    selectors: set[str] = set()
    restrictions: dict[bytes, dict[str, Any]] = {}
    language_restrictions: dict[bytes, dict[str, Any]] = {}
    threadsafe_conditions: dict[bytes, dict[str, Any]] = {}
    context_predicates: dict[bytes, dict[str, Any]] = {}
    for value in _source_values(dimension):
        if value.get("type") != "context":
            continue
        selector = value.get("selector_id")
        if isinstance(selector, str):
            selectors.add(selector)
        predicate_id = value.get("predicate_id")
        predicate_state = value.get("predicate_state")
        if isinstance(predicate_id, str) and predicate_state == "bounded":
            predicate = {
                "predicate_id": predicate_id,
                "state": predicate_state,
            }
            context_predicates[_canonical_json(predicate)] = predicate
        applicability = value.get("applicability")
        if not isinstance(applicability, dict):
            continue
        for key in observed:
            candidate = applicability.get(key)
            if isinstance(candidate, str) and candidate:
                observed[key].add(candidate)
        restriction = applicability.get("dpl_restriction")
        if isinstance(restriction, dict):
            restrictions[_canonical_json(restriction)] = restriction
        language_restriction = applicability.get("language_restriction")
        if isinstance(language_restriction, dict):
            language_restrictions[_canonical_json(language_restriction)] = (
                language_restriction
            )
        threadsafe_condition = applicability.get("threadsafe_condition")
        if isinstance(threadsafe_condition, dict):
            threadsafe_conditions[_canonical_json(threadsafe_condition)] = (
                threadsafe_condition
            )
        for key in ("context_predicate", "predicate"):
            for predicate in (value.get(key), applicability.get(key)):
                if isinstance(predicate, dict):
                    context_predicates[_canonical_json(predicate)] = predicate

    source_status = _source_contract_status(dimension)
    if source_status == "not-applicable":
        values = {key: "not-applicable" for key in observed}
    else:
        values = {
            key: next(iter(items)) if len(items) == 1 else "bounded-ambiguity"
            for key, items in observed.items()
        }
    dpl_restriction, restriction_resolved = _dpl_restriction(
        values["dpl_server"], list(restrictions.values())
    )
    language_restriction_values = list(language_restrictions.values())
    language_restriction = (
        language_restriction_values[0]
        if len(language_restriction_values) == 1
        else {"profile": "not-applicable"}
        if source_status == "not-applicable"
        else {"profile": "bounded-ambiguity"}
    )
    threadsafe_condition_values = list(threadsafe_conditions.values())
    threadsafe_condition = (
        threadsafe_condition_values[0]
        if len(threadsafe_condition_values) == 1
        else None
    )
    language_resolved = source_status == "not-applicable" or (
        len(language_restriction_values) == 1
        and values["cobol"] in {"allowed", "not-applicable"}
    )
    threadsafe_resolved = values["threadsafe"] != "conditional" or (
        threadsafe_condition is not None
    )
    status = (
        "bounded-ambiguity"
        if source_status == "bounded-ambiguity"
        or any(value == "bounded-ambiguity" for value in values.values())
        or not restriction_resolved
        or not language_resolved
        or not threadsafe_resolved
        or bool(context_predicates)
        else source_status
    )
    return {
        "status": status,
        "local_task": values["local_task"],
        "dpl_server": values["dpl_server"],
        "threadsafe": values["threadsafe"],
        "cobol": values["cobol"],
        "dpl_restriction": dpl_restriction,
        "language_restriction": language_restriction,
        "threadsafe_condition": threadsafe_condition,
        "context_predicates": [
            context_predicates[key] for key in sorted(context_predicates)
        ],
        "source_selectors": sorted(selectors),
    }


def _resource_scope(family: str) -> str:
    return {
        "builtin-function-control": "current-transaction",
        "counter-control": "named-counter",
        "file-control": "dataset",
        "program-control": "program",
        "terminal-control": "terminal",
        "queue-control": "queue",
        "journal-control": "journal",
        "spool-control": "spool",
        "security-control": "security-profile",
        "web-control": "web-endpoint",
        "web-service-control": "web-service",
        "document-control": "document",
        "bts-control": "bts-object",
        "conversation-control": "conversation",
        "transform-control": "transform",
    }.get(family, "cics-transaction")


def _capabilities(family: str, mutating: bool | None) -> list[str]:
    dependencies = {"host.cics.execute", "host.security.authorize", "host.audit"}
    if family == "file-control":
        if mutating is not True:
            dependencies.add("host.dataset.read")
        if mutating is not False:
            dependencies.add("host.dataset.write")
    elif family == "program-control":
        dependencies.add("host.program.invoke")
    elif family == "terminal-control":
        dependencies.add("host.terminal")
    elif family == "spool-control":
        if mutating is not True:
            dependencies.add("host.spool.read")
        if mutating is not False:
            dependencies.add("host.spool.write")
    elif family in {"time", "interval-control"}:
        dependencies.add("host.clock")
    return sorted(dependencies)


def _resolved_memory_effects(options: dict[str, Any]) -> set[str]:
    effects = set()
    for entry in options["entries"]:
        if entry["direction_status"] != "resolved":
            continue
        directions = set(entry["directions"])
        if directions & {"input", "input-output"}:
            effects.add("memory-read")
        if directions & {"output", "input-output"}:
            effects.add("memory-write")
    return effects


def _ir_effects(
    family: str, mutating: bool | None, label: str, options: dict[str, Any]
) -> list[str]:
    effects = {"condition", "transaction", "security", "audit"}
    if family == "file-control":
        if mutating is not True:
            effects.add("dataset-read")
        if mutating is not False:
            effects.add("dataset-write")
    elif family == "program-control":
        effects.add("program-control")
    elif family == "terminal-control":
        if mutating is not True:
            effects.add("terminal-read")
        if mutating is not False:
            effects.add("terminal-write")
    elif family == "spool-control":
        effects.add("spool")
    elif family in {"time", "interval-control"}:
        effects.add("clock")
    effects.update(_resolved_memory_effects(options))
    if label.startswith(("DELAY", "SUSPEND", "WAIT")):
        effects.add("suspension")
    return sorted(effects)


def _resource_contract(
    command: dict[str, Any],
    family: str,
    options: dict[str, Any],
    runtime_operation: dict[str, Any] | None,
    source_not_applicable: bool,
) -> dict[str, Any]:
    if source_not_applicable:
        return {
            "status": "not-applicable",
            "scope": "not-applicable",
            "selectors": [],
            "ambiguous_selectors": [],
            "excluded_output_options": [],
            "selector_authority": "not-applicable",
            "selector_completeness": "not-applicable",
            "fallback_selector": None,
            "access_intent": "not-applicable",
            "authorization": "not-applicable",
        }

    selector_names = set(RESOURCE_SELECTOR_OPTIONS_BY_FAMILY.get(family, ()))
    selector_names.update(RESOURCE_SELECTOR_OPTIONS_BY_LABEL.get(command["label"], ()))
    selectors = []
    ambiguous_selectors = []
    excluded_output_options = []
    for entry in options["entries"]:
        if entry["name"] not in selector_names:
            continue
        directions = set(entry["directions"])
        selector = {
            "option": entry["name"],
            "directions": entry["directions"],
        }
        has_input = bool(directions & {"input", "input-output"})
        exact_input = has_input and entry["direction_status"] == "resolved"
        if exact_input:
            selectors.append(selector)
        elif has_input or directions == {"unknown"}:
            ambiguous_selectors.append(selector)
        elif directions and directions <= {"output", "unknown"}:
            excluded_output_options.append(entry["name"])

    if runtime_operation is None:
        status = "bounded-ambiguity"
        scope = "bounded-ambiguity"
        selector_authority = "source-candidate-only"
        selector_completeness = "bounded-ambiguity"
        access_intent = "bounded-ambiguity"
    elif (
        runtime_operation["operation"] not in TYPED_RUNTIME_OPERATIONS
        or runtime_operation["operation"] == "Read"
    ):
        # Legacy advertisement establishes a route, not exact resource intent.
        # READ is typed but UPDATE/TOKEN make its access mode option-sensitive.
        status = "bounded-ambiguity"
        scope = _resource_scope(family)
        selector_authority = "source-bound-runtime"
        selector_completeness = "bounded-ambiguity"
        access_intent = "bounded-ambiguity"
    else:
        mutating = runtime_operation["mutating"]
        status = "bounded-ambiguity" if ambiguous_selectors else "resolved"
        scope = _resource_scope(family)
        selector_authority = "source-bound-runtime"
        selector_completeness = status
        access_intent = (
            "execute"
            if family in {"program-control", "conversation-control"}
            else "update"
            if mutating
            else "read"
        )
    has_selector_candidate = bool(selectors or ambiguous_selectors)
    return {
        "status": status,
        "scope": scope,
        "selectors": selectors,
        "ambiguous_selectors": ambiguous_selectors,
        "excluded_output_options": sorted(excluded_output_options),
        "selector_authority": selector_authority,
        "selector_completeness": selector_completeness,
        "fallback_selector": (
            "current-transaction"
            if runtime_operation is not None
            and runtime_operation["operation"] in TYPED_RUNTIME_OPERATIONS
            and runtime_operation["operation"] != "Read"
            and status == "resolved"
            and not has_selector_candidate
            else None
        ),
        "access_intent": access_intent,
        "authorization": "typed-saf-before-dispatch",
    }


def _semantic_contract(
    command: dict[str, Any],
    source_dimensions: list[dict[str, Any]],
    runtime_operation: dict[str, Any] | None,
    condition_codes: dict[str, int],
    condition_name_authority: dict[str, Any] | None,
) -> dict[str, Any]:
    family = _contract_family(command, runtime_operation)
    grammar = _grammar_contract(source_dimensions)
    source_not_applicable = grammar["status"] == "not-applicable"
    options = _option_contract(command, source_dimensions)
    if options["condition_clauses"] is not None:
        if (
            condition_name_authority is None
            or condition_name_authority.get("profile") != CONDITION_NAME_PROFILE
        ):
            raise DescriptorError("dynamic condition clause lacks its name authority")
        options["condition_clauses"]["name_authority_sha256"] = (
            condition_name_authority["conditions_sha256"]
        )
    recognition = _recognition_contract(
        command, grammar, set(_top_level_source_option_names(command, source_dimensions))
    )
    # A catalog-qualified command form owns its qualifier even when IBM renders
    # the command and valued operand as one SVG keyword (for example, REQUEST
    # PASSTICKET followed by a separate parenthesized operand).  Preserve that
    # exact row-identity requirement in the option contract as well as the
    # compact registry.  Bounded constraint sets may still carry this verified
    # required subset without claiming that every prose rule is resolved.
    options["constraints"]["required"] = sorted(
        set(options["constraints"]["required"])
        | set(recognition["required_discriminator_options"])
    )
    eib_response = _eib_response_contract(
        source_dimensions, command["eibfn"], condition_codes
    )
    applicability = _applicability_contract(source_dimensions)
    readiness = (
        "typed-runtime"
        if runtime_operation is not None
        and runtime_operation["operation"] in TYPED_RUNTIME_OPERATIONS
        else "legacy-compatibility"
        if runtime_operation is not None
        else "unready"
    )
    slug = _command_slug(command["label"])
    registry = {
        "family": family,
        "handler_id": f"mainframe-env-cics.handlers.{family}.{slug}@1",
        "readiness": readiness,
        "advertised": runtime_operation is not None,
        "runtime_operation": (
            runtime_operation["operation"] if runtime_operation is not None else None
        ),
        "legacy_execution_options": (
            runtime_operation["legacy_execution_options"]
            if runtime_operation is not None
            else []
        ),
        "unready_result": None if runtime_operation is not None else "explicit-unsupported",
    }
    registry["handler_sha256"] = _handler_digest(registry)
    resource = _resource_contract(
        command, family, options, runtime_operation, source_not_applicable
    )
    option_sensitive_semantics = []
    if runtime_operation is not None and runtime_operation["operation"] == "Read":
        option_sensitive_semantics.append(
            {
                "options": ["TOKEN", "UPDATE"],
                "predicate": "presence-selects-read-for-update-or-lock-token",
                "affected_dimensions": [
                    "resource-key",
                    "capability-intent",
                    "effect-class",
                    "cancellation",
                    "audit",
                    "recovery",
                ],
                "status": "bounded-ambiguity",
            }
        )
    elif runtime_operation is not None and runtime_operation["operation"] == "Syncpoint":
        option_sensitive_semantics.append(
            {
                "options": ["ROLLBACK"],
                "predicate": "presence-selects-rollback-otherwise-commit",
                "affected_dimensions": ["recovery"],
                "status": "resolved",
            }
        )

    if source_not_applicable:
        capability = {
            "status": "not-applicable",
            "route": None,
            "required": [],
        }
        effect = {
            "status": "not-applicable",
            "class": "not-applicable",
            "mutating": None,
            "ir_effects": [],
            "canonical_identity": None,
        }
        cancellation = {
            "status": "not-applicable",
            "finite_deadline": None,
            "live_probe": "not-applicable",
            "post_dispatch": "not-applicable",
        }
        audit = {
            "status": "not-applicable",
            "required": None,
            "deny_and_failure_paths": None,
            "atomic_with_effect_and_lifecycle": None,
        }
        recovery = {
            "status": "not-applicable",
            "durable_effect_intent": None,
            "uow_participation": "not-applicable",
            "prepare": "not-applicable",
            "rollback": "not-applicable",
            "compensation": "not-applicable",
            "replay_namespace": None,
            "unknown_outcome": "not-applicable",
            "automatic_redispatch": False,
            "fenced_reconciliation": None,
            "retention_protected": None,
        }
    elif runtime_operation is None:
        # No source dimension establishes the effect of an unready command.
        # Preserve the architectural safety envelope without guessing whether
        # the future handler mutates, observes, or only synchronizes.
        capability = {
            "status": "bounded-ambiguity",
            "route": "host.cics.execute",
            "required": ["host.audit", "host.cics.execute", "host.security.authorize"],
        }
        effect = {
            "status": "bounded-ambiguity",
            "class": "bounded-ambiguity",
            "mutating": None,
            "ir_effects": [],
            "canonical_identity": "mainframe-env.effect-canonical@1",
        }
        cancellation = {
            "status": "bounded-ambiguity",
            "finite_deadline": True,
            "live_probe": "before-dispatch-and-blocking-boundaries",
            "post_dispatch": "bounded-ambiguity",
        }
        audit = {
            "status": "bounded-ambiguity",
            "required": True,
            "deny_and_failure_paths": True,
            "atomic_with_effect_and_lifecycle": None,
        }
        recovery = {
            "status": "bounded-ambiguity",
            "durable_effect_intent": None,
            "uow_participation": "bounded-ambiguity",
            "prepare": "bounded-ambiguity",
            "rollback": "bounded-ambiguity",
            "compensation": "bounded-ambiguity",
            "replay_namespace": None,
            "unknown_outcome": "bounded-ambiguity",
            "automatic_redispatch": False,
            "fenced_reconciliation": None,
            "retention_protected": None,
        }
    elif readiness == "legacy-compatibility":
        # A legacy raw route proves advertisement and compatibility only.  Its
        # coarse historic mutation bit is not semantic authority for task
        # state, browse locks, cancellation, audit atomicity, or recovery.
        capability = {
            "status": "bounded-ambiguity",
            "route": "host.cics.execute",
            "required": _capabilities(family, None),
        }
        effect = {
            "status": "bounded-ambiguity",
            "class": "bounded-ambiguity",
            "mutating": None,
            "ir_effects": _ir_effects(family, None, command["label"], options),
            "canonical_identity": "mainframe-env.effect-canonical@1",
        }
        cancellation = {
            "status": "bounded-ambiguity",
            "finite_deadline": True,
            "live_probe": "before-dispatch-and-blocking-boundaries",
            "post_dispatch": "bounded-ambiguity",
        }
        audit = {
            "status": "bounded-ambiguity",
            "required": True,
            "deny_and_failure_paths": True,
            "atomic_with_effect_and_lifecycle": None,
        }
        recovery = {
            "status": "bounded-ambiguity",
            "durable_effect_intent": None,
            "uow_participation": "bounded-ambiguity",
            "prepare": "bounded-ambiguity",
            "rollback": "bounded-ambiguity",
            "compensation": "bounded-ambiguity",
            "replay_namespace": None,
            "unknown_outcome": "bounded-ambiguity",
            "automatic_redispatch": False,
            "fenced_reconciliation": None,
            "retention_protected": None,
        }
    else:
        mutating = runtime_operation["mutating"]
        operation = runtime_operation["operation"]
        typed_effects = set(TYPED_RUNTIME_IR_EFFECTS[operation]) | {
            "security",
            "audit",
        }
        if not _resolved_memory_effects(options) <= typed_effects:
            raise DescriptorError(
                f"{command['official_row']} typed IR effects omit resolved memory flow"
            )
        option_sensitive = operation == "Read"
        capability = {
            "status": "bounded-ambiguity" if option_sensitive else "resolved",
            "route": "host.cics.execute",
            "required": _capabilities(family, None if option_sensitive else mutating),
        }
        effect = {
            "status": "bounded-ambiguity" if option_sensitive else "resolved",
            "class": (
                "bounded-ambiguity"
                if option_sensitive
                else "mutating"
                if mutating
                else "observational"
            ),
            "mutating": None if option_sensitive else mutating,
            "ir_effects": sorted(typed_effects),
            "canonical_identity": "mainframe-env.effect-canonical@1",
        }
        cancellation = {
            "status": "bounded-ambiguity" if option_sensitive else "resolved",
            "finite_deadline": True,
            "live_probe": "before-dispatch-and-blocking-boundaries",
            "post_dispatch": (
                "bounded-ambiguity"
                if option_sensitive
                else "unknown-outcome"
                if mutating
                else "cancelled"
            ),
        }
        audit = {
            "status": "bounded-ambiguity" if option_sensitive else "resolved",
            "required": True,
            "deny_and_failure_paths": True,
            "atomic_with_effect_and_lifecycle": None if option_sensitive else mutating,
        }
        if option_sensitive:
            recovery = {
                "status": "bounded-ambiguity",
                "durable_effect_intent": None,
                "uow_participation": "bounded-ambiguity",
                "prepare": "bounded-ambiguity",
                "rollback": "bounded-ambiguity",
                "compensation": "bounded-ambiguity",
                "replay_namespace": None,
                "unknown_outcome": "bounded-ambiguity",
                "automatic_redispatch": False,
                "fenced_reconciliation": None,
                "retention_protected": None,
            }
        elif not mutating:
            recovery = {
                "status": "resolved",
                "durable_effect_intent": False,
                "uow_participation": "not-applicable",
                "prepare": "not-applicable",
                "rollback": "not-applicable",
                "compensation": "not-applicable",
                "replay_namespace": None,
                "unknown_outcome": "not-applicable",
                "automatic_redispatch": False,
                "fenced_reconciliation": False,
                "retention_protected": False,
            }
        elif runtime_operation["operation"] == "Syncpoint":
            recovery = {
                "status": "resolved",
                "durable_effect_intent": True,
                "uow_participation": "explicit-command-boundary",
                "prepare": "command-defined",
                "rollback": "command-defined",
                "compensation": "command-defined",
                "replay_namespace": "cics-effect-replay-v1",
                "unknown_outcome": "preserve",
                "automatic_redispatch": False,
                "fenced_reconciliation": True,
                "retention_protected": True,
            }
        else:
            # The runtime descriptor establishes mutation, not whether the IBM
            # service participates in a recoverable CICS UOW.  That distinction
            # stays bounded per row until a family vertical slice supplies it.
            recovery = {
                "status": "bounded-ambiguity",
                "durable_effect_intent": True,
                "uow_participation": "bounded-ambiguity",
                "prepare": "bounded-ambiguity",
                "rollback": "bounded-ambiguity",
                "compensation": "bounded-ambiguity",
                "replay_namespace": None,
                "unknown_outcome": "preserve",
                "automatic_redispatch": False,
                "fenced_reconciliation": None,
                "retention_protected": None,
            }
    return {
        "grammar": grammar,
        "option_sensitive_semantics": option_sensitive_semantics,
        "options": options,
        "eib_response": eib_response,
        "applicability": applicability,
        "resource": resource,
        "capability": capability,
        "effect": effect,
        "cancellation": cancellation,
        "audit": audit,
        "recovery": recovery,
        "registry": registry,
    }


def _dimension_dispositions(
    source_dimensions: list[dict[str, Any]], semantic: dict[str, Any]
) -> list[dict[str, str]]:
    direct = {
        "grammar": semantic["grammar"]["status"],
        "option-legality": semantic["options"]["status"],
        "operand-direction": semantic["options"]["direction_status"],
        "option-bounds": semantic["options"]["bounds_status"],
        "resource-key": semantic["resource"]["status"],
        "capability-intent": semantic["capability"]["status"],
        "eib-response": semantic["eib_response"]["status"],
        "conditions": semantic["eib_response"]["status"],
        "context-applicability": semantic["applicability"]["status"],
        "effect-class": semantic["effect"]["status"],
        "cancellation": semantic["cancellation"]["status"],
        "audit": semantic["audit"]["status"],
        "recovery": semantic["recovery"]["status"],
    }
    return [{"name": name, "status": direct[name]} for name in CONTRACT_DIMENSIONS]


def _validate_semantic_contract(command: dict[str, Any], semantic: dict[str, Any]) -> None:
    applicable = semantic["grammar"]["status"] != "not-applicable"
    if (
        applicable
        and semantic["grammar"]["status"] == "resolved"
        and not semantic["grammar"]["variants"]
    ):
        raise DescriptorError(
            f"{command['official_row']} resolved applicable grammar has no variants"
        )

    options = semantic["options"]
    if any(entry["name"] == CONDITION_NAME_OPTION for entry in options["entries"]):
        raise DescriptorError(
            f"{command['official_row']} dynamic condition name leaked into static options"
        )
    condition_clauses = options["condition_clauses"]
    if condition_clauses is not None and (
        command["label"] not in {"HANDLE CONDITION", "IGNORE CONDITION"}
        or condition_clauses["name_authority"] != CONDITION_NAME_PROFILE
        or condition_clauses["minimum_occurrences"] != 1
        or condition_clauses["maximum_occurrences"] != 16
    ):
        raise DescriptorError(
            f"{command['official_row']} dynamic condition clause differs"
        )
    for entry in options["entries"]:
        source_bound = entry["source_max_value_bytes"]
        if entry["bound_status"] == "resolved" and not isinstance(source_bound, int):
            raise DescriptorError(
                f"{command['official_row']} {entry['name']} resolved without IBM source bound"
            )
        if source_bound is not None and entry["bound_status"] != "resolved":
            raise DescriptorError(
                f"{command['official_row']} {entry['name']} source bound is not resolved"
            )
    if options["bounds_status"] == "resolved" and any(
        entry["value_shape"] != "flag" and entry["bound_status"] != "resolved"
        for entry in options["entries"]
    ):
        raise DescriptorError(
            f"{command['official_row']} option bounds resolved from host ceiling"
        )

    entries = {entry["name"]: entry for entry in options["entries"]}
    for selector in semantic["resource"]["selectors"]:
        entry = entries[selector["option"]]
        directions = set(selector["directions"])
        if (
            entry["direction_status"] != "resolved"
            or not directions & {"input", "input-output"}
            or "unknown" in directions
        ):
            raise DescriptorError(
                f"{command['official_row']} resource selector lacks resolved input direction"
            )
    for option in semantic["resource"]["excluded_output_options"]:
        if set(entries[option]["directions"]) & {"input", "input-output"}:
            raise DescriptorError(
                f"{command['official_row']} input selector was excluded as output-only"
            )
    if semantic["resource"]["fallback_selector"] is not None and (
        semantic["registry"]["readiness"] == "unready"
        or semantic["resource"]["status"] != "resolved"
    ):
        raise DescriptorError(
            f"{command['official_row']} guessed a current-transaction resource fallback"
        )

    execution_dimensions = (
        "resource",
        "capability",
        "effect",
        "cancellation",
        "audit",
        "recovery",
    )
    if not applicable and any(
        semantic[name]["status"] != "not-applicable" for name in execution_dimensions
    ):
        raise DescriptorError(
            f"{command['official_row']} internal-only row has execution semantics"
        )
    registry = semantic["registry"]
    if registry["readiness"] == "legacy-compatibility" and any(
        semantic[name]["status"] != "bounded-ambiguity"
        for name in ("resource", "capability", "effect", "cancellation", "audit", "recovery")
    ):
        raise DescriptorError(
            f"{command['official_row']} legacy route claims resolved execution semantics"
        )
    runtime_operation = registry["runtime_operation"]
    if runtime_operation in TYPED_RUNTIME_IR_EFFECTS:
        expected_effects = set(TYPED_RUNTIME_IR_EFFECTS[runtime_operation]) | {
            "audit",
            "security",
        }
        if set(semantic["effect"]["ir_effects"]) != expected_effects:
            raise DescriptorError(
                f"{command['official_row']} typed IR effect authority differs"
            )
    if runtime_operation == "Read" and (
        semantic["effect"]["status"] != "bounded-ambiguity"
        or semantic["effect"]["mutating"] is not None
        or semantic["resource"]["access_intent"] != "bounded-ambiguity"
        or semantic["capability"]["status"] != "bounded-ambiguity"
        or semantic["recovery"]["status"] != "bounded-ambiguity"
        or not semantic["option_sensitive_semantics"]
    ):
        raise DescriptorError(
            f"{command['official_row']} READ UPDATE/TOKEN semantics were flattened"
        )
    if runtime_operation == "Syncpoint" and semantic[
        "option_sensitive_semantics"
    ] != [
        {
            "options": ["ROLLBACK"],
            "predicate": "presence-selects-rollback-otherwise-commit",
            "affected_dimensions": ["recovery"],
            "status": "resolved",
        }
    ]:
        raise DescriptorError(
            f"{command['official_row']} SYNCPOINT ROLLBACK semantics were flattened"
        )
    if registry["readiness"] == "unready" and applicable:
        if (
            semantic["resource"]["status"] != "bounded-ambiguity"
            or semantic["resource"]["scope"] != "bounded-ambiguity"
            or semantic["resource"]["selector_completeness"] != "bounded-ambiguity"
            or semantic["resource"]["selector_authority"] != "source-candidate-only"
            or semantic["resource"]["fallback_selector"] is not None
        ):
            raise DescriptorError(
                f"{command['official_row']} unready resource scope was guessed"
            )
        if semantic["effect"]["status"] != "bounded-ambiguity":
            raise DescriptorError(
                f"{command['official_row']} unready effect was guessed"
            )
        if semantic["effect"]["mutating"] is not None:
            raise DescriptorError(
                f"{command['official_row']} unready mutation flag was guessed"
            )


def _catalog_recognition_head(
    label: str, option_names: set[str]
) -> tuple[list[str], list[str]]:
    tokens = label.split()
    discriminator_options = []
    while len(tokens) > 1 and tokens[-1] in option_names:
        discriminator_options.append(tokens.pop())
    return tokens, sorted(discriminator_options)


def _syntax_recognition_heads(
    variant: dict[str, Any], option_names: set[str], catalog_first: str
) -> tuple[list[list[str]], set[str]]:
    panel = variant.get("panel")
    panel_role = panel.get("role") if isinstance(panel, dict) else None
    syntax_head = variant.get("syntax_head")
    if syntax_head is not None and not isinstance(syntax_head, str):
        raise DescriptorError("syntax head is malformed")
    heads: list[list[str]] = [[]]
    discriminators: set[str] = set()
    tokens = _array(variant.get("tokens"), "recognition syntax tokens")
    for index, raw_token in enumerate(tokens):
        token = _object(raw_token, "recognition syntax token")
        if token.get("kind") != "keyword":
            break
        value = _text(token.get("value"), "recognition keyword")
        words = value.split()
        final = words[-1]
        option = final.removesuffix("(")
        next_token = (
            _object(tokens[index + 1], "recognition syntax token")
            if index + 1 < len(tokens)
            else None
        )
        separated_operand = (
            next_token is not None
            and next_token.get("kind") == "delimiter"
            and next_token.get("value") == "("
        )
        if separated_operand and final in option_names:
            for head in heads:
                head.extend(words[:-1])
            discriminators.add(final)
            break
        if final.endswith("(") and option in option_names:
            for head in heads:
                head.extend(words[:-1])
            if len(words) > 1:
                discriminators.add(option)
            break
        if value in option_names:
            discriminators.add(value)
            break
        relation = _text(token.get("relation"), "recognition keyword relation")
        if relation == "optional":
            heads = [*heads, [*heads[-1], *words]]
        else:
            for head in heads:
                head.extend(words)

    heads = [head for head in heads if head and catalog_first in head]
    if panel_role == "continuation" and not heads:
        return [], set()
    return heads, discriminators


def _recognition_contract(
    command: dict[str, Any], grammar: dict[str, Any], option_names: set[str]
) -> dict[str, Any]:
    catalog_head, catalog_discriminators = _catalog_recognition_head(
        command["label"], option_names
    )
    heads: set[tuple[str, ...]] = set()
    discriminators: set[str] = set()
    required_discriminators: set[str] = set()
    forbidden_discriminators: set[str] = set()
    source_heads = set()
    syntax_discriminator_sets: list[set[str]] = []
    for variant in grammar["variants"]:
        variant_heads, variant_discriminators = _syntax_recognition_heads(
            variant, option_names, command["label"].split()[0]
        )
        source_heads.update(tuple(head) for head in variant_heads)
        if variant_heads:
            syntax_discriminator_sets.append(set(variant_discriminators))
        raw_discriminators = variant.get("identity_discriminators", [])
        if not isinstance(raw_discriminators, list):
            raise DescriptorError("syntax identity discriminators are malformed")
        for raw_discriminator in raw_discriminators:
            discriminator = _object(
                raw_discriminator, "syntax identity discriminator"
            )
            if set(discriminator) != {"name", "state"}:
                raise DescriptorError("syntax identity discriminator fields differ")
            name = _text(discriminator.get("name"), "syntax discriminator name")
            if not re.fullmatch(r"[A-Z][A-Z0-9-]*", name):
                raise DescriptorError("syntax identity discriminator name is malformed")
            state = discriminator.get("state")
            if state == "present":
                required_discriminators.add(name)
            elif state == "absent":
                forbidden_discriminators.add(name)
            else:
                raise DescriptorError("syntax identity discriminator state differs")
            discriminators.add(name)
    if syntax_discriminator_sets:
        # The first option after a syntax head is not necessarily command
        # identity: it can be optional (HANDLE ABEND CANCEL), one branch of a
        # choice (SEND PAGE RELEASE/RETAIN), or one of two modes (TRACE ON/OFF).
        # Retain a syntax-inferred discriminator only when the official catalog
        # label also names that option as the row qualifier.  Explicit source
        # identity_discriminators above remain authoritative independently.
        catalog_qualified = set.intersection(*syntax_discriminator_sets) & set(
            catalog_discriminators
        )
        discriminators.update(catalog_qualified)
        required_discriminators.update(catalog_qualified)
    if source_heads:
        heads.update(source_heads)
    else:
        heads.add(tuple(catalog_head))
        discriminators.update(catalog_discriminators)
    if command["label"] == "HANDLE AID":
        heads = {("HANDLE", "AID")}
    if grammar["status"] == "pending":
        status = "pending"
    elif grammar["status"] == "not-applicable":
        status = "not-applicable"
    elif grammar["status"] == "bounded-ambiguity" or not source_heads:
        status = "bounded-ambiguity"
    else:
        status = "resolved"
    return {
        "recognition_status": status,
        "recognition_heads": [list(head) for head in sorted(heads)],
        "discriminator_options": sorted(discriminators),
        "required_discriminator_options": sorted(required_discriminators),
        "forbidden_discriminator_options": sorted(forbidden_discriminators),
    }


def _registry_row_material(
    command: dict[str, Any],
    source_dimensions: list[dict[str, Any]],
    semantic: dict[str, Any],
) -> dict[str, Any]:
    top_level_names = set(_top_level_source_option_names(command, source_dimensions))
    option_entries = {
        entry["name"]: entry for entry in semantic["options"]["entries"]
    }
    missing = top_level_names - option_entries.keys()
    if missing:
        raise DescriptorError(
            f"{command['official_row']} registry options lack shapes {sorted(missing)}"
        )
    recognition = _recognition_contract(command, semantic["grammar"], top_level_names)
    if (
        not recognition["recognition_heads"]
        or len({tuple(head) for head in recognition["recognition_heads"]})
        != len(recognition["recognition_heads"])
        or not set(recognition["discriminator_options"])
        <= top_level_names | set(recognition["forbidden_discriminator_options"])
        or not set(recognition["required_discriminator_options"]) <= top_level_names
        or set(recognition["required_discriminator_options"])
        & set(recognition["forbidden_discriminator_options"])
    ):
        raise DescriptorError(f"{command['official_row']} registry recognition shape differs")
    return {
        "official_row": command["official_row"],
        "label_tokens": command["label"].split(),
        **recognition,
        "cobol_applicability": semantic["applicability"]["cobol"],
        "options": [
            {
                "name": name,
                "value_shape": option_entries[name]["value_shape"],
                "direction": (
                    option_entries[name]["directions"][0]
                    if option_entries[name]["direction_status"] == "resolved"
                    else "bounded-ambiguity"
                ),
                "source_max_value_bytes": option_entries[name][
                    "source_max_value_bytes"
                ],
            }
            for name in sorted(top_level_names)
        ],
        "condition_clauses": semantic["options"]["condition_clauses"],
        "constraints": semantic["options"]["constraints"],
        "eibfn": command["eibfn"],
        **semantic["registry"],
    }


def _participant_contract(batches: list[dict[str, Any]], frozen: bool) -> dict[str, Any]:
    commands = [command for batch in batches for command in batch["commands"]]
    mutating_rows = sum(
        command["contract"]["effect"]["mutating"] is True for command in commands
    )
    bounded_effect_rows = sum(
        command["contract"]["effect"]["status"] == "bounded-ambiguity"
        for command in commands
    )
    explicit_uow_boundary_rows = sum(
        command["contract"]["recovery"]["uow_participation"]
        == "explicit-command-boundary"
        for command in commands
    )
    bounded_uow_rows = sum(
        command["contract"]["recovery"]["uow_participation"]
        == "bounded-ambiguity"
        for command in commands
    )
    return {
        "id": "INT-1601.cics-participant",
        "status": "bounded-ambiguity" if frozen else "source-pending",
        "owner": "shared-execution-coordinator-and-cics-provider",
        "mutating_rows": mutating_rows,
        "bounded_effect_rows": bounded_effect_rows,
        "explicit_uow_boundary_rows": explicit_uow_boundary_rows,
        "bounded_uow_rows": bounded_uow_rows,
        "execution_credit": 0,
        "prepare_before_dispatch": {
            "durable_effect_intent": True,
            "universal_prepare": False,
            "uow_applicability": "per-row-recovery-contract",
            "provider_dispatch_after_intent": True,
        },
        "completion": {
            "outcomes": ["committed", "rolled-back", "failed", "unknown-outcome"],
            "decision_owner": "command-family-handler",
        },
        "compensation": {
            "automatic": False,
            "scope": "service-specific-explicit-only",
        },
        "mutation_identity": {
            "canonical_schema": "mainframe-env.effect-canonical@1",
            "request_schema": "mainframe-env.cics.request@1",
            "idempotency_scope": "execution-run-unit-effect-sequence",
        },
        "effect_order": [
            "validate-bounds-and-deadline",
            "authorize-typed-resource",
            "persist-audit-and-effect-intent",
            "dispatch-once",
            "persist-result-or-unknown-outcome",
        ],
        "fencing": {
            "lease_epoch": True,
            "stale_owner_rejected": True,
        },
        "cancellation": {
            "live_probe": True,
            "finite_deadline": True,
            "post_dispatch_unknown": True,
        },
        "unknown_outcome": {
            "preserved": True,
            "automatic_redispatch": False,
        },
        "reconciliation": {
            "mode": "service-specific-fenced-observation",
            "provider_replay_namespace": "cics-effect-replay-v1",
        },
        "schema_compatibility": {
            "uow_namespace": "cics-uow",
            "undo_namespace": "cics-uow-undo",
            "read_versions": "current-and-retained-legacy",
            "rollback": "stop-admission-and-restore-compatible-backup",
        },
        "retention": {
            "protect_live_checkpoint_audit_replay": True,
            "deadline_is_conservative_lower_bound": True,
        },
        "backend_applicability": ["memory", "sqlite", "postgresql"],
    }


def build_contracts(root: Path = ROOT) -> dict[str, Any]:
    """Build the zero-credit 263-row command contract and registry shape."""
    catalog = load_catalog(root)
    commands = catalog["_application_commands"]
    existing_runtime = {
        row["official_row"]: row
        for row in catalog["_runtime_operations"]
        if row["interface"] == "api"
    }
    if len(existing_runtime) != 43:
        raise DescriptorError("CICS application runtime set must remain exactly 43 rows")

    loaded_batches = []
    for batch_id, start, end, projection_path, review_path in CONTRACT_BATCHES:
        binding, source_rows, review = _load_source_batch(
            root,
            batch_id,
            start,
            end,
            projection_path,
            review_path,
            commands,
        )
        loaded_batches.append((batch_id, start, end, binding, source_rows, review))
    frozen = all(
        review["accepted_for_freeze"] and len(source_rows) == end - start + 1
        for _, start, end, _, source_rows, review in loaded_batches
    )
    condition_codes = _condition_code_index(loaded_batches)
    condition_name_authorities = [
        review["condition_name_authority"]
        for _, _, _, _, source_rows, review in loaded_batches
        if source_rows and review["condition_name_authority"] is not None
    ]
    condition_name_authority = (
        condition_name_authorities[0] if condition_name_authorities else None
    )
    if any(
        authority != condition_name_authority
        for authority in condition_name_authorities[1:]
    ) or (frozen and len(condition_name_authorities) != len(loaded_batches)):
        raise DescriptorError("CICS source batches disagree on condition name authority")

    fact_groups = 0
    source_candidate_references = 0
    projected_commands = 0
    reviewed_commands = 0
    ambiguous_row_ids: set[str] = set()
    verified_candidates = 0
    ambiguous_candidates = 0
    registry_rows = []
    family_counts: dict[str, int] = {}
    batches = []
    for batch_id, start, end, binding, source_rows, review in loaded_batches:
        review_status = review["status"]
        verified_candidates += review["verified_candidates"]
        ambiguous_candidates += review["ambiguous_candidates"]
        batch_commands = []
        for command in commands[start - 1 : end]:
            source_row = source_rows.get(command["official_row"])
            source_accepted = False
            dimensions = []
            row_issue_ids: set[str] = set()
            scoped_ambiguity = review["ambiguity_scope"].get(
                command["official_row"], frozenset()
            )
            if source_row is None:
                for _, contract_name, _ in SOURCE_DIMENSIONS:
                    dimensions.append(
                        {
                            "name": contract_name,
                            "source_projection_state": "not-projected",
                            "verification_status": "not-projected",
                            "facts": [],
                            "issue_ids": [],
                        }
                    )
                source_status = "not-projected"
            else:
                projected_commands += 1
                for raw_dimension, (source_name, contract_name, candidate_kind) in zip(
                    source_row["dimensions"], SOURCE_DIMENSIONS, strict=True
                ):
                    dimension = _object(raw_dimension, f"{command['official_row']} {source_name}")
                    projection_state = _text(
                        dimension.get("state"), f"{command['official_row']} {source_name} state"
                    )
                    facts = _source_fact_groups(
                        _array(
                            dimension.get("candidates"),
                            f"{command['official_row']} {source_name} candidates",
                        ),
                        candidate_kind,
                        f"{command['official_row']} {source_name}",
                    )
                    issue_ids = []
                    for raw_issue in _array(
                        dimension.get("issues"),
                        f"{command['official_row']} {source_name} issues",
                    ):
                        issue = _object(raw_issue, f"{command['official_row']} source issue")
                        issue_id = _text(issue.get("issue_id"), "source issue id")
                        issue_ids.append(issue_id)
                        row_issue_ids.add(issue_id)
                    issue_ids.sort()
                    bounded_ambiguity = source_name in scoped_ambiguity or bool(
                        set(issue_ids) & review["ambiguity_issue_ids"]
                    )
                    dimensions.append(
                        {
                            "name": contract_name,
                            "source_projection_state": projection_state,
                            "verification_status": _source_verification_status(
                                review_status, projection_state, bounded_ambiguity
                            ),
                            "facts": facts,
                            "issue_ids": issue_ids,
                        }
                    )
                    fact_groups += len(facts)
                    source_candidate_references += sum(
                        len(fact["candidate_ids"]) for fact in facts
                    )
                accepted_dimensions = {
                    "verified",
                    "verified-absent",
                    "not-applicable",
                    "verified-with-bounded-ambiguity",
                }
                source_accepted = review_status in ACCEPTED_SOURCE_REVIEWS and all(
                    dimension["verification_status"] in accepted_dimensions
                    for dimension in dimensions
                )
                if source_accepted:
                    reviewed_commands += 1
                    if any(
                        dimension["verification_status"]
                        == "verified-with-bounded-ambiguity"
                        for dimension in dimensions
                    ):
                        source_status = "bounded-ambiguity"
                        ambiguous_row_ids.add(command["official_row"])
                    else:
                        source_status = "verified"
                elif review_status == "blocked":
                    source_status = "blocked-review"
                elif review_status == "stale":
                    source_status = "stale-review"
                else:
                    source_status = "projected-unreviewed"

            runtime_operation = existing_runtime.get(command["official_row"])
            semantic = _semantic_contract(
                command,
                dimensions,
                runtime_operation,
                condition_codes,
                condition_name_authority,
            )
            if source_accepted:
                unknown_execution_options = set(
                    semantic["registry"]["legacy_execution_options"]
                ) - {
                    option["name"] for option in semantic["options"]["entries"]
                }
                if unknown_execution_options:
                    raise DescriptorError(
                        f"{command['official_row']} legacy execution options are not "
                        f"source-reviewed: {sorted(unknown_execution_options)}"
                    )
            _validate_semantic_contract(command, semantic)
            dispositions = _dimension_dispositions(dimensions, semantic)
            if frozen:
                pending = [
                    row["name"] for row in dispositions if row["status"] == "pending"
                ]
                if pending:
                    raise DescriptorError(
                        f"{command['official_row']} frozen contract has pending dimensions {pending}"
                    )
                unresolved = []
                bounded = [
                    row["name"]
                    for row in dispositions
                    if row["status"] == "bounded-ambiguity"
                ]
                contract_status = (
                    "frozen-with-bounded-ambiguity" if bounded else "frozen"
                )
            else:
                unresolved = list(CONTRACT_DIMENSIONS)
                bounded = []
                contract_status = "source-pending"

            registry = semantic["registry"]
            registry_row = _registry_row_material(command, dimensions, semantic)
            registry_rows.append(registry_row)
            family_counts[registry["family"]] = family_counts.get(registry["family"], 0) + 1
            output_command = {
                **command,
                "source_status": source_status,
                "contract_status": contract_status,
                "implementation_status": (
                    registry["readiness"]
                    if runtime_operation is not None
                    else "unimplemented"
                ),
                "registration_status": registry["readiness"],
                "existing_runtime_operation": registry["runtime_operation"],
                "source_dimensions": dimensions,
                "dimension_dispositions": dispositions,
                "unresolved_dimensions": unresolved,
                "bounded_ambiguity_dimensions": bounded,
                "source_issue_ids": sorted(row_issue_ids),
                "contract": semantic,
            }
            output_command["row_contract_sha256"] = _row_contract_digest(output_command)
            batch_commands.append(output_command)
        batches.append(
            {
                "batch_id": batch_id,
                "ordinal_start": start,
                "ordinal_end": end,
                "command_count": end - start + 1,
                "source_input": binding,
                "commands": batch_commands,
            }
        )

    handler_ids = [row["handler_id"] for row in registry_rows]
    typed_rows = [row for row in registry_rows if row["readiness"] == "typed-runtime"]
    legacy_rows = [
        row for row in registry_rows if row["readiness"] == "legacy-compatibility"
    ]
    advertised_rows = [row for row in registry_rows if row["advertised"]]
    unready_rows = [row for row in registry_rows if row["readiness"] == "unready"]
    recognition_signatures: dict[bytes, str] = {}
    for row in registry_rows:
        for head in row["recognition_heads"]:
            signature = _canonical_json(
                {
                    "head": head,
                    "required_discriminators": row[
                        "required_discriminator_options"
                    ],
                    "forbidden_discriminators": row[
                        "forbidden_discriminator_options"
                    ],
                    "discriminators": row["discriminator_options"],
                    "required_options": row["constraints"]["required"],
                    "alternatives": row["constraints"]["alternatives"],
                }
            )
            previous = recognition_signatures.setdefault(
                signature, row["official_row"]
            )
            if previous != row["official_row"]:
                raise DescriptorError(
                    "CICS registry recognition signatures are not unique: "
                    f"{previous}, {row['official_row']}"
                )
    if (
        len(registry_rows) != 263
        or len(set(handler_ids)) != 263
        or len(typed_rows) != 43
        or len(legacy_rows) != 0
        or {row["runtime_operation"] for row in typed_rows}
        != TYPED_RUNTIME_OPERATIONS
        or len(advertised_rows) != 43
        or len(unready_rows) != 220
        or any(row["unready_result"] != "explicit-unsupported" for row in unready_rows)
        or any(not row["advertised"] or row["runtime_operation"] is None for row in typed_rows)
        or any(not row["advertised"] or row["runtime_operation"] is None for row in legacy_rows)
        or any(not row["legacy_execution_options"] for row in legacy_rows)
        or any(row["legacy_execution_options"] for row in typed_rows)
        or any(row["legacy_execution_options"] for row in unready_rows)
        or any(row["advertised"] or row["runtime_operation"] is not None for row in unready_rows)
        or any(
            row["handler_sha256"]
            != _handler_digest(
                {key: value for key, value in row.items() if key != "handler_sha256"}
            )
            for row in registry_rows
        )
        or any(":spi-commands-" in row["official_row"] for row in registry_rows)
        or any(":fepi-commands:" in row["official_row"] for row in registry_rows)
    ):
        raise DescriptorError("CICS 263-row registry shape or API isolation differs")

    bounded_contract_commands = (
        sum(
            command["contract_status"] == "frozen-with-bounded-ambiguity"
            for batch in batches
            for command in batch["commands"]
        )
        if frozen
        else 0
    )
    artifact_status = (
        "frozen-with-bounded-ambiguities"
        if frozen and bounded_contract_commands
        else "frozen"
        if frozen
        else "source-pending"
    )
    descriptor_path = root / CATALOG_PATH
    artifact: dict[str, Any] = {
        "schema_version": CONTRACT_SCHEMA_VERSION,
        "target_version": "0.9.0",
        "work_package": "CIC-901.command-contract",
        "status": artifact_status,
        "automatic_registration": False,
        "semantic_authority": frozen,
        "execution_authority": False,
        "coverage_credit": 0,
        "semantic_credit": 0,
        "differential_credit": 0,
        "inputs": {
            "command_descriptors": {
                "path": CATALOG_PATH.as_posix(),
                "file_sha256": _file_sha256(descriptor_path),
                "application_identity_set_sha256": application_identity_digest(commands),
            }
        },
        "contract_dimensions": list(CONTRACT_DIMENSIONS),
        "host_limits": dict(HOST_LIMITS),
        "policy_bindings": POLICY_BINDINGS,
        "condition_name_authority": condition_name_authority,
        "participant_contract": _participant_contract(batches, frozen),
        "registry": {
            "shape_commands": len(registry_rows),
            "typed_handlers": len(typed_rows),
            "legacy_compatibility_handlers": len(legacy_rows),
            "advertised_commands": len(advertised_rows),
            "unready_handlers": len(unready_rows),
            "automatic_registration": False,
            "default_handler": None,
            "unready_result": "explicit-unsupported",
            "family_counts": dict(sorted(family_counts.items())),
            "registry_sha256": _registry_digest(registry_rows),
        },
        "counts": {
            "batches": len(batches),
            "commands": len(commands),
            "source_projected_commands": projected_commands,
            "source_reviewed_commands": reviewed_commands,
            "source_ambiguous_commands": len(ambiguous_row_ids),
            "source_verified_candidates": verified_candidates,
            "source_ambiguous_candidates": ambiguous_candidates,
            "source_fact_groups": fact_groups,
            "source_candidate_references": source_candidate_references,
            "closed_contract_commands": 263 if frozen else 0,
            "bounded_contract_commands": bounded_contract_commands,
            "registry_shape_commands": len(registry_rows),
            "runtime_backed_commands": len(typed_rows) + len(legacy_rows),
            "typed_runtime_commands": len(typed_rows),
            "legacy_compatibility_commands": len(legacy_rows),
            "advertised_commands": len(advertised_rows),
            "unready_commands": len(unready_rows),
        },
        "batches": batches,
    }
    artifact["contract_sha256"] = contract_digest(artifact)
    return artifact


def render_contracts(root: Path = ROOT) -> str:
    return json.dumps(build_contracts(root), indent=2, ensure_ascii=False) + "\n"


def _rust_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def render_provider(
    root: Path = ROOT, contracts: dict[str, Any] | None = None
) -> str:
    catalog = load_catalog(root)
    contracts = contracts or build_contracts(root)
    condition_authority = contracts.get("condition_name_authority") or {}
    condition_names = condition_authority.get("allowed_names", [])
    family_variants = catalog["_families"]
    families = catalog["runtime"]["families"]
    operations = catalog["_runtime_operations"]
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "use mainframe_env_host_api::CicsOperation;",
        "",
        "#[rustfmt::skip]",
        "pub(crate) const CICS_CONDITION_NAMES: &[&str] =",
        f"    {_rust_string_slice(condition_names)};",
        "",
        "#[rustfmt::skip]",
        "pub(crate) const CICS_AID_NAMES: &[&str] =",
        f"    {_rust_string_slice(list(AID_NAMES))};",
        "",
        "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
        "pub(crate) enum CicsCommandFamily {",
    ]
    lines.extend(f"    {row['rust_variant']}," for row in families)
    lines.extend(
        [
            "}",
            "",
            "#[derive(Clone, Copy, Debug, Eq, PartialEq)]",
            "pub(crate) struct CicsCommandDescriptor {",
            "    pub(crate) operation: CicsOperation,",
            "    pub(crate) syntax: &'static str,",
            "    pub(crate) official_row: &'static str,",
            "    pub(crate) family: CicsCommandFamily,",
            "    pub(crate) mutating: bool,",
            "}",
            "",
            "pub(crate) const CICS_COMMAND_DESCRIPTORS: &[CicsCommandDescriptor] = &[",
        ]
    )
    for operation in operations:
        lines.extend(
            [
                "    CicsCommandDescriptor {",
                f"        operation: CicsOperation::{operation['operation']},",
                f"        syntax: {_rust_string(operation['label'])},",
                f"        official_row: {_rust_string(operation['official_row'])},",
                f"        family: CicsCommandFamily::{family_variants[operation['family']]},",
                f"        mutating: {str(operation['mutating']).lower()},",
                "    },",
            ]
        )
    lines.extend(
        [
            "];",
            "",
            "pub(crate) const fn command_descriptor(operation: CicsOperation) -> &'static CicsCommandDescriptor {",
            "    match operation {",
        ]
    )
    for index, operation in enumerate(operations):
        lines.append(
            f"        CicsOperation::{operation['operation']} => &CICS_COMMAND_DESCRIPTORS[{index}],"
        )
    lines.extend(["    }", "}", ""])
    return "\n".join(lines)


def render_host(root: Path = ROOT) -> str:
    catalog = load_catalog(root)
    commands = catalog["_application_commands"]
    digest = application_identity_digest(commands)
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "pub(super) const CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256: &str =",
        f"    {_rust_string(digest)};",
        "",
        "#[rustfmt::skip]",
        "pub(super) const CICS_APPLICATION_COMMAND_IDENTITIES: &[CicsApplicationCommandIdentityDescriptor] = &[",
    ]
    for command in commands:
        eibfn = bytes.fromhex(command["eibfn"])
        lines.append(
            "    CicsApplicationCommandIdentityDescriptor { "
            f"official_row: {_rust_string(command['official_row'])}, "
            f"label: {_rust_string(command['label'])}, "
            f"eibfn: [0x{eibfn[0]:02X}, 0x{eibfn[1]:02X}] "
            "},"
        )
    lines.extend(["];", ""])
    return "\n".join(lines)


def _top_level_option_names(command: dict[str, Any]) -> list[str]:
    return _top_level_source_option_names(command, command["source_dimensions"])


def _rust_string_slice(values: list[str]) -> str:
    return "&[" + ", ".join(_rust_string(value) for value in values) + "]"


def _compiler_legacy_compatibility_runtime_operation(
    catalog: dict[str, Any], profile: dict[str, Any]
) -> dict[str, Any]:
    """Cross-check `profile` against the one reviewed runtime table row it binds to."""
    matches = [
        row
        for row in catalog["_runtime_operations"]
        if row["operation"] == profile["operation"]
    ]
    if len(matches) != 1:
        raise DescriptorError(
            f"compiler legacy compatibility operation {profile['operation']} is not unique "
            "in the reviewed runtime table"
        )
    operation = matches[0]
    if (
        operation["interface"] != profile["interface"]
        or operation["official_row"] != profile["runtime_official_row"]
        or operation["family"] != profile["family"]
        or operation["mutating"] != profile["mutating"]
    ):
        raise DescriptorError(
            f"compiler legacy compatibility descriptor for {profile['operation']} "
            "differs from the reviewed runtime table"
        )
    return operation


def _compiler_legacy_compatibility_admitted_row(
    catalog: dict[str, Any], profile: dict[str, Any]
) -> None:
    """Verify the admitted `official_row` matches its catalog unit and label, when known."""
    official_row = profile["official_row"]
    if ":api-commands:" in official_row:
        matches = [
            command
            for command in catalog["_application_commands"]
            if command["official_row"] == official_row
        ]
        if len(matches) != 1 or matches[0]["label"] != profile["label"]:
            raise DescriptorError(
                f"compiler legacy compatibility admitted row {official_row} "
                "differs from the application catalog"
            )
    elif ":spi-commands-unique:" not in official_row:
        raise DescriptorError(
            f"compiler legacy compatibility official_row {official_row} must be an "
            "api-commands or spi-commands-unique row"
        )


def _compiler_legacy_compatibility_discriminators(
    catalog: dict[str, Any], profile: dict[str, Any]
) -> list[str]:
    """Derive sibling-form discriminators from the catalog labels, not a hand-list."""
    head = profile["recognition_head"]
    if len(head) != 1:
        raise DescriptorError(
            "compiler legacy compatibility recognition_head must be exactly one token"
        )
    prefix = f"{head[0]} "
    discriminators = sorted(
        command["label"].split()[1]
        for command in catalog["_application_commands"]
        if command["label"].startswith(prefix) and len(command["label"].split()) == 2
    )
    if discriminators != profile["expected_discriminators"]:
        raise DescriptorError(
            f"application {head[0]} discriminator set differs from the reviewed list"
        )
    return discriminators


def render_compiler_spi_compatibility(root: Path = ROOT) -> str:
    """Render the narrow legacy forms accepted directly by the COBOL compiler.

    Each reviewed profile in `COMPILER_LEGACY_COMPATIBILITY` admits one bounded
    legacy clause shape to a pre-existing raw runtime operation, isolated from
    the 263-row application registry: it changes no row's readiness,
    advertising, or counts.
    """
    catalog = load_catalog(root)
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "pub(super) const CICS_LEGACY_COMPATIBILITY: "
        "&[CicsLegacyCompatibilityDescriptor] = &[",
    ]
    for profile in COMPILER_LEGACY_COMPATIBILITY:
        _compiler_legacy_compatibility_runtime_operation(catalog, profile)
        _compiler_legacy_compatibility_admitted_row(catalog, profile)
        discriminators = _compiler_legacy_compatibility_discriminators(catalog, profile)
        lines.extend(
            [
                "    CicsLegacyCompatibilityDescriptor {",
                f"        official_row: {_rust_string(profile['official_row'])},",
                f"        label_tokens: {_rust_string_slice(profile['label'].split())},",
                f"        recognition_head: {_rust_string_slice(profile['recognition_head'])},",
                f"        runtime_operation: {_rust_string(profile['operation'])},",
                "        runtime_official_row: "
                f"{_rust_string(profile['runtime_official_row'])},",
                "        required_value_options: "
                f"{_rust_string_slice(profile['required_value_options'])},",
                "        optional_value_options: "
                f"{_rust_string_slice(profile['optional_value_options'])},",
                "        optional_flag_options: "
                f"{_rust_string_slice(profile['optional_flag_options'])},",
                "        application_discriminator_options: "
                f"{_rust_string_slice(discriminators)},",
                "        resp2_requires_resp: "
                f"{str(profile['resp2_requires_resp']).lower()},",
                "        reject_unknown_options: "
                f"{str(profile['reject_unknown_options']).lower()},",
                "    },",
            ]
        )
    lines.extend(["];", ""])
    return "\n".join(lines)


def render_ir_registry(root: Path = ROOT, contracts: dict[str, Any] | None = None) -> str:
    contracts = contracts or build_contracts(root)
    raw_condition_name_authority = contracts.get("condition_name_authority")
    if raw_condition_name_authority is None:
        empty_names = b"[]"
        condition_name_authority = {
            "allowed_names": [],
            "allowed_names_sha256": "sha256:"
            + hashlib.sha256(CONDITION_NAME_DOMAIN + empty_names).hexdigest(),
            "conditions_sha256": "sha256:"
            + hashlib.sha256(
                CONDITION_NAME_DOMAIN + b"pairs\0" + empty_names
            ).hexdigest(),
        }
    else:
        condition_name_authority = _object(
            raw_condition_name_authority, "condition name authority"
        )
    commands = [
        command
        for batch in contracts["batches"]
        for command in batch["commands"]
    ]
    lines = [
        "// @generated by `python3 -B tools/generate_cics_descriptors.py`; do not edit.",
        "",
        "/// Whether all three source batches were independently accepted before generation.",
        "pub const CICS_APPLICATION_REGISTRY_FROZEN: bool = "
        f"{str(contracts['status'] != 'source-pending').lower()};",
        "",
        "/// Digest of the complete 263-row non-executable registry shape.",
        "pub const CICS_APPLICATION_REGISTRY_SHA256: &str =",
        f"    {_rust_string(contracts['registry']['registry_sha256'])};",
        "",
        "/// Digest of the normalized EIBRESP condition-name array.",
        "pub const CICS_APPLICATION_CONDITION_NAMES_SHA256: &str =",
        f"    {_rust_string(condition_name_authority['allowed_names_sha256'])};",
        "",
        "/// Digest of the normalized EIBRESP condition name/code authority.",
        "pub const CICS_APPLICATION_CONDITION_AUTHORITY_SHA256: &str =",
        f"    {_rust_string(condition_name_authority['conditions_sha256'])};",
        "",
        "#[rustfmt::skip]",
        "/// Condition names accepted by dynamic HANDLE/IGNORE CONDITION clauses.",
        "pub const CICS_APPLICATION_CONDITION_NAMES: &[&str] =",
        f"    {_rust_string_slice(condition_name_authority['allowed_names'])};",
        "",
        "#[rustfmt::skip]",
        "/// AID names accepted by dynamic HANDLE AID clauses.",
        "pub const CICS_APPLICATION_AID_NAMES: &[&str] =",
        f"    {_rust_string_slice(list(AID_NAMES))};",
        "",
        "#[rustfmt::skip]",
        "/// Complete application-command registry shape in official-row order.",
        "pub const CICS_APPLICATION_REGISTRY: &[CicsApplicationRegistryDescriptor] = &[",
    ]
    for command in commands:
        registry_row = _registry_row_material(
            command, command["source_dimensions"], command["contract"]
        )
        registry = command["contract"]["registry"]
        label_tokens = _rust_string_slice(registry_row["label_tokens"])
        recognition_heads = "&[" + ", ".join(
            _rust_string_slice(head) for head in registry_row["recognition_heads"]
        ) + "]"
        discriminator_options = _rust_string_slice(
            registry_row["discriminator_options"]
        )
        required_discriminator_options = _rust_string_slice(
            registry_row["required_discriminator_options"]
        )
        forbidden_discriminator_options = _rust_string_slice(
            registry_row["forbidden_discriminator_options"]
        )
        options = []
        for option in registry_row["options"]:
            value_shape = {
                "flag": "Flag",
                "value": "Value",
                "optional-value": "OptionalValue",
                "bounded-ambiguity": "BoundedAmbiguity",
            }[option["value_shape"]]
            direction = {
                "input": "Input",
                "output": "Output",
                "input-output": "InputOutput",
                "none": "None",
                "bounded-ambiguity": "BoundedAmbiguity",
            }[option["direction"]]
            source_bound = (
                f"Some({option['source_max_value_bytes']})"
                if option["source_max_value_bytes"] is not None
                else "None"
            )
            options.append(
                "CicsApplicationOptionDescriptor { "
                f"name: {_rust_string(option['name'])}, "
                "value_shape: CicsApplicationOptionValueShape::"
                f"{value_shape}, "
                "direction: CicsApplicationOptionDirection::"
                f"{direction}, "
                f"source_max_value_bytes: {source_bound} "
                "}"
            )
        condition_clauses = registry_row["condition_clauses"]
        rendered_condition_clauses = (
            "Some(CicsApplicationConditionClauseDescriptor { "
            f"name_authority: {_rust_string(condition_clauses['name_authority'])}, "
            "name_authority_sha256: "
            f"{_rust_string(condition_clauses['name_authority_sha256'])}, "
            f"minimum_occurrences: {condition_clauses['minimum_occurrences']}, "
            f"maximum_occurrences: {condition_clauses['maximum_occurrences']}, "
            "label_operand: CicsApplicationConditionLabelOperand::"
            f"{'Optional' if condition_clauses['label_operand'] == 'optional' else 'Forbidden'} "
            "})"
            if condition_clauses is not None
            else "None"
        )
        constraints = registry_row["constraints"]
        constraint_status = {
            "resolved": "Resolved",
            "bounded-ambiguity": "BoundedAmbiguity",
            "not-applicable": "NotApplicable",
            "pending": "Pending",
        }[constraints["status"]]
        recognition_status = {
            "resolved": "Resolved",
            "bounded-ambiguity": "BoundedAmbiguity",
            "not-applicable": "NotApplicable",
            "pending": "Pending",
        }[registry_row["recognition_status"]]
        cobol_applicability = {
            "allowed": "Allowed",
            "not-applicable": "NotApplicable",
            "conditional": "Conditional",
            "bounded-ambiguity": "BoundedAmbiguity",
        }[registry_row["cobol_applicability"]]
        alternatives = "&[" + ", ".join(
            "CicsApplicationOptionAlternative { "
            f"members: {_rust_string_slice(group['members'])}, "
            f"required: {str(group['required']).lower()} "
            "}"
            for group in constraints["alternatives"]
        ) + "]"
        dependencies = "&[" + ", ".join(
            "CicsApplicationOptionDependency { "
            f"option: {_rust_string(dependency['option'])}, "
            f"requires: {_rust_string_slice(dependency['requires'])} "
            "}"
            for dependency in constraints["dependencies"]
        ) + "]"
        mutual_exclusions = "&[" + ", ".join(
            _rust_string_slice(group) for group in constraints["mutual_exclusions"]
        ) + "]"
        eibfn = bytes.fromhex(command["eibfn"])
        readiness = (
            "CicsApplicationHandlerReadiness::TypedRuntime"
            if registry["readiness"] == "typed-runtime"
            else "CicsApplicationHandlerReadiness::LegacyCompatibility"
            if registry["readiness"] == "legacy-compatibility"
            else "CicsApplicationHandlerReadiness::Unready"
        )
        runtime_operation = (
            f"Some({_rust_string(registry['runtime_operation'])})"
            if registry["runtime_operation"] is not None
            else "None"
        )
        legacy_execution_options = _rust_string_slice(
            registry["legacy_execution_options"]
        )
        lines.append(
            "    CicsApplicationRegistryDescriptor { "
            f"official_row: {_rust_string(command['official_row'])}, "
            f"label_tokens: {label_tokens}, "
            f"recognition_heads: {recognition_heads}, "
            f"discriminator_options: {discriminator_options}, "
            f"required_discriminator_options: {required_discriminator_options}, "
            f"forbidden_discriminator_options: {forbidden_discriminator_options}, "
            "recognition_status: CicsApplicationConstraintStatus::"
            f"{recognition_status}, "
            "cobol_applicability: CicsApplicationCobolApplicability::"
            f"{cobol_applicability}, "
            f"options: &[{', '.join(options)}], "
            f"condition_clauses: {rendered_condition_clauses}, "
            "top_level_options: "
            f"{_rust_string_slice([option['name'] for option in registry_row['options']])}, "
            f"required_options: {_rust_string_slice(constraints['required'])}, "
            f"alternative_groups: {alternatives}, "
            f"dependencies: {dependencies}, "
            f"mutual_exclusion_groups: {mutual_exclusions}, "
            "constraint_status: CicsApplicationConstraintStatus::"
            f"{constraint_status}, "
            f"eibfn: [0x{eibfn[0]:02X}, 0x{eibfn[1]:02X}], "
            f"family: {_rust_string(registry['family'])}, "
            f"handler_id: {_rust_string(registry['handler_id'])}, "
            f"handler_sha256: {_rust_string(registry['handler_sha256'])}, "
            f"readiness: {readiness}, "
            f"advertised: {str(registry['advertised']).lower()}, "
            f"runtime_operation: {runtime_operation}, "
            f"legacy_execution_options: {legacy_execution_options} "
            "},"
        )
    lines.extend(["];"])
    return "\n".join(lines) + "\n"


def render(root: Path = ROOT) -> str:
    """Retain the historical provider-render helper for tooling callers."""
    return render_provider(root)


def rendered_outputs(root: Path = ROOT) -> dict[Path, str]:
    contracts = build_contracts(root)
    return {
        OUTPUT_PATH: render_provider(root, contracts),
        HOST_OUTPUT_PATH: render_host(root),
        COMPILER_SPI_COMPAT_OUTPUT_PATH: render_compiler_spi_compatibility(root),
        IR_REGISTRY_OUTPUT_PATH: render_ir_registry(root, contracts),
        CONTRACT_OUTPUT_PATH: json.dumps(contracts, indent=2, ensure_ascii=False) + "\n",
    }


def check(root: Path = ROOT) -> None:
    for relative, expected in rendered_outputs(root).items():
        output = root / relative
        try:
            actual = output.read_text()
        except OSError as error:
            raise DescriptorError(f"{output}: {error}") from error
        if actual != expected:
            raise DescriptorError(
                f"{relative} is stale; run python3 -B tools/generate_cics_descriptors.py"
            )


def generate(root: Path = ROOT) -> None:
    for relative, source in rendered_outputs(root).items():
        output = root / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(source)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when generated Rust is stale")
    args = parser.parse_args()
    if args.check:
        check()
        print("cics-command-descriptors: pass")
    else:
        generate()
        print(
            "cics-command-descriptors: generated "
            f"{OUTPUT_PATH}, {HOST_OUTPUT_PATH}, {COMPILER_SPI_COMPAT_OUTPUT_PATH}, "
            f"{IR_REGISTRY_OUTPUT_PATH}, and {CONTRACT_OUTPUT_PATH}"
        )


if __name__ == "__main__":
    main()
