//! INDEXED BY names are private registers bound to a table, not storage data-names.

use super::*;

impl ReferenceMachine {
    pub(super) fn index_table(&self, name: &str) -> Option<&LayoutMetadata> {
        let name = normalize(name);
        self.layouts
            .values()
            .find(|table| table.indexes.contains(&name))
    }

    // Retain normalized occurrence numbers in the existing implicit/checkpoint slots.
    // Addressing converts them to displacements using the defining table's stride,
    // including when the index is used to reference a different table (rlrefsub).
    pub(super) fn index_displacement_for(
        &self,
        name: &str,
        occurrence: Decimal,
    ) -> Result<i128, MachineProblem> {
        let table = self
            .index_table(name)
            .ok_or(MachineProblem::UnknownStorage)?;
        if occurrence.scale != 0 || occurrence.coefficient <= 0 {
            return Err(MachineProblem::SubscriptError);
        }
        (occurrence.coefficient - 1)
            .checked_mul(table.element_length as i128)
            .ok_or(MachineProblem::SubscriptError)
    }

    pub(super) fn set_indexes(
        &mut self,
        targets: &[String],
        source: &[String],
    ) -> Result<(), MachineProblem> {
        let value = value_decimal(self.eval_value(source)?)?;
        let targets = targets
            .iter()
            .filter(|target| target.as_str() != ",")
            .collect::<Vec<_>>();
        for target in &targets {
            self.index_displacement_for(target, value)?;
        }
        for target in targets {
            self.write_decimal(target, value)?;
        }
        Ok(())
    }
}
