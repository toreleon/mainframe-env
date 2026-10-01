use super::SemanticProblem;

pub(super) fn contains_generic(value: &str) -> bool {
    value.bytes().any(|byte| matches!(byte, b'*' | b'%'))
}

pub(super) fn checked_version(version: u64) -> Result<u64, SemanticProblem> {
    version.checked_add(1).ok_or(SemanticProblem::Exhausted)
}
