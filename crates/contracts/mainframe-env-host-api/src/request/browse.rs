//! Bounds and cursor validation for dataset browse requests.

use super::{DatasetRequest, HostLimits, HostProblem};

pub(super) fn validate(request: &DatasetRequest, limits: HostLimits) -> Result<(), HostProblem> {
    match request {
        DatasetRequest::ReadBrowsePosition { expected_key, .. } if expected_key.is_empty() => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::ReadBrowsePosition { expected_key, .. }
            if expected_key.len() > limits.max_record_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::StartBrowse { key, .. } | DatasetRequest::ResetBrowse { key, .. }
            if key.len() > limits.max_record_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadBrowsePosition { cursor, .. }
        | DatasetRequest::ReadNext { cursor, .. }
        | DatasetRequest::ResetBrowse { cursor, .. }
        | DatasetRequest::EndBrowse { cursor, .. }
            if cursor.is_empty() || cursor.len() > limits.max_name_bytes =>
        {
            Err(HostProblem::Malformed)
        }
        _ => Ok(()),
    }
}
