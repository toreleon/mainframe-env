//! Source-bound J18 preparation, not a substituted DFSRRC00 controller or journey receipt.
use super::*;
use mainframe_env_host_api::{ImsDatabasePcbMetadata, ImsPcbKind, ImsPsbMetadata};

#[path = "ims_firstparty_purge_scenario.rs"]
mod scenario;

pub(super) const LABELS: [&str; 6] = [
    "BMP scheduling",
    "GN/GNP/DLET",
    "CHKP",
    "available-credit adjustment",
    "restart from checkpoint",
    "exact remaining hierarchy",
];
const PREFIX: &str = "app/app-authorization-ims-db2-mq";
const PROGRAM: &str = "CBPAUP0C";
const PRIMARY: &str = "app/app-authorization-ims-db2-mq/cbl/CBPAUP0C.cbl";
const PINS: [(&str, &str); 6] = [
    (
        PRIMARY,
        "309468a5c4745f92c6dcf6ed28b396334b2f131ced78eb43eda962a63d2b174e",
    ),
    (
        "app/app-authorization-ims-db2-mq/cpy/CIPAUSMY.cpy",
        "bb6eea3270e29ba465147652faea25923588ff3be57a4fc800fbc4b7a26fff29",
    ),
    (
        "app/app-authorization-ims-db2-mq/cpy/CIPAUDTY.cpy",
        "5d67c37c828dc2278339cdd1902a14305799a773d999a329e30c0801e9c80077",
    ),
    (
        "app/app-authorization-ims-db2-mq/jcl/CBPAUP0J.jcl",
        "99e84914708d330fd3b8d19ce8b731ed5acd9b4e0dbdc16af8823802a01b68e4",
    ),
    (
        "app/app-authorization-ims-db2-mq/ims/PSBPAUTB.psb",
        "d88a628f8d2c9491e9c7a2867235400411c4e1826150af8334a07b1a033d0210",
    ),
    (
        "app/app-authorization-ims-db2-mq/ims/DBPAUTP0.dbd",
        "ff82e8b3c1ab201db212047fdb271889e809d90f4fc47c68202c17572d485796",
    ),
];

fn problem(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.ims.firstparty_purge_preparation", detail)
}
fn need(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem(detail)) }
}

fn validate_sources(files: &BTreeMap<String, Vec<u8>>) -> Result<(), CorpusProblem> {
    need(files.len() == PINS.len(), "exact source closure required")?;
    for (path, pin) in PINS {
        let bytes = files.get(path).ok_or_else(|| problem("source missing"))?;
        need(
            format!("{:x}", Sha256::digest(bytes)) == pin,
            "source bytes differ",
        )?;
    }
    Ok(())
}

/// Compiles the original first-party body. This value has no scheduling or
/// observation authority; current DFSRRC00 selects a different Rust controller.
struct Prepared {
    program: BatchProgramDefinition,
    bundle_id: String,
    jcl: String,
}

