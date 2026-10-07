//! Pure three-valued Db2 search-condition outcomes.
//!
//! Source baseline: `ibm-db2-for-zos-13-2026-08-13`, product `SSEPEK_13.0.0`.
//! The pinned language elements have no standalone statement-catalog row:
//! - `SSEPEK_13.0.0/sqlref/src/tpc/db2z_searchconditionssql.html`, 27571 bytes,
//!   SHA-256 `79fafaf79779ef1f7b947502d99efd2ab7d87bf1046ff65602076b8fbf4e5627`;
//! - `SSEPEK_13.0.0/sqlref/src/tpc/db2z_nullpredicate.html`, 8909 bytes,
//!   SHA-256 `c1ca9abcb98036bf0666b6710fd37c26c1499fbd8e787b670fa67a8c5c02ef9d`.
//!
//! These outcomes are not SQL BOOLEAN scalar values or nullable runtime cells.
//! Callers supply already established predicate outcomes or explicit value
//! nullness; this module does not evaluate expressions or compare values.
//! `qualifies` projects an outcome to a decision without changing the outcome.
//! No executor consumes this kernel today; public integration remains pending.
//!
//! The source specifies parentheses, then NOT, AND, OR precedence; evaluation
//! order among operators at the same precedence is undefined. This kernel
//! neither changes parser precedence nor promises evaluation or short-circuit
//! order: both operands are supplied outcomes, not deferred computations.
//!
//! Source, string, count and recursion limits are inapplicable only to this
//! fixed three-state kernel: it accepts no source, strings, collections or
//! recursion, allocates nothing and performs no I/O or state mutation. Parser,
//! evaluator, backend, security and recovery gates remain required at their
//! later integration boundaries. Local tests grant no official row credit.

/// An owned predicate outcome, preserving UNKNOWN through logical operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2TruthValue {
    /// The condition holds.
    True,
    /// The condition does not hold.
    False,
    /// The condition's truth is unknown.
    Unknown,
}

impl Db2TruthValue {
    /// Combine already established outcomes using the Db2 AND truth table.
    #[must_use]
    pub const fn logical_and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    /// Combine already established outcomes using the Db2 OR truth table.
    #[must_use]
    pub const fn logical_or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    /// Negate an outcome, retaining UNKNOWN as UNKNOWN.
    #[must_use]
    pub const fn logical_not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    /// Return the TRUE-only qualification decision for this unchanged outcome.
    ///
    /// FALSE and UNKNOWN both return false. This is an explicit projection,
    /// not an implicit Boolean conversion or an implementation of row filtering.
    #[must_use]
    pub const fn qualifies(self) -> bool {
        matches!(self, Self::True)
    }

    /// Produce IS NULL (or IS NOT NULL when `negated`) from explicit nullness.
    ///
    /// `is_null` describes the value itself, never schema nullability, absent
    /// catalog metadata or empty bytes. The result is always TRUE or FALSE.
    #[must_use]
    pub const fn null_predicate(is_null: bool, negated: bool) -> Self {
        match (is_null, negated) {
            (true, false) | (false, true) => Self::True,
            (false, false) | (true, true) => Self::False,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Db2TruthValue::{self, False, True, Unknown};

    // Independently fixed from the pinned search-condition table, not computed
    // by any product method. Every ordered pair occurs in each table.
    #[test]
    fn fixed_nine_pair_and_table() {
        let cases = [
            (True, True, True),
            (True, False, False),
            (True, Unknown, Unknown),
            (False, True, False),
            (False, False, False),
            (False, Unknown, False),
            (Unknown, True, Unknown),
            (Unknown, False, False),
            (Unknown, Unknown, Unknown),
        ];
        for (left, right, expected) in cases {
            assert_eq!(left.logical_and(right), expected, "{left:?} AND {right:?}");
        }
    }

    #[test]
    fn fixed_nine_pair_or_table() {
        let cases = [
            (True, True, True),
            (True, False, True),
            (True, Unknown, True),
            (False, True, True),
            (False, False, False),
            (False, Unknown, Unknown),
            (Unknown, True, True),
            (Unknown, False, Unknown),
            (Unknown, Unknown, Unknown),
        ];
        for (left, right, expected) in cases {
            assert_eq!(left.logical_or(right), expected, "{left:?} OR {right:?}");
        }
    }

    #[test]
    fn fixed_three_not_outcomes() {
        for (value, expected) in [(True, False), (False, True), (Unknown, Unknown)] {
            assert_eq!(value.logical_not(), expected, "NOT {value:?}");
        }
    }

    #[test]
    fn explicit_nullness_and_not_null_predicate_outcomes() {
        // Nullness is an explicit fact about the value; neither bytes nor
        // missing catalog metadata are accepted by this entry point.
        for (is_null, negated, expected) in [
            (true, false, True),
            (false, false, False),
            (true, true, False),
            (false, true, True),
        ] {
            let actual = Db2TruthValue::null_predicate(is_null, negated);
            assert_eq!(actual, expected);
            assert_ne!(actual, Unknown);
        }
    }

    #[test]
    fn fixed_qualification_outcomes() {
        for (value, expected) in [(True, true), (False, false), (Unknown, false)] {
            assert_eq!(value.qualifies(), expected, "qualifies({value:?})");
        }
    }

    #[test]
    fn unknown_retained_when_qualification_rejects_it() {
        let outcome = Unknown.logical_and(True).logical_or(False);
        assert_eq!(outcome, Unknown);
        assert!(!outcome.qualifies());
        assert_eq!(outcome.logical_not(), Unknown);
        assert!(!outcome.logical_not().qualifies());
        // Both fail qualification, but negating FALSE must still differ.
        assert!(!False.qualifies());
        assert_eq!(False.logical_not(), True);
        assert!(False.logical_not().qualifies());
    }

    #[test]
    fn bounded_exhaustive_logical_laws_supplement_fixed_tables() {
        let values = [True, False, Unknown];
        for a in values {
            assert_eq!(a.logical_not().logical_not(), a);
            assert_eq!(a.logical_and(a), a);
            assert_eq!(a.logical_or(a), a);
            for b in values {
                assert_eq!(a.logical_and(b), b.logical_and(a));
                assert_eq!(a.logical_or(b), b.logical_or(a));
                assert_eq!(
                    a.logical_and(b).logical_not(),
                    a.logical_not().logical_or(b.logical_not())
                );
                assert_eq!(
                    a.logical_or(b).logical_not(),
                    a.logical_not().logical_and(b.logical_not())
                );
                for c in values {
                    assert_eq!(
                        a.logical_and(b).logical_and(c),
                        a.logical_and(b.logical_and(c))
                    );
                    assert_eq!(a.logical_or(b).logical_or(c), a.logical_or(b.logical_or(c)));
                }
            }
        }
    }

    #[test]
    fn values_are_owned_copy_static_results_and_const_usable() {
        fn require_owned<T: Copy + Send + Sync + 'static>() {}
        require_owned::<Db2TruthValue>();
        const OUTCOME: Db2TruthValue = True.logical_and(Unknown).logical_not().logical_or(False);
        const QUALIFIES: bool = OUTCOME.qualifies();
        const NULL_OUTCOME: Db2TruthValue = Db2TruthValue::null_predicate(true, true);
        assert_eq!((OUTCOME, QUALIFIES, NULL_OUTCOME), (Unknown, false, False));
        let original = Unknown;
        let copied = original;
        assert_eq!(copied.logical_or(True), True);
        assert_eq!(original, Unknown);
    }
}
