//! SEND MAP/TEXT clause admission and cursor constraints.
use super::super::{HirCicsOperation, Resolution, ResolutionFailure};
use super::Clauses;

pub(super) const SEND_MAP_CLAUSES: &[&str] =
    &["MAP", "MAPSET", "FROM", "LENGTH", "CURSOR", "RESP", "RESP2"];
pub(super) const SEND_TEXT_CLAUSES: &[&str] = &["FROM", "LENGTH", "CURSOR", "RESP", "RESP2"];
pub(super) const SEND_MAP_OPTIONS: &[&str] = &[
    "DATAONLY", "ERASE", "CURSOR", "FREEKB", "MAPONLY", "NOHANDLE",
];
pub(super) const SEND_TEXT_OPTIONS: &[&str] = &["ERASE", "FREEKB", "CURSOR", "NOHANDLE"];

pub(super) fn validate_cursor(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation == HirCicsOperation::SendText
        && options.iter().any(|option| option == "CURSOR")
        && !clauses.contains_key("CURSOR")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SEND TEXT CURSOR requires a halfword value".into(),
        ));
    }
    Ok(())
}
