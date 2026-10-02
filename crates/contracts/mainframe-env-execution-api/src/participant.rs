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
    /// The descriptor carries validated capabilities admitted at this shared boundary.
    Accepted,
    /// Integration guarantees remain unaccepted; preparation does not admit the provider.
    Pending,
}

/// Execution context in which a provider may be asked to participate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantMode {
    /// Participation in a locally owned unit of work.
    Local,
    /// Distributed context in which this participant owns its syncpoint.
    DistributedOwned,
    /// Distributed context in which an upstream owner controls completion.
    DistributedSubordinate,
}

/// Authority that owns the current syncpoint decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncpointOwner {
    /// The participant decides the current syncpoint.
    Participant,
    /// An upstream host controls the current syncpoint decision.
    UpstreamHost,
}

/// Applicability of an explicit syncpoint command in one execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplicitSyncpoint {
    /// The context admits an explicit participant syncpoint.
    Supported,
    /// The context rejects explicit syncpoint with its declared rejection response.
    Rejected,
}

/// Prepare support is distinct from a durable pre-dispatch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareCapability {
    /// The context has no applicable provider prepare phase.
    NotApplicable,
    /// A durable pre-dispatch intent exists without provider prepare support.
    DurableIntentOnly,
    /// The descriptor declares an explicit provider prepare capability.
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
    /// The participant reports failure without asserting a successful completion.
    Failed,
    /// A heuristic decision committed work.
    HeuristicCommit,
    /// A heuristic decision rolled work back.
    HeuristicRollback,
    /// Heuristic decisions produced a mixture of committed and rolled-back work.
    HeuristicMixed,
    /// Completion awaits an authoritative decision.
    InDoubt,
    /// Dispatch may have taken effect; the durable outcome cannot yet be established.
    UnknownOutcome,
}

impl ParticipantOutcome {
    /// Complete ordered outcome vocabulary used to validate descriptor partitions.
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
    /// Observe finite deadline and live cancellation before dispatch.
    ObserveDeadlineAndCancellation,
    /// Validate operands, capability and execution context.
    ValidateRequestCapabilityAndContext,
    /// Persist the canonical intent before invoking the participant.
    PersistCanonicalEffectIntent,
    /// Authorize the typed protected resource and access intent.
    AuthorizeTypedResourceAndIntent,
    /// Apply mutation only after validation and authorization.
    ApplyParticipantMutation,
    /// Publish audit and result, preserving unresolved outcomes explicitly.
    PersistAuditAndResultOrUnknown,
}

