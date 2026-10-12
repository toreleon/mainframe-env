//! Complete keyed readback of the pilot record.
use super::*;

pub(super) fn read_record_hex(dataset: &DatasetService) -> Result<String, String> {
    match dataset
        .invoke(DatasetRequest::Read {
            dataset: dataset_name()?,
            member: None,
            key: Some(encode("AA")?),
            max_records: 1,
            control: Default::default(),
        })
        .and_then(DatasetResult::require_complete)
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Records { records, .. } if records.len() == 1 => Ok(hex(&records[0])),
        other => Err(format!("CICS pilot readback returned {other:?}")),
    }
}
