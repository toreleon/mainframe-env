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
    Accepted,
    Pending,
}

/// Execution context in which a provider may be asked to participate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantMode {
    Local,
    DistributedOwned,
    DistributedSubordinate,
}

/// Authority that owns the current syncpoint decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncpointOwner {
    Participant,
    UpstreamHost,
}

/// Applicability of an explicit syncpoint command in one execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplicitSyncpoint {
    Supported,
    Rejected,
}

/// Prepare support is distinct from a durable pre-dispatch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareCapability {
    NotApplicable,
    DurableIntentOnly,
    ProviderPrepare,
}

/// Closed participant outcome vocabulary. Unsupported outcomes remain explicit
/// capability limits and may not be collapsed into success.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantOutcome {
    Committed,
    RolledBack,
    Failed,
    HeuristicCommit,
    HeuristicRollback,
    HeuristicMixed,
    InDoubt,
    UnknownOutcome,
}

impl ParticipantOutcome {
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
    ObserveDeadlineAndCancellation,
    ValidateRequestCapabilityAndContext,
    PersistCanonicalEffectIntent,
    AuthorizeTypedResourceAndIntent,
    ApplyParticipantMutation,
    PersistAuditAndResultOrUnknown,
}

/// Logical lock/CAS order. No store transaction is held across provider dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantLockStep {
    CoordinatorIntentCas,
    ParticipantStateFence,
    CanonicalResourceLocks,
    ParticipantUowReplayPublication,
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
pub struct ParticipantRejection {
    pub condition: &'static str,
    pub response: i32,
    pub response2: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantContextCapability {
    pub context_id: &'static str,
    pub mode: ParticipantMode,
    pub syncpoint_owner: SyncpointOwner,
    pub explicit_syncpoint: ExplicitSyncpoint,
    pub rejection: Option<ParticipantRejection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantFencing {
    pub effect_lease_epoch: bool,
    pub participant_row_cas: bool,
    pub stale_owner_rejected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantDeadlineCancellation {
    pub finite_deadline: bool,
    pub live_cancellation_probe: bool,
    pub pre_dispatch_stop: bool,
    pub post_dispatch_unknown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantSecurityAudit {
    pub principal_and_delegation_propagated: bool,
    pub typed_resource_authorization_before_mutation: bool,
    pub deny_and_failure_audited: bool,
    pub shared_store_atomic_publication: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantSchemas {
    pub request: &'static str,
    pub canonical_effect: &'static str,
    pub uow_namespace: &'static str,
    pub uow_write: &'static str,
    pub uow_read: &'static [&'static str],
    pub undo_namespace: &'static str,
    pub undo_read_write: &'static str,
    pub replay_namespace: &'static str,
    pub replay_write: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantCapabilities {
    pub transaction_owner: &'static str,
    pub supported_modes: &'static [ParticipantMode],
    pub rejected_modes: &'static [ParticipantMode],
    pub contexts: &'static [ParticipantContextCapability],
    pub prepare: PrepareCapability,
    pub provider_prepare: bool,
    pub commit: bool,
    pub rollback: bool,
    pub automatic_compensation: bool,
    pub compensation_after_commit: bool,
    pub compensation_scope: &'static str,
    pub reported_outcomes: &'static [ParticipantOutcome],
    pub not_produced_outcomes: &'static [ParticipantOutcome],
    pub collapse_outcomes_to_success: bool,
    pub idempotency_scope: &'static str,
    pub idempotency_lifetime: &'static str,
    pub replay_identity: &'static str,
    pub reuse_after_prune: &'static str,
    pub exactly_once: bool,
    pub fencing: ParticipantFencing,
    pub deadline_cancellation: ParticipantDeadlineCancellation,
    pub security_audit: ParticipantSecurityAudit,
    pub coordinator_recovery_owner: &'static str,
    pub participant_recovery_owner: &'static str,
    pub reconciliation: &'static str,
    pub automatic_redispatch: bool,
    pub schemas: ParticipantSchemas,
    pub retention_target: &'static str,
    pub retention_watermark: &'static str,
    pub protect_live_checkpoint_audit_replay: bool,
    pub deadline_is_retention_lower_bound: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionParticipantDescriptor {
    pub provider_id: &'static str,
    pub status: ParticipantStatus,
    pub dependency: &'static str,
    pub capabilities: Option<ParticipantCapabilities>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionParticipantContract {
    pub contract_id: &'static str,
    pub version: u16,
    pub coordinator: &'static str,
    pub canonical_effect: &'static str,
    pub store: &'static str,
    pub single_authority: bool,
    pub public_runtime_change: bool,
    pub universal_two_phase_commit: bool,
    pub exactly_once: bool,
    pub effect_order: &'static [ParticipantEffectStep],
    pub lock_order: &'static [ParticipantLockStep],
    pub store_transaction_across_dispatch: bool,
    pub resource_lock_key: &'static str,
    pub late_owner_fenced: bool,
    pub obligations: &'static [&'static str],
    pub writer_version: u16,
    pub read_versions: &'static [u16],
    pub participants: &'static [TransactionParticipantDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantContractProblem {
    IncompatibleVersion,
    InvalidContract,
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
