//! Bounds and cursor validation for dataset browse requests.

use super::{DatasetRequest, HostLimits, HostProblem};

pub(super) fn validate(request: &DatasetRequest, limits: HostLimits) -> Result<(), HostProblem> {
    match request {
        DatasetRequest::StartBrowse { key, .. } | DatasetRequest::ResetBrowse { key, .. }
            if key.len() > limits.max_record_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadNext { cursor, .. }
        | DatasetRequest::ResetBrowse { cursor, .. }
        | DatasetRequest::EndBrowse { cursor, .. }
            if cursor.is_empty() || cursor.len() > limits.max_name_bytes =>
        {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::Close {
            cursor, control, ..
        } if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > limits.max_name_bytes)
            || (control.lock
                && (control.reel_or_unit.is_some() || control.no_rewind || control.removal))
            || (control.removal && control.reel_or_unit.is_none()) =>
        {
            Err(HostProblem::Malformed)
        }
        _ => Ok(()),
    }
}
