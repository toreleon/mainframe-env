use crate::{AccessIntent, HostLimits, HostProblem, ResourceName};
use mainframe_env_execution_api::PrincipalId;

/// A closed enterprise resource class mapped to a discrete SAF class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnterpriseResourceClass {
    /// A Db2 table or view named by parsed SQL.
    Db2Table,
    /// A Db2 plan/package administration target.
    Db2Plan,
    /// A Db2 transaction whose table set is empty or not yet established.
    Db2UnitOfWork,
    /// An IMS program specification block.
    ImsPsb,
    /// An IMS database resolved through the selected PCB.
    ImsDatabase,
    /// An IMS transaction with no pending database image.
    ImsUnitOfWork,
    /// An MQ queue resolved directly or through an open handle.
    MqQueue,
    /// An MQ transaction with no pending queue operation.
    MqUnitOfWork,
}

impl EnterpriseResourceClass {
    /// Stable SAF class used for the resource decision.
    #[must_use]
    pub const fn saf_class(self) -> &'static str {
        match self {
            Self::Db2Table => "DB2TABLE",
            Self::Db2Plan => "DB2PLAN",
            Self::Db2UnitOfWork => "DB2UOW",
            Self::ImsPsb => "IMSPSB",
            Self::ImsDatabase => "IMSDB",
            Self::ImsUnitOfWork => "IMSUOW",
            Self::MqQueue => "MQQUEUE",
            Self::MqUnitOfWork => "MQUOW",
        }
    }
}

/// One typed, resource-specific decision required before enterprise dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnterpriseResource {
    /// Closed class that selects the SAF policy namespace.
    pub class: EnterpriseResourceClass,
    /// Bounded canonical resource name inside the class.
    pub name: ResourceName,
    /// Requested read, execute, update, control, or alter intent.
    pub intent: AccessIntent,
}

impl EnterpriseResource {
    /// Build a bounded resource decision from an already parsed provider name.
    pub fn new(
        class: EnterpriseResourceClass,
        name: impl Into<String>,
        intent: AccessIntent,
    ) -> Result<Self, HostProblem> {
        Ok(Self {
            class,
            name: ResourceName::new(name, HostLimits::default().max_name_bytes)
                .map_err(|_| HostProblem::Malformed)?,
            intent,
        })
    }
}

/// Fail-closed policy boundary injected into enterprise providers.
pub trait EnterpriseAuthorizer: Send + Sync {
    /// Require a positive decision for `principal` and the exact typed resource.
    fn authorize(
        &self,
        principal: &PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem>;
}
