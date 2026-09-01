use mainframe_env_host_api::HostProblem;

mod generated {
    include!("generated/ams_grammar.rs");
}

pub const AMS_GRAMMAR_CONTRACT: &str = "mainframe-env.ams-grammar@1";
pub use generated::AMS_GRAMMAR_SHA256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AmsGrammarEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
    pub capability: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmsCommand {
    id: &'static str,
    label: &'static str,
    source: String,
    capability: Option<&'static str>,
}

impl AmsCommand {
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.label
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub const fn capability(&self) -> Option<&'static str> {
        self.capability
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmsRegister {
    MaxCc,
    LastCc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmsComparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AmsStatement {
    Command(AmsCommand),
    Set {
        register: AmsRegister,
        value: u8,
    },
    If {
        register: AmsRegister,
        comparison: AmsComparison,
        value: u8,
        action: Box<AmsStatement>,
    },
}

pub fn parse_idcams_control(control: &[u8]) -> Result<Vec<AmsStatement>, HostProblem> {
    let control = std::str::from_utf8(control)
        .map_err(|_| HostProblem::Malformed)?
        .to_ascii_uppercase();
    let logical = logical_statements(&control)?;
    logical
        .into_iter()
        .map(|statement| parse_statement(&statement, false))
        .collect()
}

pub fn validate_idcams_control(control: &[u8]) -> Result<usize, HostProblem> {
    let statements = parse_idcams_control(control)?;
    validate_statements(&statements)?;
    Ok(statements.len())
}

#[cfg(test)]
pub(crate) fn grammar() -> &'static [AmsGrammarEntry] {
    generated::AMS_GRAMMAR
}

fn logical_statements(control: &str) -> Result<Vec<String>, HostProblem> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut balance = 0i32;
    let mut continued = false;
    for raw in control.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let recognized =
            command_entry(line).is_some() || line.starts_with("IF ") || line.starts_with("SET ");
        if recognized && !current.is_empty() && balance == 0 && !continued {
            statements.push(current.trim().to_string());
            current.clear();
        } else if !recognized && current.is_empty() {
            return Err(HostProblem::Unsupported);
        } else if !recognized && balance == 0 && !continued {
            statements.push(current.trim().to_string());
            current.clear();
            return Err(HostProblem::Unsupported);
        }
        if !current.is_empty() {
            current.push(' ');
        }
        continued = line.ends_with('-');
        let part = line.trim_end_matches('-').trim_end();
        balance = balance
            .checked_add(
                i32::try_from(part.bytes().filter(|byte| *byte == b'(').count())
                    .map_err(|_| HostProblem::ResourceExhausted)?,
            )
            .and_then(|value| {
                value.checked_sub(
                    i32::try_from(part.bytes().filter(|byte| *byte == b')').count()).ok()?,
                )
            })
            .ok_or(HostProblem::ResourceExhausted)?;
        if balance < 0 {
            return Err(HostProblem::Malformed);
        }
        current.push_str(part);
        if current.len() > 1024 * 1024 || statements.len() >= 4096 {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    if balance != 0 || continued {
        return Err(HostProblem::Malformed);
    }
    if !current.is_empty() {
        statements.push(current.trim().to_string());
    }
    if statements.is_empty() {
        Err(HostProblem::Malformed)
    } else {
        Ok(statements)
    }
}

fn parse_statement(statement: &str, nested: bool) -> Result<AmsStatement, HostProblem> {
    if statement.starts_with("SET ") {
        return parse_set(statement);
    }
    if statement.starts_with("IF ") {
        if nested {
            return Err(HostProblem::ResourceExhausted);
        }
        return parse_if(statement);
    }
    let entry = command_entry(statement).ok_or(HostProblem::Unsupported)?;
    Ok(AmsStatement::Command(AmsCommand {
        id: entry.id,
        label: entry.label,
        source: statement.into(),
        capability: entry.capability,
    }))
}

fn parse_set(statement: &str) -> Result<AmsStatement, HostProblem> {
    let normalized = statement.replace('=', " = ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    if tokens.len() != 4 || tokens[0] != "SET" || tokens[2] != "=" {
        return Err(HostProblem::Malformed);
    }
    Ok(AmsStatement::Set {
        register: register(tokens[1])?,
        value: condition_code(tokens[3])?,
    })
}

fn parse_if(statement: &str) -> Result<AmsStatement, HostProblem> {
    let (condition, action) = statement
        .strip_prefix("IF ")
        .and_then(|value| value.split_once(" THEN "))
        .ok_or(HostProblem::Malformed)?;
    let tokens = condition.split_whitespace().collect::<Vec<_>>();
    if tokens.len() != 3 {
        return Err(HostProblem::Malformed);
    }
    let comparison = match tokens[1] {
        "EQ" | "=" => AmsComparison::Equal,
        "NE" | "¬=" => AmsComparison::NotEqual,
        "LT" | "<" => AmsComparison::Less,
        "LE" | "<=" => AmsComparison::LessOrEqual,
        "GT" | ">" => AmsComparison::Greater,
        "GE" | ">=" => AmsComparison::GreaterOrEqual,
        _ => return Err(HostProblem::Malformed),
    };
    Ok(AmsStatement::If {
        register: register(tokens[0])?,
        comparison,
        value: condition_code(tokens[2])?,
        action: Box::new(parse_statement(action.trim(), true)?),
    })
}

fn register(value: &str) -> Result<AmsRegister, HostProblem> {
    match value {
        "MAXCC" => Ok(AmsRegister::MaxCc),
        "LASTCC" => Ok(AmsRegister::LastCc),
        _ => Err(HostProblem::Malformed),
    }
}

fn condition_code(value: &str) -> Result<u8, HostProblem> {
    let value = value.parse::<u8>().map_err(|_| HostProblem::Malformed)?;
    if value > 16 || !value.is_multiple_of(4) {
        Err(HostProblem::Malformed)
    } else {
        Ok(value)
    }
}

fn command_entry(statement: &str) -> Option<&'static AmsGrammarEntry> {
    let tokens = statement
        .split_whitespace()
        .map(|token| token.trim_matches(['(', ')', ',']))
        .collect::<Vec<_>>();
    generated::AMS_GRAMMAR
        .iter()
        .filter(|entry| {
            tokens.len() >= entry.keywords.len()
                && entry
                    .keywords
                    .iter()
                    .zip(&tokens)
                    .all(|(expected, actual)| expected == actual)
        })
        .max_by_key(|entry| entry.keywords.len())
}

