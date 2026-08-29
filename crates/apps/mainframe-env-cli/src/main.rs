use clap::{Parser, Subcommand, ValueEnum};
use mainframe_env_compiler::{CobolCompiler, CobolCompilerLimits};
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Machine, MachineDrive,
    MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mainframe-env",
    version,
    about = "mainframe-env 0.1 local tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Compile {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t=Format::Free)]
        format: Format,
    },
    Run {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t=Format::Free)]
        format: Format,
    },
    Inspect {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t=Format::Free)]
        format: Format,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Fixed,
    Free,
}

fn main() {
    if let Err(problem) = run(Cli::parse()) {
        eprintln!("mainframe-env: {problem}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Compile { source, format } => {
            let result = compile(&source, format, CompilationMode::Executable)?;
            match result {
                CompilerResult::Published {
                    artifact,
                    diagnostics,
                } => {
                    println!(
                        "artifact sha256:{} bytes={} diagnostics={}",
                        artifact.id().to_hex(),
                        artifact.payload().len(),
                        diagnostics.len()
                    );
                    Ok(())
                }
                other => Err(format!("compilation did not publish: {other:?}")),
            }
        }
        Command::Inspect { source, format } => {
            let bundle = bundle(&source, format)?;
            let analysis = CobolCompiler::default().analyze(&bundle);
            if let Some(semantic) = analysis.semantic {
                println!(
                    "program={} storage={} items={}",
                    semantic.program_id,
                    semantic.storage_bytes,
                    semantic.layouts.len()
                );
            }
            if let Some(hir) = analysis.hir {
                println!(
                    "statements={} unsupported={:?}",
                    hir.statements.len(),
                    hir.unsupported()
                );
            }
            for diagnostic in analysis.diagnostics {
                println!("{}: {}", diagnostic.code(), diagnostic.public_message());
            }
            Ok(())
        }
        Command::Run { source, format } => {
            let result = compile(&source, format, CompilationMode::Executable)?;
            let CompilerResult::Published { artifact, .. } = result else {
                return Err(format!("compilation failed: {result:?}"));
            };
            let limits = InvocationLimits::default();
            let invocation = invocation(
                ArtifactRef::new(format!("sha256:{}", artifact.id().to_hex()), limits)
                    .map_err(|error| error.to_string())?,
            )?;
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                invocation,
                CodecLimits::default(),
            )
            .map_err(|problem| format!("{problem:?}"))?;
            let mut resume = MachineResume::Start;
            loop {
                match machine.drive(
                    resume,
                    Quantum::new(10_000, 16 * 1024 * 1024).expect("non-zero quantum"),
                ) {
                    MachineDrive::Continue => resume = MachineResume::Start,
                    MachineDrive::Completed(completion) => {
                        print!("{}", String::from_utf8_lossy(completion.output.bytes()));
                        return Ok(());
                    }
                    MachineDrive::HostCall(effect) => {
                        return Err(format!(
                            "local CLI has no provider for {:?}",
                            effect.request.required_capability(limits)
                        ));
                    }
                    MachineDrive::Failed(problem) => return Err(problem.public_message),
                    other => return Err(format!("execution stopped with {other:?}")),
                }
            }
        }
    }
}

fn compile(
    path: &PathBuf,
    format: Format,
    mode: CompilationMode,
) -> Result<CompilerResult, String> {
    let source = bundle(path, format)?;
    CobolCompiler::new(CobolCompilerLimits::default())
        .compile(CompilerRequest {
            source,
            mode,
            target: CompileTarget::new("reference").map_err(|error| error.to_string())?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())
}

fn bundle(path: &PathBuf, format: Format) -> Result<SourceBundle, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("source path has no portable file name")?;
    let limits = SourceLimits::default();
    let logical =
        LogicalPath::new(name, limits.max_path_bytes).map_err(|error| error.to_string())?;
    let file = SourceFile::input(
        name,
        bytes,
        match format {
            Format::Fixed => SourceFormat::Fixed,
            Format::Free => SourceFormat::Free,
        },
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| error.to_string())?;
    SourceBundle::new(&logical, vec![file], BTreeMap::new(), Vec::new(), limits)
        .map_err(|error| error.to_string())
}

fn invocation(artifact: ArtifactRef) -> Result<Invocation, String> {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("cli-request", limits).map_err(|e| e.to_string())?,
        ExecutionId::new("cli-execution", limits).map_err(|e| e.to_string())?,
        RunUnitId::new("cli-run-unit", limits).map_err(|e| e.to_string())?,
        None,
        Selector::new("program:COBOL:CLI", limits).map_err(|e| e.to_string())?,
        artifact,
        Principal::new(
            PrincipalId::new("CLIUSER", limits).map_err(|e| e.to_string())?,
            BTreeSet::new(),
            limits,
        )
        .map_err(|e| e.to_string())?,
        ServiceClass::Interactive,
        0,
        u64::MAX,
        TraceId::new("cli-trace", limits).map_err(|e| e.to_string())?,
        IdempotencyKey::new("cli-idempotency", limits).map_err(|e| e.to_string())?,
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn cli_contract_is_well_formed() {
        Cli::command().debug_assert();
    }
}
