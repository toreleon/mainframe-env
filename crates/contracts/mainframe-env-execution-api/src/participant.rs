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
    /// The frozen contract admits this binding; runtime evidence remains separately owned.
    Accepted,
    /// No capabilities are admitted yet; the provider name grants no participation permission.
    Pending,
}

/// Execution context in which a provider may be asked to participate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantMode {
    /// One provider owns the local recoverable work and its syncpoint.
    Local,
    /// The participant owns the distributed operation's decision in the declared context.
    DistributedOwned,
    /// An upstream authority owns the decision; local explicit syncpoints may be forbidden.
    DistributedSubordinate,
}

/// Authority that owns the current syncpoint decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncpointOwner {
    /// The admitted participant context can decide its recoverable work.
    Participant,
    /// The embedding host retains the decision; the provider cannot substitute its own.
    UpstreamHost,
}

/// Applicability of an explicit syncpoint command in one execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplicitSyncpoint {
    /// The context admits an explicit command, subject to its ordinary controls and authorization.
    Supported,
    /// The command returns the declared rejection before changing the unit of work.
    Rejected,
}

/// Prepare support is distinct from a durable pre-dispatch intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareCapability {
    /// This binding has no prepare phase.
    NotApplicable,
    /// A durable dispatch intent exists, but is neither a prepare vote nor two-phase commit.
    DurableIntentOnly,
    /// The provider exposes an actual prepare operation requiring its own acceptance evidence.
    ProviderPrepare,
}

/// Closed participant outcome vocabulary. Unsupported outcomes remain explicit
/// capability limits and may not be collapsed into success.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ParticipantOutcome {
    /// The participant authoritatively reports its work committed.
    Committed,
    /// Uncommitted recoverable work was rolled back; this is not post-commit compensation.
    RolledBack,
    /// A known failure was reported, distinct from uncertainty about mutation.
    Failed,
    /// A heuristic decision committed work outside the ordinary decision protocol.
    HeuristicCommit,
    /// A heuristic decision rolled work back outside the ordinary decision protocol.
    HeuristicRollback,
    /// Heuristic decisions produced both committed and rolled-back portions.
    HeuristicMixed,
    /// The participant reports an unresolved transaction decision.
    InDoubt,
    /// Dispatch may have mutated state; only fenced authoritative reconciliation can resolve it.
    UnknownOutcome,
}

impl ParticipantOutcome {
    /// Exhaustive vocabulary to partition into reported and explicitly unproduced outcomes.
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
    /// Observe live controls before proceeding toward dispatch.
    ObserveDeadlineAndCancellation,
    /// Check bounded shape, selected capability and context applicability.
    ValidateRequestCapabilityAndContext,
    /// Retain the original canonical intent before invoking a mutating provider.
    PersistCanonicalEffectIntent,
    /// Obtain the resource-specific authorization decision before mutation.
    AuthorizeTypedResourceAndIntent,
    /// Invoke the participant's sole semantic authority for the admitted occurrence.
    ApplyParticipantMutation,
    /// Retain audit and result, preserving explicit unknown when completion is uncertain.
    PersistAuditAndResultOrUnknown,
}

