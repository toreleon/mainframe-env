//! Existing machine output, task-priority and cursor accessors.
use super::*;

impl ReferenceMachine {
    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    /// Current task priority after any completed CICS scheduling command.
    #[must_use]
    pub fn invocation_priority(&self) -> u8 {
        self.invocation.priority
    }
    #[must_use]
    pub fn dataset_cursors(&self) -> &BTreeMap<String, String> {
        &self.dataset_cursors
    }
    pub fn install_dataset_cursors(
        &mut self,
        cursors: BTreeMap<String, String>,
    ) -> Result<(), MachineProblem> {
        if cursors.len() > self.invocation.limits.max_frames as usize
            || cursors.iter().any(|(dataset, cursor)| {
                DatasetName::new(dataset, 128).is_err() || cursor.is_empty() || cursor.len() > 128
            })
        {
            return Err(MachineProblem::ResourceExhausted);
        }
        self.dataset_cursors = cursors;
        Ok(())
    }
}