fn validate_statements(statements: &[AmsStatement]) -> Result<(), HostProblem> {
    for statement in statements {
        match statement {
            AmsStatement::Command(command) => validate_command(command)?,
            AmsStatement::Set { .. } => {}
            AmsStatement::If { action, .. } => {
                validate_statements(std::slice::from_ref(action.as_ref()))?
            }
        }
    }
    Ok(())
}

fn validate_command(command: &AmsCommand) -> Result<(), HostProblem> {
    if command.capability.is_some() {
        return Ok(());
    }
    let source = command.source();
    let valid = match command.id() {
        "listcat" => true,
        "shcds" => {
            operand(source, &["DATASET"]).is_some() || operand(source, &["TRANSACTION"]).is_some()
        }
        "allocate" | "define-nonvsam" => operand(source, &["DATASET", "NAME"]).is_some(),
        "alter" | "delete" => bare_target(source, command.label()).is_some(),
        "bldindex" => {
            operand(source, &["INDATASET"]).is_some() && operand(source, &["OUTDATASET"]).is_some()
        }
        "dcollect" => operand(source, &["OUTFILE", "OFILE"]).is_some(),
        "define-alias" => {
            operand(source, &["NAME"]).is_some() && operand(source, &["RELATE"]).is_some()
        }
        "define-alternateindex" => {
            operand(source, &["NAME"]).is_some()
                && operand(source, &["RELATE"]).is_some()
                && pair_operand(source, "KEYS").is_some()
        }
        "define-cluster" => operand(source, &["NAME"]).is_some(),
        "define-generationdatagroup" => {
            operand(source, &["NAME"]).is_some() && numeric_operand(source, "LIMIT").is_some()
        }
        "define-path" => {
            operand(source, &["NAME"]).is_some() && operand(source, &["PATHENTRY"]).is_some()
        }
        "define-usercatalog" => operand(source, &["NAME"]).is_some(),
        "diagnose" | "examine" | "listdata" | "verify" => {
            operand(source, &["INDATASET", "DATASET", "ENTRIES"]).is_some()
                || bare_target(source, command.label()).is_some()
        }
        "export" => {
            operand(source, &["ENTRIES", "INDATASET"]).is_some()
                && operand(source, &["OUTFILE", "OFILE"]).is_some()
        }
        "export-disconnect" => {
            operand(source, &["ENTRIES"]).is_some()
                && operand(source, &["OUTFILE", "OFILE"]).is_some()
        }
        "import" | "import-connect" => operand(source, &["INFILE", "IFILE"]).is_some(),
        "print" => operand(source, &["INDATASET", "INFILE"]).is_some(),
        "repro" => {
            operand(source, &["INDATASET", "INFILE", "IFILE"]).is_some()
                && operand(source, &["OUTDATASET", "OUTFILE", "OFILE"]).is_some()
        }
        "recover" => {
            operand(source, &["INDATASET", "DATASET"]).is_some()
                && operand(source, &["INFILE", "IFILE"]).is_some()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(HostProblem::Malformed)
    }
}

pub(crate) fn compare(left: u8, comparison: AmsComparison, right: u8) -> bool {
    match comparison {
        AmsComparison::Equal => left == right,
        AmsComparison::NotEqual => left != right,
        AmsComparison::Less => left < right,
        AmsComparison::LessOrEqual => left <= right,
        AmsComparison::Greater => left > right,
        AmsComparison::GreaterOrEqual => left >= right,
    }
}

pub(crate) fn operand(statement: &str, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| balanced_operand(statement, name))
}

