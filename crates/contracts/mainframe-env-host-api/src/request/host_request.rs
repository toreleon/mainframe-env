//! Existing host request capability, mutation and validation authority.
use super::*;

impl HostRequest {
    #[must_use]
    /// Return the built-in coarse capability identity for this request; this does not perform resource authorization.
    /// Panics if supplied identity limits cannot admit the built-in capability spelling.
    pub fn required_capability(&self, limits: InvocationLimits) -> CapabilityId {
        let name = match self {
            Self::Dataset(
                DatasetRequest::Capabilities
                | DatasetRequest::List { .. }
                | DatasetRequest::Attributes { .. }
                | DatasetRequest::Describe { .. }
                | DatasetRequest::Diagnose { .. }
                | DatasetRequest::ResolveCatalog { .. }
                | DatasetRequest::ListCatalog { .. }
                | DatasetRequest::ListVolumes { .. }
                | DatasetRequest::ListLocks { .. }
                | DatasetRequest::TvsStatus { .. }
                | DatasetRequest::ListMembers { .. }
                | DatasetRequest::ReadMemberGeneration { .. }
                | DatasetRequest::Read { .. }
                | DatasetRequest::ReadGeneric { .. }
                | DatasetRequest::ReadConcatenation { .. }
                | DatasetRequest::ReadRelative { .. }
                | DatasetRequest::ReadRba { .. }
                | DatasetRequest::ReadSequential { .. }
                | DatasetRequest::Snapshot { .. }
                | DatasetRequest::ResolveGeneration { .. }
                | DatasetRequest::ReadBrowsePosition { .. }
                | DatasetRequest::ReadNext { .. }
                | DatasetRequest::StartBrowse { .. }
                | DatasetRequest::ResetBrowse { .. }
                | DatasetRequest::EndBrowse { .. }
                | DatasetRequest::Close { .. },
            ) => "host.dataset.read",
            Self::Dataset(_) => "host.dataset.write",
            Self::Program(_) => "host.program.invoke",
            Self::Spool(SpoolRequest::List { .. } | SpoolRequest::Read { .. }) => "host.spool.read",
            Self::Spool(_) => "host.spool.write",
            Self::Terminal(_) => "host.terminal",
            Self::Security(SecurityRequest::Audit(_)) => "host.audit",
            Self::Security(_) => "host.security.authorize",
            Self::Clock(_) => "host.clock",
            Self::State(StateRequest::Get { .. }) => "host.state.read",
            Self::State(_) => "host.state.write",
            Self::Cics(_) => "host.cics.execute",
            Self::Db2(request) if request.operation.is_mutating() => "host.db2.write",
            Self::Db2(_) => "host.db2.read",
            Self::Ims(request) if request.operation.is_mutating() => "host.ims.write",
            Self::Ims(_) => "host.ims.read",
            Self::ImsRecovery(_) => "host.ims.write",
            Self::ImsNavigation(_) => "host.ims.write",
            Self::ImsGsam(_) => "host.ims.write",
            Self::ImsPcbFeedbackV1(_) => "host.ims.write",
            Self::Mq(_) | Self::MqMqi(_) => "host.mq.write",
        };
        CapabilityId::new(name, limits).expect("built-in capability identities are valid")
    }

