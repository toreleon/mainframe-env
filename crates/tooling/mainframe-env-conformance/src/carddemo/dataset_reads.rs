//! Whole-input dataset observations for utility verification.
use super::*;

pub(super) fn utility_records(
    server: &ProductServer,
    name: &str,
    member: Option<&str>,
) -> Result<Vec<Vec<u8>>, CorpusProblem> {
    match server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new(name, 128).map_err(|_| {
                CorpusProblem::new("carddemo.utility.dataset", "dataset name is invalid")
            })?,
            member: member
                .map(|member| {
                    MemberName::new(member, 8).map_err(|_| {
                        CorpusProblem::new("carddemo.utility.dataset", "member name is invalid")
                    })
                })
                .transpose()?,
            key: None,
            max_records: 4_096,
            control: Default::default(),
        })
        .and_then(DatasetResult::require_complete)
        .map_err(terminal_problem)?
    {
        DatasetResult::Records { records, .. } => Ok(records),
        _ => Err(CorpusProblem::new(
            "carddemo.utility.dataset",
            "dataset read returned the wrong result",
        )),
    }
}