fn balanced_operand(statement: &str, name: &str) -> Option<String> {
    let bytes = statement.as_bytes();
    let mut search_from = 0usize;
    while let Some(relative) = statement.get(search_from..)?.find(name) {
        let at = search_from.checked_add(relative)?;
        let after_name = at.checked_add(name.len())?;
        let left_boundary = at == 0
            || !bytes
                .get(at.checked_sub(1)?)
                .is_some_and(u8::is_ascii_alphanumeric);
        let mut open = after_name;
        while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
            open = open.checked_add(1)?;
        }
        if left_boundary && bytes.get(open) == Some(&b'(') {
            let value_start = open.checked_add(1)?;
            let mut depth = 1u32;
            let mut quote = None;
            let mut position = value_start;
            while let Some(byte) = bytes.get(position).copied() {
                if matches!(byte, b'\'' | b'"') {
                    if quote == Some(byte) {
                        quote = None;
                    } else if quote.is_none() {
                        quote = Some(byte);
                    }
                } else if quote.is_none() {
                    if byte == b'(' {
                        depth = depth.checked_add(1)?;
                    } else if byte == b')' {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            let value = statement.get(value_start..position)?.trim();
                            let value = if value.len() >= 2
                                && matches!(value.as_bytes().first(), Some(b'\'' | b'"'))
                                && value.as_bytes().first() == value.as_bytes().last()
                            {
                                value.get(1..value.len().checked_sub(1)?)?
                            } else {
                                value
                            };
                            return (!value.is_empty()).then(|| value.to_string());
                        }
                    }
                }
                position = position.checked_add(1)?;
            }
            return None;
        }
        search_from = after_name;
    }
    None
}

pub(crate) fn numeric_operand(statement: &str, name: &str) -> Option<u32> {
    operand(statement, &[name])?.parse().ok()
}

pub(crate) fn pair_operand(statement: &str, name: &str) -> Option<(u32, u32)> {
    let value = operand(statement, &[name])?;
    let values = value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|value| !value.is_empty())
        .map(str::parse)
        .collect::<Result<Vec<u32>, _>>()
        .ok()?;
    (values.len() == 2).then(|| (values[0], values[1]))
}

