#!/usr/bin/env python3
"""Guard the additive INT-1601 coordinator/CICS participant bindings."""

from pathlib import Path
import re
import json


ROOT = Path(__file__).resolve().parents[1]
TEST_MODULE = re.compile(r"(?m)^\s*#\[cfg\(test\)\]")


def production_source(path: Path) -> str:
    source = path.read_text()
    marker = TEST_MODULE.search(source)
    return source if marker is None else source[: marker.start()]


def require_fragments(source: str, fragments: tuple[str, ...], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in source]
    if missing:
        raise ValueError(f"{label} participant binding is incomplete: {missing}")


def check(root: Path = ROOT) -> None:
    contract = json.loads((root / "conformance/0.16/contracts/transaction-participant.json").read_text())
    ims = next(p for p in contract["participants"] if p["provider_id"] == "ims")
    preparation = ims.get("preparation")
    if preparation is not None:
        if ims["status"] != "pending" or ims["capabilities"] is not None:
            raise ValueError("IMS preparation must not admit a participant")
        require_fragments(
            (root / preparation["contract_test"]).read_text(),
            (
                "fn memory_local_commit_rollback_replay_and_batch_limit()",
                "fn authorization_and_malformed_failures_preserve_all_rows()",
                "fn sqlite_process_restart_preserves_commit_undo_and_unknown_replay()",
                "fn sqlite_child_phase()",
                "ims_providers(", "std::process::Command::new",
                "HostProblem::UnknownOutcome", "ParticipantStatus::Pending",
            ),
            "IMS local preparation tests (not acceptance evidence)",
        )
    coordinator = production_source(
        root / "crates/kernel/mainframe-env-interpreter/src/coordinator.rs"
    )
    require_fragments(
        coordinator,
        (
            "read_transaction_participant_contract(1)",
            "pub fn transaction_participant_contract(",
            "pub fn transaction_participant_descriptor(",
            ".participant(provider_id)",
        ),
        "execution coordinator",
    )

    cics = production_source(
        root / "crates/providers/mainframe-env-cics/src/handlers/recovery.rs"
    )
    require_fragments(
        cics,
        (
            "transaction_participant_contract_v1().participant(\"cics\")",
            "pub(crate) fn transaction_participant_descriptor(",
            ".contexts",
            "ExplicitSyncpoint::Supported",
            "ExplicitSyncpoint::Rejected",
            "let rejection = context",
            ".rejection",
            ".ok_or(HostProblem::InfrastructureFailure)?",
        ),
        "CICS recovery handler",
    )
    forbidden_cics = (
        'b"dpl-without-synconreturn" | b"dpl-executionset-subset"',
        'name: "INVREQ".into(),\n            response: 16,\n            response2: 200',
    )
    present = [fragment for fragment in forbidden_cics if fragment in cics]
    if present:
        raise ValueError(f"CICS retains a private participant shortcut: {present}")

    for provider in ["db2", "ims", "mq"]:
        source = production_source(
            root / f"crates/providers/mainframe-env-{provider}/src/service.rs"
        )
        if "transaction_participant_contract_v1" in source:
            raise ValueError(f"{provider} gained a participant binding while pending")


if __name__ == "__main__":
    check()
    print("transaction participant bindings: pass")
