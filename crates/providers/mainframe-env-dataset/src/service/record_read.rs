//! Bounded record selection, including alternate-index identities.
use super::*;

pub(super) fn read(state: &State, request: &DatasetRequest) -> Result<DatasetResult, HostProblem> {
    let DatasetRequest::Read {
        dataset,
        member,
        key,
        max_records,
        ..
    } = request
    else {
        return Err(HostProblem::Malformed);
    };
    if member.is_some() && state.alternate_indexes.contains_key(dataset.as_str()) {
        return Err(HostProblem::Unsupported);
    }
    let (records, identities, version) =
        if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
            let base = entry_text(state, &index.base)?;
            let ordered = alternate_identities(base, index)?;
            let selected = ordered
                .into_iter()
                .filter(|(alternate, _)| key.as_ref().is_none_or(|key| alternate == key))
                .filter(|(_, identity)| keyed_record(base, identity).is_some())
                .take(*max_records as usize + 1)
                .collect::<Vec<_>>();
            if key.is_some() && selected.is_empty() {
                return Err(condition("NOTFND", 13));
            }
            let identities = selected
                .iter()
                .map(|(_, identity)| identity.clone())
                .collect::<Vec<_>>();
            let records = identities
                .iter()
                .map(|identity| keyed_record(base, identity).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or(HostProblem::InfrastructureFailure)?;
            (records, identities, index.version)
        } else {
            let entry = entry(state, dataset)?;
            if relative(entry.attributes.organization) {
                if member.is_some() || key.is_some() {
                    return Err(HostProblem::Unsupported);
                }
                let selected = entry
                    .relative_records
                    .iter()
                    .take(*max_records as usize + 1)
                    .map(|(record_number, record)| {
                        (record.clone(), record_number.to_be_bytes().to_vec())
                    })
                    .collect::<Vec<_>>();
                return Ok(bounded_records(
                    selected.iter().map(|(record, _)| record.clone()).collect(),
                    selected.into_iter().map(|(_, identity)| identity).collect(),
                    entry.version,
                    *max_records as usize,
                ));
            }
            let records = if let Some(member) = member {
                member_records(entry, member, 0)?
                    .iter()
                    .take(*max_records as usize + 1)
                    .cloned()
                    .collect()
            } else if let Some(key) = key {
                let record = keyed_record(entry, key)
                    .cloned()
                    .ok_or_else(|| condition("NOTFND", 13))?;
                vec![record]
            } else {
                ordered_records(entry)?
                    .into_iter()
                    .take(*max_records as usize + 1)
                    .cloned()
                    .collect()
            };
            let identities = records
                .iter()
                .enumerate()
                .map(|(position, record)| record_identity(entry, record, position))
                .collect::<Result<Vec<_>, _>>()?;
            (records, identities, entry.version)
        };
    Ok(bounded_records(
        records,
        identities,
        version,
        *max_records as usize,
    ))
}

pub(super) fn bounded_records(
    mut records: Vec<Vec<u8>>,
    mut identities: Vec<Vec<u8>>,
    version: u64,
    max: usize,
) -> DatasetResult {
    let more = records.len() > max;
    records.truncate(max);
    identities.truncate(max);
    DatasetResult::Records {
        records,
        identities,
        version,
        more,
    }
}
