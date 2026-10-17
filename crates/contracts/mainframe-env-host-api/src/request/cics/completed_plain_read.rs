//! Pinned plain READ DATASET spelling naming an actual configured CICS file alias.
//! Structural eligibility alone grants no task, result or SAF authority.
use super::{CicsOperation, CicsRequest};

impl CicsRequest {
    /// Eligible structure only. The original core owner, provider command lease,
    /// configured alias, successful receipt and same-store atomic fence remain required.
    #[must_use]
    pub fn is_completed_plain_read_replay(&self) -> bool {
        self.operation == CicsOperation::Read
            && self.mutation.is_none()
            && !self.is_mutating()
            && self.arguments.contains_key("DATASET")
            && self.arguments.contains_key("RIDFLD")
            && ![
                "FILE",
                "OPTION.UPDATE",
                "TOKEN",
                "REQID",
                "CURSOR",
                "CURSOR.REQID",
                "SYSID",
                "OPTION.GENERIC",
                "OPTION.RBA",
                "OPTION.RRN",
                "OPTION.XRBA",
            ]
            .iter()
            .any(|name| self.arguments.contains_key(*name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CicsConditionPolicy;
    use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
    use std::collections::BTreeMap;
    fn request() -> CicsRequest {
        let value = |bytes: &[u8]| {
            BoundedPayload::new(
                "mainframe-env.cics.literal@1",
                bytes.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        CicsRequest {
            operation: CicsOperation::Read,
            condition_policy: CicsConditionPolicy::NoHandle,
            mutation: None,
            arguments: BTreeMap::from([
                ("DATASET".into(), value(b"ACCOUNT")),
                ("RIDFLD".into(), value(b"AA")),
            ]),
        }
    }
    #[test]
    fn completed_plain_read_shape_refuses_update_browse_cursor_and_alternate_address_forms() {
        assert!(request().is_completed_plain_read_replay());
        for name in [
            "FILE",
            "OPTION.UPDATE",
            "TOKEN",
            "REQID",
            "CURSOR",
            "CURSOR.REQID",
            "SYSID",
            "OPTION.GENERIC",
            "OPTION.RBA",
            "OPTION.RRN",
            "OPTION.XRBA",
        ] {
            let mut offered = request();
            offered.arguments.insert(
                name.into(),
                BoundedPayload::new(
                    "mainframe-env.cics.literal@1",
                    b"X".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            assert!(!offered.is_completed_plain_read_replay(), "{name}");
        }
        for operation in [
            CicsOperation::StartBrowse,
            CicsOperation::ReadNext,
            CicsOperation::ReadPrev,
            CicsOperation::Rewrite,
            CicsOperation::Write,
        ] {
            let mut offered = request();
            offered.operation = operation;
            assert!(!offered.is_completed_plain_read_replay());
        }
    }
    #[test]
    fn completed_plain_read_shape_requires_both_original_file_and_key() {
        for name in ["DATASET", "RIDFLD"] {
            let mut offered = request();
            offered.arguments.remove(name);
            assert!(!offered.is_completed_plain_read_replay());
        }
    }
}
