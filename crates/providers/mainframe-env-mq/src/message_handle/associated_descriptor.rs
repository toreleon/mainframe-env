//! Source-reviewed associated MQMD1/property state in the SAME HMSG entry.
//! The registry borrow outlives physical publication. Drop aborts, not backout.

use super::*;
use mainframe_env_host_api::mq_md_value::MqMdValue;
use mainframe_env_host_api::mq_mqi::property::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_status::MqReviewedStatus;
use mainframe_env_host_api::{MqMessageAdoption, MqMessageCandidate};

#[derive(Clone, Debug)]
pub(super) struct AssociatedProperties {
    descriptor: MqMdValue,
    values: Vec<Property>,
}
#[derive(Clone, Debug)]
struct Property {
    name: MqPropertyName,
    descriptor: MqPropertyDescriptor,
    value: MqPropertyData,
}
impl AssociatedProperties {
    fn initial() -> Self {
        Self {
            descriptor: mq_property_initial_descriptor(),
            values: Vec::new(),
        }
    }
    pub(super) fn bytes(&self) -> Option<usize> {
        // Exact generated fixed widths, plus the inaccessible identity/version.
        let md = mq_property_md_fields().iter().try_fold(
            self.descriptor
                .fields()
                .struc_id
                .len()
                .checked_add(self.descriptor.version().to_be_bytes().len())?,
            |n, f| n.checked_add(f.width),
        )?;
        self.values.iter().try_fold(md, |n, v| {
            n.checked_add(v.name.as_str().len())?
                .checked_add(v.value.bytes.len())?
                .checked_add(v.descriptor.struc_id.len())?
                .checked_add(
                    [
                        v.descriptor.version,
                        v.descriptor.options,
                        v.descriptor.support,
                        v.descriptor.context,
                        v.descriptor.copy_options,
                    ]
                    .len()
                    .checked_mul(std::mem::size_of::<i32>())?,
                )
        })
    }
    fn property(&self, name: &MqPropertyName) -> Result<Property, MqHandleKernelProblem> {
        if let Some(field) = name.descriptor_field() {
            let definition = mq_property_md_fields()
                .iter()
                .find(|v| v.name == field)
                .ok_or(MqHandleKernelProblem::UnsupportedWire)?;
            let bytes = mq_property_descriptor_bytes(&self.descriptor, field)
                .ok_or(MqHandleKernelProblem::UnsupportedWire)?;
            Ok(Property {
                name: name.clone(),
                descriptor: MqPropertyDescriptor::descriptor_output(),
                value: MqPropertyData {
                    kind: definition.kind,
                    encoding: fact("MQENC_NATIVE_ZOS"),
                    // Property numeric representation is native z/OS, independent
                    // of the associated message BODY's observed Encoding field.
                    ccsid: profile_ccsid(),
                    bytes,
                },
            })
        } else {
            self.values
                .binary_search_by(|v| v.name.as_str().cmp(name.as_str()))
                .map(|i| self.values[i].clone())
                .map_err(|_| MqHandleKernelProblem::NotFound)
        }
    }
    fn set(
        &mut self,
        name: MqPropertyName,
        descriptor: MqPropertyDescriptor,
        value: MqPropertyData,
        limits: MqMessageLimits,
    ) -> Result<(), MqHandleKernelProblem> {
        if let Some(field) = name.descriptor_field() {
            mq_property_set_descriptor_bytes(&mut self.descriptor, field, &value.bytes)
                .map_err(|_| MqHandleKernelProblem::UnsupportedWire)?;
        } else {
            let item = Property {
                name,
                descriptor,
                value,
            };
            // No mixed-content property hierarchy, independent of sorting.
            if self
                .values
                .iter()
                .any(|v| hierarchical_conflict(v.name.as_str(), item.name.as_str()))
            {
                return Err(MqHandleKernelProblem::UnsupportedWire);
            }
            match self
                .values
                .binary_search_by(|v| v.name.as_str().cmp(item.name.as_str()))
            {
                Ok(i) => self.values[i] = item,
                Err(i) => {
                    if self.values.len() >= limits.properties {
                        return Err(MqHandleKernelProblem::Capacity);
                    }
                    self.values
                        .try_reserve(1)
                        .map_err(|_| MqHandleKernelProblem::Capacity)?;
                    self.values.insert(i, item);
                }
            }
            let total = self
                .values
                .iter()
                .try_fold(0usize, |n, v| {
                    n.checked_add(v.name.as_str().len())?
                        .checked_add(v.value.bytes.len())
                })
                .ok_or(MqHandleKernelProblem::Capacity)?;
            if total > limits.property_total_bytes {
                return Err(MqHandleKernelProblem::Capacity);
            }
        }
        Ok(())
    }
    fn delete(&mut self, name: &MqPropertyName) -> Result<bool, MqHandleKernelProblem> {
        if let Some(field) = name.descriptor_field() {
            let default = mq_property_descriptor_bytes(&mq_property_initial_descriptor(), field)
                .ok_or(MqHandleKernelProblem::UnsupportedWire)?;
            mq_property_set_descriptor_bytes(&mut self.descriptor, field, &default)
                .map_err(|_| MqHandleKernelProblem::UnsupportedWire)?;
            Ok(true)
        } else {
            match self
                .values
                .binary_search_by(|v| v.name.as_str().cmp(name.as_str()))
            {
                Ok(i) => {
                    self.values.remove(i);
                    Ok(true)
                }
                Err(_) => Ok(false),
            }
        }
    }
}
fn hierarchical_conflict(a: &str, b: &str) -> bool {
    a.strip_prefix(b).is_some_and(|s| s.starts_with('.'))
        || b.strip_prefix(a).is_some_and(|s| s.starts_with('.'))
}
fn fact(symbol: &str) -> i32 {
    mq_property_numeric_identities()
        .iter()
        .find(|v| v.symbol == symbol)
        .expect("generated reviewed fact")
        .value
}
fn profile_ccsid() -> i32 {
    // Extract the fixed reviewed encoding from the checked constructor instead
    // of introducing a second numeric authority in the provider.
    mq_property_profile_ccsid()
}
enum Change {
    Create {
        owner: MqHandleOwner,
        connection: MqHconn,
        value: AssociatedProperties,
    },
    Replace(usize, AssociatedProperties),
    Delete(usize),
    Observe,
}

