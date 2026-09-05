use crate::{Completeness, DiagnosticCode, FailureCategory, Phase, Redaction, Severity};
use mainframe_env_source::FileId;
use std::fmt;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticLimits {
    pub max_message_bytes: usize,
    pub max_help_bytes: usize,
    pub max_related: usize,
    pub max_related_message_bytes: usize,
}

impl Default for DiagnosticLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 4096,
            max_help_bytes: 4096,
            max_related: 32,
            max_related_message_bytes: 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub file: FileId,
    pub bytes: Range<usize>,
}

impl SourceSpan {
    pub fn new(file: FileId, bytes: Range<usize>) -> Result<Self, DiagnosticProblem> {
        if bytes.start > bytes.end {
            Err(DiagnosticProblem::InvalidSpan)
        } else {
            Ok(Self { file, bytes })
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelatedSpan {
    pub span: SourceSpan,
    pub message: String,
    pub redaction: Redaction,
}

impl RelatedSpan {
    pub fn new(
        span: SourceSpan,
        message: impl Into<String>,
        redaction: Redaction,
        limits: DiagnosticLimits,
    ) -> Result<Self, DiagnosticProblem> {
        let message = message.into();
        if message.len() > limits.max_related_message_bytes {
            return Err(DiagnosticProblem::MessageTooLarge);
        }
        Ok(Self {
            span,
            message,
            redaction,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    severity: Severity,
    phase: Phase,
    category: FailureCategory,
    completeness: Completeness,
    message: String,
    primary: Option<SourceSpan>,
    related: Vec<RelatedSpan>,
    help: Option<String>,
    redaction: Redaction,
}

impl Diagnostic {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        code: DiagnosticCode,
        severity: Severity,
        phase: Phase,
        category: FailureCategory,
        completeness: Completeness,
        message: impl Into<String>,
        primary: Option<SourceSpan>,
        redaction: Redaction,
        limits: DiagnosticLimits,
    ) -> Result<Self, DiagnosticProblem> {
        let message = message.into();
        if message.is_empty() || message.len() > limits.max_message_bytes {
            return Err(DiagnosticProblem::MessageTooLarge);
        }
        Ok(Self {
            code,
            severity,
            phase,
            category,
            completeness,
            message,
            primary,
            related: Vec::new(),
            help: None,
            redaction,
        })
    }

    pub fn add_related(
        &mut self,
        related: RelatedSpan,
        limits: DiagnosticLimits,
    ) -> Result<(), DiagnosticProblem> {
        if self.related.len() >= limits.max_related {
            return Err(DiagnosticProblem::TooManyRelatedSpans);
        }
        self.related.push(related);
        Ok(())
    }

    pub fn set_help(
        &mut self,
        help: impl Into<String>,
        limits: DiagnosticLimits,
    ) -> Result<(), DiagnosticProblem> {
        let help = help.into();
        if help.len() > limits.max_help_bytes {
            return Err(DiagnosticProblem::HelpTooLarge);
        }
        self.help = Some(help);
        Ok(())
    }

    #[must_use]
    pub fn code(&self) -> &DiagnosticCode {
        &self.code
    }

    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    #[must_use]
    pub const fn category(&self) -> FailureCategory {
        self.category
    }

    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    #[must_use]
    pub fn primary(&self) -> Option<&SourceSpan> {
        self.primary.as_ref()
    }

    #[must_use]
    pub fn related(&self) -> &[RelatedSpan] {
        &self.related
    }

    #[must_use]
    pub fn public_message(&self) -> &str {
        match self.redaction {
            Redaction::Public => &self.message,
            Redaction::Confidential | Redaction::Secret => "[redacted]",
        }
    }

    #[must_use]
    pub fn public_help(&self) -> Option<&str> {
        if self.redaction == Redaction::Public {
            self.help.as_deref()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionProblem {
    pub code: DiagnosticCode,
    pub category: FailureCategory,
    pub phase: Phase,
    pub public_message: String,
    pub retryable: bool,
    pub unknown_outcome: bool,
}

impl ExecutionProblem {
    /// Uncertainty is safety-critical across adapters. Honor either typed
    /// representation, including diagnostics constructed by older callers.
    #[must_use]
    pub fn has_unknown_outcome(&self) -> bool {
        self.unknown_outcome || self.category == FailureCategory::UnknownOutcome
    }

    pub fn new(
        code: DiagnosticCode,
        category: FailureCategory,
        phase: Phase,
        public_message: impl Into<String>,
        retryable: bool,
        unknown_outcome: bool,
        limits: DiagnosticLimits,
    ) -> Result<Self, DiagnosticProblem> {
        let public_message = public_message.into();
        if public_message.is_empty() || public_message.len() > limits.max_message_bytes {
            return Err(DiagnosticProblem::MessageTooLarge);
        }
        if unknown_outcome && category != FailureCategory::UnknownOutcome {
            return Err(DiagnosticProblem::InconsistentFailure);
        }
        Ok(Self {
            code,
            category,
            phase,
            public_message,
            retryable,
            unknown_outcome,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticProblem {
    InvalidCode,
    InvalidSpan,
    MessageTooLarge,
    HelpTooLarge,
    TooManyRelatedSpans,
    InconsistentFailure,
}

impl fmt::Display for DiagnosticProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            match self {
                Self::InvalidCode => "diagnostic code is invalid",
                Self::InvalidSpan => "source span is invalid",
                Self::MessageTooLarge => "diagnostic message is empty or exceeds its bound",
                Self::HelpTooLarge => "diagnostic help exceeds its bound",
                Self::TooManyRelatedSpans => "diagnostic related-span bound exceeded",
                Self::InconsistentFailure => "execution problem flags contradict its category",
            }
        )
    }
}

impl std::error::Error for DiagnosticProblem {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_message_is_redacted() {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new("MESEC0001").unwrap(),
            Severity::Error,
            Phase::Host,
            FailureCategory::Unauthorized,
            Completeness::Failed,
            "token=top-secret",
            None,
            Redaction::Secret,
            DiagnosticLimits::default(),
        )
        .unwrap();
        assert_eq!(diagnostic.public_message(), "[redacted]");
    }

    #[test]
    fn related_spans_are_bounded() {
        let limits = DiagnosticLimits {
            max_related: 0,
            ..DiagnosticLimits::default()
        };
        let mut diagnostic = Diagnostic::new(
            DiagnosticCode::new("MECOB0001").unwrap(),
            Severity::Error,
            Phase::Parse,
            FailureCategory::MalformedInput,
            Completeness::Incomplete,
            "bad token",
            None,
            Redaction::Public,
            limits,
        )
        .unwrap();
        let file = mainframe_env_source::SourceFile::input(
            "main.cbl",
            b"x".to_vec(),
            mainframe_env_source::SourceFormat::Free,
            mainframe_env_source::SourceEncoding::Utf8,
            mainframe_env_source::SourceLimits::default(),
        )
        .unwrap();
        let related = RelatedSpan::new(
            SourceSpan::new(file.id(), 0..1).unwrap(),
            "origin",
            Redaction::Public,
            limits,
        )
        .unwrap();
        assert_eq!(
            diagnostic.add_related(related, limits),
            Err(DiagnosticProblem::TooManyRelatedSpans)
        );
    }

    #[test]
    fn hardening_49_uncertainty_is_detected_from_category_or_flag() {
        let mut problem = ExecutionProblem::new(
            DiagnosticCode::new("MEEXEC0049").unwrap(),
            FailureCategory::UnknownOutcome,
            Phase::Execute,
            "uncertain effect",
            false,
            false,
            DiagnosticLimits::default(),
        )
        .unwrap();
        assert!(problem.has_unknown_outcome());
        problem.category = FailureCategory::ProviderFailure;
        assert!(!problem.has_unknown_outcome());
        problem.unknown_outcome = true;
        assert!(problem.has_unknown_outcome());
    }

    #[test]
    fn invalid_code_is_rejected() {
        assert_eq!(
            DiagnosticCode::new("lower"),
            Err(DiagnosticProblem::InvalidCode)
        );
    }
}
