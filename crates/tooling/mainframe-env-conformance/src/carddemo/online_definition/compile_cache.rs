//! Test-only immutable compilation memo; no service, oracle or installation state.
use super::{
    CobolCompiler, CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CorpusProblem,
    OnlineProgramDefinition, SemanticModel, SourceBundle,
};
use mainframe_env_compiler::{
    COBOL_COMPILER_GENERATION, COBOL_HIR_DIALECTS, CORE_MIR_DIALECT, CobolCompilerLimits,
    cobol_hir_catalog, core_mir_catalog, core_mir_profile,
};
use mainframe_env_compiler_api::{ARTIFACT_CONTRACT, COMPILER_CONTRACT};
use mainframe_env_ir::{IR_ENVELOPE_CONTRACT, LegalityProfile, OperationCatalog};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

// A full cache falls through to actual compilation. It never refuses a valid input.
const MAX_ENTRIES: usize = 16;
type Compiled = (OnlineProgramDefinition, SemanticModel);

#[derive(Clone, Eq, PartialEq)]
struct Key {
    name: String,
    primary: String,
    request: CompilerRequest,
    limits: CobolCompilerLimits,
    hir_catalog: OperationCatalog,
    core_catalog: OperationCatalog,
    profile: LegalityProfile,
    compiler_generation: &'static str,
    compiler_contract: &'static str,
    artifact_contract: &'static str,
    ir_contract: &'static str,
    hir_dialects: Vec<&'static str>,
    core_dialect: &'static str,
}

impl Key {
    fn selected(
        compiler: &CobolCompiler,
        name: &str,
        primary: &str,
        bundle: &SourceBundle,
    ) -> Result<Self, CorpusProblem> {
        Ok(Self {
            name: name.into(),
            primary: primary.into(),
            request: CompilerRequest {
                source: bundle.clone(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").map_err(|error| {
                    CorpusProblem::new("carddemo.online.target_invalid", error.to_string())
                })?,
                options: CompileOptions::new(BTreeMap::new()).map_err(|error| {
                    CorpusProblem::new("carddemo.online.options_invalid", error.to_string())
                })?,
            },
            limits: compiler.limits(),
            hir_catalog: cobol_hir_catalog(),
            core_catalog: core_mir_catalog(),
            profile: core_mir_profile(),
            compiler_generation: COBOL_COMPILER_GENERATION,
            compiler_contract: COMPILER_CONTRACT,
            artifact_contract: ARTIFACT_CONTRACT,
            ir_contract: IR_ENVELOPE_CONTRACT,
            hir_dialects: COBOL_HIR_DIALECTS.to_vec(),
            core_dialect: CORE_MIR_DIALECT,
        })
    }
}

struct Memo<T> {
    entries: Vec<(Key, T)>,
}

impl<T: Clone> Memo<T> {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get_or_compile<E>(
        &mut self,
        key: &Key,
        compile: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        if let Some((_, value)) = self.entries.iter().find(|(stored, _)| stored == key) {
            return Ok(value.clone());
        }
        // This is the unchanged actual compiler owner; only successful owned results survive.
        let value = compile()?;
        if self.entries.len() < MAX_ENTRIES {
            self.entries.push((key.clone(), value.clone()));
        }
        Ok(value)
    }
}

pub(super) fn compile(
    compiler: &CobolCompiler,
    name: &str,
    primary: &str,
    bundle: &SourceBundle,
) -> Result<Compiled, CorpusProblem> {
    static CACHE: OnceLock<Mutex<Memo<Compiled>>> = OnceLock::new();
    let key = Key::selected(compiler, name, primary, bundle)?;
    // Serialize only pure compilation. No task, provider, artifact store or oracle enters this lock.
    let mut memo = CACHE
        .get_or_init(|| Mutex::new(Memo::new()))
        .lock()
        .map_err(|_| {
            CorpusProblem::new(
                "carddemo.fixture.compile_cache",
                "compilation cache lock poisoned",
            )
        })?;
    memo.get_or_compile(&key, || {
        super::compile_online_program(compiler, name, primary, bundle)
    })
}

#[path = "compile_cache_tests.rs"]
mod tests;