/// Logical lock/CAS order. No store transaction is held across provider dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantLockStep {
    /// Fence the coordinator intent through compare-and-swap.
    CoordinatorIntentCas,
    /// Establish the participant's state ownership fence.
    ParticipantStateFence,
    /// Acquire locks in canonical resource identity order.
    CanonicalResourceLocks,
    /// Publish participant UOW state and replay together.
    ParticipantUowReplayPublication,
    /// Fence the coordinator's final result publication.
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
/// Context-specific condition and numeric responses for rejected syncpoints.
pub struct ParticipantRejection {
    /// Stable condition name returned by the rejected context.
    pub condition: &'static str,
    /// Primary numeric response paired with the condition.
    pub response: i32,
    /// Secondary numeric response refining the condition.
    pub response2: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Named execution context and its explicit syncpoint ownership rule.
pub struct ParticipantContextCapability {
    /// Stable context identity within the provider's descriptor.
    pub context_id: &'static str,
    /// Transaction mode classified by this context.
    pub mode: ParticipantMode,
    /// Authority permitted to decide completion in this context.
    pub syncpoint_owner: SyncpointOwner,
    /// Whether the context accepts an explicit syncpoint.
    pub explicit_syncpoint: ExplicitSyncpoint,
    /// Required response when syncpoint is rejected; absent for supported contexts.
    pub rejection: Option<ParticipantRejection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared protections against stale effect and participant owners.
pub struct ParticipantFencing {
    /// Whether effect ownership is fenced by a lease epoch.
    pub effect_lease_epoch: bool,
    /// Whether participant publication compares its observed row version.
    pub participant_row_cas: bool,
    /// Whether an obsolete owner is rejected before publication.
    pub stale_owner_rejected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared pre-dispatch controls and post-dispatch uncertainty handling.
pub struct ParticipantDeadlineCancellation {
    /// Whether every effect carries a finite deadline.
    pub finite_deadline: bool,
    /// Whether execution observes live cancellation requests.
    pub live_cancellation_probe: bool,
    /// Whether cancellation or timeout can stop work before dispatch.
    pub pre_dispatch_stop: bool,
    /// Whether unresolved post-dispatch completion remains unknown.
    pub post_dispatch_unknown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared propagation, authorization and atomic audit guarantees.
pub struct ParticipantSecurityAudit {
    /// Whether principal and delegation reach the participant boundary.
    pub principal_and_delegation_propagated: bool,
    /// Whether typed resource authorization precedes mutation.
    pub typed_resource_authorization_before_mutation: bool,
    /// Whether denial and failure decisions receive audit records.
    pub deny_and_failure_audited: bool,
    /// Whether results and audit sharing store authority publish atomically.
    pub shared_store_atomic_publication: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Writer/reader identities and namespaces owned by the participant.
pub struct ParticipantSchemas {
    /// Versioned owned request contract identity.
    pub request: &'static str,
    /// Canonical encoding contract used for effect identity.
    pub canonical_effect: &'static str,
    /// Shared-store namespace retaining participant unit-of-work state.
    pub uow_namespace: &'static str,
    /// Unit-of-work format emitted by the current writer.
    pub uow_write: &'static str,
    /// Retained unit-of-work formats accepted by the reader.
    pub uow_read: &'static [&'static str],
    /// Shared-store namespace retaining undo state.
    pub undo_namespace: &'static str,
    /// Undo format shared by the declared reader and writer.
    pub undo_read_write: &'static str,
    /// Shared-store namespace retaining replay receipts.
    pub replay_namespace: &'static str,
    /// Replay format emitted by this participant.
    pub replay_write: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Declared guarantees of an accepted participant; presence requires contract validation.
pub struct ParticipantCapabilities {
    /// Named authority owning transaction state.
    pub transaction_owner: &'static str,
    /// Modes admitted by this descriptor.
    pub supported_modes: &'static [ParticipantMode],
    /// Modes explicitly excluded from participation.
    pub rejected_modes: &'static [ParticipantMode],
    /// Per-context syncpoint ownership and rejection rules.
    pub contexts: &'static [ParticipantContextCapability],
    /// Applicability of prepare independently of durable intent.
    pub prepare: PrepareCapability,
    /// Whether the provider implements an explicit prepare phase.
    pub provider_prepare: bool,
    /// Whether the participant supports commit.
    pub commit: bool,
    /// Whether the participant supports rollback within its declared scope.
    pub rollback: bool,
    /// Whether compensation is performed automatically.
    pub automatic_compensation: bool,
    /// Whether committed effects can be compensated by this boundary.
    pub compensation_after_commit: bool,
    /// Named boundary of permitted compensation.
    pub compensation_scope: &'static str,
    /// Outcomes the participant may emit; disjoint from not-produced outcomes.
    pub reported_outcomes: &'static [ParticipantOutcome],
    /// Outcomes excluded by this capability set, completing the closed vocabulary.
    pub not_produced_outcomes: &'static [ParticipantOutcome],
    /// Whether distinct outcomes are collapsed; version one requires false.
    pub collapse_outcomes_to_success: bool,
    /// Named domain in which an idempotency identity is unique.
    pub idempotency_scope: &'static str,
    /// Declared duration or retention rule protecting replay identity.
    pub idempotency_lifetime: &'static str,
    /// Definition of the canonical identity used for replay matching.
    pub replay_identity: &'static str,
    /// Declared rule for identities after replay receipt pruning.
    pub reuse_after_prune: &'static str,
    /// Exactly-once claim flag; version one does not admit that claim.
    pub exactly_once: bool,
    /// Required lease, row-version and stale-owner protections.
    pub fencing: ParticipantFencing,
    /// Declared live controls and uncertainty handling.
    pub deadline_cancellation: ParticipantDeadlineCancellation,
    /// Declared principal, authorization and audit publication guarantees.
    pub security_audit: ParticipantSecurityAudit,
    /// Authority reconciling coordinator state.
    pub coordinator_recovery_owner: &'static str,
    /// Authority reconciling participant state.
    pub participant_recovery_owner: &'static str,
    /// Declared procedure for authoritative outcome observation.
    pub reconciliation: &'static str,
    /// Whether ambiguous mutation is redispatched; version one requires false.
    pub automatic_redispatch: bool,
    /// Owned retained formats and their shared-store namespaces.
    pub schemas: ParticipantSchemas,
    /// Named retention authority for participant state.
    pub retention_target: &'static str,
    /// Declared rule bounding safely prunable state.
    pub retention_watermark: &'static str,
    /// Whether live checkpoint, audit and replay references prevent pruning.
    pub protect_live_checkpoint_audit_replay: bool,
    /// Whether the effect deadline sets a minimum retention boundary.
    pub deadline_is_retention_lower_bound: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Provider readiness and optional accepted capabilities; local preparation is not admission.
pub struct TransactionParticipantDescriptor {
    /// Provider identity selected by the shared participant registry.
    pub provider_id: &'static str,
    /// Accepted or pending integration disposition.
    pub status: ParticipantStatus,
    /// Declared prerequisite work-package identity.
    pub dependency: &'static str,
    /// Accepted guarantees; pending providers carry no accepted capability value.
    pub capabilities: Option<ParticipantCapabilities>,
    /// Optional local preparation is descriptive only; it never admits a participant.
    pub preparation_scope: Option<&'static str>,
    /// Optional local preparation test locator, without acceptance credit.
    pub preparation_contract_test: Option<&'static str>,
    /// Mandatory obligations still blocking acceptance of the prepared provider.
    pub blocked_obligations: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Versioned shared authority for participant admission, ordering and retained schema compatibility.
pub struct TransactionParticipantContract {
    /// Stable participant contract identity checked by the reader.
    pub contract_id: &'static str,
    /// Descriptor generation accepted by validation.
    pub version: u16,
    /// Existing coordinator contract owning syncpoint decisions.
    pub coordinator: &'static str,
    /// Shared canonical effect identity contract.
    pub canonical_effect: &'static str,
    /// Shared durable-store contract used by participants.
    pub store: &'static str,
    /// Whether one coordinator authority owns this boundary; required true.
    pub single_authority: bool,
    /// Whether this descriptor introduces public runtime behavior; required false.
    pub public_runtime_change: bool,
    /// Universal two-phase-commit claim; version one requires false.
    pub universal_two_phase_commit: bool,
    /// Exactly-once claim; version one requires false.
    pub exactly_once: bool,
    /// Required validation, intent, authorization, mutation and result ordering.
    pub effect_order: &'static [ParticipantEffectStep],
    /// Required coordinator, participant and resource lock/CAS ordering.
    pub lock_order: &'static [ParticipantLockStep],
    /// Whether a store transaction spans dispatch; required false.
    pub store_transaction_across_dispatch: bool,
    /// Canonical provider/resource identity recipe used for lock ordering.
    pub resource_lock_key: &'static str,
    /// Whether a late owner is fenced from publication; required true.
    pub late_owner_fenced: bool,
    /// Exact mandatory obligation identities for this contract generation.
    pub obligations: &'static [&'static str],
    /// Current descriptor writer generation.
    pub writer_version: u16,
    /// Explicit accepted reader generations; unknown versions fail closed.
    pub read_versions: &'static [u16],
    /// Ordered provider descriptors, including honest pending dispositions.
    pub participants: &'static [TransactionParticipantDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Failure to read or validate the shared participant authority.
pub enum ParticipantContractProblem {
    /// The requested reader generation is unsupported.
    IncompatibleVersion,
    /// The descriptor violates the frozen admission or compatibility invariants.
    InvalidContract,
    /// No provider has the requested exact identity.
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
    /// Look up an exact provider identity; unknown providers return an explicit error.
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
        || capabilities.schemas.uow_read != ["MECU1", "MECU2"]
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
