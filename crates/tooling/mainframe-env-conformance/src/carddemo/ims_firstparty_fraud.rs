//! Test-only first-party fraud preparation and exact scenario comparisons.
//! No J17 application observation is produced by this diagnostic namespace.
use crate::carddemo::ims_firstparty_readonly;
use crate::carddemo::*;
use mainframe_env_store_api::ArtifactRecord;

#[path = "ims_firstparty_fraud_owner.rs"]
mod owner;
#[path = "ims_firstparty_fraud_reference.rs"]
mod reference;
#[path = "ims_firstparty_fraud_tests.rs"]
mod tests;

const FIRST: &str = "app/app-authorization-ims-db2-mq/cbl/COPAUS1C.cbl";
const SECOND: &str = "app/app-authorization-ims-db2-mq/cbl/COPAUS2C.cbl";
const DCL: &str = "app/app-authorization-ims-db2-mq/dcl/AUTHFRDS.dcl";
const EXTRA_PINS: &[(&str, &str)] = &[
    (
        "app/app-authorization-ims-db2-mq/cbl/COPAUS1C.cbl",
        "27a969cbee69426fa1056053e676041430e99399912f0e27ee1f1a454093c21e",
    ),
    (
        "app/app-authorization-ims-db2-mq/cbl/COPAUS2C.cbl",
        "57232060f8bdaecc679b74e93fcf91d93cffd400326c001bd187ab5fad14789e",
    ),
    (
        "app/app-authorization-ims-db2-mq/dcl/AUTHFRDS.dcl",
        "737c2bb757ef256976ac7658e071d1833fc2ea24d7010dbeff137e6c98fb66a6",
    ),
    (
        "app/app-authorization-ims-db2-mq/ddl/AUTHFRDS.ddl",
        "1634f898360955f32872bd9c35533513e2879135018681b92caa51c787abaeaf",
    ),
    (
        "app/app-authorization-ims-db2-mq/bms/COPAU01.bms",
        "35b86cd8d764c831fa4d696802eef58c0f6c2654c54904569494123e2c93fd94",
    ),
    (
        "app/app-authorization-ims-db2-mq/cpy-bms/COPAU01.cpy",
        "bee60b3bfeb37d5df0d0ff3182f564bd1aa8cb1f36100c4f7ed4fc42108a7937",
    ),
];

fn problem(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.ims.firstparty_fraud_source_diagnostic", detail)
}
fn need(value: bool, detail: &str) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem(detail)) }
}
fn sources(corpus: &Path) -> Result<(), CorpusProblem> {
    ims_firstparty_readonly::sources(corpus)?;
    for (path, expected) in EXTRA_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        need(
            format!("{:x}", Sha256::digest(bytes)) == *expected,
            "fraud source differs",
        )?;
    }
    Ok(())
}

// The generic bundle only enables Db2 for the transaction-type directory.
// Add the original authorization DCL and existing SQL-precompile option only
// to the actual COPAUS2C source, without rewriting that source or SQL text.
fn fraud_bundle(corpus: &Path, original: &SourceBundle) -> Result<SourceBundle, CorpusProblem> {
    let primary = original
        .file(original.primary())
        .ok_or_else(|| problem("unexpected initial fraud bundle"))?
        .path();
    need(
        primary.as_str() == SECOND
            && original.options().is_empty()
            && original.provenance().is_empty(),
        "unexpected initial fraud bundle",
    )?;
    let limits = SourceLimits::default();
    let mut files = original.files().to_vec();
    let dcl = source_file(corpus, DCL, limits)?;
    need(
        !files.iter().any(|file| file.path() == dcl.path()),
        "duplicate DCL",
    )?;
    let logical = dcl.path().clone();
    files.push(dcl);
    let mut libraries = original.libraries().to_vec();
    let library = SourceLibrary::new("authorization-db2-dcl", vec![logical], limits)
        .map_err(|_| problem("authorization DCL library refused"))?;
    libraries.insert(libraries.len().saturating_sub(1), library);
    SourceBundle::with_libraries(
        primary,
        files,
        libraries,
        BTreeMap::from([("cobol.sql-precompile".into(), "true".into())]),
        Vec::new(),
        limits,
    )
    .map_err(|_| problem("authorization source bundle refused"))
}

