use super::*;

impl ReferenceMachine {
    pub(super) fn commit_arithmetic(
        &mut self,
        assignments: Vec<(Vec<String>, Decimal, bool)>,
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        if preserve_failed_receiver {
            self.commit_decimal_assignments_receiver_local(assignments, true)
        } else {
            self.commit_decimal_assignments(assignments).map(|()| false)
        }
    }

    pub(super) fn commit_decimal_assignments(
        &mut self,
        assignments: Vec<(Vec<String>, Decimal, bool)>,
    ) -> Result<(), MachineProblem> {
        let staged = assignments
            .into_iter()
            .map(|assignment| self.stage_legacy_decimal_assignment(assignment, false))
            .collect::<Result<Vec<_>, _>>()?;
        for (reference, bytes) in staged {
            self.write_reference(&reference, &bytes)?;
        }
        Ok(())
    }

    pub(super) fn commit_decimal_assignments_receiver_local(
        &mut self,
        assignments: Vec<(Vec<String>, Decimal, bool)>,
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        let mut staged = Vec::with_capacity(assignments.len());
        let mut receiver_size_error = false;
        for assignment in assignments {
            match self.stage_legacy_decimal_assignment(assignment.clone(), preserve_failed_receiver)
            {
                Ok(write) => staged.push(write),
                Err(MachineProblem::SizeError) => {
                    receiver_size_error = true;
                    if !preserve_failed_receiver {
                        staged.push(self.stage_truncated_legacy_decimal_assignment(assignment)?);
                    }
                }
                Err(problem) => return Err(problem),
            }
        }
        for (reference, bytes) in staged {
            self.write_reference(&reference, &bytes)?;
        }
        Ok(receiver_size_error)
    }

    fn stage_legacy_decimal_assignment(
        &self,
        (target, value, rounded): (Vec<String>, Decimal, bool),
        check_edited_size: bool,
    ) -> Result<(ResolvedReference, Vec<u8>), MachineProblem> {
        let reference = self.reference(&target)?;
        if reference.length != reference.layout.length || !is_numeric(reference.layout.category) {
            return Err(MachineProblem::DataException);
        }
        let value = if matches!(
            reference.layout.category,
            LayoutCategory::FloatShort | LayoutCategory::FloatLong
        ) {
            value
        } else if rounded {
            decimal_rescale_rounded(value, reference.layout.scale)?
        } else {
            decimal_rescale(value, reference.layout.scale)?
        };
        if check_edited_size
            && reference.layout.category == LayoutCategory::NumericEdited
            && decimal_exceeds_picture(&reference.layout, value)
        {
            return Err(MachineProblem::SizeError);
        }
        let bytes = encode_decimal(&reference.layout, value)?;
        Ok((reference, bytes))
    }

    fn stage_truncated_legacy_decimal_assignment(
        &self,
        (target, value, rounded): (Vec<String>, Decimal, bool),
    ) -> Result<(ResolvedReference, Vec<u8>), MachineProblem> {
        let reference = self.reference(&target)?;
        if reference.length != reference.layout.length
            || !is_numeric(reference.layout.category)
            || matches!(
                reference.layout.category,
                LayoutCategory::FloatShort | LayoutCategory::FloatLong
            )
        {
            return Err(MachineProblem::DataException);
        }
        let value = if rounded {
            decimal_rescale_rounded(value, reference.layout.scale)?
        } else {
            decimal_rescale(value, reference.layout.scale)?
        };
        let digits =
            u32::try_from(reference.layout.digits).map_err(|_| MachineProblem::SizeError)?;
        let modulus = ten_power(digits)?;
        let truncated = Decimal {
            coefficient: value.coefficient % modulus,
            scale: value.scale,
        };
        let bytes = encode_decimal(&reference.layout, truncated)?;
        Ok((reference, bytes))
    }
}
