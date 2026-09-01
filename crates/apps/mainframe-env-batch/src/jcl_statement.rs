use crate::{
    DdParameterId, ExecParameterId, JclCatalogSupport, JclGeneratedIdentity, JclParameterOutcome,
    JclRecordKind, JclStatementId, JclSyntaxAnalysis, JclValueShape, JobParameterId,
    OutputParameterId,
};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity, SourceSpan,
};
use mainframe_env_source::SourceRange;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JclParameterIdentity {
    Dd(DdParameterId),
    Exec(ExecParameterId),
    Job(JobParameterId),
    Output(OutputParameterId),
}

impl JclParameterIdentity {
    #[must_use]
    pub fn generated(self) -> JclGeneratedIdentity {
        match self {
            Self::Dd(id) => id.descriptor().generated_identity(),
            Self::Exec(id) => id.descriptor().generated_identity(),
            Self::Job(id) => id.descriptor().generated_identity(),
            Self::Output(id) => id.descriptor().generated_identity(),
        }
    }

    #[must_use]
    pub const fn outcome(self) -> JclParameterOutcome {
        match self {
            Self::Dd(_) => JclParameterOutcome::AllocationAttribute,
            Self::Exec(_) => JclParameterOutcome::StepAttribute,
            Self::Job(_) => JclParameterOutcome::JobAttribute,
            Self::Output(_) => JclParameterOutcome::OutputAttribute,
        }
    }

    #[must_use]
    pub fn validation(self) -> JclValueShape {
        match self {
            Self::Dd(id) => id.descriptor().validation,
            Self::Exec(id) => id.descriptor().validation,
            Self::Job(id) => id.descriptor().validation,
            Self::Output(id) => id.descriptor().validation,
        }
    }

    #[must_use]
    pub fn minimum(self) -> Option<u64> {
        match self {
            Self::Dd(id) => id.descriptor().minimum,
            Self::Exec(id) => id.descriptor().minimum,
            Self::Job(id) => id.descriptor().minimum,
            Self::Output(id) => id.descriptor().minimum,
        }
    }

    #[must_use]
    pub fn maximum(self) -> Option<u64> {
        match self {
            Self::Dd(id) => id.descriptor().maximum,
            Self::Exec(id) => id.descriptor().maximum,
            Self::Job(id) => id.descriptor().maximum,
            Self::Output(id) => id.descriptor().maximum,
        }
    }

