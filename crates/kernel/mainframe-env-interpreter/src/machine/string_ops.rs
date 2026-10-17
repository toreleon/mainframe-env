//! Private STRING operand consumption through the machine's checked reference owner.

use super::{
    Decimal, MachineProblem, ReferenceMachine, decimal_text, display_literal, find_bytes,
    matching_close, normalize, position,
};

impl ReferenceMachine {
    pub(super) fn string_op(&mut self, args: &[String]) -> Result<bool, MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let mut value = Vec::new();
        let mut index = 0usize;
        while index < into {
            let mut source = self.string_operand(&args[..into], &mut index)?;
            if args.get(index).is_some_and(|token| token == "DELIMITED") {
                if args.get(index + 1).is_none_or(|token| token != "BY") {
                    return Err(MachineProblem::InvalidOperation);
                }
                index += 2;
                if args.get(index).is_some_and(|token| token == "SIZE") {
                    index += 1;
                } else {
                    let delimiter = self.string_operand(&args[..into], &mut index)?;
                    if delimiter.is_empty() {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    if let Some(position) = find_bytes(&source, &delimiter) {
                        source.truncate(position);
                    }
                }
            }
            value.extend(source);
        }
        let target_end = position(&args[into + 1..], "WITH")
            .or_else(|| position(&args[into + 1..], "ON"))
            .map(|offset| into + 1 + offset)
            .unwrap_or(args.len());
        let target = self.reference(&args[into + 1..target_end])?;
        let pointer = position(args, "POINTER")
            .and_then(|at| args.get(at + 1))
            .cloned();
        let start = pointer
            .as_deref()
            .map(|name| self.decimal(name))
            .transpose()?
            .map_or(1, |value| {
                if value.scale == 0 {
                    usize::try_from(value.coefficient).unwrap_or(0)
                } else {
                    0
                }
            });
        if start == 0 {
            return Err(MachineProblem::DataException);
        }
        let mut target_bytes = self.read_reference(&target)?;
        let offset = start - 1;
        let available = target_bytes.len().saturating_sub(offset);
        let copied = available.min(value.len());
        if copied > 0 {
            target_bytes[offset..offset + copied].copy_from_slice(&value[..copied]);
            self.write_reference(&target, &target_bytes)?;
        }
        if let Some(pointer) = pointer {
            self.write_decimal(
                &pointer,
                Decimal {
                    coefficient: i128::try_from(start.saturating_add(copied))
                        .map_err(|_| MachineProblem::ResourceExhausted)?,
                    scale: 0,
                },
            )?;
        }
        let overflow = offset > target_bytes.len() || copied < value.len();
        Ok(overflow)
    }

    fn string_operand(
        &self,
        args: &[String],
        index: &mut usize,
    ) -> Result<Vec<u8>, MachineProblem> {
        let start = *index;
        let token = args.get(start).ok_or(MachineProblem::InvalidOperation)?;
        *index += 1;
        if display_literal(token)
            || matches!(
                normalize(token).as_str(),
                "SPACE"
                    | "SPACES"
                    | "ZERO"
                    | "ZEROS"
                    | "ZEROES"
                    | "LOW-VALUE"
                    | "LOW-VALUES"
                    | "HIGH-VALUE"
                    | "HIGH-VALUES"
                    | "NULL"
                    | "NULLS"
            )
            || decimal_text(token).is_some()
            || (self.layout(token).is_none() && self.implicit.contains_key(&normalize(token)))
        {
            return self.resolve(token);
        }
        // Keep qualification and each balanced subscript/reference modification together.
        // The shared reference owner checks indexes and selects raw PIC storage bytes.
        while args
            .get(*index)
            .is_some_and(|token| matches!(token.as_str(), "OF" | "IN"))
        {
            if args.get(*index + 1).is_none() {
                return Err(MachineProblem::InvalidOperation);
            }
            *index += 2;
        }
        while args.get(*index).is_some_and(|token| token == "(") {
            *index = matching_close(args, *index).ok_or(MachineProblem::InvalidOperation)? + 1;
        }
        self.read_reference(&self.reference(&args[start..*index])?)
    }
}
