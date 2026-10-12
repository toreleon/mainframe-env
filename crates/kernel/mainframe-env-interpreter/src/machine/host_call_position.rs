use super::*;

impl ReferenceMachine {
    #[must_use]
    pub fn position_summary(&self) -> String {
        let pc = self.host_failure_pc.unwrap_or(self.pc);
        self.operations.get(pc).map_or_else(
            || format!("pc={pc} completed"),
            |operation| {
                format!(
                    "pc={} {} {:?} role={:?} scope={:?} text={:?}",
                    pc,
                    operation.identity.name(),
                    arguments(operation),
                    optional_text_attribute(operation, "control_role"),
                    optional_text_attribute(operation, "control_scope"),
                    optional_text_attribute(operation, "control_text")
                )
            },
        )
    }

    pub(super) fn host_call_issuing_pc(&self, kind: &PendingKind) -> usize {
        // Generated SORT effects run after execution has advanced to the continuation.
        match (kind, self.sort_io.as_ref()) {
            (PendingKind::SortRead | PendingKind::SortWrite, Some(state)) => state.sort_pc,
            _ => self.pc,
        }
    }
}