    #[must_use]
    pub fn choices(self) -> &'static [&'static str] {
        match self {
            Self::Dd(id) => id.descriptor().choices,
            Self::Exec(id) => id.descriptor().choices,
            Self::Job(id) => id.descriptor().choices,
            Self::Output(id) => id.descriptor().choices,
        }
    }

    #[must_use]
    pub fn support(self) -> JclCatalogSupport {
        match self {
            Self::Dd(id) => id.descriptor().support,
            Self::Exec(id) => id.descriptor().support,
            Self::Job(id) => id.descriptor().support,
            Self::Output(id) => id.descriptor().support,
        }
    }

    #[must_use]
    pub fn sensitive(self) -> bool {
        match self {
            Self::Dd(id) => id.descriptor().sensitive,
            Self::Exec(id) => id.descriptor().sensitive,
            Self::Job(id) => id.descriptor().sensitive,
            Self::Output(id) => id.descriptor().sensitive,
        }
    }

    #[must_use]
    pub fn capability(self) -> &'static str {
        match self {
            Self::Dd(id) => id.descriptor().capability,
            Self::Exec(id) => id.descriptor().capability,
            Self::Job(id) => id.descriptor().capability,
            Self::Output(id) => id.descriptor().capability,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclParsedParameter {
    identity: JclParameterIdentity,
    source_keyword: String,
    raw_value: String,
    positional: bool,
}

impl JclParsedParameter {
    #[must_use]
    pub const fn identity(&self) -> JclParameterIdentity {
        self.identity
    }

    #[must_use]
    pub fn source_keyword(&self) -> &str {
        &self.source_keyword
    }

    #[must_use]
    pub fn raw_value(&self) -> &str {
        &self.raw_value
    }

    #[must_use]
    pub const fn positional(&self) -> bool {
        self.positional
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclParsedStatement {
    ordinal: u32,
    identity: JclStatementId,
    source_operation: String,
    name: Option<String>,
    operands: String,
    parameters: Vec<JclParsedParameter>,
    inline_data: Vec<u8>,
    sources: Vec<SourceRange>,
    line: usize,
    end_line: usize,
}

impl JclParsedStatement {
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[must_use]
    pub const fn identity(&self) -> JclStatementId {
        self.identity
    }

    #[must_use]
    pub fn generated_identity(&self) -> JclGeneratedIdentity {
        self.identity.descriptor().generated_identity()
    }

    #[must_use]
    pub fn source_operation(&self) -> &str {
        &self.source_operation
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn operands(&self) -> &str {
        &self.operands
    }

    #[must_use]
    pub fn parameters(&self) -> &[JclParsedParameter] {
        &self.parameters
    }

    #[must_use]
    pub fn inline_data(&self) -> &[u8] {
        &self.inline_data
    }

    #[must_use]
    pub fn sources(&self) -> &[SourceRange] {
        &self.sources
    }

    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub const fn end_line(&self) -> usize {
        self.end_line
    }
}

#[derive(Clone, Debug)]
pub struct JclStatementAnalysis {
    statements: Vec<JclParsedStatement>,
    diagnostics: Vec<Diagnostic>,
}

impl JclStatementAnalysis {
    #[must_use]
    pub fn statements(&self) -> &[JclParsedStatement] {
        &self.statements
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Builds typed statement and parameter views over the lossless source tree.
/// Unknown forms recover at the next physical record and never become plan
/// nodes.
#[must_use]
pub fn parse_jcl_statements(syntax: &JclSyntaxAnalysis) -> JclStatementAnalysis {
    let text = syntax.syntax().text();
    let records = syntax.syntax().records();
    let mut diagnostics = syntax.diagnostics().to_vec();
    let mut statements = Vec::new();
    let mut index = 0usize;
    let mut ordinal = 0u32;
    while index < records.len() {
        let record = &records[index];
        let simple = match record.kind() {
            JclRecordKind::Comment => Some((JclStatementId::Comment, None, String::new())),
            JclRecordKind::Null => Some((JclStatementId::Null, None, String::new())),
            JclRecordKind::Delimiter => Some((JclStatementId::Delimiter, None, String::new())),
            _ => None,
        };
        if let Some((identity, name, operands)) = simple {
            ordinal += 1;
            statements.push(JclParsedStatement {
                ordinal,
                identity,
                source_operation: identity.descriptor().keyword.into(),
                name,
                operands,
                parameters: Vec::new(),
                inline_data: Vec::new(),
                sources: vec![record.span().clone()],
                line: record.line(),
                end_line: record.line(),
            });
            index += 1;
            continue;
        }
        if record.kind() != JclRecordKind::Statement {
            index += 1;
            continue;
        }
        let Some(fields) = record.fields() else {
            index += 1;
            continue;
        };
        let name = source_field(text, fields.name());
        let operation = source_field(text, fields.operation());
        let mut operands = source_field(text, fields.operands()).trim_end().to_string();
        let line = record.line();
        let mut end_line = line;
        let mut sources = vec![record.span().clone()];
        index += 1;
        while index < records.len() && records[index].kind() == JclRecordKind::Continuation {
            let continuation = &records[index];
            let continued = source_field(text, continuation.content_bytes())
                .strip_prefix("//")
                .unwrap_or_default()
                .trim();
            if operands.ends_with(',') {
                operands.push_str(continued);
            } else {
                operands.push(' ');
                operands.push_str(continued);
            }
            sources.push(continuation.span().clone());
            end_line = continuation.line();
            index += 1;
        }
        let Some(identity) = statement_identity(name, operation) else {
            diagnostics.push(statement_diagnostic(
                "MEJCL0720",
                "JCL statement form is not present in the pinned 20-form catalog",
                record.span(),
            ));
            continue;
        };
        let parsed_name = if name.is_empty() && identity == JclStatementId::Dd {
            Some("*".into())
        } else {
            (!name.is_empty()).then(|| name.to_ascii_uppercase())
        };
        let mut parameters = Vec::new();
        if let Err(message) = parse_parameters(identity, &operands, &mut parameters) {
            diagnostics.push(statement_diagnostic("MEJCL0721", &message, record.span()));
        }
        let mut inline_data = Vec::new();
        if matches!(identity, JclStatementId::Dd | JclStatementId::Cntl) {
            while index < records.len() && records[index].kind() == JclRecordKind::InStreamData {
                let data = &records[index];
                inline_data.extend_from_slice(source_field(text, data.content_bytes()).as_bytes());
                inline_data
                    .extend_from_slice(source_field(text, data.terminator_bytes()).as_bytes());
                sources.push(data.span().clone());
                end_line = data.line();
                index += 1;
            }
        }
        ordinal += 1;
        statements.push(JclParsedStatement {
            ordinal,
            identity,
            source_operation: if operation.is_empty() {
                identity.descriptor().keyword.into()
            } else {
                operation.to_ascii_uppercase()
            },
            name: parsed_name,
            operands,
            parameters,
            inline_data,
            sources,
            line,
            end_line,
        });
    }
    JclStatementAnalysis {
        statements,
        diagnostics,
    }
}

pub(crate) fn parse_effective_parameters(
    statement: JclStatementId,
    operands: &str,
) -> Result<Vec<JclParsedParameter>, String> {
    let mut output = Vec::new();
    parse_parameters(statement, operands, &mut output)?;
    Ok(output)
}

fn statement_identity(name: &str, operation: &str) -> Option<JclStatementId> {
    if operation.is_empty() && !name.is_empty() {
        Some(JclStatementId::JclCommand)
    } else {
        JclStatementId::from_keyword(operation)
    }
}

fn parse_parameters(
    statement: JclStatementId,
    operands: &str,
    output: &mut Vec<JclParsedParameter>,
) -> Result<(), String> {
    if !matches!(
        statement,
        JclStatementId::Dd | JclStatementId::Exec | JclStatementId::Job | JclStatementId::Output
    ) {
        return Ok(());
    }
    let values = top_level_operands(operands)?;
    let procedure_exec = statement == JclStatementId::Exec
        && values.iter().any(|value| {
            let value = value.trim();
            !value.is_empty()
                && (!value.contains('=') || value.to_ascii_uppercase().starts_with("PROC="))
        });
    let mut positional = 0usize;
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            positional += 1;
            continue;
        }
        let (source_keyword, raw_value, is_positional) =
            if let Some((keyword, value)) = value.split_once('=') {
                (
                    keyword.trim().to_ascii_uppercase(),
                    value.trim().to_string(),
                    false,
                )
            } else {
                (String::new(), value.to_string(), true)
            };
        let identity = match statement {
            JclStatementId::Dd => {
                if is_positional {
                    let keyword = raw_value.to_ascii_uppercase();
                    match keyword.as_str() {
                        "*" => Some(JclParameterIdentity::Dd(DdParameterId::Asterisk)),
                        "DATA" => Some(JclParameterIdentity::Dd(DdParameterId::Data)),
                        "DUMMY" => Some(JclParameterIdentity::Dd(DdParameterId::Dummy)),
                        _ => None,
                    }
                } else {
                    DdParameterId::from_keyword(&source_keyword).map(JclParameterIdentity::Dd)
                }
            }
            JclStatementId::Exec => {
                if is_positional
                    || (procedure_exec && ExecParameterId::from_keyword(&source_keyword).is_none())
                {
                    Some(JclParameterIdentity::Exec(
                        ExecParameterId::ProcAndProcedureName,
                    ))
                } else {
                    ExecParameterId::from_keyword(&source_keyword).map(JclParameterIdentity::Exec)
                }
            }
            JclStatementId::Job => {
                if is_positional {
                    match positional {
                        0 => Some(JclParameterIdentity::Job(
                            JobParameterId::AccountingInformation,
                        )),
                        1 => Some(JclParameterIdentity::Job(JobParameterId::ProgrammersName)),
                        _ => None,
                    }
                } else {
                    JobParameterId::from_keyword(&source_keyword).map(JclParameterIdentity::Job)
                }
            }
            JclStatementId::Output => (!is_positional)
                .then(|| OutputParameterId::from_keyword(&source_keyword))
                .flatten()
                .map(JclParameterIdentity::Output),
            _ => unreachable!("parameter statements were filtered"),
        }
        .ok_or_else(|| {
            format!(
                "operand {value:?} is not in the generated {} parameter catalog",
                statement.descriptor().keyword
            )
        })?;
        output.push(JclParsedParameter {
            identity,
            source_keyword: if is_positional {
                identity.generated().keyword().to_string()
            } else {
                source_keyword
            },
            raw_value,
            positional: is_positional,
        });
        positional += usize::from(is_positional);
    }
    Ok(())
}

fn top_level_operands(value: &str) -> Result<Vec<&str>, String> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let bytes = value.as_bytes();
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() && depth > 0 => depth -= 1,
            b')' if quote.is_none() => return Err("operand has an unmatched ')'".into()),
            b',' if quote.is_none() && depth == 0 => {
                values.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return Err("operand has an unterminated quoted string".into());
    }
    if depth != 0 {
        return Err("operand has an unterminated parenthesized value".into());
    }
    values.push(&value[start..]);
    Ok(values)
}

fn source_field<'a>(text: &'a str, range: &std::ops::Range<usize>) -> &'a str {
    text.get(range.clone()).unwrap_or_default()
}

fn statement_diagnostic(code: &str, message: &str, source: &SourceRange) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).expect("static JCL diagnostic code"),
        Severity::Error,
        Phase::Parse,
        FailureCategory::MalformedInput,
        Completeness::Incomplete,
        message,
        Some(SourceSpan::new(source.file, source.bytes.clone()).expect("validated source range")),
        Redaction::Public,
        DiagnosticLimits::default(),
    )
    .expect("bounded JCL statement diagnostic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DD_PARAMETERS, EXEC_PARAMETERS, JCL_STATEMENTS, JOB_PARAMETERS, JclBundle, JclSyntaxLimits,
        OUTPUT_PARAMETERS, analyze_jcl_syntax,
    };
    use std::collections::BTreeSet;

    fn parse(source: &str) -> JclStatementAnalysis {
        let syntax = analyze_jcl_syntax(
            &JclBundle {
                primary: source.into(),
                ..JclBundle::default()
            },
            JclSyntaxLimits::default(),
        )
        .unwrap();
        parse_jcl_statements(&syntax)
    }

    #[test]
    fn recognizes_all_twenty_statement_catalog_identities() {
        let fixtures = [
            "//DISPLAY",
            "//C COMMAND 'D A,L'",
            "//* comment",
            "//C CNTL\n//E ENDCNTL",
            "//D DD DUMMY",
            "/*",
            "//E ENDCNTL",
            "//S EXEC PGM=IEFBR14",
            "// EXPORT SYMLIST=(A)",
            "// IF (RC=0) THEN",
            "//I INCLUDE MEMBER=A",
            "//L JCLLIB ORDER=A",
            "//J JOB CLASS=A",
            "//",
            "//O OUTPUT CLASS=A",
            "// PEND",
            "//P PROC A=B",
            "//S SCHEDULE HOLD",
            "// SET A=B",
            "//X XMIT DEST",
        ];
        let mut recognized = BTreeSet::new();
        for fixture in fixtures {
            let analysis = parse(&format!("{fixture}\n"));
            assert!(
                analysis.diagnostics().is_empty(),
                "fixture failed: {fixture}"
            );
            recognized.insert(analysis.statements()[0].identity());
        }
        assert_eq!(recognized.len(), JCL_STATEMENTS.len(), "{recognized:?}");
    }

    #[test]
    fn generated_parameter_families_are_selected_without_operand_loss() {
        let analysis = parse(
            "//J JOB (A),'PROGRAMMER',CLASS=A,PRTY=3\n//S EXEC PGM=IEFBR14,PARM='A,B'\n//D DD DSNAME=U.DATA,DISP=(NEW,CATLG),RECFM=FB\n//O OUTPUT CLASS=A,DEST=LOCAL\n",
        );
        assert!(analysis.is_complete(), "{:?}", analysis.diagnostics());
        assert_eq!(analysis.statements()[0].parameters().len(), 4);
        assert_eq!(analysis.statements()[1].parameters().len(), 2);
        assert_eq!(analysis.statements()[2].parameters().len(), 3);
        assert_eq!(analysis.statements()[3].parameters().len(), 2);
    }

    #[test]
    fn continuation_and_inline_bytes_remain_attached_to_the_dd_statement() {
        let analysis = parse(
            "//J JOB CLASS=A\n//S EXEC PGM=IEBGENER\n//IN DD DATA,\n//             DLM=@@\nONE\r\nTWO\n@@\n",
        );
        assert!(analysis.is_complete(), "{:?}", analysis.diagnostics());
        let dd = &analysis.statements()[2];
        assert_eq!(dd.end_line(), 6);
        assert_eq!(dd.inline_data(), b"ONE\r\nTWO\n");
        assert_eq!(dd.sources().len(), 4);
    }

    #[test]
    fn unknown_statement_and_parameter_recover_at_following_records() {
        let analysis = parse("//J JOB CLASS=A,NOPE=X\n//B BOGUS X\n//S EXEC PGM=IEFBR14\n");
        assert_eq!(analysis.diagnostics().len(), 2);
        assert_eq!(analysis.statements().len(), 2);
        assert_eq!(analysis.statements()[1].identity(), JclStatementId::Exec);
    }

    #[test]
    fn cntl_data_is_lossless_until_endcntl_even_when_it_looks_like_jcl() {
        let analysis = parse(
            "//C CNTL\nCONTROL ONE\n//NOT JCL INSIDE CONTROL DATA\n//E ENDCNTL\n//J JOB CLASS=A\n",
        );
        assert!(analysis.is_complete(), "{:?}", analysis.diagnostics());
        assert_eq!(analysis.statements()[0].identity(), JclStatementId::Cntl);
        assert_eq!(
            analysis.statements()[0].inline_data(),
            b"CONTROL ONE\n//NOT JCL INSIDE CONTROL DATA\n"
        );
        assert_eq!(analysis.statements()[1].identity(), JclStatementId::Endcntl);
        assert_eq!(analysis.statements()[2].identity(), JclStatementId::Job);
    }

    #[test]
    fn every_generated_parameter_descriptor_has_a_planner_outcome() {
        for identity in DD_PARAMETERS
            .iter()
            .map(|entry| JclParameterIdentity::Dd(entry.id))
            .chain(
                EXEC_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Exec(entry.id)),
            )
            .chain(
                JOB_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Job(entry.id)),
            )
            .chain(
                OUTPUT_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Output(entry.id)),
            )
        {
            assert!(matches!(
                identity.outcome(),
                JclParameterOutcome::AllocationAttribute
                    | JclParameterOutcome::StepAttribute
                    | JclParameterOutcome::JobAttribute
                    | JclParameterOutcome::OutputAttribute
            ));
        }
    }
}