fn prepare(corpus: &Path) -> Result<Prepared, CorpusProblem> {
    let files = PINS
        .iter()
        .map(|(path, _)| {
            Ok((
                (*path).to_string(),
                read_corpus_file(corpus, &corpus.join(path))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    validate_sources(&files)?;
    let limits = SourceLimits::default();
    let members = [
        format!("{PREFIX}/cpy/CIPAUSMY.cpy"),
        format!("{PREFIX}/cpy/CIPAUDTY.cpy"),
    ];
    let source_files = std::iter::once(PRIMARY)
        .chain(members.iter().map(String::as_str))
        .map(|path| {
            SourceFile::input(
                path,
                files[path].clone(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .map_err(package_problem)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let library = SourceLibrary::new(
        "j18-pinned-application-copies",
        members
            .iter()
            .map(|path| LogicalPath::new(path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(package_problem)?,
        limits,
    )
    .map_err(package_problem)?;
    let primary = LogicalPath::new(PRIMARY, limits.max_path_bytes).map_err(package_problem)?;
    let bundle = SourceBundle::with_libraries(
        &primary,
        source_files,
        vec![library],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .map_err(package_problem)?;
    let bundle_id = bundle.id().to_hex();
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(package_problem)?,
            options: CompileOptions::new(BTreeMap::new()).map_err(package_problem)?,
        })
        .map_err(package_problem)?;
    let CompilerResult::Published { artifact, .. } = result else {
        return Err(problem(
            "original CBPAUP0C did not compile; no substituted body",
        ));
    };
    let jcl = String::from_utf8(files[&format!("{PREFIX}/jcl/CBPAUP0J.jcl")].clone())
        .map_err(package_problem)?;
    Ok(Prepared {
        program: BatchProgramDefinition::current(PROGRAM, &artifact),
        bundle_id,
        jcl,
    })
}

fn compare_original_plan(
    prepared: &Prepared,
    plan: &mainframe_env_batch::JobPlan,
) -> Result<(), CorpusProblem> {
    let original = mainframe_env_batch::parse_jcl(
        &mainframe_env_batch::JclBundle {
            primary: prepared.jcl.clone(),
            ..Default::default()
        },
        mainframe_env_batch::JclLimits::default(),
    )
    .map_err(package_problem)?;
    need(
        *plan == original,
        "complete parsed plan differs from the pinned JCL",
    )?;
    need(
        plan.source == prepared.jcl
            && plan.name == "CBPAUP0J"
            && plan.restart_step.is_none()
            && plan.steps.len() == 1,
        "original whole JCL/step required",
    )?;
    let step = &plan.steps[0];
    let parameter = step
        .parameter
        .as_deref()
        .unwrap_or_default()
        .trim_matches('\'');
    let controls = step
        .dds
        .iter()
        .filter(|dd| dd.name == "SYSIN")
        .collect::<Vec<_>>();
    need(
        step.name == "STEP01"
            && step.program == "DFSRRC00"
            && parameter == "BMP,CBPAUP0C,PSBPAUTB"
            && matches!(controls.as_slice(), [dd] if dd.inline_data == b"00,00001,00001,Y\n"),
        "BMP program/PSB/SYSIN must not be replaced",
    )
}

/// Declaration precheck only, not proof of a runtime allocation. In the current
/// catalog this deliberately refuses: no metadata variant can supply the IO PCB.
/// Never replace source PCB2 by database PCB1 to get through that boundary.
fn require_bmp_declarations(
    psb: &ImsPsbMetadata,
) -> Result<&ImsDatabasePcbMetadata, CorpusProblem> {
    need(
        psb.name == "PSBPAUTB"
            && psb.database_level == mainframe_env_host_api::ImsDbLevel::Current
            && psb.pcbs.len() == 2
            && psb.pcbs[0].host_kind() == ImsPcbKind::Io,
        "exact BMP IO-PCB1/database-PCB2 declaration unavailable",
    )?;
    let mainframe_env_host_api::ImsPcbMetadata::Database(database) = &psb.pcbs[1] else {
        return Err(problem("PCB2 is not the source database PCB"));
    };
    need(
        database.name == "PAUTBPCB"
            && database.database == "DBPAUTP0"
            && database.database_version == Some(1)
            && database.processing_options == "AP"
            && database.secondary_index.is_none()
            && database
                .sensitive_segments
                .iter()
                .map(|row| row.name.as_str())
                .eq(["PAUTSUM0", "PAUTDTL1"])
            && database.sensitive_segments[0].parent.is_none()
            && database.sensitive_segments[1].parent.as_deref() == Some("PAUTSUM0")
            && database
                .sensitive_segments
                .iter()
                .all(|row| row.processing_options.is_none()),
        "wrong selected database PCB2",
    )?;
    Ok(database)
}

#[cfg(test)]
#[path = "ims_firstparty_purge_tests.rs"]
mod tests;
