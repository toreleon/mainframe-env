//! Additive provider-neutral transaction participant capabilities.
//!
//! This module describes participants consumed by the existing execution
//! coordinator. It is not a dispatcher, transaction manager, or recovery log.

use std::collections::BTreeSet;

/// Stable early participant boundary frozen by INT-1601.
pub const TRANSACTION_PARTICIPANT_CONTRACT: &str = "mainframe-env.transaction-participant@1";
/// Current writer and only accepted reader version.
pub const TRANSACTION_PARTICIPANT_VERSION: u16 = 1;

const REQUIRED_OBLIGATIONS: &[&str] = &[
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
];

/// Whether a provider's owned participant binding is ready for integration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantStatus {
    /// The provider binding has an accepted capability declaration.
    Accepted,
    /// The provider binding awaits its declared integration dependency.
    Pending,
}

/// Execution context in which a provider may be asked to participate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantMode {
    /// Participation in a local transaction context.
    Local,
    /// Distributed participation with ownership of the syncpoint decision.
    DistributedOwned,
    /// Distributed participation under an upstream syncpoint authority.
    DistributedSubordinate,
}

/// Authority that owns the current syncpoint decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncpointOwner {
    /// The participant owns the syncpoint decision for this context.
    Participant,
    /// An upstream host owns the syncpoint decision for this context.
    UpstreamHost,
}

/// Applicability of an explicit syncpoint command in one execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplicitSyncpoint {
    /// The context permits an explicit syncpoint request.
    Supported,
    /// The context rejects an explicit syncpoint with its declared response.
    Rejected,
}

/// Prepare support is distinct from a durable pre-dispatch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareCapability {
    /// Prepare is not applicable to the declared binding.
    NotApplicable,
    /// A pre-dispatch intent is durable, without a provider prepare operation.
    DurableIntentOnly,
    /// The binding declares support for a provider prepare operation.
    ProviderPrepare,
}

/// Closed participant outcome vocabulary. Unsupported outcomes remain explicit
/// capability limits and may not be collapsed into success.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantOutcome {
    /// The participant reports committed work.
    Committed,
    /// The participant reports rolled-back work.
    RolledBack,
    /// The participant reports failure rather than successful completion.
    Failed,
    /// A heuristic decision is reported as commit.
    HeuristicCommit,
    /// A heuristic decision is reported as rollback.
    HeuristicRollback,
    /// A heuristic result includes both committed and rolled-back work.
    HeuristicMixed,
    /// A transaction decision remains in doubt.
    InDoubt,
    /// The observed evidence does not establish the operation's outcome.
    ///
    /// This value must remain distinct from success; it is not permission to replay.
    UnknownOutcome,
}

impl ParticipantOutcome {
    /// Every outcome in the closed vocabulary, used to validate complete capability partitions.
    pub const ALL: [Self; 8] = [
        Self::Committed,
        Self::RolledBack,
        Self::Failed,
        Self::HeuristicCommit,
        Self::HeuristicRollback,
        Self::HeuristicMixed,
        Self::InDoubt,
        Self::UnknownOutcome,
    ];
}

/// Shared coordinator/participant effect ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantEffectStep {
    /// Observe deadline and cancellation controls before dispatch.
    ObserveDeadlineAndCancellation,
    /// Validate the request, required capability, and participant context.
    ValidateRequestCapabilityAndContext,
    /// Record canonical effect intent durably before participant mutation.
    PersistCanonicalEffectIntent,
    /// Authorize the typed resource and intended operation.
    AuthorizeTypedResourceAndIntent,
    /// Apply the authorized participant mutation.
    ApplyParticipantMutation,
    /// Publish audit and the result, preserving an unknown outcome when needed.
    PersistAuditAndResultOrUnknown,
}

/// Logical lock/CAS order. No store transaction is held across provider dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantLockStep {
    /// Compare and swap the coordinator's effect-intent state.
    CoordinatorIntentCas,
    /// Fence ownership of the participant state.
    ParticipantStateFence,
    /// Acquire resource locks using the declared canonical resource identity.
    CanonicalResourceLocks,
    /// Publish participant unit-of-work and replay state.
    ParticipantUowReplayPublication,
    /// Compare and swap the coordinator's effect-result state.
    CoordinatorResultCas,
}

