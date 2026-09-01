use crate::{JclGeneratedIdentity, JclRecordKind, JclSyntaxAnalysis, Jes2StatementId};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity, SourceSpan,
};
use mainframe_env_source::SourceRange;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclParsedJecl {
    ordinal: u32,
    identity: Jes2StatementId,
    operands: String,
    source: SourceRange,
    line: usize,
}

impl JclParsedJecl {
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[must_use]
    pub const fn identity(&self) -> Jes2StatementId {
        self.identity
    }

    #[must_use]
    pub fn generated_identity(&self) -> JclGeneratedIdentity {
        self.identity.descriptor().generated_identity()
    }

    #[must_use]
    pub fn operands(&self) -> &str {
        &self.operands
    }

    #[must_use]
    pub fn source(&self) -> &SourceRange {
        &self.source
    }

    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclExpandedJecl {
    statement: JclParsedJecl,
    invocation_sites: Vec<SourceRange>,
}

impl JclExpandedJecl {
    pub(crate) fn primary(statement: JclParsedJecl) -> Self {
        Self {
            statement,
            invocation_sites: Vec::new(),
        }
    }

    #[must_use]
    pub fn statement(&self) -> &JclParsedJecl {
        &self.statement
    }

    #[must_use]
    pub fn invocation_sites(&self) -> &[SourceRange] {
        &self.invocation_sites
    }
}

#[derive(Clone, Debug)]
pub struct Jes2StatementAnalysis {
    statements: Vec<JclParsedJecl>,
    diagnostics: Vec<Diagnostic>,
}

impl Jes2StatementAnalysis {
    #[must_use]
    pub fn statements(&self) -> &[JclParsedJecl] {
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

#[must_use]
pub fn parse_jes2_statements(syntax: &JclSyntaxAnalysis) -> Jes2StatementAnalysis {
    let text = syntax.syntax().text();
    let mut statements = Vec::new();
    let mut diagnostics = Vec::new();
    for record in syntax
        .syntax()
        .records()
        .iter()
        .filter(|record| record.kind() == JclRecordKind::Jecl)
    {
        let raw = text.get(record.content_bytes().clone()).unwrap_or_default();
        let content = raw.get(2..raw.len().min(72)).unwrap_or_default().trim_end();
        let (keyword, operands) = if let Some(command) = content.strip_prefix('$') {
            ("$", command.trim_start())
        } else {
            let split = content.find(char::is_whitespace).unwrap_or(content.len());
            (&content[..split], content[split..].trim_start())
        };
        let Some(identity) = Jes2StatementId::from_keyword(keyword) else {
            diagnostics.push(jecl_diagnostic(
                "MEJCL0761",
                "JES2 JECL statement is not in the pinned 13-form catalog",
                record.span(),
            ));
            continue;
        };
        let ordinal = match u32::try_from(statements.len() + 1) {
            Ok(ordinal) => ordinal,
            Err(_) => {
                diagnostics.push(jecl_diagnostic(
                    "MEJCL0762",
                    "JES2 JECL statement count exceeds its identity bound",
                    record.span(),
                ));
                continue;
            }
        };
        statements.push(JclParsedJecl {
            ordinal,
            identity,
            operands: operands.to_string(),
            source: record.span().clone(),
            line: record.line(),
        });
    }
    Jes2StatementAnalysis {
        statements,
        diagnostics,
    }
}

fn jecl_diagnostic(code: &str, message: &str, source: &SourceRange) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).expect("static JECL diagnostic code"),
        Severity::Error,
        Phase::Parse,
        FailureCategory::MalformedInput,
        Completeness::Incomplete,
        message,
        Some(SourceSpan::new(source.file, source.bytes.clone()).expect("validated JECL source")),
        Redaction::Public,
        DiagnosticLimits::default(),
    )
    .expect("bounded JECL diagnostic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JES2_STATEMENTS, JclBundle, JclSyntaxLimits, analyze_jcl_syntax};
    use std::collections::BTreeSet;

    fn analyze(source: &str) -> Jes2StatementAnalysis {
        let syntax = analyze_jcl_syntax(
            &JclBundle {
                primary: source.into(),
                ..JclBundle::default()
            },
            JclSyntaxLimits::default(),
        )
        .unwrap();
        parse_jes2_statements(&syntax)
    }

    #[test]
    fn recognizes_all_thirteen_jes2_jecl_statements() {
        let source = "/*$D A,L\n/*JOBPARM SYSAFF=SYS1\n/*MESSAGE HELLO\n/*NETACCT 1234\n/*NOTIFY USER1\n/*OUTPUT DEST=LOCAL\n/*PRIORITY 10\n/*ROUTE PRINT LOCAL\n/*SETUP VOL1\n/*SIGNOFF\n/*SIGNON 1\n/*XEQ NODE1\n/*XMIT NODE1\n";
        let analysis = analyze(source);
        assert!(analysis.is_complete());
        assert_eq!(analysis.statements().len(), 13);
        assert_eq!(
            analysis
                .statements()
                .iter()
                .map(JclParsedJecl::identity)
                .collect::<BTreeSet<_>>()
                .len(),
            JES2_STATEMENTS.len()
        );
    }

    #[test]
    fn unknown_jecl_recovers_at_the_next_control_record() {
        let analysis = analyze("/*BOGUS X\n/*MESSAGE OK\n");
        assert_eq!(analysis.diagnostics().len(), 1);
        assert_eq!(analysis.statements().len(), 1);
        assert_eq!(
            analysis.statements()[0].identity(),
            Jes2StatementId::Message
        );
    }
}