    #[must_use]
    /// Classify state-changing effects for outer replay admission, including CICS token-producing reads.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Dataset(
                DatasetRequest::Create { .. }
                    | DatasetRequest::Define { .. }
                    | DatasetRequest::Alter { .. }
                    | DatasetRequest::SetLifecycle { .. }
                    | DatasetRequest::RecordBackup { .. }
                    | DatasetRequest::Restore { .. }
                    | DatasetRequest::DefineCatalog { .. }
                    | DatasetRequest::SetCatalogConnection { .. }
                    | DatasetRequest::DefineAlias { .. }
                    | DatasetRequest::DefineMemberAlias { .. }
                    | DatasetRequest::WriteMemberGeneration { .. }
                    | DatasetRequest::DeleteMemberGeneration { .. }
                    | DatasetRequest::AcquireLock { .. }
                    | DatasetRequest::ReleaseLock { .. }
                    | DatasetRequest::BeginTvs { .. }
                    | DatasetRequest::StageTvs { .. }
                    | DatasetRequest::CompleteTvs { .. }
                    | DatasetRequest::ReconcileTvs { .. }
                    | DatasetRequest::Write { .. }
                    | DatasetRequest::Append { .. }
                    | DatasetRequest::Truncate { .. }
                    | DatasetRequest::RewriteRecord { .. }
                    | DatasetRequest::DeleteRecord { .. }
                    | DatasetRequest::DefineAlternateIndex { .. }
                    | DatasetRequest::BuildAlternateIndex { .. }
                    | DatasetRequest::DefinePath { .. }
                    | DatasetRequest::WriteRelative { .. }
                    | DatasetRequest::DeleteRelative { .. }
                    | DatasetRequest::WriteRba { .. }
                    | DatasetRequest::DefineGenerationGroup { .. }
                    | DatasetRequest::CreateGeneration { .. }
                    | DatasetRequest::Rename { .. }
                    | DatasetRequest::Delete { .. }
            ) | Self::Spool(
                SpoolRequest::Append { .. }
                    | SpoolRequest::Seal { .. }
                    | SpoolRequest::Purge { .. }
            ) | Self::Program(
                ProgramRequest::Call { .. }
                    | ProgramRequest::Invoke { .. }
                    | ProgramRequest::Link { .. }
                    | ProgramRequest::Xctl { .. }
                    | ProgramRequest::Return { .. }
                    | ProgramRequest::Cancel { .. }
                    | ProgramRequest::Abend { .. }
            ) | Self::State(StateRequest::Put { .. } | StateRequest::Delete { .. })
        ) || matches!(self, Self::Cics(request) if request.is_mutating())
            || matches!(self, Self::Db2(request) if request.operation.is_mutating())
            || matches!(self, Self::Ims(request) if request.operation.is_mutating())
            || matches!(self, Self::ImsRecovery(_))
            || matches!(self, Self::ImsNavigation(_))
            || matches!(self, Self::ImsGsam(_))
            || matches!(self, Self::ImsPcbFeedbackV1(_))
            || matches!(self, Self::Mq(request) if request.operation.is_mutating())
            || matches!(self, Self::MqMqi(_))
    }

    #[must_use]
    /// Borrow the explicit nested mutation identity when this request form carries one; no identity is synthesized.
    pub fn mutation(&self) -> Option<&Mutation> {
        match self {
            Self::Dataset(
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
                | DatasetRequest::DefineAlternateIndex { mutation, .. }
                | DatasetRequest::BuildAlternateIndex { mutation, .. }
                | DatasetRequest::DefinePath { mutation, .. }
                | DatasetRequest::WriteRelative { mutation, .. }
                | DatasetRequest::DeleteRelative { mutation, .. }
                | DatasetRequest::WriteRba { mutation, .. }
                | DatasetRequest::DefineGenerationGroup { mutation, .. }
                | DatasetRequest::CreateGeneration { mutation, .. }
                | DatasetRequest::Rename { mutation, .. }
                | DatasetRequest::Delete { mutation, .. },
            )
            | Self::Spool(
                SpoolRequest::Append { mutation, .. }
                | SpoolRequest::Seal { mutation, .. }
                | SpoolRequest::Purge { mutation, .. },
            )
            | Self::State(
                StateRequest::Put { mutation, .. } | StateRequest::Delete { mutation, .. },
            ) => Some(mutation),
            Self::Cics(request) => request.mutation.as_ref(),
            Self::Db2(request) => request.mutation.as_ref(),
            Self::Ims(request) => request.mutation.as_ref(),
            Self::ImsRecovery(request) => Some(&request.mutation),
            Self::ImsNavigation(request) => request.request.mutation.as_ref(),
            Self::ImsGsam(request) => request.request.mutation.as_ref(),
            Self::ImsPcbFeedbackV1(request) => request.request.mutation.as_ref(),
            Self::Mq(request) => request.mutation.as_ref(),
            Self::MqMqi(request) => Some(&request.mutation),
            _ => None,
        }
    }

    /// Check represented request shape and bounds before dispatch; provider capabilities, permissions and live state still require admission.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::MqMqi(request) => request.validate(limits),
            Self::ImsRecovery(request) => request.validate(limits),
            Self::Dataset(request) => validate_dataset(request, limits),
            Self::Program(ProgramRequest::Call {
                service: Some(service),
                ..
            }) if service.abi_version == 0 => Err(HostProblem::Malformed),
            Self::Program(ProgramRequest::Link {
                selection: Some(selection),
                ..
            }) if !selection.is_valid() => Err(HostProblem::Malformed),
            Self::Program(ProgramRequest::Cancel { programs })
                if programs.is_empty() || programs.len() > limits.max_fields =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolRequest::Append {
                file,
                records,
                mutation,
                ..
            }) => {
                validate_spool_file(file, limits)?;
                validate_records(records, limits)?;
                mutation.validate(limits)
            }
            Self::Spool(SpoolRequest::Read {
                file, max_records, ..
            }) => {
                validate_spool_file(file, limits)?;
                if *max_records == 0 || *max_records as usize > limits.max_records {
                    Err(HostProblem::ResourceExhausted)
                } else {
                    Ok(())
                }
            }
            Self::Spool(SpoolRequest::Seal { file, mutation, .. }) => {
                validate_spool_file(file, limits)?;
                mutation.validate(limits)
            }
            Self::Spool(SpoolRequest::Purge { mutation, .. }) => mutation.validate(limits),
            Self::Terminal(
                TerminalRequest::Write { fields, .. } | TerminalRequest::Input { fields, .. },
            ) => validate_fields(fields, limits),
            Self::Security(SecurityRequest::Audit(event)) => {
                if event.fields.len() > limits.max_audit_fields {
                    Err(HostProblem::ResourceExhausted)
                } else {
                    Ok(())
                }
            }
            Self::State(StateRequest::Put {
                value, mutation, ..
            }) => {
                if value.len() > limits.max_state_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                mutation.validate(limits)
            }
            Self::State(StateRequest::Delete { mutation, .. }) => mutation.validate(limits),
            Self::Cics(request) => request.validate(limits),
            Self::Db2(request) => {
                if request.statement.len() > limits.max_state_bytes
                    || request.cursor.as_ref().is_some_and(|cursor| {
                        cursor.is_empty() || cursor.len() > limits.max_name_bytes
                    })
                    || request.inputs.len() > limits.max_fields
                    || request.outputs.len() > limits.max_fields
                    || request.max_rows as usize > limits.max_records
                    || request.inputs.iter().any(|(name, variable)| {
                        name.is_empty()
                            || name.len() > limits.max_name_bytes
                            || variable.value.len() > limits.max_record_bytes
                    })
                    || request
                        .outputs
                        .iter()
                        .any(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if request.operation.is_mutating() {
                    request
                        .mutation
                        .as_ref()
                        .ok_or(HostProblem::MissingIdempotency)?
                        .validate(limits)?;
                }
                Ok(())
            }
            Self::ImsNavigation(request) => request.validate(limits),
            Self::ImsGsam(request) => request.validate(limits),
            Self::ImsPcbFeedbackV1(request) => request.validate(limits),
            Self::Ims(request) => {
                if (request.operation == ImsOperation::System) != request.system.is_some()
                    || request.q_class.is_some_and(|class| !class.is_valid())
                    || request.q_class.is_some()
                        && !matches!(
                            request.operation,
                            ImsOperation::GetUnique
                                | ImsOperation::GetNext
                                | ImsOperation::GetNextParent
                                | ImsOperation::GetHoldUnique
                                | ImsOperation::GetHoldNext
                                | ImsOperation::GetHoldNextParent
                        )
                {
                    return Err(HostProblem::Malformed);
                }
                if request.pcb == 0 && request.operation != ImsOperation::System
                    || request.segments.len() > limits.max_fields
                    || request.data.len() > limits.max_record_bytes
                    || request.qualifiers.len() > limits.max_fields
                    || request.max_segments == 0
                    || request.max_segments as usize > limits.max_records
                    || request
                        .psb
                        .as_ref()
                        .is_some_and(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                    || request
                        .segments
                        .iter()
                        .any(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                    || request.qualifiers.iter().any(|qualifier| {
                        qualifier.segment.is_empty()
                            || qualifier.segment.len() > limits.max_name_bytes
                            || qualifier.field.is_empty()
                            || qualifier.field.len() > limits.max_name_bytes
                            || qualifier.value.len() > limits.max_record_bytes
                    })
                    || request
                        .checkpoint_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > limits.max_name_bytes)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if let Some(system) = &request.system {
                    system.validate(limits)?;
                }
                if request.operation.is_mutating() {
                    request
                        .mutation
                        .as_ref()
                        .ok_or(HostProblem::MissingIdempotency)?
                        .validate(limits)?;
                }
                Ok(())
            }
            Self::Mq(request) => request.validate(limits),
            _ => Ok(()),
        }
    }
}