/// Logical lock/CAS order. No store transaction is held across provider dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantLockStep {
    /// Fence ownership of the original retained coordinator intent.
    CoordinatorIntentCas,
    /// Validate participant state under its live owner/epoch fence.
    ParticipantStateFence,
    /// Acquire provider/resource locks in deterministic identity order.
    CanonicalResourceLocks,
    /// Publish the participant's UOW and replay changes together.
    ParticipantUowReplayPublication,
    /// The coordinator, not the participant, finalizes the retained effect result.
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
/// Source-mapped rejection for an explicit syncpoint in a subordinate context.
pub struct ParticipantRejection {
    /// Application condition identity; it is not a store or infrastructure error.
    pub condition: &'static str,
    /// Primary application response in the declared provider's response domain.
    pub response: i32,
    /// Secondary response detail, interpreted only with the primary response and context.
    pub response2: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Applicability declaration for one named context, not a minted runtime permit.
pub struct ParticipantContextCapability {
    /// Stable context key selected by trusted admission, not inferred from request text.
    pub context_id: &'static str,
    /// Transaction topology admitted for this context.
    pub mode: ParticipantMode,
    /// Authority that retains the syncpoint decision.
    pub syncpoint_owner: SyncpointOwner,
    /// Whether an application may issue an explicit syncpoint here.
    pub explicit_syncpoint: ExplicitSyncpoint,
    /// Required rejection when explicit syncpoint is forbidden; absent for supported contexts.
    pub rejection: Option<ParticipantRejection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Required stale-owner protections, declared separately from runtime fence values.
pub struct ParticipantFencing {
    /// Effect completion checks the coordinator's current lease epoch.
    pub effect_lease_epoch: bool,
    /// Participant publication compares exact durable row versions.
    pub participant_row_cas: bool,
    /// A displaced owner cannot publish over a newer owner.
    pub stale_owner_rejected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Live control obligations at the provider dispatch boundary.
pub struct ParticipantDeadlineCancellation {
    /// Every admitted operation has a finite deadline in its caller's logical clock domain.
    pub finite_deadline: bool,
    /// Dispatch observes a shared live probe, not only a copied cancellation snapshot.
    pub live_cancellation_probe: bool,
    /// A control that wins before dispatch prevents the operation.
    pub pre_dispatch_stop: bool,
    /// A control observed after uncertain dispatch does not prove mutation absent.
    pub post_dispatch_unknown: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Security and audit obligations; these flags themselves authorize no resource.
pub struct ParticipantSecurityAudit {
    /// Original principal and delegation remain attributable across admitted boundaries.
    pub principal_and_delegation_propagated: bool,
    /// The resource/intent decision precedes the participant mutation.
    pub typed_resource_authorization_before_mutation: bool,
    /// Denials and failures remain durable security observations.
    pub deny_and_failure_audited: bool,
    /// State and audit sharing a store authority publish in one physical transaction.
    pub shared_store_atomic_publication: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Provider-owned schema and namespace identifiers used for compatible recovery.
pub struct ParticipantSchemas {
    /// Versioned typed request contract retained by the binding.
    pub request: &'static str,
    /// Shared effect encoding domain; provider-local digests cannot replace it.
    pub canonical_effect: &'static str,
    /// Durable namespace for the provider's unit-of-work rows.
    pub uow_namespace: &'static str,
    /// Schema emitted by the current UOW writer.
    pub uow_write: &'static str,
    /// Exhaustive older/current UOW schemas accepted by the binding's readers.
    pub uow_read: &'static [&'static str],
    /// Durable namespace holding the participant's rollback data.
    pub undo_namespace: &'static str,
    /// Undo schema understood by both reader and writer.
    pub undo_read_write: &'static str,
    /// Namespace retaining authoritative participant replay results.
    pub replay_namespace: &'static str,
    /// Replay schema emitted by the current writer.
    pub replay_write: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Frozen capability limits and obligations of an accepted binding.
///
/// These are declarations checked by the contract validator, not evidence that
/// an arbitrary runtime or provider instance has satisfied them.
pub struct ParticipantCapabilities {
    /// Named authority responsible for transaction decisions.
    pub transaction_owner: &'static str,
    /// Modes admitted by the accepted binding.
    pub supported_modes: &'static [ParticipantMode],
    /// Modes deliberately refused, disjoint from the supported set.
    pub rejected_modes: &'static [ParticipantMode],
    /// Context-specific decision ownership and explicit-syncpoint rules.
    pub contexts: &'static [ParticipantContextCapability],
    /// Prepare applicability, distinct from recording a dispatch intent.
    pub prepare: PrepareCapability,
    /// Whether this binding supplies a real provider prepare operation.
    pub provider_prepare: bool,
    /// Commit is supported only in the declared applicable contexts.
    pub commit: bool,
    /// Rollback is supported only for the declared recoverable work.
    pub rollback: bool,
    /// Whether compensation is automatic; the frozen v1 binding requires false.
    pub automatic_compensation: bool,
    /// Whether committed work may be compensated; v1 admits no such capability.
    pub compensation_after_commit: bool,
    /// Named boundary of any compensation, separate from ordinary rollback.
    pub compensation_scope: &'static str,
    /// Outcomes the participant may actually report, without normalization to success.
    pub reported_outcomes: &'static [ParticipantOutcome],
    /// Remaining outcomes it explicitly does not produce; together the sets exhaust the vocabulary.
    pub not_produced_outcomes: &'static [ParticipantOutcome],
    /// Whether distinctions may be erased; the validator requires false.
    pub collapse_outcomes_to_success: bool,
    /// Identity domain within which an occurrence's replay is scoped.
    pub idempotency_scope: &'static str,
    /// Declared durable retention lifetime for idempotent observations.
    pub idempotency_lifetime: &'static str,
    /// Canonical identity compared on replay; unrelated or metadata-only identities cannot replace it.
    pub replay_identity: &'static str,
    /// Policy for reuse after safe pruning, which is not an exactly-once guarantee.
    pub reuse_after_prune: &'static str,
    /// Exactly-once claim; the frozen contract requires false.
    pub exactly_once: bool,
    /// Required lease, row and displaced-owner protections.
    pub fencing: ParticipantFencing,
    /// Required live-control checks and post-dispatch uncertainty handling.
    pub deadline_cancellation: ParticipantDeadlineCancellation,
    /// Required principal, resource decision and audit-publication protections.
    pub security_audit: ParticipantSecurityAudit,
    /// Authority that owns effect intent/result reconciliation.
    pub coordinator_recovery_owner: &'static str,
    /// Authority that interprets the participant's UOW/replay recovery state.
    pub participant_recovery_owner: &'static str,
    /// Fenced observation procedure for resolving uncertain effects without redispatch.
    pub reconciliation: &'static str,
    /// Whether uncertain mutations are retried automatically; v1 requires false.
    pub automatic_redispatch: bool,
    /// Durable namespaces and admitted reader/writer versions.
    pub schemas: ParticipantSchemas,
    /// Retention family used to age the participant's retained observations.
    pub retention_target: &'static str,
    /// Named age authority; it does not override live recovery dependencies.
    pub retention_watermark: &'static str,
    /// Retention must preserve every live checkpoint, audit and replay reference.
    pub protect_live_checkpoint_audit_replay: bool,
    /// The original invocation deadline is a conservative lower bound on retention age.
    pub deadline_is_retention_lower_bound: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One provider's admission status in the frozen participant inventory.
pub struct TransactionParticipantDescriptor {
    /// Stable provider identity used for lookup, not runtime dispatch authority.
    pub provider_id: &'static str,
    /// Whether a binding is accepted or remains capability-pending.
    pub status: ParticipantStatus,
    /// Named prerequisite whose separate evidence is needed for integration.
    pub dependency: &'static str,
    /// Present for accepted bindings and absent for pending ones.
    pub capabilities: Option<ParticipantCapabilities>,
    /// Optional local preparation is descriptive only; it never admits a participant.
    pub preparation_scope: Option<&'static str>,
    /// Optional local preparation test locator, without acceptance credit.
    pub preparation_contract_test: Option<&'static str>,
    /// Mandatory obligations still blocking acceptance of the prepared provider.
    pub blocked_obligations: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Versioned description consumed by the existing coordinator, not another transaction manager.
pub struct TransactionParticipantContract {
    /// Stable contract domain, equal to [`TRANSACTION_PARTICIPANT_CONTRACT`] in v1.
    pub contract_id: &'static str,
    /// Contract vocabulary version, distinct from provider storage schema versions.
    pub version: u16,
    /// Sole shared execution coordinator identity.
    pub coordinator: &'static str,
    /// Frozen shared host-effect encoding domain.
    pub canonical_effect: &'static str,
    /// Shared store contract required for publication and recovery.
    pub store: &'static str,
    /// Declares one execution authority rather than competing provider coordinators.
    pub single_authority: bool,
    /// Whether this descriptor itself changes public runtime dispatch; v1 requires false.
    pub public_runtime_change: bool,
    /// Universal two-phase-commit claim; v1 requires false.
    pub universal_two_phase_commit: bool,
    /// Universal exactly-once claim; v1 requires false.
    pub exactly_once: bool,
    /// Exhaustive required order from live controls through durable audit/result.
    pub effect_order: &'static [ParticipantEffectStep],
    /// Logical fence/lock/CAS order, not one physical transaction across dispatch.
    pub lock_order: &'static [ParticipantLockStep],
    /// Whether a store transaction spans provider dispatch; the validator requires false.
    pub store_transaction_across_dispatch: bool,
    /// Canonical provider/resource identity order for resource locks.
    pub resource_lock_key: &'static str,
    /// Completion cannot overwrite a newer recovery/lease owner.
    pub late_owner_fenced: bool,
    /// Required acceptance obligation identities, independent of execution verdicts.
    pub obligations: &'static [&'static str],
    /// Version emitted by the contract writer.
    pub writer_version: u16,
    /// Exhaustive compatible reader versions; v1 accepts only one.
    pub read_versions: &'static [u16],
    /// Fixed provider inventory with accepted/pending bindings.
    pub participants: &'static [TransactionParticipantDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Fail-closed reader, invariant or provider-lookup rejection.
pub enum ParticipantContractProblem {
    /// The requested version is not in the retained contract's reader set.
    IncompatibleVersion,
    /// The descriptor violates the frozen authority, ordering or capability invariants.
    InvalidContract,
    /// No entry exists for the exact supplied provider identity.
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
    /// Borrow the exact provider entry, including its pending status.
    ///
    /// Returns [`ParticipantContractProblem::UnknownParticipant`] for an absent name;
    /// lookup neither validates the entire contract nor accepts a pending binding.
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
