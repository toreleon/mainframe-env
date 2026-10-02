//! Actual resolved SAF resources retained for output replay, not policy permits.
use super::*;
const MAX_RESOURCES: usize = 512;
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
    resources: Mutex<Vec<StoredResource>>,
}
impl<'a> Capture<'a> {
    pub(super) fn new(delegate: &'a dyn EnterpriseAuthorizer) -> Self {
        Self {
            delegate,
            resources: Mutex::new(Vec::new()),
        }
    }
    pub(super) fn into_resources(self) -> Result<Vec<StoredResource>, HostProblem> {
        let resources = self
            .resources
            .into_inner()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        validate(&resources)?;
        Ok(resources)
    }
}
impl EnterpriseAuthorizer for Capture<'_> {
    fn authorize(
        &self,
        principal: &mainframe_env_execution_api::PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        let projected = StoredResource::capture(resource)?;
        let mut resources = self
            .resources
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if resources.len() == MAX_RESOURCES {
            return Err(HostProblem::ResourceExhausted);
        }
        self.delegate.authorize(principal, resource)?;
        resources.push(projected);
        Ok(())
    }
}
