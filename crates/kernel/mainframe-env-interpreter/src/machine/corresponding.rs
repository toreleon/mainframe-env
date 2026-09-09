use super::*;

impl ReferenceMachine {
    pub(super) fn add_or_subtract_corresponding(
        &mut self,
        name: &str,
        args: &[String],
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        let separator = position(args, if name == "add" { "TO" } else { "FROM" })
            .ok_or(MachineProblem::InvalidOperation)?;
        if separator <= 1 {
            return Err(MachineProblem::InvalidOperation);
        }
        let target_end = args[separator + 1..]
            .iter()
            .position(|token| matches!(token.as_str(), "ROUNDED" | "ON" | "NOT"))
            .map_or(args.len(), |offset| separator + 1 + offset);
        let source = self.reference(&args[1..separator])?;
        let target = self.reference(&args[separator + 1..target_end])?;
        if !is_group(source.layout.category) || !is_group(target.layout.category) {
            return Err(MachineProblem::DataException);
        }
        let rounded = args[target_end..].iter().any(|token| token == "ROUNDED");
        let sources = self
            .group_numeric_descendants(&source.layout.name)
            .into_iter()
            .filter_map(|layout| {
                self.corresponding_key(&layout, &source.layout.name)
                    .map(|key| (key, layout))
            })
            .fold(
                BTreeMap::<Vec<String>, Vec<LayoutMetadata>>::new(),
                |mut candidates, (key, layout)| {
                    candidates.entry(key).or_default().push(layout);
                    candidates
                },
            );
        let targets = self
            .group_numeric_descendants(&target.layout.name)
            .into_iter()
            .filter_map(|layout| {
                self.corresponding_key(&layout, &target.layout.name)
                    .map(|key| (key, layout))
            })
            .collect::<Vec<_>>();
        let target_counts = targets.iter().fold(
            BTreeMap::<Vec<String>, usize>::new(),
            |mut counts, (key, _)| {
                *counts.entry(key.clone()).or_default() += 1;
                counts
            },
        );
        let mut assignments = Vec::new();
        for (key, target_layout) in targets {
            let Some(matching) = sources.get(&key) else {
                continue;
            };
            if matching.len() != 1 || target_counts.get(&key) != Some(&1) {
                continue;
            }
            let source_reference =
                self.corresponding_occurrence_reference(&source, matching[0].clone())?;
            let target_reference =
                self.corresponding_occurrence_reference(&target, target_layout)?;
            let source_value = decode_decimal(
                &source_reference.layout,
                &self.read_reference(&source_reference)?,
            )?;
            let target_value = decode_decimal(
                &target_reference.layout,
                &self.read_reference(&target_reference)?,
            )?;
            assignments.push((
                target_reference,
                if name == "add" {
                    decimal_add(self.arithmetic_mode, target_value, source_value)?
                } else {
                    decimal_subtract(self.arithmetic_mode, target_value, source_value)?
                },
                rounded,
            ));
        }
        self.commit_corresponding_assignments(assignments, preserve_failed_receiver)
    }

    fn group_numeric_descendants(&self, root: &str) -> Vec<LayoutMetadata> {
        self.layouts
            .values()
            .filter(|layout| self.corresponding_item_eligible(layout, root))
            .cloned()
            .collect()
    }

    fn corresponding_item_eligible(&self, layout: &LayoutMetadata, root: &str) -> bool {
        if !is_numeric(layout.category) || layout.simple_name == "FILLER" {
            return false;
        }
        let mut subordinate = layout;
        loop {
            if subordinate.alias_of.is_some()
                || subordinate.occurs_clause
                || subordinate.category == LayoutCategory::Rename
            {
                return false;
            }
            let Some(parent) = subordinate.parent.as_deref() else {
                return false;
            };
            if parent == root {
                return true;
            }
            let Some(parent) = self.layouts.get(parent) else {
                return false;
            };
            subordinate = parent;
        }
    }

    fn corresponding_key(&self, layout: &LayoutMetadata, root: &str) -> Option<Vec<String>> {
        let mut key = vec![layout.simple_name.clone()];
        let mut parent = layout.parent.as_deref()?;
        while parent != root {
            let qualifier = self.layouts.get(parent)?;
            if qualifier.simple_name != "FILLER" {
                key.push(qualifier.simple_name.clone());
            }
            parent = qualifier.parent.as_deref()?;
        }
        Some(key)
    }

    fn corresponding_occurrence_reference(
        &self,
        selected: &ResolvedReference,
        layout: LayoutMetadata,
    ) -> Result<ResolvedReference, MachineProblem> {
        let relative = layout
            .offset
            .checked_sub(selected.layout.offset)
            .ok_or(MachineProblem::InvalidOperation)?;
        if relative
            .checked_add(layout.length)
            .is_none_or(|end| end > selected.layout.element_length)
        {
            return Err(MachineProblem::InvalidOperation);
        }
        Ok(ResolvedReference {
            length: layout.length,
            layout,
            offset: selected.offset,
        })
    }

    fn commit_corresponding_assignments(
        &mut self,
        assignments: Vec<(ResolvedReference, Decimal, bool)>,
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        let mut staged = Vec::with_capacity(assignments.len());
        let mut receiver_size_error = false;
        for (reference, value, rounded) in assignments {
            let value = if rounded {
                decimal_rescale_rounded(value, reference.layout.scale)
            } else {
                decimal_rescale(value, reference.layout.scale)
            }?;
            match encode_decimal(&reference.layout, value) {
                Ok(bytes) => staged.push((reference, bytes)),
                Err(MachineProblem::SizeError) => {
                    receiver_size_error = true;
                    if !preserve_failed_receiver {
                        let digits = u32::try_from(reference.layout.digits)
                            .map_err(|_| MachineProblem::SizeError)?;
                        let modulus = ten_power(digits)?;
                        let truncated = Decimal {
                            coefficient: value.coefficient % modulus,
                            scale: value.scale,
                        };
                        staged.push((
                            reference.clone(),
                            encode_decimal(&reference.layout, truncated)?,
                        ));
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
}
