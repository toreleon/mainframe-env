//! Actual resolved SAF resources retained for output replay, not policy permits.
use super::*;
const MAX_RESOURCES: usize = 512;
pub(super) fn authorize_property(
    authorizer: &dyn EnterpriseAuthorizer,
    invocation: &Invocation,
    intent: AccessIntent,
) -> Result<(), HostProblem> {
    authorizer.authorize(
        invocation.principal.id(),
        &EnterpriseResource::new(EnterpriseResourceClass::MqUnitOfWork, "CURRENT", intent)?,
    )
}
pub(super) fn bounded_resources<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Vec<StoredResource>, D::Error> {
    struct Resources;
    impl<'de> serde::de::Visitor<'de> for Resources {
        type Value = Vec<StoredResource>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded resolved MQ resources")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<StoredResource>()? {
                if values.len() == MAX_RESOURCES {
                    return Err(serde::de::Error::custom("resource ceiling"));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    decoder.deserialize_seq(Resources)
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Class {
    Queue,
    UnitOfWork,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Intent {
    Read,
    Execute,
    Update,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredResource {
    class: Class,
    name: String,
    intent: Intent,
}
impl StoredResource {
    fn capture(resource: &EnterpriseResource) -> Result<Self, HostProblem> {
        Ok(Self {
            class: match resource.class {
                EnterpriseResourceClass::MqQueue => Class::Queue,
                EnterpriseResourceClass::MqUnitOfWork => Class::UnitOfWork,
                _ => return Err(HostProblem::Unsupported),
            },
            name: resource.name.as_str().into(),
            intent: match resource.intent {
                AccessIntent::Read => Intent::Read,
                AccessIntent::Execute => Intent::Execute,
                AccessIntent::Update => Intent::Update,
                _ => return Err(HostProblem::Unsupported),
            },
        })
    }
    fn resource(&self) -> Result<EnterpriseResource, HostProblem> {
        let class = match self.class {
            Class::Queue => {
                crate::MqObjectName::new(&self.name).map_err(|_| HostProblem::Malformed)?;
                EnterpriseResourceClass::MqQueue
            }
            Class::UnitOfWork if self.name == "CURRENT" => EnterpriseResourceClass::MqUnitOfWork,
            _ => return Err(HostProblem::Malformed),
        };
        EnterpriseResource::new(
            class,
            self.name.clone(),
            match self.intent {
                Intent::Read => AccessIntent::Read,
                Intent::Execute => AccessIntent::Execute,
                Intent::Update => AccessIntent::Update,
            },
        )
    }
}
pub(super) fn validate(resources: &[StoredResource]) -> Result<(), HostProblem> {
    if resources.is_empty() || resources.len() > MAX_RESOURCES {
        return Err(HostProblem::Malformed);
    }
    for r in resources {
        r.resource()?;
    }
    Ok(())
}
pub(super) fn replay(
    resources: &[StoredResource],
    authorizer: &dyn EnterpriseAuthorizer,
    principal: &mainframe_env_execution_api::PrincipalId,
) -> Result<(), HostProblem> {
    validate(resources)?;
    for r in resources {
        authorizer.authorize(principal, &r.resource()?)?;
    }
    Ok(())
}

/// The delegate is the mandatory real authorizer; this records no inferred permit.
pub(super) struct Capture<'a> {
    delegate: &'a dyn EnterpriseAuthorizer,
    resources: Mutex<CapturedResources>,
}
#[derive(Default)]
struct CapturedResources {
    recorded: Vec<StoredResource>,
    reserved: usize,
}
/// Only a bounded collection slot, never a SAF or selected-state permit.
struct CaptureSlot<'a> {
    resources: &'a Mutex<CapturedResources>,
    active: bool,
}
impl CaptureSlot<'_> {
    fn record(mut self, value: StoredResource) -> Result<(), HostProblem> {
        let mut resources = self
            .resources
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        resources.reserved -= 1;
        self.active = false;
        resources.recorded.push(value);
        Ok(())
    }
}
impl Drop for CaptureSlot<'_> {
    fn drop(&mut self) {
        if self.active {
            let mut resources = self.resources.lock().unwrap_or_else(|p| p.into_inner());
            resources.reserved -= 1;
        }
    }
}
impl<'a> Capture<'a> {
    pub(super) fn new(delegate: &'a dyn EnterpriseAuthorizer) -> Self {
        Self {
            delegate,
            resources: Mutex::new(CapturedResources::default()),
        }
    }
    pub(super) fn into_resources(self) -> Result<Vec<StoredResource>, HostProblem> {
        let resources = self
            .resources
            .into_inner()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if resources.reserved != 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        validate(&resources.recorded)?;
        Ok(resources.recorded)
    }
    fn capture(
        &self,
        resource: &EnterpriseResource,
        authorize: impl FnOnce() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        let projected = StoredResource::capture(resource)?;
        let slot = {
            let mut resources = self
                .resources
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if resources.recorded.len() + resources.reserved == MAX_RESOURCES {
                return Err(HostProblem::ResourceExhausted);
            }
            resources.reserved += 1;
            CaptureSlot {
                resources: &self.resources,
                active: true,
            }
        };
        // No capture, selected, frame or backend mutex is acquired here. The
        // real caller still owns all admission/publication obligations.
        authorize()?;
        slot.record(projected)
    }
}
impl EnterpriseAuthorizer for Capture<'_> {
    fn authorize(
        &self,
        principal: &mainframe_env_execution_api::PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        self.capture(resource, || self.delegate.authorize(principal, resource))
    }
}

#[cfg(test)]
#[path = "authorization/capture_tests.rs"]
mod capture_tests;
