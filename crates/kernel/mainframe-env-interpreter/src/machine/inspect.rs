use super::*;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Characters,
    All,
    Leading,
    First,
}

struct Phrase {
    mode: Mode,
    from: Vec<u8>,
    to: Vec<u8>,
    target: Option<ResolvedReference>,
    range: Range<usize>,
    next_leading: usize,
    finished: bool,
    count: usize,
}

struct Cursor<'a> {
    args: &'a [String],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn eat(&mut self, word: &str) -> bool {
        if self.args.get(self.at).is_some_and(|token| token == word) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, word: &str) -> Result<(), MachineProblem> {
        if self.eat(word) {
            Ok(())
        } else {
            Err(MachineProblem::InvalidOperation)
        }
    }

    fn mode(&mut self) -> Option<Mode> {
        for (word, mode) in [
            ("CHARACTERS", Mode::Characters),
            ("ALL", Mode::All),
            ("LEADING", Mode::Leading),
            ("FIRST", Mode::First),
        ] {
            if self.eat(word) {
                return Some(mode);
            }
        }
        None
    }

    fn operand(&mut self) -> Result<&'a [String], MachineProblem> {
        let start = self.at;
        let token = self
            .args
            .get(self.at)
            .ok_or(MachineProblem::InvalidOperation)?;
        if matches!(
            token.as_str(),
            "CHARACTERS"
                | "ALL"
                | "LEADING"
                | "FIRST"
                | "BY"
                | "FOR"
                | "BEFORE"
                | "AFTER"
                | "INITIAL"
                | "TALLYING"
                | "REPLACING"
                | "CONVERTING"
        ) {
            return Err(MachineProblem::InvalidOperation);
        }
        self.at += 1;
        loop {
            if self.eat("OF") || self.eat("IN") {
                // Qualifiers are names, followed by optional subscripts.
                let qualifier = self
                    .args
                    .get(self.at)
                    .ok_or(MachineProblem::InvalidOperation)?;
                if display_literal(qualifier) {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.at += 1;
            } else if self.args.get(self.at).is_some_and(|token| token == "(") {
                self.at =
                    matching_close(self.args, self.at).ok_or(MachineProblem::InvalidOperation)? + 1;
            } else {
                break;
            }
        }
        Ok(&self.args[start..self.at])
    }

    fn value(&mut self, machine: &ReferenceMachine) -> Result<Vec<u8>, MachineProblem> {
        let tokens = self.operand()?;
        if tokens.len() == 1
            && (display_literal(&tokens[0])
                || matches!(
                    tokens[0].as_str(),
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
                ))
        {
            machine.resolve(&tokens[0])
        } else {
            machine.read_reference(&machine.reference(tokens)?)
        }
    }

    fn range(
        &mut self,
        machine: &ReferenceMachine,
        source: &[u8],
    ) -> Result<Range<usize>, MachineProblem> {
        let mut before = None;
        let mut after = None;
        loop {
            let delimiter = if self.eat("BEFORE") {
                &mut before
            } else if self.eat("AFTER") {
                &mut after
            } else {
                break;
            };
            if delimiter.is_some() {
                return Err(MachineProblem::InvalidOperation);
            }
            self.eat("INITIAL");
            let value = self.value(machine)?;
            if value.is_empty() {
                return Err(MachineProblem::InvalidOperation);
            }
            *delimiter = Some(value);
        }
        let start = match after {
            Some(after) => find_bytes(source, &after).map_or(source.len(), |at| at + after.len()),
            None => 0,
        };
        let end = before
            .as_ref()
            .and_then(|before| find_bytes(source, before))
            .unwrap_or(source.len());
        Ok(start..end.max(start))
    }
}

