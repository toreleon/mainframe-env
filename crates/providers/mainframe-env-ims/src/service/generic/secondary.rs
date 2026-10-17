//! Bounded metadata admission for the physical-root secondary sequence.

use super::*;

impl ImsService {
    /// Check the existing secondary metadata support predicate without I/O or mutation.
    pub fn validate_secondary_metadata(
        &self,
        metadata: &ImsMetadataCatalog,
    ) -> Result<(), HostProblem> {
        validate_catalog(metadata)
    }
}

pub(super) fn validate_catalog(metadata: &ImsMetadataCatalog) -> Result<(), HostProblem> {
    for pcb in metadata.psbs.iter().flat_map(|psb| &psb.pcbs) {
        let ImsPcbMetadata::Database(pcb) = pcb else {
            continue;
        };
        let Some(name) = &pcb.secondary_index else {
            continue;
        };
        let database = metadata
            .databases
            .iter()
            .find(|database| normalize(&database.name) == normalize(&pcb.database))
            .ok_or(HostProblem::Malformed)?;
        let index = database
            .secondary_indexes
            .iter()
            .find(|index| normalize(&index.name) == normalize(name))
            .ok_or(HostProblem::Malformed)?;
        let target = database
            .segments
            .iter()
            .find(|segment| normalize(&segment.name) == normalize(&index.target_segment))
            .ok_or(HostProblem::Malformed)?;
        let source = database
            .segments
            .iter()
            .find(|segment| normalize(&segment.name) == normalize(&index.source_segment))
            .ok_or(HostProblem::Malformed)?;
        if index.source_fields.iter().any(|name| {
            source
                .fields
                .iter()
                .find(|field| {
                    field
                        .name
                        .as_deref()
                        .is_some_and(|field| normalize(field) == normalize(name))
                })
                .is_none_or(|field| {
                    field
                        .offset
                        .checked_add(field.length)
                        .is_none_or(|end| end > source.min_length)
                })
        }) {
            return Err(HostProblem::Unsupported);
        }
        if target.parent.is_some() || database.organization == crate::ImsDatabaseOrganization::Dedb
        {
            return Err(HostProblem::Unsupported);
        }
    }
    Ok(())
}

pub(super) fn restricted_mutation(
    engine: &DatabaseEngine,
    pcb: &ImsDatabasePcbMetadata,
    target: &str,
    operation: ImsOperation,
) -> bool {
    matches!(operation, ImsOperation::Insert | ImsOperation::Delete)
        && pcb.secondary_index.as_ref().is_some_and(|name| {
            engine
                .definition()
                .secondary_indexes
                .iter()
                .any(|index| index.name == normalize(name) && index.target_segment() == target)
        })
}