const REQUIRED_EFFECT_ORDER: &[ParticipantEffectStep] = &[
    ParticipantEffectStep::ObserveDeadlineAndCancellation,
    ParticipantEffectStep::ValidateRequestCapabilityAndContext,
    ParticipantEffectStep::PersistCanonicalEffectIntent,
    ParticipantEffectStep::AuthorizeTypedResourceAndIntent,
    ParticipantEffectStep::ApplyParticipantMutation,
    ParticipantEffectStep::PersistAuditAndResultOrUnknown,
];

const REQUIRED_LOCK_ORDER: &[ParticipantLockStep] = &[
    ParticipantLockStep::CoordinatorIntentCas,
    ParticipantLockStep::ParticipantStateFence,
    ParticipantLockStep::CanonicalResourceLocks,
    ParticipantLockStep::ParticipantUowReplayPublication,
    ParticipantLockStep::CoordinatorResultCas,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared condition and response pair for a rejected participant context.
pub struct ParticipantRejection {
    /// Condition name declared for the rejection.
    pub condition: &'static str,
    /// Primary response value declared for the rejection.
    pub response: i32,
    /// Secondary response value declared for the rejection.
    pub response2: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Syncpoint applicability and authority declared for one named execution context.
pub struct ParticipantContextCapability {
    /// Stable name selecting this execution-context declaration.
    pub context_id: &'static str,
    /// Local, owned distributed, or subordinate distributed participation mode.
    pub mode: ParticipantMode,
    /// Authority responsible for the context's syncpoint decision.
    pub syncpoint_owner: SyncpointOwner,
    /// Whether an explicit syncpoint is supported in this context.
    pub explicit_syncpoint: ExplicitSyncpoint,
    /// Declared condition and responses when explicit syncpoint is rejected.
    pub rejection: Option<ParticipantRejection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared controls that reject stale participant or effect owners.
///
/// These flags describe integration requirements; they do not acquire leases or
/// perform compare-and-swap operations.
pub struct ParticipantFencing {
    /// Whether effect ownership is fenced with a lease epoch.
    pub effect_lease_epoch: bool,
    /// Whether participant row publication uses compare-and-swap fencing.
    pub participant_row_cas: bool,
    /// Whether operations from stale owners are rejected.
    pub stale_owner_rejected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared treatment of deadlines and live cancellation around dispatch.
///
/// Stopping before dispatch and preserving uncertainty after dispatch are
/// separate capabilities; this record does not itself interrupt provider work.
pub struct ParticipantDeadlineCancellation {
    /// Whether participation requires a finite execution deadline.
    pub finite_deadline: bool,
    /// Whether participation observes a shared live cancellation signal.
    pub live_cancellation_probe: bool,
    /// Whether observed controls can stop work before provider dispatch.
    pub pre_dispatch_stop: bool,
    /// Whether controls observed after dispatch preserve unresolved work as unknown.
    pub post_dispatch_unknown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared identity, authorization, and audit obligations at the participant boundary.
pub struct ParticipantSecurityAudit {
    /// Whether caller identity and delegation cross the participant boundary.
    pub principal_and_delegation_propagated: bool,
    /// Whether typed-resource authorization precedes participant mutation.
    pub typed_resource_authorization_before_mutation: bool,
    /// Whether authorization denial and operation failure are audited.
    pub deny_and_failure_audited: bool,
    /// Whether shared-store publication of the relevant records is atomic.
    pub shared_store_atomic_publication: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Identifiers for request, canonical effect, and participant persistence codecs.
///
/// Namespace and codec declarations carry no encoded records or migration logic.
pub struct ParticipantSchemas {
    /// Schema identifier for participant requests.
    pub request: &'static str,
    /// Schema identifier for canonical effects used by the coordinator.
    pub canonical_effect: &'static str,
    /// Store namespace containing participant unit-of-work records.
    pub uow_namespace: &'static str,
    /// Default unit-of-work codec written by the binding.
    pub uow_write: &'static str,
    /// Conditional codec when the effect actor differs from the original task owner.
    pub uow_frame_write: Option<&'static str>,
    /// Unit-of-work codecs accepted for reading, including declared legacy formats.
    pub uow_read: &'static [&'static str],
    /// Store namespace containing participant undo records.
    pub undo_namespace: &'static str,
    /// Codec used for both reading and writing undo records.
    pub undo_read_write: &'static str,
    /// Store namespace containing effect replay records.
    pub replay_namespace: &'static str,
    /// Codec written for effect replay records.
    pub replay_write: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Accepted participant's declared transaction, recovery, and retention capabilities.
///
/// Outcome sets distinguish reported results from results the binding does not
/// produce. Idempotency and replay declarations do not imply exactly-once work,
/// and durable intent does not imply provider prepare support.
pub struct ParticipantCapabilities {
    /// Declared owner of the participant transaction.
    pub transaction_owner: &'static str,
    /// Participation modes accepted by this binding.
    pub supported_modes: &'static [ParticipantMode],
    /// Participation modes explicitly rejected by this binding.
    pub rejected_modes: &'static [ParticipantMode],
    /// Named context declarations refining mode and syncpoint ownership.
    pub contexts: &'static [ParticipantContextCapability],
    /// Declared prepare category, distinct from durable intent recording.
    pub prepare: PrepareCapability,
    /// Whether the binding exposes a provider prepare operation.
    pub provider_prepare: bool,
    /// Whether the binding supports transaction commit.
    pub commit: bool,
    /// Whether the binding supports transaction rollback.
    pub rollback: bool,
    /// Whether compensation is performed automatically.
    pub automatic_compensation: bool,
    /// Whether compensation is supported after committed work.
    pub compensation_after_commit: bool,
    /// Declared scope and applicability of explicit compensation.
    pub compensation_scope: &'static str,
    /// Outcome vocabulary produced by this binding.
    pub reported_outcomes: &'static [ParticipantOutcome],
    /// Outcomes explicitly excluded from the binding's produced results.
    pub not_produced_outcomes: &'static [ParticipantOutcome],
    /// Whether distinct outcomes are collapsed into success; version one forbids this.
    pub collapse_outcomes_to_success: bool,
    /// Identity scope used to recognize repeated participant operations.
    pub idempotency_scope: &'static str,
    /// Declared retention interval or policy controlling idempotency protection.
    pub idempotency_lifetime: &'static str,
    /// Declared request/result identity required for replay matching.
    pub replay_identity: &'static str,
    /// Declared treatment of an identity reused after replay records are pruned.
    pub reuse_after_prune: &'static str,
    /// Whether exactly-once execution is claimed; the accepted v1 binding sets false.
    pub exactly_once: bool,
    /// Lease and row controls protecting participant ownership.
    pub fencing: ParticipantFencing,
    /// Declared control handling before and after dispatch.
    pub deadline_cancellation: ParticipantDeadlineCancellation,
    /// Declared propagation, authorization, and audit requirements.
    pub security_audit: ParticipantSecurityAudit,
    /// Owner responsible for recovering coordinator effect-journal state.
    pub coordinator_recovery_owner: &'static str,
    /// Owner responsible for recovering participant state and replay records.
    pub participant_recovery_owner: &'static str,
    /// Declared process for observing and resolving uncertain participant state.
    pub reconciliation: &'static str,
    /// Whether recovery automatically dispatches work again; v1 forbids this.
    pub automatic_redispatch: bool,
    /// Request and persistence codec declarations for the binding.
    pub schemas: ParticipantSchemas,
    /// Participant record category targeted by retention policy.
    pub retention_target: &'static str,
    /// Watermark used to determine retention eligibility.
    pub retention_watermark: &'static str,
    /// Whether retention protects live checkpoint, audit, and replay dependencies.
    pub protect_live_checkpoint_audit_replay: bool,
    /// Whether the execution deadline sets a lower bound on retention.
    pub deadline_is_retention_lower_bound: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Provider identity and integration status with optional accepted capabilities.
///
/// The version-one validator requires capabilities for the accepted binding and
/// leaves pending bindings without capabilities.
pub struct TransactionParticipantDescriptor {
    /// Stable provider identifier used for exact participant lookup.
    pub provider_id: &'static str,
    /// Accepted or pending integration state of the binding.
    pub status: ParticipantStatus,
    /// Named acceptance boundary or prerequisite for this binding.
    pub dependency: &'static str,
    /// Accepted capabilities when present; pending v1 bindings carry `None`.
    pub capabilities: Option<ParticipantCapabilities>,
    /// Optional local preparation is descriptive only; it never admits a participant.
    pub preparation_scope: Option<&'static str>,
    /// Optional local preparation test locator, without acceptance credit.
    pub preparation_contract_test: Option<&'static str>,
    /// Mandatory obligations still blocking acceptance of the prepared provider.
    pub blocked_obligations: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Versioned declaration of coordinator authority and participant obligations.
///
/// Validation checks the frozen version-one invariants; the declaration does not
/// execute transactions, recover records, or certify provider behavior.
pub struct TransactionParticipantContract {
    /// Stable identifier of this transaction-participant contract generation.
    pub contract_id: &'static str,
    /// Declared contract version.
    pub version: u16,
    /// Identifier of the execution coordinator owning effect coordination.
    pub coordinator: &'static str,
    /// Canonical effect contract shared with the coordinator.
    pub canonical_effect: &'static str,
    /// Store contract used for participant persistence.
    pub store: &'static str,
    /// Whether coordination has one authority; required by the v1 validator.
    pub single_authority: bool,
    /// Whether the declaration changes the public runtime; v1 requires false.
    pub public_runtime_change: bool,
    /// Whether universal two-phase commit is claimed; v1 requires false.
    pub universal_two_phase_commit: bool,
    /// Whether exactly-once execution is claimed; v1 requires false.
    pub exactly_once: bool,
    /// Required logical sequence from control observation to result publication.
    pub effect_order: &'static [ParticipantEffectStep],
    /// Required logical ordering of coordinator and participant fencing/publication.
    pub lock_order: &'static [ParticipantLockStep],
    /// Whether a store transaction spans dispatch; v1 requires false.
    pub store_transaction_across_dispatch: bool,
    /// Declared canonical key shape used to order resource locks.
    pub resource_lock_key: &'static str,
    /// Whether late owners are fenced from publication; required by v1.
    pub late_owner_fenced: bool,
    /// Frozen integration-obligation identifiers checked by validation.
    pub obligations: &'static [&'static str],
    /// Contract version emitted by the current writer.
    pub writer_version: u16,
    /// Contract versions accepted by the reader.
    pub read_versions: &'static [u16],
    /// Provider declarations including accepted and explicitly pending bindings.
    pub participants: &'static [TransactionParticipantDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Failure to read, validate, or select a participant from the declared contract.
pub enum ParticipantContractProblem {
    /// The requested version is not in the accepted reader versions.
    IncompatibleVersion,
    /// The declaration violates a frozen version-one invariant.
    InvalidContract,
    /// No participant has the exact requested provider identifier.
    UnknownParticipant,
}

include!("generated/transaction_participant_v1.rs");

/// Return the validated version-one authority.
#[must_use]
pub fn transaction_participant_contract_v1() -> &'static TransactionParticipantContract {
    &GENERATED_TRANSACTION_PARTICIPANT_V1
}

/// Read exactly one supported contract version. Unknown versions fail closed.
pub fn read_transaction_participant_contract(
    version: u16,
) -> Result<&'static TransactionParticipantContract, ParticipantContractProblem> {
    let contract = transaction_participant_contract_v1();
    if !contract.read_versions.contains(&version) {
        return Err(ParticipantContractProblem::IncompatibleVersion);
    }
    contract.validate()?;
    Ok(contract)
}

impl TransactionParticipantContract {
    /// Borrow the participant with an exactly matching provider identifier.
    ///
    /// Unknown identifiers fail closed. This lookup does not validate the contract
    /// or require the selected participant to be accepted.
    pub fn participant(
        &self,
        provider_id: &str,
    ) -> Result<&TransactionParticipantDescriptor, ParticipantContractProblem> {
        self.participants
            .iter()
            .find(|participant| participant.provider_id == provider_id)
            .ok_or(ParticipantContractProblem::UnknownParticipant)
    }

    /// Validate the single-authority, ordering, outcome, and pending-provider invariants.
    pub fn validate(&self) -> Result<(), ParticipantContractProblem> {
        if self.contract_id != TRANSACTION_PARTICIPANT_CONTRACT
            || self.version != TRANSACTION_PARTICIPANT_VERSION
            || self.writer_version != TRANSACTION_PARTICIPANT_VERSION
            || self.read_versions != [TRANSACTION_PARTICIPANT_VERSION]
            || self.coordinator != "mainframe-env.execution-coordinator@1"
            || self.canonical_effect != "mainframe-env.effect-canonical@1"
            || self.store != "mainframe-env.store@2"
            || !self.single_authority
            || self.public_runtime_change
            || self.universal_two_phase_commit
            || self.exactly_once
            || self.effect_order != REQUIRED_EFFECT_ORDER
            || self.lock_order != REQUIRED_LOCK_ORDER
            || self.store_transaction_across_dispatch
            || self.resource_lock_key != "provider-id/resource-identity"
            || !self.late_owner_fenced
            || self.obligations != REQUIRED_OBLIGATIONS
            || self.participants.len() != 4
        {
            return Err(ParticipantContractProblem::InvalidContract);
        }
        for participant in self.participants {
            match participant.preparation_scope {
                None if participant.preparation_contract_test.is_none()
                    && participant.blocked_obligations.is_empty() => {}
                Some("ims-local-database-provider-route")
                    if participant.provider_id == "ims"
                        && participant.status == ParticipantStatus::Pending
                        && participant.capabilities.is_none()
                        && participant.preparation_contract_test
                            == Some(
                                "crates/providers/mainframe-env-ims/tests/participant_contract.rs",
                            )
                        && participant.blocked_obligations
                            == [
                                "INT-1601.owner",
                                "INT-1601.modes",
                                "INT-1601.ordering",
                                "INT-1601.fencing",
                                "INT-1601.deadline-cancellation",
                                "INT-1601.security-audit",
                                "INT-1601.retention",
                                "INT-1601.compatibility",
                            ] => {}
                _ => return Err(ParticipantContractProblem::InvalidContract),
            }
        }

        let providers = self
            .participants
            .iter()
            .map(|participant| participant.provider_id)
            .collect::<BTreeSet<_>>();
        if providers != BTreeSet::from(["cics", "db2", "ims", "mq"])
            || self.participants[0].provider_id != "cics"
            || self.participants[0].status != ParticipantStatus::Accepted
            || self.participants[0].capabilities.is_none()
            || self.participants[1..].iter().any(|participant| {
                participant.status != ParticipantStatus::Pending
                    || participant.capabilities.is_some()
            })
        {
            return Err(ParticipantContractProblem::InvalidContract);
        }
        validate_accepted_capabilities(
            self.participants[0]
                .capabilities
                .as_ref()
                .ok_or(ParticipantContractProblem::InvalidContract)?,
        )
    }
}

fn validate_accepted_capabilities(
    capabilities: &ParticipantCapabilities,
) -> Result<(), ParticipantContractProblem> {
    let reported = capabilities
        .reported_outcomes
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let not_produced = capabilities
        .not_produced_outcomes
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let all = ParticipantOutcome::ALL.into_iter().collect::<BTreeSet<_>>();
    let contexts = capabilities
        .contexts
        .iter()
        .map(|context| context.context_id)
        .collect::<BTreeSet<_>>();
    if reported.len() != capabilities.reported_outcomes.len()
        || not_produced.len() != capabilities.not_produced_outcomes.len()
        || !reported.is_disjoint(&not_produced)
        || reported
            .union(&not_produced)
            .copied()
            .collect::<BTreeSet<_>>()
            != all
        || !reported.contains(&ParticipantOutcome::UnknownOutcome)
        || capabilities.supported_modes
            != [ParticipantMode::Local, ParticipantMode::DistributedOwned]
        || capabilities.rejected_modes != [ParticipantMode::DistributedSubordinate]
        || contexts
            != BTreeSet::from([
                "local",
                "dpl-synconreturn",
                "dpl-without-synconreturn",
                "dpl-executionset-subset",
            ])
        || capabilities.prepare != PrepareCapability::DurableIntentOnly
        || capabilities.provider_prepare
        || !capabilities.commit
        || !capabilities.rollback
        || capabilities.automatic_compensation
        || capabilities.compensation_after_commit
        || capabilities.collapse_outcomes_to_success
        || capabilities.exactly_once
        || capabilities.automatic_redispatch
        || !capabilities.fencing.effect_lease_epoch
        || !capabilities.fencing.participant_row_cas
        || !capabilities.fencing.stale_owner_rejected
        || !capabilities.deadline_cancellation.finite_deadline
        || !capabilities.deadline_cancellation.live_cancellation_probe
        || !capabilities.deadline_cancellation.pre_dispatch_stop
        || !capabilities.deadline_cancellation.post_dispatch_unknown
        || !capabilities
            .security_audit
            .principal_and_delegation_propagated
        || !capabilities
            .security_audit
            .typed_resource_authorization_before_mutation
        || !capabilities.security_audit.deny_and_failure_audited
        || !capabilities.security_audit.shared_store_atomic_publication
        || capabilities.schemas.canonical_effect != "mainframe-env.effect-canonical@1"
        || capabilities.schemas.uow_write != "MECU2"
        || !matches!(
            (
                capabilities.schemas.uow_frame_write,
                capabilities.schemas.uow_read
            ),
            (None, ["MECU1", "MECU2"]) | (Some("MECU3"), ["MECU1", "MECU2", "MECU3"])
        )
        || !capabilities.protect_live_checkpoint_audit_replay
        || !capabilities.deadline_is_retention_lower_bound
    {
        return Err(ParticipantContractProblem::InvalidContract);
    }
    for context in capabilities.contexts {
        let valid = match context.explicit_syncpoint {
            ExplicitSyncpoint::Supported => {
                context.syncpoint_owner == SyncpointOwner::Participant
                    && context.rejection.is_none()
            }
            ExplicitSyncpoint::Rejected => {
                context.syncpoint_owner == SyncpointOwner::UpstreamHost
                    && context.rejection
                        == Some(ParticipantRejection {
                            condition: "INVREQ",
                            response: 16,
                            response2: 200,
                        })
            }
        };
        if !valid {
            return Err(ParticipantContractProblem::InvalidContract);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ims_preparation_never_admits_or_hides_missing_guarantees() {
        let contract = transaction_participant_contract_v1();
        let ims = contract.participant("ims").unwrap();
        assert_eq!(ims.status, ParticipantStatus::Pending);
        assert!(ims.capabilities.is_none());
        assert_eq!(
            ims.preparation_scope,
            Some("ims-local-database-provider-route")
        );
        let mut participants = contract.participants.to_vec();
        participants[2].blocked_obligations = &[];
        let changed = TransactionParticipantContract {
            participants: Box::leak(participants.into_boxed_slice()),
            ..*contract
        };
        assert_eq!(
            changed.validate(),
            Err(ParticipantContractProblem::InvalidContract)
        );
    }

    #[test]
    fn generated_v1_is_valid_and_future_providers_stay_pending() {
        let contract = transaction_participant_contract_v1();
        assert_eq!(contract.validate(), Ok(()));
        assert_eq!(
            contract.participant("cics").unwrap().status,
            ParticipantStatus::Accepted
        );
        for provider in ["db2", "ims", "mq"] {
            let participant = contract.participant(provider).unwrap();
            assert_eq!(participant.status, ParticipantStatus::Pending);
            assert!(participant.capabilities.is_none());
        }
    }

    #[test]
    fn reader_accepts_only_version_one() {
        assert!(read_transaction_participant_contract(1).is_ok());
        assert_eq!(
            read_transaction_participant_contract(0),
            Err(ParticipantContractProblem::IncompatibleVersion)
        );
        assert_eq!(
            read_transaction_participant_contract(2),
            Err(ParticipantContractProblem::IncompatibleVersion)
        );
    }

    #[test]
    fn frame_codec_declaration_preserves_legacy_and_rejects_incomplete_readers() {
        let mut capabilities = transaction_participant_contract_v1()
            .participant("cics")
            .unwrap()
            .capabilities
            .unwrap();
        assert_eq!(capabilities.schemas.uow_write, "MECU2");
        assert_eq!(capabilities.schemas.uow_frame_write, Some("MECU3"));
        assert_eq!(capabilities.schemas.uow_read, ["MECU1", "MECU2", "MECU3"]);
        capabilities.schemas.uow_read = &["MECU1", "MECU2"];
        assert_eq!(
            validate_accepted_capabilities(&capabilities),
            Err(ParticipantContractProblem::InvalidContract)
        );
        capabilities.schemas.uow_frame_write = None;
        assert_eq!(validate_accepted_capabilities(&capabilities), Ok(()));
        capabilities.schemas.uow_read = &["MECU1", "MECU2", "MECU3"];
        assert_eq!(
            validate_accepted_capabilities(&capabilities),
            Err(ParticipantContractProblem::InvalidContract)
        );
        capabilities.schemas.uow_frame_write = Some("MECU4");
        assert_eq!(
            validate_accepted_capabilities(&capabilities),
            Err(ParticipantContractProblem::InvalidContract)
        );
    }

    #[test]
    fn cics_contexts_partition_owned_and_subordinate_syncpoints() {
        let capabilities = transaction_participant_contract_v1()
            .participant("cics")
            .unwrap()
            .capabilities
            .as_ref()
            .unwrap();
        for context in capabilities.contexts {
            if matches!(context.context_id, "local" | "dpl-synconreturn") {
                assert_eq!(context.syncpoint_owner, SyncpointOwner::Participant);
                assert_eq!(context.explicit_syncpoint, ExplicitSyncpoint::Supported);
            } else {
                assert_eq!(context.syncpoint_owner, SyncpointOwner::UpstreamHost);
                assert_eq!(context.explicit_syncpoint, ExplicitSyncpoint::Rejected);
            }
        }
    }
}