struct Prepared {
    online: OnlineApplicationDefinition,
    records: BTreeMap<String, ArtifactRecord>,
    source_ids: BTreeMap<String, String>,
}
impl Prepared {
    fn new(corpus: &Path) -> Result<Self, CorpusProblem> {
        sources(corpus)?;
        let (mut online, readonly_record, readonly_source) =
            ims_firstparty_readonly::definition(corpus)?;
        let mut records = BTreeMap::from([("COPAUS0C".into(), readonly_record)]);
        let mut source_ids = BTreeMap::from([("COPAUS0C".into(), readonly_source)]);
        let mut detail_semantic = None;
        for (path, original) in explicit_carddemo_bundles(corpus)?
            .into_iter()
            .filter(|(path, _)| matches!(path.as_str(), FIRST | SECOND))
        {
            let bundle = if path == SECOND {
                fraud_bundle(corpus, &original)?
            } else {
                original
            };
            let program = if path == FIRST {
                "COPAUS1C"
            } else {
                "COPAUS2C"
            };
            need(
                bundle
                    .file(bundle.primary())
                    .is_some_and(|file| file.path().as_str() == path),
                "wrong compiled primary",
            )?;
            let compiler = CobolCompiler::default();
            let analyzed = compiler.analyze(&bundle);
            let semantic = analyzed
                .semantic
                .ok_or_else(|| problem("original fraud source did not analyze"))?;
            if program == "COPAUS1C" {
                detail_semantic = Some(semantic);
            }
            let compiled = compiler
                .compile(CompilerRequest {
                    source: bundle.clone(),
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference")
                        .map_err(|_| problem("compile target"))?,
                    options: CompileOptions::new(BTreeMap::new())
                        .map_err(|_| problem("compile options"))?,
                })
                .map_err(|_| problem("original fraud source compilation refused"))?;
            let CompilerResult::Published { artifact, .. } = compiled else {
                return Err(problem("original fraud source was not published"));
            };
            let definition = OnlineProgramDefinition::current(program, &artifact);
            let record = source_artifact_binding::publisher_record(&definition)?;
            need(
                records.insert(program.into(), record).is_none(),
                "duplicate source program",
            )?;
            need(
                source_ids
                    .insert(program.into(), bundle.id().to_hex())
                    .is_none(),
                "duplicate source id",
            )?;
            online.programs.push(definition);
        }
        need(
            records.len() == 3 && online.programs.len() == 3,
            "fraud pair incomplete",
        )?;
        let semantic = detail_semantic.ok_or_else(|| problem("detail semantic absent"))?;
        let map = bms::carddemo_map(
            corpus,
            "app/app-authorization-ims-db2-mq/bms/COPAU01.bms",
            &[semantic],
        )?;
        need(
            map.mapset == "COPAU01" && map.map == "COPAU1A" && map.fields.len() == 27,
            "detail BMS map differs",
        )?;
        online.maps.push(map);
        let csd = String::from_utf8(read_corpus_file(
            corpus,
            &corpus.join("app/app-authorization-ims-db2-mq/csd/CRDDEMO2.csd"),
        )?)
        .map_err(|_| problem("CSD encoding"))?;
        let cpvd = parse_csd(&csd)
            .map_err(package_problem)?
            .into_iter()
            .filter(|row| row.kind == "TRANSACTION" && row.name == "CPVD")
            .collect::<Vec<_>>();
        need(
            matches!(cpvd.as_slice(), [row] if row.properties.get("PROGRAM").map(String::as_str)
            == Some("COPAUS1C")),
            "CPVD source route differs",
        )?;
        need(
            online
                .transactions
                .insert("CPVD".into(), "COPAUS1C".into())
                .is_none(),
            "duplicate detail transaction",
        )?;
        Ok(Self {
            online,
            records,
            source_ids,
        })
    }
}
