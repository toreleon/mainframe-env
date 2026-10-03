//! Existing checked corpus-counter bound.

use super::*;

pub(super) fn checked_total(
    current: usize,
    increment: usize,
    name: &str,
) -> Result<usize, CorpusProblem> {
    current.checked_add(increment).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.control.resource_exhausted",
            format!("{name} counter overflow"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_accepts_the_maximum_and_refuses_overflow() {
        assert_eq!(
            checked_total(usize::MAX - 1, 1, "records").unwrap(),
            usize::MAX
        );
        assert_eq!(checked_total(0, 0, "records").unwrap(), 0);
        assert!(checked_total(usize::MAX, 1, "records").is_err());
    }
}
