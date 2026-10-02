//! Independent public vectors for the owned search-condition outcome domain.
//! Db2 13 search-conditions and NULL-predicate pins, not expression execution.

use mainframe_env_db2::Db2TruthValue::{self, False, True, Unknown};

#[test]
fn public_truth_tables_preserve_every_ordered_unknown_case() {
    let vectors = [
        (True, True, True, True),
        (True, False, False, True),
        (True, Unknown, Unknown, True),
        (False, True, False, True),
        (False, False, False, False),
        (False, Unknown, False, Unknown),
        (Unknown, True, Unknown, True),
        (Unknown, False, False, Unknown),
        (Unknown, Unknown, Unknown, Unknown),
    ];
    for (left, right, conjunction, disjunction) in vectors {
        assert_eq!(left.logical_and(right), conjunction);
        assert_eq!(left.logical_or(right), disjunction);
    }
    for (input, negation, qualifies) in [
        (True, False, true),
        (False, True, false),
        (Unknown, Unknown, false),
    ] {
        assert_eq!(input.logical_not(), negation);
        assert_eq!(input.qualifies(), qualifies);
    }
}

#[test]
fn public_null_predicates_are_two_valued_without_byte_inference() {
    for (is_null, is_not_null, expected) in [
        (true, false, True),
        (false, false, False),
        (true, true, False),
        (false, true, True),
    ] {
        let result = Db2TruthValue::null_predicate(is_null, is_not_null);
        assert_eq!(result, expected);
        assert_ne!(result, Unknown);
    }
}

#[test]
fn public_compositions_do_not_coerce_unknown_to_false_before_negation() {
    let unchanged = Unknown.logical_and(True).logical_or(False);
    assert_eq!(unchanged, Unknown);
    assert_eq!(unchanged.logical_not(), Unknown);
    assert!(!unchanged.qualifies());
    assert!(!unchanged.logical_not().qualifies());
    assert_eq!(Unknown.logical_or(True).logical_not(), False);
    assert_eq!(Unknown.logical_and(False).logical_not(), True);
    assert!(Unknown.logical_and(False).logical_not().qualifies());
}

#[test]
fn public_outcomes_are_immutable_owned_const_values() {
    fn owned<T: Copy + Send + Sync + 'static>(value: T) -> T {
        value
    }
    const RESULT: Db2TruthValue = Unknown.logical_or(False).logical_not();
    const KEEP: bool = RESULT.qualifies();
    const NULL_RESULT: Db2TruthValue = Db2TruthValue::null_predicate(false, true);
    assert_eq!((owned(RESULT), KEEP, NULL_RESULT), (Unknown, false, True));
    let original = owned(Unknown);
    let result = original.logical_and(False);
    assert_eq!(original, Unknown);
    assert_eq!(result, False);
}