pub(crate) fn bare_target(statement: &str, label: &str) -> Option<String> {
    statement
        .strip_prefix(label)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.split_whitespace().next())
        .map(|value| value.trim_matches(['(', ')', '\'', '"']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_grammar_is_complete_unique_and_matches_labels() {
        assert_eq!(grammar().len(), 31);
        for (position, command) in grammar().iter().enumerate() {
            assert!(
                grammar()[..position]
                    .iter()
                    .all(|prior| prior.id != command.id && prior.label != command.label)
            );
            assert_eq!(command.label, command.keywords.join(" "));
        }
    }

    #[test]
    fn operand_parser_preserves_nested_values_and_token_boundaries() {
        let statement = "DEFINE CLUSTER (NAME(USER.A) SPACE(TRACKS(2 1)) NEWNAME(USER.B))";
        assert_eq!(
            operand(statement, &["SPACE"]).as_deref(),
            Some("TRACKS(2 1)")
        );
        assert_eq!(operand(statement, &["TRACKS"]).as_deref(), Some("2 1"));
        assert_eq!(operand(statement, &["NAME"]).as_deref(), Some("USER.A"));
        assert_eq!(operand(statement, &["NEWNAME"]).as_deref(), Some("USER.B"));
    }

    #[test]
    fn all_command_forms_and_modal_controls_parse() {
        let controls = [
            "ALLOCATE DATASET(USER.A)",
            "ALTER USER.A",
            "ALTER LIBRARYENTRY NAME(LIB)",
            "ALTER VOLUMEENTRY NAME(VOL001)",
            "BLDINDEX INDATASET(USER.A) OUTDATASET(USER.A.AIX)",
            "CREATE LIBRARYENTRY NAME(LIB)",
            "CREATE VOLUMEENTRY NAME(VOL001)",
            "DCOLLECT OUTFILE(OUT)",
            "DEFINE ALIAS (NAME(USER.ALIAS) RELATE(USER.A))",
            "DEFINE ALTERNATEINDEX (NAME(USER.A.AIX) RELATE(USER.A) KEYS(2 0))",
            "DEFINE CLUSTER (NAME(USER.A) INDEXED KEYS(2 0) RECORDSIZE(4 4))",
            "DEFINE GENERATIONDATAGROUP (NAME(USER.GDG) LIMIT(3))",
            "DEFINE NONVSAM (NAME(USER.PS))",
            "DEFINE PAGESPACE (NAME(PAGE.ONE))",
            "DEFINE PATH (NAME(USER.A.PATH) PATHENTRY(USER.A.AIX))",
            "DEFINE USERCATALOG (NAME(USER.CAT))",
            "DELETE USER.A",
            "DIAGNOSE USER.A",
            "EXAMINE USER.A",
            "EXPORT ENTRIES(USER.A) OUTFILE(OUT)",
            "EXPORT DISCONNECT ENTRIES(USER.CAT) OUTFILE(OUT)",
            "IMPORT INFILE(IN)",
            "IMPORT CONNECT INFILE(IN)",
            "LISTCAT",
            "LISTDATA USER.A",
            "PRINT INDATASET(USER.A)",
            "REPRO INDATASET(USER.A) OUTDATASET(USER.B)",
            "RECOVER INDATASET(USER.A) INFILE(IN)",
            "SETCACHE NAME(VOL001)",
            "SHCDS DATASET(USER.A)",
            "VERIFY USER.A",
            "IF MAXCC LE 08 THEN SET MAXCC = 0",
        ];
        assert_eq!(
            validate_idcams_control(controls.join("\n").as_bytes()),
            Ok(32)
        );
    }

    #[test]
    fn unknown_unbalanced_and_invalid_modal_forms_fail_closed() {
        for control in [
            "UNKNOWN CONTROL",
            "DEFINE CLUSTER (NAME(USER.A)",
            "IF MAXCC MAYBE 8 THEN SET MAXCC = 0",
            "SET MAXCC = 3",
        ] {
            assert!(validate_idcams_control(control.as_bytes()).is_err());
        }
    }
}
