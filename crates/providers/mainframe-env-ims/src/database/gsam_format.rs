//! Format/length projection shared by the public adapter, image and recovery owners.
use super::*;
use mainframe_env_host_api::{HostProblem, ImsGsamRecordFormat, ImsGsamRequest, ImsOperation};

pub(crate) fn validate_definition_route(
    definition: &DatabaseDefinition,
) -> Result<(), HostProblem> {
    let record = definition.segments.first().ok_or(HostProblem::Malformed)?;
    match &definition.gsam_format {
        Some(format) => format.validate(record.min_length, record.max_length),
        None if record.min_length == record.max_length => Ok(()),
        None => Err(HostProblem::Unsupported),
    }
}

pub(crate) fn validate_route(
    definition: &DatabaseDefinition,
    call: &ImsGsamRequest,
) -> Result<(), HostProblem> {
    validate_definition_route(definition)?;
    let undefined = definition
        .gsam_format
        .as_ref()
        .is_some_and(|f| f.record_format == ImsGsamRecordFormat::U);
    if call.request.operation == ImsOperation::Insert {
        if undefined != call.undefined_length.is_some() {
            return Err(HostProblem::Malformed);
        }
        if let Some(format) = &definition.gsam_format {
            let record = &definition.segments[0];
            format.validate_area(&call.request.data, record.min_length, record.max_length)?;
        }
    } else if call.undefined_length.is_some() {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(crate) fn output_length(definition: &DatabaseDefinition, view: &RecordView) -> Option<u32> {
    definition
        .gsam_format
        .as_ref()
        .filter(|f| f.record_format == ImsGsamRecordFormat::U)
        .map(|_| view.data.len() as u32)
}

/// Bind declared characteristics and complete application-area bounds at CHKP/XRST.
pub(crate) fn identity(definition: &DatabaseDefinition) -> Result<Option<[u8; 32]>, HostProblem> {
    validate_definition_route(definition)?;
    definition
        .gsam_format
        .as_ref()
        .map(|format| {
            let record = &definition.segments[0];
            let bytes = serde_json::to_vec(&(format, record.min_length, record.max_length))
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let mut hash = Sha256::new();
            hash.update(b"mainframe-env.ims-gsam-format@1\0");
            hash.update(bytes);
            Ok(hash.finalize().into())
        })
        .transpose()
}
