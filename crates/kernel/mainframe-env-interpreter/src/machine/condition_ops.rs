//! Ordinary condition evaluation over the existing machine state and operand owners.

use super::{
    CobolValue, Decimal, MachineProblem, ReferenceMachine, abbreviated_condition,
    cobol_collation_key, compare, comparison_pair, decimal_aligned, decimal_string, decimal_text,
    normalize_comparison_tokens, padded_bytes, strip_condition_parentheses, top_level_position,
    value_bytes, value_decimal,
};

impl ReferenceMachine {
    pub(super) fn eval_condition(&self, tokens: &[String]) -> Result<bool, MachineProblem> {
        let normalized;
        let tokens = if tokens.windows(2).any(comparison_pair) {
            normalized = normalize_comparison_tokens(tokens);
            normalized.as_slice()
        } else {
            tokens
        };
        let tokens = strip_condition_parentheses(tokens);
        let tokens = if tokens.last().is_some_and(|token| token == "THEN") {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        if let Some(or) = top_level_position(tokens, "OR") {
            let left = &tokens[..or];
            let right_tokens = &tokens[or + 1..];
            let right = if self.is_standalone_condition(right_tokens)? {
                right_tokens.to_vec()
            } else {
                abbreviated_condition(left, right_tokens)
            };
            return Ok(self.eval_condition(left)? || self.eval_condition(&right)?);
        }
        if let Some(and) = top_level_position(tokens, "AND") {
            let left = &tokens[..and];
            let right_tokens = &tokens[and + 1..];
            let right = if self.is_standalone_condition(right_tokens)? {
                right_tokens.to_vec()
            } else {
                abbreviated_condition(left, right_tokens)
            };
            return Ok(self.eval_condition(left)? && self.eval_condition(&right)?);
        }
        if tokens.first().is_some_and(|token| token == "NOT") {
            return Ok(!self.eval_condition(&tokens[1..])?);
        }
        if let Some(matched) = self.condition_name_matches(tokens)? {
            return Ok(matched);
        }
        if tokens.len() >= 2
            && !tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
                )
            })
        {
            let class = tokens.last().map(String::as_str).unwrap_or_default();
            if matches!(
                class,
                "NUMERIC" | "ALPHABETIC" | "POSITIVE" | "NEGATIVE" | "ZERO"
            ) {
                let is = tokens
                    .iter()
                    .position(|token| token == "IS")
                    .unwrap_or(tokens.len() - 1);
                let negate = tokens[..tokens.len() - 1]
                    .iter()
                    .any(|token| token == "NOT");
                let value_end =
                    is - usize::from(tokens[..is].last().is_some_and(|token| token == "NOT"));
                let value = self.eval_value(&tokens[..value_end])?;
                let matched = match class {
                    "NUMERIC" => match value {
                        CobolValue::Decimal(_) => true,
                        CobolValue::Bytes(bytes) => {
                            decimal_text(&String::from_utf8_lossy(&bytes)).is_some()
                        }
                    },
                    "ALPHABETIC" => value_bytes(value)?
                        .iter()
                        .all(|byte| byte.is_ascii_alphabetic() || *byte == b' '),
                    "POSITIVE" => value_decimal(value)?.coefficient > 0,
                    "NEGATIVE" => value_decimal(value)?.coefficient < 0,
                    "ZERO" => value_decimal(value)?.coefficient == 0,
                    _ => false,
                };
                return Ok(matched != negate);
            }
        }
        if tokens.len() < 3 {
            return Err(MachineProblem::UnsupportedForm);
        }
        let operator_index = tokens
            .iter()
            .position(|token| {
                matches!(
                    token.as_str(),
                    "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
                )
            })
            .ok_or(MachineProblem::UnsupportedForm)?;
        let mut operator = tokens[operator_index].as_str();
        let negate = tokens[..operator_index]
            .last()
            .is_some_and(|token| token == "NOT");
        let mut left_end = operator_index - usize::from(negate);
        if tokens[..left_end].last().is_some_and(|token| token == "IS") {
            left_end -= 1;
        }
        let mut right_start = operator_index + 1;
        if tokens.get(right_start).is_some_and(|token| token == "TO") {
            right_start += 1;
        }
        if operator == "EQUAL" {
            operator = "=";
        } else if operator == "GREATER" {
            operator = ">";
            if tokens.get(right_start).is_some_and(|token| token == "THAN") {
                right_start += 1;
            }
        } else if operator == "LESS" {
            operator = "<";
            if tokens.get(right_start).is_some_and(|token| token == "THAN") {
                right_start += 1;
            }
        }
        let left = self.eval_value(&tokens[..left_end])?;
        let right = if matches!(left, CobolValue::Decimal(_))
            && tokens[right_start..].len() == 1
            && matches!(tokens[right_start].as_str(), "ZERO" | "ZEROS" | "ZEROES")
        {
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            })
        } else {
            self.eval_value(&tokens[right_start..])?
        };
        match (left, right) {
            (CobolValue::Decimal(left), CobolValue::Decimal(right)) => {
                let (left, right) = decimal_aligned(left, right)?;
                Ok(compare(left.coefficient, operator, right.coefficient) != negate)
            }
            (CobolValue::Bytes(mut left), CobolValue::Bytes(mut right)) => {
                // Width comes from the evaluated opposing operand, including selected references.
                if let Some(byte) = explicit_figurative_byte(&tokens[..left_end]) {
                    left.resize(right.len(), byte);
                }
                if let Some(byte) = explicit_figurative_byte(&tokens[right_start..]) {
                    right.resize(left.len(), byte);
                }
                let (left, right) = padded_bytes(left, right);
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
                let matched = match operator {
                    "=" => left == right,
                    "<>" => left != right,
                    ">" => left > right,
                    "<" => left < right,
                    ">=" => left >= right,
                    "<=" => left <= right,
                    _ => false,
                };
                Ok(matched != negate)
            }
            (CobolValue::Decimal(left), CobolValue::Bytes(right))
                if matches!(operator, "=" | "<>") =>
            {
                let left = self
                    .reference(&tokens[..left_end])
                    .and_then(|reference| self.read_reference(&reference))
                    .unwrap_or_else(|_| decimal_string(left).into_bytes());
                let (left, right) = padded_bytes(left, right);
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
                Ok((if operator == "=" {
                    left == right
                } else {
                    left != right
                }) != negate)
            }
            (CobolValue::Bytes(left), CobolValue::Decimal(right))
                if matches!(operator, "=" | "<>") =>
            {
                let right = self
                    .reference(&tokens[right_start..])
                    .and_then(|reference| self.read_reference(&reference))
                    .unwrap_or_else(|_| decimal_string(right).into_bytes());
                let (left, right) = padded_bytes(left, right);
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
                Ok((if operator == "=" {
                    left == right
                } else {
                    left != right
                }) != negate)
            }
            _ => Err(MachineProblem::DataException),
        }
    }
}

fn explicit_figurative_byte(tokens: &[String]) -> Option<u8> {
    let [token] = tokens else {
        return None;
    };
    match token.as_str() {
        "SPACE" | "SPACES" => Some(b' '),
        "ZERO" | "ZEROS" | "ZEROES" => Some(b'0'),
        "LOW-VALUE" | "LOW-VALUES" | "NULL" | "NULLS" => Some(0),
        "HIGH-VALUE" | "HIGH-VALUES" => Some(0xff),
        _ => None,
    }
}