pub(crate) struct PropertyStage<'a> {
    registry: MqMessageCandidate<'a>,
    entries: &'a mut Vec<Properties>,
    change: Change,
    pub(crate) output: MqMqiOutput,
    pub(crate) status: MqReviewedStatus,
}
impl PropertyStage<'_> {
    /// Caller must have known atomic audited publication. Infallible reserved
    /// adoption returns only the real registry-issued creation result.
    pub(crate) fn adopt(self) -> Option<MqHmsg> {
        let live = match self.registry.adopt() {
            MqMessageAdoption::Created(v) => Some(v),
            _ => None,
        };
        match self.change {
            Change::Create {
                owner,
                connection,
                value,
            } => {
                self.entries.push(Properties {
                    handle: live.expect("prepared create"),
                    owner,
                    connection,
                    contents: PropertyContents::Reviewed(value),
                });
            }
            Change::Replace(i, value) => {
                self.entries[i].contents = PropertyContents::Reviewed(value)
            }
            Change::Delete(i) => {
                self.entries.remove(i);
            }
            Change::Observe => {}
        }
        live
    }
}
impl MqHandleKernel {
    pub(crate) fn stage_property(
        &mut self,
        owner: MqHandleOwner,
        request: &MqPropertyRequest,
        limits: MqMqiLimits,
    ) -> Result<PropertyStage<'_>, MqHandleKernelProblem> {
        request
            .validate(limits)
            .map_err(|_| MqHandleKernelProblem::UnsupportedWire)?;
        let connection = request.connection();
        let mut completion = "MQCC_OK";
        let mut reason = "MQRC_NONE";
        let (change, output) = match request {
            MqPropertyRequest::Create { .. } => {
                self.properties
                    .try_reserve(1)
                    .map_err(|_| MqHandleKernelProblem::Capacity)?;
                let value = AssociatedProperties::initial();
                self.check_associated_total(None, &value)?;
                // Historical placeholder is assigned ONLY after registry staging.
                (
                    Some(Change::Create {
                        owner,
                        connection,
                        value,
                    }),
                    None,
                )
            }
            _ => {
                let handle = request
                    .handle()
                    .ok_or(MqHandleKernelProblem::UnsupportedWire)?;
                self.validate(owner, connection, handle.into())?;
                let i = self.index(handle.into())?;
                let PropertyContents::Reviewed(current) = &self.properties[i].contents else {
                    return Err(MqHandleKernelProblem::UnsupportedWire);
                };
                if self.properties[i].owner != owner || self.properties[i].connection != connection
                {
                    return Err(MqHandleKernelProblem::Handle(MqHandleProblem::CrossOwner));
                }
                match request {
                    MqPropertyRequest::Set {
                        name,
                        descriptor,
                        value,
                        ..
                    } => {
                        let mut next = current.clone();
                        next.set(name.clone(), descriptor.clone(), value.clone(), self.limits)?;
                        self.check_associated_total(Some(i), &next)?;
                        let pd = if name.descriptor_field().is_some() {
                            MqPropertyDescriptor::descriptor_output()
                        } else {
                            descriptor.clone()
                        };
                        (
                            Some(Change::Replace(i, next)),
                            Some(MqPropertyObservation::Set(pd)),
                        )
                    }
                    MqPropertyRequest::Delete { name, .. } => {
                        let mut next = current.clone();
                        let deleted = next.delete(name)?;
                        if !deleted {
                            completion = "MQCC_WARNING";
                            reason = "MQRC_PROPERTY_NOT_AVAILABLE";
                        }
                        (
                            Some(Change::Replace(i, next)),
                            Some(if deleted {
                                MqPropertyObservation::PropertyDeleted
                            } else {
                                MqPropertyObservation::Absent
                            }),
                        )
                    }
                    MqPropertyRequest::DeleteHandle { .. } => (
                        Some(Change::Delete(i)),
                        Some(MqPropertyObservation::HandleDeleted),
                    ),
                    MqPropertyRequest::Inquire {
                        name,
                        name_capacity,
                        value_capacity,
                        ..
                    } => match current.property(name) {
                        Err(MqHandleKernelProblem::NotFound) => {
                            completion = "MQCC_FAILED";
                            reason = "MQRC_PROPERTY_NOT_AVAILABLE";
                            (Some(Change::Observe), Some(MqPropertyObservation::Absent))
                        }
                        Err(error) => return Err(error),
                        Ok(value) => {
                            let name_short = name.as_str().len() > *name_capacity;
                            let value_short = value.value.bytes.len() > *value_capacity;
                            if name_short && value_short {
                                return Err(MqHandleKernelProblem::UnsupportedWire);
                            }
                            if name_short || value_short {
                                completion = "MQCC_FAILED";
                                reason = if name_short {
                                    "MQRC_PROPERTY_NAME_TOO_BIG"
                                } else {
                                    "MQRC_PROPERTY_VALUE_TOO_BIG"
                                };
                            }
                            let observed = MqPropertyInquiryObservation {
                                descriptor: value.descriptor,
                                kind: value.value.kind,
                                returned_encoding: value.value.encoding,
                                returned_ccsid: if value.value.kind == MqPropertyType::String {
                                    Some(value.value.ccsid)
                                } else {
                                    None
                                },
                                returned_name: name.as_str().as_bytes()
                                    [..name.as_str().len().min(*name_capacity)]
                                    .to_vec(),
                                name_length: i32::try_from(name.as_str().len())
                                    .map_err(|_| MqHandleKernelProblem::Capacity)?,
                                name_ccsid: profile_ccsid(),
                                data_length: i32::try_from(value.value.bytes.len())
                                    .map_err(|_| MqHandleKernelProblem::Capacity)?,
                                copied_value: value.value.bytes
                                    [..value.value.bytes.len().min(*value_capacity)]
                                    .to_vec(),
                            };
                            (
                                Some(Change::Observe),
                                Some(MqPropertyObservation::Inquired(observed)),
                            )
                        }
                    },
                    _ => return Err(MqHandleKernelProblem::UnsupportedWire),
                }
            }
        };
        let status = MqReviewedStatus::from_symbols(request.call(), completion, reason)
            .map_err(|_| MqHandleKernelProblem::UnsupportedWire)?;
        let registry = match request {
            MqPropertyRequest::Create { .. } => {
                self.registry.stage_message_create(owner, connection)?
            }
            MqPropertyRequest::DeleteHandle { handle, .. } => self
                .registry
                .stage_message_delete(owner, connection, *handle)?,
            _ => self.registry.stage_message_use(
                owner,
                connection,
                request.handle().expect("existing message"),
            )?,
        };
        let change = change.expect("finite property change");
        let output = match &change {
            Change::Create { .. } => {
                MqMqiOutput::MessageHandle(registry.provisional().expect("prepared create"))
            }
            _ => MqMqiOutput::PropertyObservation(output.expect("finite observed output")),
        };
        Ok(PropertyStage {
            registry,
            entries: &mut self.properties,
            change,
            output,
            status,
        })
    }
    fn check_associated_total(
        &self,
        replaced: Option<usize>,
        next: &AssociatedProperties,
    ) -> Result<(), MqHandleKernelProblem> {
        let total = self
            .properties
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != replaced)
            .try_fold(
                next.bytes().ok_or(MqHandleKernelProblem::Capacity)?,
                |n, (_, v)| n.checked_add(v.bytes()?),
            )
            .ok_or(MqHandleKernelProblem::Capacity)?;
        if total > MAX_KERNEL_PROPERTY_BYTES {
            Err(MqHandleKernelProblem::Capacity)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
