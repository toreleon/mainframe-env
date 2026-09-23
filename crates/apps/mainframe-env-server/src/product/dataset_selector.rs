//! Resolve dataset authorization scope before gateway dispatch.

use mainframe_env_host_api::DatasetRequest;

pub(super) fn scope(request: &DatasetRequest) -> Option<&str> {
    match request {
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
    }
}
