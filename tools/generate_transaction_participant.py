#!/usr/bin/env python3
"""Validate and project the provider-neutral INT-1601 participant contract."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACT_PATH = Path("conformance/0.16/contracts/transaction-participant.json")
FIXTURE_PATH = Path(
    "conformance/0.16/fixtures/transaction-participant-compatibility.json"
)
OUTPUT_PATH = Path(
    "crates/contracts/mainframe-env-execution-api/src/generated/"
    "transaction_participant_v1.rs"
)

MODES = {
    "local": "ParticipantMode::Local",
    "distributed-owned": "ParticipantMode::DistributedOwned",
    "distributed-subordinate": "ParticipantMode::DistributedSubordinate",
}
OWNERS = {
    "participant": "SyncpointOwner::Participant",
    "upstream-host": "SyncpointOwner::UpstreamHost",
}
SYNCPOINTS = {
    "supported": "ExplicitSyncpoint::Supported",
    "rejected": "ExplicitSyncpoint::Rejected",
}
PREPARE = {
    "not-applicable": "PrepareCapability::NotApplicable",
    "durable-intent-only": "PrepareCapability::DurableIntentOnly",
    "provider-prepare": "PrepareCapability::ProviderPrepare",
}
OUTCOMES = {
    "committed": "ParticipantOutcome::Committed",
    "rolled-back": "ParticipantOutcome::RolledBack",
    "failed": "ParticipantOutcome::Failed",
    "heuristic-commit": "ParticipantOutcome::HeuristicCommit",
    "heuristic-rollback": "ParticipantOutcome::HeuristicRollback",
    "heuristic-mixed": "ParticipantOutcome::HeuristicMixed",
    "in-doubt": "ParticipantOutcome::InDoubt",
    "unknown-outcome": "ParticipantOutcome::UnknownOutcome",
}
EFFECT_ORDER = [
    "observe-deadline-and-cancellation",
    "validate-request-capability-and-context",
    "persist-canonical-effect-intent",
    "authorize-typed-resource-and-intent",
    "apply-participant-mutation",
    "persist-audit-and-result-or-unknown",
]
EFFECT_VARIANTS = [
    "ParticipantEffectStep::ObserveDeadlineAndCancellation",
    "ParticipantEffectStep::ValidateRequestCapabilityAndContext",
    "ParticipantEffectStep::PersistCanonicalEffectIntent",
    "ParticipantEffectStep::AuthorizeTypedResourceAndIntent",
    "ParticipantEffectStep::ApplyParticipantMutation",
    "ParticipantEffectStep::PersistAuditAndResultOrUnknown",
]
LOCK_ORDER = [
    "coordinator-intent-cas",
    "participant-state-fence",
    "canonical-resource-locks",
    "participant-uow-replay-publication",
    "coordinator-result-cas",
]
LOCK_VARIANTS = [
    "ParticipantLockStep::CoordinatorIntentCas",
    "ParticipantLockStep::ParticipantStateFence",
    "ParticipantLockStep::CanonicalResourceLocks",
    "ParticipantLockStep::ParticipantUowReplayPublication",
    "ParticipantLockStep::CoordinatorResultCas",
]
OBLIGATIONS = [
    "INT-1601.owner",
    "INT-1601.modes",
    "INT-1601.prepare",
    "INT-1601.completion",
    "INT-1601.compensation",
    "INT-1601.outcomes",
    "INT-1601.idempotency",
    "INT-1601.ordering",
    "INT-1601.fencing",
    "INT-1601.deadline-cancellation",
    "INT-1601.security-audit",
    "INT-1601.recovery-schema",
    "INT-1601.retention",
    "INT-1601.compatibility",
]
PROVIDERS = ["cics", "db2", "ims", "mq"]


class ContractError(ValueError):
    """The readable authority is malformed or semantically unsafe."""


def load(root: Path, relative: Path) -> dict[str, Any]:
    try:
        value = json.loads((root / relative).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"invalid JSON {relative}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"{relative} must contain an object")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def object_field(value: dict[str, Any], name: str) -> dict[str, Any]:
    result = value.get(name)
    if not isinstance(result, dict):
        raise ContractError(f"{name} must be an object")
    return result


def list_field(value: dict[str, Any], name: str) -> list[Any]:
    result = value.get(name)
    if not isinstance(result, list):
        raise ContractError(f"{name} must be an array")
    return result


def validate_contract(contract: dict[str, Any]) -> None:
    require(
        contract.get("schema_version")
        == "mainframe-env.transaction-participant-contract@1"
        and contract.get("contract_id") == "mainframe-env.transaction-participant@1"
        and contract.get("contract_version") == 1
        and contract.get("work_package") == "INT-1601.participant-schema"
        and contract.get("status") == "early-additive-prerequisite",
        "participant contract identity differs",
    )
    authority = object_field(contract, "authority")
    require(
        authority
        == {
            "coordinator": "mainframe-env.execution-coordinator@1",
            "canonical_effect": "mainframe-env.effect-canonical@1",
            "store": "mainframe-env.store@2",
            "single_authority": True,
            "public_runtime_change": False,
            "universal_two_phase_commit": False,
            "exactly_once": False,
        },
        "participant authority must retain one coordinator without 2PC/exactly-once",
    )
    vocabulary = object_field(contract, "vocabulary")
    require(
        vocabulary.get("modes") == list(MODES)
        and vocabulary.get("prepare") == list(PREPARE)
        and vocabulary.get("outcomes") == list(OUTCOMES),
        "participant vocabulary differs",
    )
    ordering = object_field(contract, "ordering")
    require(
        ordering
        == {
            "effect": EFFECT_ORDER,
            "locks": LOCK_ORDER,
            "store_transaction_across_dispatch": False,
            "resource_lock_key": "provider-id/resource-identity",
            "late_owner_fenced": True,
        },
        "shared participant effect/lock order differs",
    )
    require(contract.get("obligations") == OBLIGATIONS, "obligation inventory differs")
    compatibility = object_field(contract, "compatibility")
    require(
        compatibility
        == {
            "writer_version": 1,
            "read_versions": [1],
            "unknown_version": "reject",
            "change_policy": "additive-only-until-version-bump",
            "coordinator_schema_owner": "execution-and-store-maintainers",
            "participant_schema_owner": "owning-provider-maintainer",
        },
        "participant reader compatibility differs",
    )

    participants = list_field(contract, "participants")
    require(
        [participant.get("provider_id") for participant in participants] == PROVIDERS,
        "participants must be ordered cics/db2/ims/mq",
    )
    cics = participants[0]
    require(
        cics.get("status") == "accepted"
        and cics.get("dependency") == "accepted-v0.9-cics-boundary",
        "CICS must be the sole accepted early participant",
    )
    validate_cics_capabilities(object_field(cics, "capabilities"))
    for participant, version in zip(participants[1:], ["0.13", "0.14", "0.15"]):
        require(
            participant.get("status") == "pending"
            and participant.get("capabilities") is None
            and participant.get("dependency")
            == f"v{version}-owned-participant-binding",
            f"{participant.get('provider_id')} must remain capability-pending",
        )


def validate_cics_capabilities(capabilities: dict[str, Any]) -> None:
    require(
        capabilities.get("transaction_owner") == "cics-task-or-dpl-mirror"
        and capabilities.get("supported_modes") == ["local", "distributed-owned"]
        and capabilities.get("rejected_modes") == ["distributed-subordinate"]
        and capabilities.get("prepare") == "durable-intent-only"
        and capabilities.get("provider_prepare") is False
        and capabilities.get("commit") is True
        and capabilities.get("rollback") is True
        and capabilities.get("ordering") == "shared-v1",
        "CICS ownership, mode, prepare, or completion declaration differs",
    )
    contexts = list_field(capabilities, "contexts")
    expected_contexts = [
        ("local", "local", "participant", "supported"),
        ("dpl-synconreturn", "distributed-owned", "participant", "supported"),
        (
            "dpl-without-synconreturn",
            "distributed-subordinate",
            "upstream-host",
            "rejected",
        ),
        (
            "dpl-executionset-subset",
            "distributed-subordinate",
            "upstream-host",
            "rejected",
        ),
    ]
    require(len(contexts) == len(expected_contexts), "CICS context count differs")
    for context, expected in zip(contexts, expected_contexts):
        actual = tuple(
            context.get(name)
            for name in ("context_id", "mode", "syncpoint_owner", "explicit_syncpoint")
        )
        require(actual == expected, f"CICS context differs: {actual}")
        rejection = context.get("rejection")
        if expected[3] == "supported":
            require(rejection is None, "owned CICS context cannot have a rejection")
        else:
            require(
                rejection
                == {"condition": "INVREQ", "response": 16, "response2": 200},
                "subordinate CICS context must reject with INVREQ 16/200",
            )

    compensation = object_field(capabilities, "compensation")
    require(
        compensation
        == {
            "automatic": False,
            "after_commit": False,
            "scope": "service-specific-explicit-only",
        },
        "CICS compensation limits differ",
    )
    outcomes = object_field(capabilities, "outcomes")
    reported = outcomes.get("reported")
    not_produced = outcomes.get("not_produced")
    require(
        isinstance(reported, list)
        and isinstance(not_produced, list)
        and len(reported) == len(set(reported))
        and len(not_produced) == len(set(not_produced))
        and set(reported).isdisjoint(not_produced)
        and set(reported) | set(not_produced) == set(OUTCOMES)
        and "unknown-outcome" in reported
        and outcomes.get("collapse_to_success") is False,
        "CICS outcomes must partition the closed vocabulary without collapse",
    )
    require(
        object_field(capabilities, "idempotency")
        == {
            "scope": "execution-run-unit-effect-sequence",
            "lifetime": "retention-policy-idempotency-ticks",
            "replay": "exact-canonical-request-and-result",
            "reuse_after_prune": "new-operation",
            "exactly_once": False,
        },
        "CICS idempotency declaration differs",
    )
    require(
        object_field(capabilities, "fencing")
        == {
            "effect_lease_epoch": True,
            "participant_row_cas": True,
            "stale_owner_rejected": True,
        },
        "CICS fencing declaration differs",
    )
    require(
        object_field(capabilities, "deadline_cancellation")
        == {
            "finite_deadline": True,
            "live_cancellation_probe": True,
            "pre_dispatch_stop": True,
            "post_dispatch_uncertainty": "unknown-outcome",
        },
        "CICS deadline/cancellation declaration differs",
    )
    require(
        object_field(capabilities, "security_audit")
        == {
            "principal_and_delegation_propagated": True,
            "typed_resource_authorization_before_mutation": True,
            "deny_and_failure_audited": True,
            "shared_store_atomic_publication": True,
        },
        "CICS security/audit declaration differs",
    )
    require(
        object_field(capabilities, "recovery")
        == {
            "coordinator_owner": "execution-coordinator-effect-journal",
            "participant_owner": "cics-uow-and-replay-rows",
            "reconciliation": "service-specific-fenced-observation",
            "automatic_redispatch": False,
        },
        "CICS recovery declaration differs",
    )
    require(
        object_field(capabilities, "schemas")
        == {
            "request": "mainframe-env.cics.request@1",
            "canonical_effect": "mainframe-env.effect-canonical@1",
            "uow_namespace": "cics-uow",
            "uow_write": "MECU2",
            "uow_read": ["MECU1", "MECU2"],
            "undo_namespace": "cics-uow-undo",
            "undo_read_write": "MECUNDO1",
            "replay_namespace": "cics-effect-replay-v1",
            "replay_write": "MECER003",
        },
        "CICS schema ownership declaration differs",
    )
    require(
        object_field(capabilities, "retention")
        == {
            "target": "cics-unit-of-work",
            "watermark": "idempotency",
            "protect_live_checkpoint_audit_replay": True,
            "deadline_is_lower_bound": True,
        },
        "CICS retention declaration differs",
    )


def validate_fixtures(fixtures: dict[str, Any], contract: dict[str, Any]) -> None:
    require(
        fixtures.get("schema_version")
        == "mainframe-env.transaction-participant-fixtures@1"
        and fixtures.get("contract_version") == contract.get("contract_version"),
        "participant fixture identity differs",
    )
    readers = list_field(fixtures, "reader_cases")
    require(
        readers
        == [
            {"id": "read-v1", "version": 1, "expected": "accepted"},
            {"id": "reject-v0", "version": 0, "expected": "incompatible-version"},
            {"id": "reject-v2", "version": 2, "expected": "incompatible-version"},
        ],
        "participant read-version fixtures differ",
    )
    cases = list_field(fixtures, "cics_cases")
    require(
        [case.get("execution_context") for case in cases]
        == [
            "local",
            "local",
            "dpl-synconreturn",
            "dpl-synconreturn",
            "dpl-without-synconreturn",
            "dpl-executionset-subset",
        ],
        "CICS compatibility context fixtures differ",
    )
    context_contract = {
        context["context_id"]: context
        for context in contract["participants"][0]["capabilities"]["contexts"]
    }
    for case in cases:
        context = context_contract[case["execution_context"]]
        require(
            case.get("mode") == context["mode"]
            and case.get("syncpoint_owner") == context["syncpoint_owner"],
            f"fixture {case.get('id')} contradicts its CICS context",
        )
        expected = object_field(case, "expected")
        if context["explicit_syncpoint"] == "rejected":
            require(
                expected
                == {
                    **context["rejection"],
                    "outcome": None,
                    "uow_state": "absent",
                },
                f"fixture {case.get('id')} does not preserve rejection/no-mutation",
            )
        if case.get("remote_outcome") is not None:
            require(
                case.get("execution_context") == "dpl-synconreturn",
                "remote outcome is valid only for owned DPL",
            )


def rust_string(value: str) -> str:
    return json.dumps(value)


def rust_bool(value: bool) -> str:
    return "true" if value else "false"


def rust_slice(values: list[Any], render) -> str:
    return "&[" + ", ".join(render(value) for value in values) + "]"


def render_rejection(value: dict[str, Any] | None) -> str:
    if value is None:
        return "None"
    return (
        "Some(ParticipantRejection { "
        f"condition: {rust_string(value['condition'])}, "
        f"response: {value['response']}, response2: {value['response2']} "
        "})"
    )


def render_context(value: dict[str, Any]) -> str:
    return (
        "ParticipantContextCapability { "
        f"context_id: {rust_string(value['context_id'])}, "
        f"mode: {MODES[value['mode']]}, "
        f"syncpoint_owner: {OWNERS[value['syncpoint_owner']]}, "
        f"explicit_syncpoint: {SYNCPOINTS[value['explicit_syncpoint']]}, "
        f"rejection: {render_rejection(value['rejection'])} "
        "}"
    )


def render_capabilities(value: dict[str, Any]) -> str:
    compensation = value["compensation"]
    outcomes = value["outcomes"]
    idempotency = value["idempotency"]
    fencing = value["fencing"]
    deadline = value["deadline_cancellation"]
    security = value["security_audit"]
    recovery = value["recovery"]
    schemas = value["schemas"]
    retention = value["retention"]
    fields = [
        f"transaction_owner: {rust_string(value['transaction_owner'])}",
        f"supported_modes: {rust_slice(value['supported_modes'], MODES.__getitem__)}",
        f"rejected_modes: {rust_slice(value['rejected_modes'], MODES.__getitem__)}",
        f"contexts: {rust_slice(value['contexts'], render_context)}",
        f"prepare: {PREPARE[value['prepare']]}",
        f"provider_prepare: {rust_bool(value['provider_prepare'])}",
        f"commit: {rust_bool(value['commit'])}",
        f"rollback: {rust_bool(value['rollback'])}",
        f"automatic_compensation: {rust_bool(compensation['automatic'])}",
        f"compensation_after_commit: {rust_bool(compensation['after_commit'])}",
        f"compensation_scope: {rust_string(compensation['scope'])}",
        f"reported_outcomes: {rust_slice(outcomes['reported'], OUTCOMES.__getitem__)}",
        f"not_produced_outcomes: {rust_slice(outcomes['not_produced'], OUTCOMES.__getitem__)}",
        f"collapse_outcomes_to_success: {rust_bool(outcomes['collapse_to_success'])}",
        f"idempotency_scope: {rust_string(idempotency['scope'])}",
        f"idempotency_lifetime: {rust_string(idempotency['lifetime'])}",
        f"replay_identity: {rust_string(idempotency['replay'])}",
        f"reuse_after_prune: {rust_string(idempotency['reuse_after_prune'])}",
        f"exactly_once: {rust_bool(idempotency['exactly_once'])}",
        (
            "fencing: ParticipantFencing { "
            f"effect_lease_epoch: {rust_bool(fencing['effect_lease_epoch'])}, "
            f"participant_row_cas: {rust_bool(fencing['participant_row_cas'])}, "
            f"stale_owner_rejected: {rust_bool(fencing['stale_owner_rejected'])} "
            "}"
        ),
        (
            "deadline_cancellation: ParticipantDeadlineCancellation { "
            f"finite_deadline: {rust_bool(deadline['finite_deadline'])}, "
            f"live_cancellation_probe: {rust_bool(deadline['live_cancellation_probe'])}, "
            f"pre_dispatch_stop: {rust_bool(deadline['pre_dispatch_stop'])}, "
            "post_dispatch_unknown: true }"
        ),
        (
            "security_audit: ParticipantSecurityAudit { "
            "principal_and_delegation_propagated: "
            f"{rust_bool(security['principal_and_delegation_propagated'])}, "
            "typed_resource_authorization_before_mutation: "
            f"{rust_bool(security['typed_resource_authorization_before_mutation'])}, "
            f"deny_and_failure_audited: {rust_bool(security['deny_and_failure_audited'])}, "
            "shared_store_atomic_publication: "
            f"{rust_bool(security['shared_store_atomic_publication'])} }}"
        ),
        f"coordinator_recovery_owner: {rust_string(recovery['coordinator_owner'])}",
        f"participant_recovery_owner: {rust_string(recovery['participant_owner'])}",
        f"reconciliation: {rust_string(recovery['reconciliation'])}",
        f"automatic_redispatch: {rust_bool(recovery['automatic_redispatch'])}",
        (
            "schemas: ParticipantSchemas { "
            f"request: {rust_string(schemas['request'])}, "
            f"canonical_effect: {rust_string(schemas['canonical_effect'])}, "
            f"uow_namespace: {rust_string(schemas['uow_namespace'])}, "
            f"uow_write: {rust_string(schemas['uow_write'])}, "
            f"uow_read: {rust_slice(schemas['uow_read'], rust_string)}, "
            f"undo_namespace: {rust_string(schemas['undo_namespace'])}, "
            f"undo_read_write: {rust_string(schemas['undo_read_write'])}, "
            f"replay_namespace: {rust_string(schemas['replay_namespace'])}, "
            f"replay_write: {rust_string(schemas['replay_write'])} "
            "}"
        ),
        f"retention_target: {rust_string(retention['target'])}",
        f"retention_watermark: {rust_string(retention['watermark'])}",
        "protect_live_checkpoint_audit_replay: "
        f"{rust_bool(retention['protect_live_checkpoint_audit_replay'])}",
        "deadline_is_retention_lower_bound: "
        f"{rust_bool(retention['deadline_is_lower_bound'])}",
    ]
    return "ParticipantCapabilities {\n        " + ",\n        ".join(fields) + "\n    }"


def render_participant(value: dict[str, Any]) -> str:
    status = {
        "accepted": "ParticipantStatus::Accepted",
        "pending": "ParticipantStatus::Pending",
    }[value["status"]]
    capabilities = (
        f"Some({render_capabilities(value['capabilities'])})"
        if value["capabilities"] is not None
        else "None"
    )
    return (
        "TransactionParticipantDescriptor {\n"
        f"        provider_id: {rust_string(value['provider_id'])},\n"
        f"        status: {status},\n"
        f"        dependency: {rust_string(value['dependency'])},\n"
        f"        capabilities: {capabilities},\n"
        "    }"
    )


def render(contract: dict[str, Any]) -> str:
    authority = contract["authority"]
    ordering = contract["ordering"]
    compatibility = contract["compatibility"]
    participants = ",\n    ".join(
        render_participant(participant) for participant in contract["participants"]
    )
    obligations = rust_slice(contract["obligations"], rust_string)
    effect_order = rust_slice(EFFECT_VARIANTS, str)
    lock_order = rust_slice(LOCK_VARIANTS, str)
    read_versions = rust_slice(compatibility["read_versions"], str)
    return f"""// @generated by `tools/generate_transaction_participant.py`; do not edit.

