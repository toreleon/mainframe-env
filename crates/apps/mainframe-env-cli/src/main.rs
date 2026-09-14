use clap::{Parser, Subcommand, ValueEnum};
use mainframe_env_cics::cics_abi_library;
use mainframe_env_compiler::{CobolCompiler, CobolCompilerLimits};
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_db2::db2_abi_library;
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_mq::mq_abi_library;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits, materialize_host_abi_libraries,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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
        #[arg(long = "library", value_name = "DIRECTORY")]
        libraries: Vec<PathBuf>,
    },
    Run {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t=Format::Free)]
        format: Format,
        #[arg(long = "library", value_name = "DIRECTORY")]
        libraries: Vec<PathBuf>,
    },
    Inspect {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t=Format::Free)]
        format: Format,
        #[arg(long = "library", value_name = "DIRECTORY")]
        libraries: Vec<PathBuf>,
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
        Command::Compile {
            source,
            format,
            libraries,
        } => {
            let result = compile(&source, format, &libraries, CompilationMode::Executable)?;
            match result {
                CompilerResult::Published {
                    artifact,
                    diagnostics,
                } => {
                    println!(
                        "artifact {} semantic={} bytes={} diagnostics={}",
                        artifact.content_id().to_reference(),
                        artifact.semantic_id().to_reference(),
                        artifact.payload().len(),
                        diagnostics.len()
                    );
                    Ok(())
                }
                other => Err(format!("compilation did not publish: {other:?}")),
            }
        }
        Command::Inspect {
            source,
            format,
            libraries,
        } => {
            let bundle = bundle(&source, format, &libraries)?;
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
        Command::Run {
            source,
            format,
            libraries,
        } => {
            let result = compile(&source, format, &libraries, CompilationMode::Executable)?;
            let CompilerResult::Published { artifact, .. } = result else {
                return Err(format!("compilation failed: {result:?}"));
            };
            let limits = InvocationLimits::default();
            let invocation = invocation(
                ArtifactRef::new(artifact.content_id().to_reference(), limits)
                    .map_err(|error| error.to_string())?,
            )?;
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .map_err(|problem| format!("{problem:?}"))?;
            match ExecutionCoordinator::local(CoordinatorLimits::default()).execute(
                &mut machine,
                &invocation,
                ExecutionControl::default(),
            ) {
                ExecutionOutcome::Completed(completion) => {
                    print!("{}", String::from_utf8_lossy(completion.output.bytes()));
                    Ok(())
                }
                ExecutionOutcome::Rejected(problem)
                | ExecutionOutcome::ResourceExhausted(problem)
                | ExecutionOutcome::ProviderFailure(problem)
                | ExecutionOutcome::InfrastructureFailure(problem) => Err(problem.public_message),
                other => Err(format!("execution stopped with {other:?}")),
            }
        }
    }
}

fn compile(
    path: &PathBuf,
    format: Format,
    libraries: &[PathBuf],
    mode: CompilationMode,
) -> Result<CompilerResult, String> {
    let source = bundle(path, format, libraries)?;
    CobolCompiler::new(CobolCompilerLimits::default())
        .compile(CompilerRequest {
            source,
            mode,
            target: CompileTarget::new("reference").map_err(|error| error.to_string())?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|error| error.to_string())?,
        })
        .map_err(|error| error.to_string())
}

