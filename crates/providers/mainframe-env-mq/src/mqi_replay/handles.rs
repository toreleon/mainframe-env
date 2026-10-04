//! Exact dynamic-open storage fields, using the frozen names/kinds/validators.
//! The sole handle projection is host-owned MqHandleObservation, never live Serde.
use super::*;

#[derive(Serialize)]
enum DynamicKind {
    Temporary,
    Permanent,
}
impl<'de> Deserialize<'de> for DynamicKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        match value.as_str() {
            "Temporary" => Ok(Self::Temporary),
            "Permanent" => Ok(Self::Permanent),
            _ => Err(serde::de::Error::custom("unknown dynamic kind")),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dynamic {
    handle: MqHandleObservation,
    model_name: String,
    queue_name: String,
    kind: DynamicKind,
}
impl Dynamic {
    pub(super) fn capture(value: &MqDynamicQueueOpenResult) -> Self {
        Self {
            handle: MqHandleObservation::from(MqHandle::Object(value.handle)),
            model_name: value.model.as_str().into(),
            queue_name: value.name.as_str().into(),
            kind: match value.kind {
                MqRouteDynamicKind::Temporary => DynamicKind::Temporary,
                MqRouteDynamicKind::Permanent => DynamicKind::Permanent,
            },
        }
    }
    pub(super) fn restore(self) -> Result<MqDynamicQueueOpenResult, ReplayError> {
        Ok(MqDynamicQueueOpenResult {
            handle: self.handle.historical_object()?,
            model: MqRouteName::new(self.model_name).map_err(|_| ReplayError::Malformed)?,
            name: MqRouteName::new(self.queue_name).map_err(|_| ReplayError::Malformed)?,
            kind: match self.kind {
                DynamicKind::Temporary => MqRouteDynamicKind::Temporary,
                DynamicKind::Permanent => MqRouteDynamicKind::Permanent,
            },
        })
    }
}
