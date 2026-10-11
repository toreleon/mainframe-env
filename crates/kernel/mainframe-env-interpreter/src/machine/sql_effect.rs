//! Existing SQL request construction, mechanically owned beside the machine.
use super::*;

impl ReferenceMachine {
    pub(super) fn sql_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let opcode_index = args
            .iter()
            .position(|token| !matches!(token.as_str(), "SQL" | "END-EXEC"))
            .ok_or(MachineProblem::InvalidOperation)?;
        let opcode = args[opcode_index].to_ascii_uppercase();
        if opcode == "DECLARE" {
            let cursor = args
                .get(opcode_index + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            if args
                .get(opcode_index + 2)
                .is_some_and(|token| token == "CURSOR")
            {
                self.sql_cursors.insert(normalize(cursor), args.to_vec());
            }
            return Ok(Step::Next);
        }
        let mut statement_tokens = args.to_vec();
        let cursor = if matches!(opcode.as_str(), "OPEN" | "FETCH" | "CLOSE") {
            Some(
                args.get(opcode_index + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .to_ascii_uppercase(),
            )
        } else {
            None
        };
        if opcode == "OPEN" {
            statement_tokens = self
                .sql_cursors
                .get(&normalize(cursor.as_deref().unwrap_or_default()))
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
        }
        let into = position(&statement_tokens, "INTO");
        let from = position(&statement_tokens, "FROM");
        let mut inputs = BTreeMap::new();
        let mut targets = Vec::new();
        for (index, token) in statement_tokens.iter().enumerate() {
            if !token.starts_with(':') {
                continue;
            }
            for raw_name in token.trim_start_matches(':').split(':') {
                let name = normalize(raw_name);
                if self.layout(&name).is_none() && !self.implicit.contains_key(&name) {
                    continue;
                }
                let write = matches!(opcode.as_str(), "SELECT" | "FETCH")
                    && into.is_some_and(|into| index > into)
                    && from.is_none_or(|from| index < from);
                if write {
                    targets.push(name);
                } else {
                    inputs.insert(
                        name.clone(),
                        Db2HostVariable {
                            value: self.read(&name)?,
                            indicator: None,
                        },
                    );
                }
            }
        }
        let operation = match opcode.as_str() {
            "SELECT"
                if statement_tokens
                    .iter()
                    .any(|token| token.starts_with("COUNT")) =>
            {
                Db2Operation::Count
            }
            "SELECT" => Db2Operation::Select,
            "INSERT" => Db2Operation::Insert,
            "UPDATE" => Db2Operation::Update,
            "DELETE" => Db2Operation::Delete,
            "OPEN" => Db2Operation::OpenCursor,
            "FETCH" => Db2Operation::FetchCursor,
            "CLOSE" => Db2Operation::CloseCursor,
            "COMMIT" => Db2Operation::Commit,
            "ROLLBACK" => Db2Operation::Rollback,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let mutation = operation
            .is_mutating()
            .then(|| self.mutation())
            .transpose()?;
        self.effect(
            HostRequest::Db2(Db2Request {
                operation,
                statement: statement_tokens
                    .iter()
                    .filter(|token| !matches!(token.as_str(), "SQL" | "END-EXEC"))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
                cursor,
                inputs,
                outputs: targets.clone(),
                max_rows: 1,
                mutation,
            }),
            PendingKind::Db2 { targets },
        )
    }
}