fn bundle(
    path: &PathBuf,
    format: Format,
    library_roots: &[PathBuf],
) -> Result<SourceBundle, String> {
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
        source_format(format),
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| error.to_string())?;
    if library_roots.is_empty() {
        return SourceBundle::new(&logical, vec![file], BTreeMap::new(), Vec::new(), limits)
            .map_err(|error| error.to_string());
    }
    let mut files = vec![file];
    let mut libraries = Vec::with_capacity(library_roots.len() + 1);
    for (index, root) in library_roots.iter().enumerate() {
        let mut loaded = Vec::new();
        collect_library_files(root, root, index, format, limits, &mut loaded)?;
        loaded.sort_by(|left, right| left.0.cmp(&right.0));
        let members = loaded
            .iter()
            .map(|(path, _)| LogicalPath::new(path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        libraries.push(
            SourceLibrary::new(format!("cli-{index:03}"), members, limits)
                .map_err(|error| error.to_string())?,
        );
        files.extend(loaded.into_iter().map(|(_, file)| file));
    }
    let abi = materialize_host_abi_libraries(
        &[cics_abi_library(), db2_abi_library(), mq_abi_library()],
        limits,
    )
    .map_err(|error| error.to_string())?;
    files.extend(abi.files);
    libraries.extend(abi.libraries);
    SourceBundle::with_libraries(
        &logical,
        files,
        libraries,
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .map_err(|error| error.to_string())
}

const fn source_format(format: Format) -> SourceFormat {
    match format {
        Format::Fixed => SourceFormat::Fixed,
        Format::Free => SourceFormat::Free,
    }
}

fn collect_library_files(
    root: &Path,
    directory: &Path,
    library_index: usize,
    format: Format,
    limits: SourceLimits,
    output: &mut Vec<(String, SourceFile)>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("source library is unavailable: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("source library is unavailable: {error}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("source library metadata failed: {error}"))?;
        if file_type.is_symlink() {
            return Err("source libraries cannot contain symbolic links".into());
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_library_files(root, &path, library_index, format, limits, output)?;
            continue;
        }
        if !file_type.is_file() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .ok()
            .and_then(Path::to_str)
            .ok_or("source library contains a non-portable path")?
            .replace('\\', "/");
        let logical = format!("library/{library_index:03}/{relative}");
        let bytes =
            fs::read(&path).map_err(|error| format!("source library read failed: {error}"))?;
        let file = SourceFile::input(
            &logical,
            bytes,
            source_format(format),
            SourceEncoding::Utf8,
            limits,
        )
        .map_err(|error| error.to_string())?;
        output.push((logical, file));
        if output.len() > limits.max_files {
            return Err("source library file limit exceeded".into());
        }
    }
    Ok(())
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
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mainframe-env-cli-source-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cli_contract_is_well_formed() {
        Cli::command().debug_assert();
    }

    /// Level-88 condition-names declared on an alphanumeric group item,
    /// before the group's subordinate items, must compile, evaluate in
    /// `IF`, and write correctly in `SET ... TO TRUE`. Mirrors
    /// `app/cbl/COACTUPC.cbl:101-112`'s `WS-EDIT-US-PHONE-NUM-FLGS` group.
    ///
    /// Per IBM Enterprise COBOL 6.5:
    /// - condition-names may be attached to an alphanumeric group
    ///   (`SS6SG3_6.5/lr/ref/rlddeva2.html`);
    /// - `SET ... TO TRUE` stores the condition-name's value under the VALUE
    ///   clause rules (`SS6SG3_6.5/lr/ref/rlpssetd.html`);
    /// - a figurative constant in a VALUE clause takes the length of the
    ///   associated data item (`SS6SG3_6.5/lr/ref/rllancon.html`), so
    ///   `LOW-VALUES` on this 3-byte group fills all 3 bytes.
    #[test]
    fn level88_group_condition_names_evaluate_and_set_to_true_writes_bytes() {
        let fixture = Fixture::new();
        let source = fixture.0.join("LEVEL88G.cbl");
        fs::write(
            &source,
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LEVEL88G.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-GROUP.\n 88 WS-GROUP-INVALID VALUE '000'.\n 88 WS-GROUP-VALID VALUE LOW-VALUES.\n 05 WS-FLAG-A PIC X(01) VALUE '0'.\n 05 WS-FLAG-B PIC X(01) VALUE '0'.\n 05 WS-FLAG-C PIC X(01) VALUE '0'.\nPROCEDURE DIVISION.\nIF WS-GROUP-INVALID DISPLAY 'BEFORE-TRUE' ELSE DISPLAY 'BEFORE-FALSE' END-IF.\nSET WS-GROUP-VALID TO TRUE.\nDISPLAY WS-GROUP.\nIF WS-GROUP-INVALID DISPLAY 'AFTER-TRUE' ELSE DISPLAY 'AFTER-FALSE' END-IF.\nIF WS-GROUP-VALID DISPLAY 'VALID-TRUE' ELSE DISPLAY 'VALID-FALSE' END-IF.\nSTOP RUN.\n",
        )
        .unwrap();
        let result = compile(&source, Format::Free, &[], CompilationMode::Executable).unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("group-level condition-name program did not publish: {result:?}");
        };
        let limits = InvocationLimits::default();
        let request_invocation =
            invocation(ArtifactRef::new(artifact.content_id().to_reference(), limits).unwrap())
                .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            request_invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        match ExecutionCoordinator::local(CoordinatorLimits::default()).execute(
            &mut machine,
            &request_invocation,
            ExecutionControl::default(),
        ) {
            ExecutionOutcome::Completed(completion) => {
                assert_eq!(completion.return_code, 0);
                let mut expected = Vec::new();
                expected.extend_from_slice(b"BEFORE-TRUE\n");
                expected.extend_from_slice(&[0x00, 0x00, 0x00]);
                expected.extend_from_slice(b"\n");
                expected.extend_from_slice(b"AFTER-FALSE\n");
                expected.extend_from_slice(b"VALID-TRUE\n");
                assert_eq!(completion.output.bytes(), expected.as_slice());
            }
            other => panic!("{other:?}"),
        }
    }

    /// `app/cbl/CSUTLDTC.cbl:60-62`: `FEEDBACK-TOKEN-VALUE` puts
    /// hexadecimal-notation level-88 values on a mixed-usage 8-byte group
    /// (a BINARY subgroup plus two PIC X items). Per IBM Enterprise COBOL 6.5
    /// (`SS6SG3_6.5/lr/ref/rllitahx.html`), a hexadecimal-notation literal is
    /// an alphanumeric literal and is valid wherever one is; its decoded
    /// bytes are what `IF` compares and what `SET ... TO TRUE` stores.
    #[test]
    fn level88_hexadecimal_notation_on_a_group_evaluates_and_sets_to_true_writes_bytes() {
        let fixture = Fixture::new();
        let source = fixture.0.join("LEVEL88H.cbl");
        fs::write(
            &source,
            b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LEVEL88H.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 FEEDBACK-CODE.\n 02 FEEDBACK-TOKEN-VALUE.\n 88 FC-INVALID-DATE VALUE X'0000000000000000'.\n 88 FC-INSUFFICIENT-DATA VALUE X'000309CB59C3C5C5'.\n 03 CASE-1-CONDITION-ID.\n 04 SEVERITY PIC S9(4) BINARY VALUE 0.\n 04 MSG-NO PIC S9(4) BINARY VALUE 0.\n 03 CASE-SEV-CTL PIC X VALUE X'00'.\n 03 FACILITY-ID PIC XXX VALUE X'000000'.\nPROCEDURE DIVISION.\nIF FC-INVALID-DATE DISPLAY 'BEFORE-TRUE' ELSE DISPLAY 'BEFORE-FALSE' END-IF.\nSET FC-INSUFFICIENT-DATA TO TRUE.\nDISPLAY FEEDBACK-TOKEN-VALUE.\nIF FC-INVALID-DATE DISPLAY 'AFTER-TRUE' ELSE DISPLAY 'AFTER-FALSE' END-IF.\nIF FC-INSUFFICIENT-DATA DISPLAY 'INSUFFICIENT-TRUE' ELSE DISPLAY 'INSUFFICIENT-FALSE' END-IF.\nSTOP RUN.\n",
        )
        .unwrap();
        let result = compile(&source, Format::Free, &[], CompilationMode::Executable).unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("hexadecimal group-level condition-name program did not publish: {result:?}");
        };
        let limits = InvocationLimits::default();
        let request_invocation =
            invocation(ArtifactRef::new(artifact.content_id().to_reference(), limits).unwrap())
                .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            request_invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        match ExecutionCoordinator::local(CoordinatorLimits::default()).execute(
            &mut machine,
            &request_invocation,
            ExecutionControl::default(),
        ) {
            ExecutionOutcome::Completed(completion) => {
                assert_eq!(completion.return_code, 0);
                let mut expected = Vec::new();
                expected.extend_from_slice(b"BEFORE-TRUE\n");
                expected.extend_from_slice(&[0x00, 0x03, 0x09, 0xCB, 0x59, 0xC3, 0xC5, 0xC5]);
                expected.extend_from_slice(b"\n");
                expected.extend_from_slice(b"AFTER-FALSE\n");
                expected.extend_from_slice(b"INSUFFICIENT-TRUE\n");
                assert_eq!(completion.output.bytes(), expected.as_slice());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cli_builds_ordered_fixed_libraries_with_subsystem_abi_sources() {
        let fixture = Fixture::new();
        let source = fixture.0.join("MAIN.cbl");
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(&source, b"       COPY REC.\n").unwrap();
        fs::write(first.join("REC.cpy"), b"       01 FIRST-VALUE PIC X.\n").unwrap();
        fs::write(second.join("REC.cpy"), b"       01 SECOND-VALUE PIC X.\n").unwrap();
        let bundle = bundle(&source, Format::Fixed, &[first, second]).unwrap();
        assert_eq!(bundle.libraries().len(), 5);
        assert!(
            String::from_utf8_lossy(bundle.resolve_library_member("REC").unwrap().bytes())
                .contains("FIRST-VALUE")
        );
        assert!(bundle.resolve_library_member("DFHAID").is_ok());
    }

    #[test]
    fn cli_rejects_missing_and_ambiguous_library_directories() {
        let fixture = Fixture::new();
        let source = fixture.0.join("MAIN.cbl");
        let library = fixture.0.join("copy");
        fs::create_dir_all(&library).unwrap();
        fs::write(&source, b"       COPY REC.\n").unwrap();
        fs::write(library.join("REC.cpy"), b"A").unwrap();
        fs::write(library.join("REC.copy"), b"B").unwrap();
        assert!(bundle(&source, Format::Fixed, &[library]).is_err());
        assert!(bundle(&source, Format::Fixed, &[fixture.0.join("missing")]).is_err());
    }
}
