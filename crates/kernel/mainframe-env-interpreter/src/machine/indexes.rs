//! INDEXED BY names are private registers bound to a table, not storage data-names.

use super::*;

impl ReferenceMachine {
    pub(super) fn search_start(
        &self,
        args: &[String],
        index_name: &String,
    ) -> Result<usize, MachineProblem> {
        if args.iter().any(|token| token == "ALL") {
            return Ok(1);
        }
        let value = value_decimal(self.eval_value(std::slice::from_ref(index_name))?)?;
        if value.scale != 0 || value.coefficient <= 0 {
            return Err(MachineProblem::SubscriptError);
        }
        usize::try_from(value.coefficient).map_err(|_| MachineProblem::SubscriptError)
    }

    pub(super) fn validate_index_adjustment(
        &self,
        target: &str,
        amount: Decimal,
        value: Decimal,
        source: &[String],
    ) -> Result<(), MachineProblem> {
        if self.index_table(target).is_some() {
            let zero_literal = source.len() == 1
                && decimal_text(&source[0]).is_some_and(|value| value.coefficient == 0);
            if amount.scale != 0 || zero_literal {
                return Err(MachineProblem::DataException);
            }
            self.index_displacement_for(target, value)?;
        }
        Ok(())
    }

    pub(super) fn has_index_target(&self, targets: &[String]) -> bool {
        targets
            .iter()
            .any(|target| self.index_table(target).is_some())
    }

    pub(super) fn checked_subscript_offset(
        &self,
        tokens: &[String],
        index: usize,
        occurs: usize,
        stride: usize,
        offset: usize,
    ) -> Result<usize, MachineProblem> {
        let displacement = if tokens.len() == 1 && self.index_table(&tokens[0]).is_some() {
            usize::try_from(self.index_displacement_for(
                &tokens[0],
                Decimal {
                    coefficient: index as i128,
                    scale: 0,
                },
            )?)
            .map_err(|_| MachineProblem::SubscriptError)?
        } else {
            index
                .checked_sub(1)
                .and_then(|index| index.checked_mul(stride))
                .ok_or(MachineProblem::SubscriptError)?
        };
        if displacement
            .checked_add(stride)
            .is_none_or(|end| occurs.checked_mul(stride).is_none_or(|extent| end > extent))
        {
            return Err(MachineProblem::SubscriptError);
        }
        offset
            .checked_add(displacement)
            .ok_or(MachineProblem::SubscriptError)
    }

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
