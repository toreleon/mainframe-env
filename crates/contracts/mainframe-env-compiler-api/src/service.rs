use crate::{PublishedArtifact, VerifiedHir};
use mainframe_env_diagnostics::{Completeness, Diagnostic, DiagnosticProblem};
use mainframe_env_source::SourceBundle;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationMode {
    Analyze,
    Executable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileTarget(String);

impl CompileTarget {
    pub fn new(value: impl Into<String>) -> Result<Self, CompilerProblem> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(CompilerProblem::InvalidTarget);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileOptions {
    values: BTreeMap<String, String>,
}

impl CompileOptions {
    pub fn new(values: BTreeMap<String, String>) -> Result<Self, CompilerProblem> {
        if values.len() > 128
            || values
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 256 || value.len() > 1024)
        {
            return Err(CompilerProblem::OptionsLimitExceeded);
        }
        Ok(Self { values })
    }

    #[must_use]
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRequest {
    pub source: SourceBundle,
    pub mode: CompilationMode,
    pub target: CompileTarget,
    pub options: CompileOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerResult {
    Analysis {
        hir: Option<VerifiedHir>,
        diagnostics: Vec<Diagnostic>,
        completeness: Completeness,
    },
    Published {
        artifact: PublishedArtifact,
        diagnostics: Vec<Diagnostic>,
    },
    Failed {
        diagnostics: Vec<Diagnostic>,
        completeness: Completeness,
    },
}

pub trait CompilerService: Send + Sync {
    fn compile(&self, request: CompilerRequest) -> Result<CompilerResult, CompilerProblem>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerProblem {
    InvalidTarget,
    OptionsLimitExceeded,
    TooManyDiagnostics,
    IncompleteStage,
    Verification(String),
    Legality(String),
    ArtifactLimitExceeded,
    InvalidGeneration,
    Diagnostic(DiagnosticProblem),
}

impl fmt::Display for CompilerProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "compiler contract failed: {self:?}")
    }
}

impl std::error::Error for CompilerProblem {}