#[rustfmt::skip]
const GENERATED_TRANSACTION_PARTICIPANT_V1: TransactionParticipantContract =
    TransactionParticipantContract {{
        contract_id: {rust_string(contract['contract_id'])},
        version: {contract['contract_version']},
        coordinator: {rust_string(authority['coordinator'])},
        canonical_effect: {rust_string(authority['canonical_effect'])},
        store: {rust_string(authority['store'])},
        single_authority: {rust_bool(authority['single_authority'])},
        public_runtime_change: {rust_bool(authority['public_runtime_change'])},
        universal_two_phase_commit: {rust_bool(authority['universal_two_phase_commit'])},
        exactly_once: {rust_bool(authority['exactly_once'])},
        effect_order: {effect_order},
        lock_order: {lock_order},
        store_transaction_across_dispatch: {rust_bool(ordering['store_transaction_across_dispatch'])},
        resource_lock_key: {rust_string(ordering['resource_lock_key'])},
        late_owner_fenced: {rust_bool(ordering['late_owner_fenced'])},
        obligations: {obligations},
        writer_version: {compatibility['writer_version']},
        read_versions: {read_versions},
        participants: &[
    {participants}
        ],
    }};
"""


def run(root: Path, check: bool) -> None:
    contract = load(root, CONTRACT_PATH)
    fixtures = load(root, FIXTURE_PATH)
    validate_contract(contract)
    validate_fixtures(fixtures, contract)
    output = render(contract)
    path = root / OUTPUT_PATH
    if check:
        try:
            current = path.read_text()
        except OSError as error:
            raise ContractError(f"generated participant projection is missing: {error}") from error
        require(current == output, "generated participant projection is stale")
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        run(ROOT, args.check)
    except ContractError as error:
        parser.error(str(error))
    if args.check:
        print("transaction participant contract: pass")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
