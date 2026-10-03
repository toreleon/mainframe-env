//! Effective stored policy, not input-only MD writeback or call status calculation.
use super::*;
use mainframe_env_host_api::mq_raw_layout::{MqRawInitialValue, MqRawLayoutKind, mq_raw_layout};

pub(super) fn effective(
    catalog: &MqObjectCatalog,
    queue: &crate::MqObjectName,
    put: &MqMqiFullPut,
) -> Result<(i32, i32), HostProblem> {
    let attrs = catalog
        .native_attributes()
        .ok_or(HostProblem::Unsupported)?;
    let queue = attrs
        .queues
        .iter()
        .find(|q| q.name == *queue)
        .ok_or(HostProblem::Unsupported)?;
    let initial = |name: &str, value: i32| {
        mq_raw_layout(MqRawLayoutKind::Md1)
            .fields
            .iter()
            .any(|f| f.name == name && f.initial == MqRawInitialValue::Long(value))
    };
    let fields = put.message.descriptor.fields();
    let priority = if initial("Priority", fields.priority) {
        queue
            .producer_defaults
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .priority
    } else {
        fields.priority
    };
    // q097395_1255–1260: an above-max request needs warning/capped-placement
    // observations. The finite synchronous success DTO cannot model that pair.
    // Refuse before mutation, without clipping or inventing an MQ failure.
    if priority < 0 || priority > attrs.max_priority {
        return Err(HostProblem::Unsupported);
    }
    let persistence = if initial("Persistence", fields.persistence) {
        match queue
            .producer_defaults
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .persistence
        {
            crate::MqNativePersistence::NonPersistent => 0,
            crate::MqNativePersistence::Persistent => 1,
        }
    } else {
        fields.persistence
    };
    // Same already-reviewed MQPER0/1 used by the full payload kernel. No
    // abstract/default sentinel reaches durable payloads or cold retention.
    if !matches!(persistence, 0 | 1) {
        return Err(HostProblem::Unsupported);
    }
    Ok((priority, persistence))
}
