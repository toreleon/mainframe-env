//! Host-environment and syncpoint-owner applicability for IBM MQI calls.

/// `MQCC_FAILED`, returned for an MQI call that is invalid in its environment.
pub const MQCC_FAILED: i32 = 2;
/// `MQRC_ENVIRONMENT_ERROR`, returned when the host owns the syncpoint.
pub const MQRC_ENVIRONMENT_ERROR: i32 = 2012;

/// MQI calls whose applicability depends on the unit-of-work coordinator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqSyncpointCall {
    /// `MQBACK` — back out queue-manager coordinated work.
    Back,
    /// `MQBEGIN` — begin a queue-manager coordinated global unit of work.
    Begin,
    /// `MQCMIT` — commit queue-manager coordinated work.
    Commit,
}

impl MqSyncpointCall {
    /// Stable official MQ catalog row for this call.
    #[must_use]
    pub const fn official_row(self) -> &'static str {
        match self {
            Self::Back => "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0001",
            Self::Begin => "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0002",
            Self::Commit => "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0007",
        }
    }

    /// Exact IBM MQI symbol.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Back => "MQBACK",
            Self::Begin => "MQBEGIN",
            Self::Commit => "MQCMIT",
        }
    }
}

/// Execution environment relevant to MQI syncpoint applicability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqHostEnvironment {
    /// A z/OS batch or TSO task.
    ZosBatch,
    /// An IMS batch DL/I program, which follows the z/OS batch rule.
    ZosImsBatchDli,
    /// A CICS application task.
    ZosCics,
    /// An IMS application other than batch DL/I.
    ZosIms,
    /// An application using MQ client bindings.
    MqiClient,
    /// Non-z/OS bindings connected directly to a queue manager.
    OtherBindings,
}

/// Authority that owns completion of the current unit of work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqSyncpointOwner {
    /// IBM MQ coordinates the unit of work.
    QueueManager,
    /// CICS, IMS, RRS, or another external coordinator owns completion.
    HostCoordinator,
}

/// Result of applying one direct MQI call in an execution context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqContextDisposition {
    /// The direct MQI call is applicable; other call validation still applies.
    Allowed,
    /// The direct call must fail without changing MQ state.
    Rejected {
        /// MQ completion code.
        completion_code: i32,
        /// MQ reason code.
        reason_code: i32,
    },
}

const ENVIRONMENT_REJECTION: MqContextDisposition = MqContextDisposition::Rejected {
    completion_code: MQCC_FAILED,
    reason_code: MQRC_ENVIRONMENT_ERROR,
};

/// Resolve direct-call applicability for the pinned MQI syncpoint surface.
///
/// This function describes MQI calls issued by an application. An internal
/// participant action dispatched by the owning host coordinator is not a
/// direct MQI call and is validated through its separate provenance contract.
#[must_use]
pub const fn mq_syncpoint_context_disposition(
    call: MqSyncpointCall,
    environment: MqHostEnvironment,
    owner: MqSyncpointOwner,
) -> MqContextDisposition {
    if matches!(owner, MqSyncpointOwner::HostCoordinator) {
        return ENVIRONMENT_REJECTION;
    }
    match (call, environment) {
        (
            MqSyncpointCall::Back | MqSyncpointCall::Commit,
            MqHostEnvironment::ZosCics | MqHostEnvironment::ZosIms,
        )
        | (
            MqSyncpointCall::Begin,
            MqHostEnvironment::ZosCics | MqHostEnvironment::ZosIms | MqHostEnvironment::MqiClient,
        ) => ENVIRONMENT_REJECTION,
        _ => MqContextDisposition::Allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mq_mqi_call_identity, official_semantic_identity};

    const CALLS: [MqSyncpointCall; 3] = [
        MqSyncpointCall::Back,
        MqSyncpointCall::Begin,
        MqSyncpointCall::Commit,
    ];
    const ENVIRONMENTS: [MqHostEnvironment; 6] = [
        MqHostEnvironment::ZosBatch,
        MqHostEnvironment::ZosImsBatchDli,
        MqHostEnvironment::ZosCics,
        MqHostEnvironment::ZosIms,
        MqHostEnvironment::MqiClient,
        MqHostEnvironment::OtherBindings,
    ];

    #[test]
    fn syncpoint_calls_bind_to_the_pinned_catalog_and_topics() {
        for call in CALLS {
            let official = official_semantic_identity(call.official_row()).unwrap();
            let mq = mq_mqi_call_identity(call.official_row()).unwrap();
            assert_eq!(official.label, call.label());
            assert_eq!(mq.label, call.label());
            assert_eq!(mq.topic_sha256.len(), 64);
        }
    }

    #[test]
    fn host_coordinator_rejects_every_direct_syncpoint_call() {
        for call in CALLS {
            for environment in ENVIRONMENTS {
                assert_eq!(
                    mq_syncpoint_context_disposition(
                        call,
                        environment,
                        MqSyncpointOwner::HostCoordinator,
                    ),
                    ENVIRONMENT_REJECTION
                );
            }
        }
    }

    #[test]
    fn cics_and_non_batch_ims_reject_direct_commit_and_backout() {
        for call in [MqSyncpointCall::Back, MqSyncpointCall::Commit] {
            for environment in [MqHostEnvironment::ZosCics, MqHostEnvironment::ZosIms] {
                assert_eq!(
                    mq_syncpoint_context_disposition(
                        call,
                        environment,
                        MqSyncpointOwner::QueueManager,
                    ),
                    ENVIRONMENT_REJECTION
                );
            }
            for environment in [
                MqHostEnvironment::ZosBatch,
                MqHostEnvironment::ZosImsBatchDli,
                MqHostEnvironment::MqiClient,
                MqHostEnvironment::OtherBindings,
            ] {
                assert_eq!(
                    mq_syncpoint_context_disposition(
                        call,
                        environment,
                        MqSyncpointOwner::QueueManager,
                    ),
                    MqContextDisposition::Allowed
                );
            }
        }
    }

    #[test]
    fn mqbegin_rejects_host_owned_and_client_environments() {
        for environment in [
            MqHostEnvironment::ZosCics,
            MqHostEnvironment::ZosIms,
            MqHostEnvironment::MqiClient,
        ] {
            assert_eq!(
                mq_syncpoint_context_disposition(
                    MqSyncpointCall::Begin,
                    environment,
                    MqSyncpointOwner::QueueManager,
                ),
                ENVIRONMENT_REJECTION
            );
        }
        for environment in [
            MqHostEnvironment::ZosBatch,
            MqHostEnvironment::ZosImsBatchDli,
            MqHostEnvironment::OtherBindings,
        ] {
            assert_eq!(
                mq_syncpoint_context_disposition(
                    MqSyncpointCall::Begin,
                    environment,
                    MqSyncpointOwner::QueueManager,
                ),
                MqContextDisposition::Allowed
            );
        }
    }
}
