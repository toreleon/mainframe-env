//! Existing dataset request/response input projections.

#[cfg(test)]
use super::dataset_helpers::{dataset_attributes, join_records, records_for_write};

pub(super) fn wildcard(pattern: &str, value: &str) -> bool {
    pattern == "*"
        || pattern.eq_ignore_ascii_case(value)
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| value.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_fixed_record_projection_and_defaults_are_unchanged() {
        let defaults = dataset_attributes(&serde_json::json!({})).unwrap();
        assert_eq!(defaults.logical_record_length, 80);
        assert_eq!(defaults.ccsid, Some(37));
        let attributes = dataset_attributes(&serde_json::json!({"lrecl": 4})).unwrap();
        let records = records_for_write(b"AB\r\nCD\n", &attributes).unwrap();
        assert_eq!(records, vec![b"AB  ".to_vec(), b"CD  ".to_vec()]);
        assert_eq!(join_records(records), b"AB\nCD".to_vec());
        assert!(records_for_write(b"ABCDE", &attributes).is_err());
        assert!(dataset_attributes(&serde_json::json!({"dsorg": "UNKNOWN"})).is_err());
    }

    #[test]
    fn wildcard_retains_exact_case_and_prefix_rules() {
        assert!(wildcard("*", "USER.DATA"));
        assert!(wildcard("user.data", "USER.DATA"));
        assert!(wildcard("USER.*", "USER.DATA"));
        assert!(!wildcard("user.*", "USER.DATA"));
        assert!(!wildcard("USER.*", "OTHER.DATA"));
    }
}