impl ReferenceMachine {
    pub(super) fn inspect_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let control = ["CONVERTING", "REPLACING", "TALLYING"]
            .into_iter()
            .filter_map(|keyword| position(args, keyword))
            .min()
            .ok_or(MachineProblem::InvalidOperation)?;
        let reference = self.reference(&args[..control])?;
        let source = self.read_reference(&reference)?;
        let mut cursor = Cursor {
            args,
            at: control + 1,
        };
        if args[control] == "CONVERTING" {
            let converting = control;
            if args.get(converting + 2).is_none_or(|token| token != "TO") {
                return Err(MachineProblem::InvalidOperation);
            }
            let from = self.resolve(
                args.get(converting + 1)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            let to = self.resolve(
                args.get(converting + 3)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            if from.is_empty() || from.len() != to.len() {
                return Err(MachineProblem::DataException);
            }
            let range = self.inspect_range(&source, args)?;
            let mut converted = source.clone();
            for byte in &mut converted[range.clone()] {
                *byte = from
                    .iter()
                    .position(|candidate| candidate == byte)
                    .map_or(*byte, |index| to[index]);
            }
            return self.write_reference(&reference, &converted);
        }
        let tallying = args[control] == "TALLYING";
        let mut phrases = Vec::new();
        let mut target = None;
        let mut mode = None;
        while cursor.at < args.len() {
            // A counter followed by FOR starts a new group; otherwise a qualifier
            // remains in effect for subsequent comparands or replacement pairs.
            if tallying {
                let saved = cursor.at;
                if let Ok(tokens) = cursor.operand() {
                    if cursor.eat("FOR") {
                        let counter = self.reference(tokens)?;
                        decode_decimal(&counter.layout, &self.read_reference(&counter)?)?;
                        target = Some(counter);
                        mode = None;
                    } else {
                        cursor.at = saved;
                    }
                }
                if target.is_none() {
                    return Err(MachineProblem::InvalidOperation);
                }
            }
            if let Some(next) = cursor.mode() {
                mode = Some(next);
            }
            let active = mode.ok_or(MachineProblem::InvalidOperation)?;
            if tallying && active == Mode::First {
                return Err(MachineProblem::InvalidOperation);
            }
            let from = if active == Mode::Characters {
                vec![0]
            } else {
                cursor.value(self)?
            };
            let to = if tallying {
                Vec::new()
            } else {
                cursor.expect("BY")?;
                cursor.value(self)?
            };
            if from.is_empty() || (!tallying && from.len() != to.len()) {
                return Err(MachineProblem::DataException);
            }
            let range = cursor.range(self, &source)?;
            let next_leading = range.start;
            phrases.push(Phrase {
                mode: active,
                from,
                to,
                target: target.clone(),
                range,
                next_leading,
                finished: false,
                count: 0,
            });
            if active == Mode::Characters {
                mode = None;
            }
        }
        if phrases.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let mut replaced = source.clone();
        let mut at = 0;
        // IBM Enterprise COBOL 6.5, rlpsinspa/rlpsinspb: comparands are tried
        // in source order at each position; the first match consumes its width.
        while at < source.len() {
            let mut width = 1;
            for phrase in &mut phrases {
                if phrase.finished || at < phrase.range.start || at >= phrase.range.end {
                    continue;
                }
                if phrase.mode == Mode::Leading && at != phrase.next_leading {
                    continue;
                }
                let end = at + phrase.from.len();
                let matches = end <= phrase.range.end
                    && (phrase.mode == Mode::Characters
                        || source
                            .get(at..end)
                            .is_some_and(|bytes| bytes == phrase.from));
                if !matches {
                    if phrase.mode == Mode::Leading {
                        phrase.finished = true;
                    }
                    continue;
                }
                phrase.count += 1;
                width = phrase.from.len();
                phrase.next_leading = end;
                if phrase.mode == Mode::First {
                    phrase.finished = true;
                }
                if !tallying {
                    replaced[at..end].copy_from_slice(&phrase.to);
                }
                break;
            }
            at += width;
        }
        if tallying {
            for phrase in phrases {
                let target = phrase.target.ok_or(MachineProblem::InvalidOperation)?;
                let next = decimal_add(
                    self.arithmetic_mode,
                    decode_decimal(&target.layout, &self.read_reference(&target)?)?,
                    Decimal {
                        coefficient: phrase.count as i128,
                        scale: 0,
                    },
                )?;
                let next = decimal_rescale(next, target.layout.scale)?;
                self.write_reference(&target, &encode_decimal(&target.layout, next)?)?;
            }
            Ok(())
        } else {
            self.write_reference(&reference, &replaced)
        }
    }
    fn inspect_range(
        &self,
        source: &[u8],
        args: &[String],
    ) -> Result<std::ops::Range<usize>, MachineProblem> {
        let delimiter = |keyword: &str| -> Result<Option<Vec<u8>>, MachineProblem> {
            position(args, keyword)
                .map(|at| {
                    let at = at
                        + 1
                        + usize::from(args.get(at + 1).is_some_and(|token| token == "INITIAL"));
                    self.resolve(args.get(at).ok_or(MachineProblem::InvalidOperation)?)
                })
                .transpose()
        };
        let after = delimiter("AFTER")?;
        let before = delimiter("BEFORE")?;
        let start = if let Some(after) = after {
            let Some(at) = find_bytes(source, &after) else {
                return Ok(source.len()..source.len());
            };
            at.checked_add(after.len())
                .ok_or(MachineProblem::ResourceExhausted)?
        } else {
            0
        };
        let end = before
            .as_ref()
            .and_then(|before| find_bytes(&source[start..], before))
            .map_or(source.len(), |at| start + at);
        Ok(start..end)
    }
}
