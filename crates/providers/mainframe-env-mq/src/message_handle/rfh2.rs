//! Touched-entry conversion candidates on the sole live message registry.
//! No provisional token escapes; dropping the registry borrow aborts unchanged.
use super::associated_descriptor::{Change, PropertyStage};
use super::*;
use mainframe_env_host_api::mq_mqi::rfh2::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_status::MqReviewedStatus;

fn problem(p: MqPropertyProblem) -> MqHandleKernelProblem {
    match p {
        MqPropertyProblem::Capacity => MqHandleKernelProblem::Capacity,
        _ => MqHandleKernelProblem::UnsupportedWire,
    }
}
impl MqHandleKernel {
    pub(crate) fn associated_message_count(&self) -> usize {
        self.properties.len()
    }

    pub(crate) fn stage_rfh2(
        &mut self,
        owner: MqHandleOwner,
        request: &MqMqiRfh2Request,
        limits: MqMqiLimits,
    ) -> Result<PropertyStage<'_>, MqHandleKernelProblem> {
        request.validate(limits).map_err(problem)?;
        let connection = request.connection();
        let handle = request.handle();
        self.validate(owner, connection, handle.into())?;
        let i = self.index(handle.into())?;
        let entry = &self.properties[i];
        if entry.owner != owner || entry.connection != connection {
            return Err(MqHandleProblem::CrossOwner.into());
        }
        let PropertyContents::Reviewed(current) = &entry.contents else {
            return Err(MqHandleKernelProblem::UnsupportedWire);
        };
        let mut reason = "MQRC_NONE";
        let mut change = Change::Observe;
        let mut observation = MqRfh2Observation {
            descriptor: None,
            data_length: None,
            buffer: MqRfh2BufferObservation::Unchanged,
        };
        match request {
            MqMqiRfh2Request::BufferToHandle {
                descriptor, buffer, ..
            } => {
                if !current.values.is_empty() {
                    return Err(MqHandleKernelProblem::UnsupportedWire);
                }
                match mq_rfh2_decode(descriptor, buffer, limits) {
                    Err(MqPropertyProblem::Value) => {
                        reason = "MQRC_RFH_ERROR";
                    }
                    Err(p) => return Err(problem(p)),
                    Ok(import) => {
                        let mut next = current.clone();
                        next.descriptor = descriptor.clone();
                        for (name, pd, value) in import.properties {
                            next.set(name, pd, value, limits.message)?;
                        }
                        self.check_associated_total(Some(i), &next)?;
                        change = Change::Replace(i, next);
                        observation.data_length = Some(
                            i32::try_from(buffer.len())
                                .map_err(|_| MqHandleKernelProblem::Capacity)?,
                        );
                        // Opaque tail remains in the unchanged original application
                        // buffer; it is not attached as an unchecked payload sidecar.
                    }
                }
            }
            MqMqiRfh2Request::HandleToBuffer {
                name,
                descriptor,
                options,
                buffer_capacity,
                ..
            } => match current.property(name) {
                Err(MqHandleKernelProblem::NotFound) => {
                    reason = "MQRC_PROPERTY_NOT_AVAILABLE";
                    observation.data_length = Some(0);
                }
                Err(error) => return Err(error),
                Ok(value) => {
                    let required =
                        mq_rfh2_required_length(name, &value.descriptor, &value.value, limits)
                            .map_err(problem)?;
                    observation.data_length =
                        Some(i32::try_from(required).map_err(|_| MqHandleKernelProblem::Capacity)?);
                    if required > *buffer_capacity {
                        reason = "MQRC_PROPERTY_VALUE_TOO_BIG";
                    } else {
                        let bytes = mq_rfh2_encode(name, &value.descriptor, &value.value, limits)
                            .map_err(problem)?;
                        observation.descriptor =
                            Some(mq_rfh2_outer_descriptor(descriptor).map_err(problem)?);
                        observation.buffer = MqRfh2BufferObservation::WrittenPrefix(bytes);
                        if options.deletes() {
                            let mut next = current.clone();
                            if !next.delete(name)? {
                                return Err(MqHandleKernelProblem::NotFound);
                            }
                            change = Change::Replace(i, next);
                        }
                    }
                }
            },
        }
        let completion = if reason == "MQRC_NONE" {
            "MQCC_OK"
        } else {
            "MQCC_FAILED"
        };
        let status = MqReviewedStatus::from_symbols(request.call(), completion, reason)
            .map_err(|_| MqHandleKernelProblem::UnsupportedWire)?;
        observation
            .validate(request.call(), limits)
            .map_err(problem)?;
        if !observation.validate_status(status) {
            return Err(MqHandleKernelProblem::UnsupportedWire);
        }
        let registry = self.registry.stage_message_use(owner, connection, handle)?;
        Ok(PropertyStage {
            registry,
            entries: &mut self.properties,
            change,
            output: MqMqiOutput::Rfh2Observation(observation),
            status,
        })
    }
}
