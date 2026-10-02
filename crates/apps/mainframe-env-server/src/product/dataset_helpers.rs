use super::*;

pub(super) fn dataset_mutation(request: &DatasetRequest) -> Option<&Mutation> {
    match request {
        DatasetRequest::Create { mutation, .. }
        | DatasetRequest::Define { mutation, .. }
        | DatasetRequest::Alter { mutation, .. }
        | DatasetRequest::SetLifecycle { mutation, .. }
        | DatasetRequest::RecordBackup { mutation, .. }
        | DatasetRequest::Restore { mutation, .. }
        | DatasetRequest::DefineCatalog { mutation, .. }
        | DatasetRequest::SetCatalogConnection { mutation, .. }
        | DatasetRequest::DefineAlias { mutation, .. }
        | DatasetRequest::DefineMemberAlias { mutation, .. }
        | DatasetRequest::WriteMemberGeneration { mutation, .. }
        | DatasetRequest::DeleteMemberGeneration { mutation, .. }
        | DatasetRequest::AcquireLock { mutation, .. }
        | DatasetRequest::ReleaseLock { mutation, .. }
        | DatasetRequest::BeginTvs { mutation, .. }
        | DatasetRequest::StageTvs { mutation, .. }
        | DatasetRequest::CompleteTvs { mutation, .. }
        | DatasetRequest::ReconcileTvs { mutation, .. }
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::Append { mutation, .. }
        | DatasetRequest::Truncate { mutation, .. }
        | DatasetRequest::RewriteRecord { mutation, .. }
        | DatasetRequest::DeleteRecord { mutation, .. }
        | DatasetRequest::WriteRelative { mutation, .. }
        | DatasetRequest::DeleteRelative { mutation, .. }
        | DatasetRequest::WriteRba { mutation, .. }
        | DatasetRequest::DefineAlternateIndex { mutation, .. }
        | DatasetRequest::BuildAlternateIndex { mutation, .. }
        | DatasetRequest::DefinePath { mutation, .. }
        | DatasetRequest::DefineGenerationGroup { mutation, .. }
        | DatasetRequest::CreateGeneration { mutation, .. }
        | DatasetRequest::Rename { mutation, .. }
        | DatasetRequest::Delete { mutation, .. } => Some(mutation),
        _ => None,
    }
}

pub(super) fn member_name(value: Option<String>) -> Result<Option<MemberName>, GatewayProblem> {
    value
        .map(|value| {
            MemberName::new(value.to_ascii_uppercase(), 8)
                .map_err(|_| gateway_problem(HostProblem::Malformed))
        })
        .transpose()
}

pub(super) fn dataset_attributes(value: &Value) -> Result<DatasetAttributes, HostProblem> {
    let organization = match value
        .get("dsorg")
        .and_then(Value::as_str)
        .unwrap_or("PS")
        .to_ascii_uppercase()
        .as_str()
    {
        "PS" => DatasetOrganization::Sequential,
        "PO" => DatasetOrganization::Partitioned,
        "PO-E" => DatasetOrganization::PartitionedExtended,
        "VS" | "KSDS" => DatasetOrganization::KeySequenced,
        "ESDS" => DatasetOrganization::EntrySequenced,
        "RRDS" => DatasetOrganization::Relative,
        "VRRDS" => DatasetOrganization::VariableRelative,
        "LDS" => DatasetOrganization::Linear,
        _ => return Err(HostProblem::Unsupported),
    };
    let record_format = match value
        .get("recfm")
        .and_then(Value::as_str)
        .unwrap_or("FB")
        .to_ascii_uppercase()
        .as_str()
    {
        "F" => RecordFormat::Fixed,
        "FB" => RecordFormat::FixedBlocked,
        "FBS" => RecordFormat::FixedBlockedStandard,
        "V" => RecordFormat::Variable,
        "VB" => RecordFormat::VariableBlocked,
        "VS" => RecordFormat::VariableSpanned,
        "VBS" => RecordFormat::VariableBlockedSpanned,
        "U" => RecordFormat::Undefined,
        "LINE" => RecordFormat::Line,
        _ => return Err(HostProblem::Unsupported),
    };
    let logical_record_length = value.get("lrecl").and_then(Value::as_u64).unwrap_or(80);
    let attributes = DatasetAttributes {
        organization,
        record_format,
        logical_record_length: u32::try_from(logical_record_length)
            .map_err(|_| HostProblem::ResourceExhausted)?,
        key_offset: value
            .get("key_offset")
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).map_err(|_| HostProblem::ResourceExhausted))
            .transpose()?,
        key_length: value
            .get("key_length")
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).map_err(|_| HostProblem::ResourceExhausted))
            .transpose()?,
        ccsid: Some(37),
    };
    attributes.validate(HostLimits::default())?;
    Ok(attributes)
}

pub(super) fn records_for_write(
    bytes: &[u8],
    attributes: &DatasetAttributes,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let mut records = bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
        .map(|record| record.strip_suffix(b"\r").unwrap_or(record).to_vec())
        .collect::<Vec<_>>();
    if records.is_empty() {
        records.push(Vec::new());
    }
    if matches!(
        attributes.record_format,
        RecordFormat::Fixed | RecordFormat::FixedBlocked
    ) {
        for record in &mut records {
            let length = attributes.logical_record_length as usize;
            if record.len() > length {
                return Err(HostProblem::Condition {
                    name: "LENGERR".into(),
                    response: 22,
                    response2: 0,
                });
            }
            record.resize(length, b' ');
        }
    }
    Ok(records)
}

pub(super) fn join_records(records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, mut record) in records.into_iter().enumerate() {
        while record.last() == Some(&b' ') {
            record.pop();
        }
        if index > 0 {
            output.push(b'\n');
        }
        output.extend_from_slice(&record);
    }
    output
}
