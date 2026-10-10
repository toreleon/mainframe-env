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

impl ProductServer {
    pub(super) fn dataset_call(
        &self,
        principal: &str,
        request: DatasetRequest,
    ) -> Result<DatasetResult, GatewayProblem> {
        if let DatasetRequest::ReadConcatenation { datasets, .. } = &request {
            for dataset in datasets {
                self.authorize_resource(
                    principal,
                    "DATASET",
                    dataset.as_str(),
                    AccessIntent::Read,
                )?;
            }
        }
        if let DatasetRequest::DefineAlias { target, .. } = &request {
            self.authorize_resource(principal, "DATASET", target.as_str(), AccessIntent::Read)?;
        }
        if let DatasetRequest::BuildAlternateIndex { base, .. } = &request {
            self.authorize_resource(principal, "DATASET", base.as_str(), AccessIntent::Read)?;
        }
        if let DatasetRequest::TvsStatus { owner, .. }
        | DatasetRequest::AcquireLock { owner, .. }
        | DatasetRequest::ReleaseLock { owner, .. }
        | DatasetRequest::BeginTvs { owner, .. }
        | DatasetRequest::StageTvs { owner, .. }
        | DatasetRequest::CompleteTvs { owner, .. }
        | DatasetRequest::ReconcileTvs { owner, .. } = &request
            && owner.as_str() != principal
        {
            return Err(gateway_problem(HostProblem::Unauthorized));
        }
        let dataset = match &request {
            DatasetRequest::Capabilities
            | DatasetRequest::List { .. }
            | DatasetRequest::TvsStatus { .. }
            | DatasetRequest::BeginTvs { .. }
            | DatasetRequest::CompleteTvs { .. }
            | DatasetRequest::ReconcileTvs { .. } => None,
            DatasetRequest::ListCatalog { pattern, .. } => Some(pattern.as_str()),
            DatasetRequest::ListVolumes { .. } => Some("VOLUME.**"),
            DatasetRequest::ReadConcatenation { .. } => None,
            DatasetRequest::Rename { from, .. } => Some(from.as_str()),
            DatasetRequest::ResolveCatalog { name } => Some(name.as_str()),
            DatasetRequest::DefineCatalog { catalog, .. }
            | DatasetRequest::SetCatalogConnection { catalog, .. } => Some(catalog.as_str()),
            DatasetRequest::DefineAlias { alias, .. } => Some(alias.as_str()),
            DatasetRequest::Attributes { dataset }
            | DatasetRequest::Describe { dataset }
            | DatasetRequest::Diagnose { dataset }
            | DatasetRequest::ListLocks { dataset, .. }
            | DatasetRequest::ListMembers { dataset, .. }
            | DatasetRequest::Read { dataset, .. }
            | DatasetRequest::ReadGeneric { dataset, .. }
            | DatasetRequest::ReadRelative { dataset, .. }
            | DatasetRequest::ReadRba { dataset, .. }
            | DatasetRequest::ReadSequential { dataset, .. }
            | DatasetRequest::Snapshot { dataset, .. }
            | DatasetRequest::ReadMemberGeneration { dataset, .. }
            | DatasetRequest::Create { dataset, .. }
            | DatasetRequest::Define { dataset, .. }
            | DatasetRequest::Alter { dataset, .. }
            | DatasetRequest::SetLifecycle { dataset, .. }
            | DatasetRequest::RecordBackup { dataset, .. }
            | DatasetRequest::Restore { dataset, .. }
            | DatasetRequest::DefineMemberAlias { dataset, .. }
            | DatasetRequest::WriteMemberGeneration { dataset, .. }
            | DatasetRequest::DeleteMemberGeneration { dataset, .. }
            | DatasetRequest::AcquireLock { dataset, .. }
            | DatasetRequest::ReleaseLock { dataset, .. }
            | DatasetRequest::Write { dataset, .. }
            | DatasetRequest::Append { dataset, .. }
            | DatasetRequest::Truncate { dataset, .. }
            | DatasetRequest::RewriteRecord { dataset, .. }
            | DatasetRequest::DeleteRecord { dataset, .. }
            | DatasetRequest::WriteRelative { dataset, .. }
            | DatasetRequest::DeleteRelative { dataset, .. }
            | DatasetRequest::WriteRba { dataset, .. }
            | DatasetRequest::Delete { dataset, .. }
            | DatasetRequest::StartBrowse { dataset, .. }
            | DatasetRequest::ResetBrowse { dataset, .. }
            | DatasetRequest::ReadBrowsePosition { dataset, .. }
            | DatasetRequest::ReadNext { dataset, .. }
            | DatasetRequest::EndBrowse { dataset, .. }
            | DatasetRequest::Close { dataset, .. } => Some(dataset.as_str()),
            DatasetRequest::DefinePath { path, .. } => Some(path.as_str()),
            DatasetRequest::BuildAlternateIndex { index, .. } => Some(index.as_str()),
            DatasetRequest::StageTvs { operation, .. } => Some(match operation {
                mainframe_env_host_api::TvsRecordOperation::Insert { dataset, .. }
                | mainframe_env_host_api::TvsRecordOperation::Rewrite { dataset, .. }
                | mainframe_env_host_api::TvsRecordOperation::Delete { dataset, .. } => {
                    dataset.as_str()
                }
            }),
            DatasetRequest::DefineAlternateIndex { base, .. }
            | DatasetRequest::DefineGenerationGroup { base, .. }
            | DatasetRequest::CreateGeneration { base, .. }
            | DatasetRequest::ResolveGeneration { base, .. } => Some(base.as_str()),
        };
        if let Some(dataset) = dataset {
            self.authorize_resource(
                principal,
                "DATASET",
                dataset,
                if matches!(
                    request,
                    DatasetRequest::Attributes { .. }
                        | DatasetRequest::Describe { .. }
                        | DatasetRequest::Diagnose { .. }
                        | DatasetRequest::ListLocks { .. }
                        | DatasetRequest::TvsStatus { .. }
                        | DatasetRequest::ListMembers { .. }
                        | DatasetRequest::Read { .. }
                        | DatasetRequest::ReadGeneric { .. }
                        | DatasetRequest::ReadRelative { .. }
                        | DatasetRequest::ReadRba { .. }
                        | DatasetRequest::ReadSequential { .. }
                        | DatasetRequest::Snapshot { .. }
                        | DatasetRequest::ReadMemberGeneration { .. }
                        | DatasetRequest::ResolveCatalog { .. }
                        | DatasetRequest::ResolveGeneration { .. }
                        | DatasetRequest::ListCatalog { .. }
                        | DatasetRequest::ListVolumes { .. }
                        | DatasetRequest::StartBrowse { .. }
                        | DatasetRequest::ResetBrowse { .. }
                        | DatasetRequest::ReadBrowsePosition { .. }
                        | DatasetRequest::ReadNext { .. }
                        | DatasetRequest::EndBrowse { .. }
                ) {
                    AccessIntent::Read
                } else {
                    AccessIntent::Update
                },
            )?;
        }
        let capability = if dataset_mutation(&request).is_some() {
            "host.dataset.write"
        } else {
            "host.dataset.read"
        };
        let invocation = self
            .invocation(
                principal,
                "zosmf:dataset",
                ServiceClass::System,
                &[capability],
            )
            .map_err(gateway_problem)?;
        let sequence = self.next_sequence().map_err(gateway_problem)?;
        let mutation = dataset_mutation(&request);
        let idempotency_key = mutation.map(|mutation| mutation.idempotency_key.clone());
        let result = self
            .host
            .invoke(
                &invocation,
                self.jes_tick().map_err(gateway_problem)?,
                invocation.cancellation_requested(),
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: mutation.map_or(sequence, |mutation| mutation.sequence),
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key,
                    request: HostRequest::Dataset(request),
                },
            )
            .persist_with(|audit| self.store.record_audit(audit).map_err(store_error));
        match result.outcome.map_err(gateway_problem)? {
            HostResult::Dataset(result) => Ok(result),
            _ => Err(gateway_problem(HostProblem::ProviderFailure)),
        }
    }
}
