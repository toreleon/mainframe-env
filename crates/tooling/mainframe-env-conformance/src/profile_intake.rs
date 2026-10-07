//! Read-only, deterministic intake of a pinned external application corpus.

use mainframe_env_application::parse_bms;
use mainframe_env_batch::{
    AmsStatement, JclBundle, JclConversionLimits, JclSyntaxLimits, analyze_jcl_syntax, convert_jcl,
    parse_idcams_control, parse_jcl_statements, utility_disposition,
};
use mainframe_env_compiler::{
    CobolCompiler, CobolCompilerLimits, PROCEDURE_STATEMENTS, procedure_statement_descriptor,
};
use mainframe_env_diagnostics::Diagnostic;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

mod pinned;
use pinned::PinnedFiles;

const NOTICE: &str =
    "A row match records recognition only; it is not an execution or conformance claim.";

#[derive(Debug)]
pub enum IntakeError {
    Manifest(String),
    Pin(String),
    Io(String),
    Schema(String),
}

impl std::fmt::Display for IntakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(s) => write!(f, "manifest: {s}"),
            Self::Pin(s) => write!(f, "pin: {s}"),
            Self::Io(s) => write!(f, "I/O: {s}"),
            Self::Schema(s) => write!(f, "schema: {s}"),
        }
    }
}
impl std::error::Error for IntakeError {}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub schema_version: String,
    pub origin: String,
    pub commit: String,
    pub license_id: String,
    pub license_file: String,
    pub layout: Vec<LayoutRule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutRule {
    pub glob: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema_version: &'static str,
    pub notice: &'static str,
    pub corpus: CorpusPin,
    pub members: Vec<Member>,
    pub summary: Summary,
}

#[derive(Clone, Debug, Serialize)]
pub struct CorpusPin {
    pub origin: String,
    pub commit: String,
    pub license_id: String,
    pub license_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    pub path: String,
    pub kind: String,
    pub analysis: String,
    pub status: String,
    pub stages: BTreeMap<String, String>,
    pub diagnostics: Vec<IntakeDiagnostic>,
    pub constructs: Vec<Construct>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntakeDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<Span>,
    pub cause: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Construct {
    pub name: String,
    pub family: String,
    pub row_id: Option<String>,
    pub registry: String,
    pub baseline: Option<String>,
    pub product_support: String,
    pub coverage_ledger_status: String,
    pub reason: Option<String>,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub by_kind: BTreeMap<String, BTreeMap<String, usize>>,
    pub top_gaps: Vec<Gap>,
    pub environment: Vec<EnvironmentGap>,
    pub failing_members: Vec<Failure>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnvironmentGap {
    pub cause: String,
    pub code: String,
    pub member_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Gap {
    pub name: String,
    pub family: String,
    pub member_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Failure {
    pub path: String,
    pub first_diagnostic: IntakeDiagnostic,
}

#[derive(Default)]
struct Catalog(BTreeMap<(String, String), (String, String)>);

impl Catalog {
    fn load(root: &Path) -> Result<Self, IntakeError> {
        let mut catalog = Self::default();
        for (file, family) in [
            ("cics.json", "cics"),
            ("db2.json", "db2"),
            ("dataset-vsam-ams.json", "ams"),
        ] {
            let bytes = fs::read(
                root.join("conformance/subsystems/coverage/catalogs")
                    .join(file),
            )
            .map_err(|e| IntakeError::Io(e.to_string()))?;
            let document: Value =
                serde_json::from_slice(&bytes).map_err(|e| IntakeError::Schema(e.to_string()))?;
            let baseline = document["baseline_id"]
                .as_str()
                .ok_or_else(|| IntakeError::Schema(file.into()))?;
            for unit in document["units"]
                .as_array()
                .ok_or_else(|| IntakeError::Schema(file.into()))?
            {
                for row in unit["rows"]
                    .as_array()
                    .ok_or_else(|| IntakeError::Schema(file.into()))?
                {
                    let (Some(label), Some(id)) = (row["label"].as_str(), row["id"].as_str())
                    else {
                        continue;
                    };
                    let label = if family == "ams" {
                        label.split_once(". ").map_or(label, |(_, tail)| tail)
                    } else {
                        label
                    };
                    catalog.0.insert(
                        (family.into(), normalize(label)),
                        (id.into(), baseline.into()),
                    );
                }
            }
        }
        Ok(catalog)
    }

    fn construct(
        &self,
        family: &str,
        name: &str,
        support: &str,
        reason: Option<&str>,
        span: Option<Span>,
    ) -> Construct {
        let match_row = self.0.get(&(family.into(), normalize(name)));
        construct(
            name,
            family,
            match_row.map(|r| r.0.as_str()),
            support,
            reason,
            span,
        )
    }
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn construct(
    name: &str,
    family: &str,
    row_id: Option<&str>,
    support: &str,
    reason: Option<&str>,
    span: Option<Span>,
) -> Construct {
    Construct {
        name: name.into(),
        family: family.into(),
        row_id: row_id.map(str::to_string),
        registry: if row_id.is_some() {
            "matched"
        } else {
            "unregistered"
        }
        .into(),
        baseline: row_id
            .and_then(|id| id.split(':').next())
            .map(str::to_string),
        product_support: support.into(),
        coverage_ledger_status: if row_id.is_some() {
            "uncredited"
        } else {
            "not-applicable"
        }
        .into(),
        reason: if row_id.is_none() {
            Some(reason.unwrap_or("no-matching-row").into())
        } else {
            None
        },
        span,
    }
}

fn git(corpus: &Path, args: &[&str]) -> Result<String, IntakeError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(corpus)
        .output()
        .map_err(|e| IntakeError::Pin(e.to_string()))?;
    if !output.status.success() {
        return Err(IntakeError::Pin(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn verify_pin(
    manifest: &CorpusManifest,
    corpus: &Path,
    files: &PinnedFiles<'_>,
) -> Result<CorpusPin, IntakeError> {
    if git(corpus, &["rev-parse", "HEAD"])? != manifest.commit {
        return Err(IntakeError::Pin("commit mismatch".into()));
    }
    if git(corpus, &["remote", "get-url", "origin"])? != manifest.origin {
        return Err(IntakeError::Pin("origin mismatch".into()));
    }
    if !git(corpus, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
        return Err(IntakeError::Pin("dirty tree".into()));
    }
    let license = files.read(&manifest.license_file)?;
    Ok(CorpusPin {
        origin: manifest.origin.clone(),
        commit: manifest.commit.clone(),
        license_id: manifest.license_id.clone(),
        license_sha256: format!("sha256:{:x}", Sha256::digest(&license)),
    })
}

fn matches(pattern: &str, path: &str) -> bool {
    let mut rest = path;
    let parts: Vec<_> = pattern.split('*').collect();
    if !rest.starts_with(parts[0]) {
        return false;
    }
    rest = &rest[parts[0].len()..];
    for (i, part) in parts.iter().enumerate().skip(1) {
        if i == parts.len() - 1 {
            return rest.ends_with(part);
        }
        let Some(at) = rest.find(part) else {
            return false;
        };
        rest = &rest[at + part.len()..];
    }
    rest.is_empty()
}

fn diagnostic(d: &Diagnostic) -> IntakeDiagnostic {
    IntakeDiagnostic {
        code: d.code().as_str().into(),
        message: d.public_message().into(),
        span: d.primary().map(|s| Span {
            start: s.bytes.start,
            end: s.bytes.end,
        }),
        cause: None,
    }
}

fn simple_member(path: &str, kind: &str, analysis: &str, status: &str) -> Member {
    Member {
        path: path.into(),
        kind: kind.into(),
        analysis: analysis.into(),
        status: status.into(),
        stages: BTreeMap::new(),
        diagnostics: Vec::new(),
        constructs: Vec::new(),
    }
}

/// Verify a pin, scan only tracked source paths, and analyze each classified member.
pub fn run(
    manifest_path: &Path,
    corpus: &Path,
    repository_root: &Path,
) -> Result<Report, IntakeError> {
    let bytes = fs::read(manifest_path).map_err(|e| IntakeError::Manifest(e.to_string()))?;
    let manifest: CorpusManifest =
        serde_json::from_slice(&bytes).map_err(|e| IntakeError::Manifest(e.to_string()))?;
    if manifest.schema_version != "mainframe-env.profile-corpus@1"
        || manifest.layout.is_empty()
        || manifest.commit.len() != 40
        || !manifest.commit.bytes().all(|b| b.is_ascii_hexdigit())
        || manifest.origin.is_empty()
        || manifest.license_id.is_empty()
        || manifest.license_file.starts_with('/')
        || manifest.license_file.contains("..")
        || manifest.layout.iter().any(|rule| {
            rule.glob.is_empty()
                || rule.glob.starts_with('/')
                || rule.glob.contains("..")
                || !["cobol", "copybook", "jcl", "bms", "ignore", "other"]
                    .contains(&rule.kind.as_str())
        })
    {
        return Err(IntakeError::Manifest("invalid pin or layout".into()));
    }
    let files = PinnedFiles::open(corpus, &manifest.commit)?;
    let pin = verify_pin(&manifest, corpus, &files)?;
    let catalog = Catalog::load(repository_root)?;
    let paths = files.paths();
    let mut kinds = BTreeMap::new();
    for path in &paths {
        let matching = manifest
            .layout
            .iter()
            .filter(|rule| matches(&rule.glob, path))
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(IntakeError::Manifest(format!("overlapping layout: {path}")));
        }
        kinds.insert(
            path.clone(),
            matching
                .first()
                .map_or("unclassified", |r| r.kind.as_str())
                .to_string(),
        );
    }
    let copies = paths
        .iter()
        .filter(|p| kinds[*p] == "copybook")
        .cloned()
        .collect::<Vec<_>>();
    let mut members = Vec::new();
    for path in paths {
        let kind = &kinds[&path];
        let bytes = files.read(&path)?;
        let member = match kind.as_str() {
            "cobol" => analyze_cobol(&path, bytes, &copies, &files, &catalog),
            "jcl" => analyze_jcl(&path, bytes, &catalog),
            "bms" => analyze_bms(&path, &bytes),
            "copybook" | "ignore" | "other" => {
                simple_member(&path, kind, "not-applicable", "skipped")
            }
            _ => simple_member(&path, kind, "not-applicable", "unclassified"),
        };
        members.push(member);
    }
    let summary = summarize(&members);
    Ok(Report {
        schema_version: "mainframe-env.profile-intake@1",
        notice: NOTICE,
        corpus: pin,
        members,
        summary,
    })
}

fn analyze_cobol(
    path: &str,
    bytes: Vec<u8>,
    copies: &[String],
    corpus: &PinnedFiles<'_>,
    catalog: &Catalog,
) -> Member {
    let mut member = simple_member(path, "cobol", "available", "failed");
    let limits = SourceLimits::default();
    let make_file = |name: &str, bytes| {
        SourceFile::input(
            name,
            bytes,
            SourceFormat::Fixed,
            SourceEncoding::Utf8,
            limits,
        )
    };
    let Ok(primary) = make_file(path, bytes.clone()) else {
        member
            .diagnostics
            .push(local_error("MEINTAKE001", "source limits exceeded"));
        return member;
    };
    let Ok(primary_path) = LogicalPath::new(path, limits.max_path_bytes) else {
        member
            .diagnostics
            .push(local_error("MEINTAKE001", "invalid source path"));
        return member;
    };
    let mut files = vec![primary];
    let mut by_dir: BTreeMap<String, Vec<LogicalPath>> = BTreeMap::new();
    for copy in copies {
        let Ok(copy_bytes) = corpus.read(copy) else {
            member
                .diagnostics
                .push(local_error("MEINTAKE001", "cannot read pinned copybook"));
            return member;
        };
        let Ok(file) = make_file(copy, copy_bytes) else {
            continue;
        };
        let Ok(logical) = LogicalPath::new(copy, limits.max_path_bytes) else {
            continue;
        };
        by_dir
            .entry(
                Path::new(copy)
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_string_lossy()
                    .into(),
            )
            .or_default()
            .push(logical);
        files.push(file);
    }
    let libraries = by_dir
        .into_iter()
        .enumerate()
        .filter_map(|(i, (_, paths))| {
            SourceLibrary::new(format!("copybooks{i}"), paths, limits).ok()
        })
        .collect();
    let options = if fixed_source_area(&bytes).contains("EXEC SQL") {
        BTreeMap::from([("cobol.sql-precompile".into(), "true".into())])
    } else {
        BTreeMap::new()
    };
    let Ok(bundle) =
        SourceBundle::with_libraries(&primary_path, files, libraries, options, Vec::new(), limits)
    else {
        member
            .diagnostics
            .push(local_error("MEINTAKE001", "invalid copybook closure"));
        return member;
    };
    let analysis = CobolCompiler::new(CobolCompilerLimits::default()).analyze(&bundle);
    member.stages.insert(
        "syntax".into(),
        if analysis.syntax.is_some() {
            "complete"
        } else {
            "failed"
        }
        .into(),
    );
    member.stages.insert(
        "semantic".into(),
        if analysis.semantic.is_some() {
            "complete"
        } else {
            "failed"
        }
        .into(),
    );
    member.stages.insert(
        "hir".into(),
        if analysis.hir.is_some() {
            "complete"
        } else {
            "failed"
        }
        .into(),
    );
    member
        .diagnostics
        .extend(analysis.diagnostics.iter().map(diagnostic));
    member.status = if analysis.hir.is_some() {
        if member.diagnostics.is_empty() {
            "complete"
        } else {
            "unsupported"
        }
    } else {
        "failed"
    }
    .into();
    if let Some(hir) = analysis.hir {
        for statement in &hir.statements {
            if let Some(official) = statement.official {
                let row = procedure_statement_descriptor(official).row_id;
                member.constructs.push(construct(
                    statement.kind.slug(),
                    "cobol",
                    Some(row),
                    "unknown",
                    None,
                    statement.location.as_ref().map(|s| Span {
                        start: s.bytes.start,
                        end: s.bytes.end,
                    }),
                ));
            }
        }
    } else {
        let source = String::from_utf8_lossy(&bytes).to_ascii_uppercase();
        for diagnostic in &member.diagnostics {
            let message = diagnostic.message.to_ascii_uppercase();
            for descriptor in PROCEDURE_STATEMENTS {
                let keyword = descriptor.id.replace('-', " ").to_ascii_uppercase();
                if message.contains(&keyword) && source.contains(&format!("{keyword} ")) {
                    member.constructs.push(construct(
                        &keyword,
                        "cobol",
                        Some(descriptor.row_id),
                        "unknown",
                        None,
                        None,
                    ));
                }
            }
        }
    }
    // The compiler may stop before HIR construction. Preserve embedded commands as
    // source observations while retaining its real stage failure and diagnostics.
    let upper = fixed_source_area(&bytes);
    for (family, prefix) in [("cics", "EXEC CICS"), ("db2", "EXEC SQL")] {
        let mut offset = 0;
        while let Some(start) = upper[offset..].find(prefix).map(|i| i + offset) {
            let tail = &upper[start + prefix.len()..];
            let Some(end_rel) = tail.find("END-EXEC") else {
                break;
            };
            let content = tail[..end_rel].trim();
            let name = catalog_statement_name(catalog, family, content);
            member.constructs.push(catalog.construct(
                family,
                &name,
                "unknown",
                None,
                Some(Span {
                    start,
                    end: start + prefix.len() + end_rel + "END-EXEC".len(),
                }),
            ));
            offset = start + prefix.len() + end_rel + "END-EXEC".len();
        }
    }
    member
}

fn fixed_source_area(bytes: &[u8]) -> String {
    let mut visible = bytes.to_vec();
    for line in visible.split_inclusive_mut(|byte| *byte == b'\n') {
        let comment = matches!(line.get(6), Some(b'*' | b'/'));
        for (column, byte) in line.iter_mut().enumerate() {
            if *byte != b'\n' && (comment || !(7..72).contains(&column)) {
                *byte = b' ';
            }
        }
    }
    String::from_utf8_lossy(&visible).to_ascii_uppercase()
}

fn catalog_statement_name(catalog: &Catalog, family: &str, content: &str) -> String {
    let normalized = normalize(content);
    catalog
        .0
        .keys()
        .filter(|(row_family, label)| {
            if row_family != family {
                return false;
            }
            normalized == *label
                || normalized.starts_with(&format!("{label} "))
                || normalized.starts_with(&format!("{label}("))
                || (family == "db2"
                    && label == "DECLARE CURSOR"
                    && normalized.starts_with("DECLARE ")
                    && normalized.split_whitespace().any(|word| word == "CURSOR"))
                || (family == "db2"
                    && label == "SET ASSIGNMENT-STATEMENT"
                    && normalized.starts_with("SET :"))
        })
        .map(|(_, label)| label.clone())
        .max_by_key(String::len)
        .unwrap_or_else(|| {
            content
                .split_whitespace()
                .next()
                .unwrap_or("UNKNOWN")
                .into()
        })
}

fn analyze_jcl(path: &str, bytes: Vec<u8>, catalog: &Catalog) -> Member {
    let mut member = simple_member(path, "jcl", "available", "failed");
    let Ok(text) = String::from_utf8(bytes) else {
        member
            .diagnostics
            .push(local_error("MEINTAKE002", "JCL is not UTF-8"));
        return member;
    };
    let bundle = JclBundle {
        primary: text.clone(),
        ..JclBundle::default()
    };
    let Ok(syntax) = analyze_jcl_syntax(&bundle, JclSyntaxLimits::default()) else {
        member
            .diagnostics
            .push(local_error("MEINTAKE002", "JCL syntax analysis failed"));
        return member;
    };
    let parsed = parse_jcl_statements(&syntax);
    member.stages.insert("syntax".into(), "complete".into());
    member.stages.insert(
        "statements".into(),
        if parsed.is_complete() {
            "complete"
        } else {
            "failed"
        }
        .into(),
    );
    member
        .diagnostics
        .extend(parsed.diagnostics().iter().map(diagnostic));
    let mut idcams = false;
    for statement in parsed.statements() {
        let identity = statement.generated_identity();
        let support =
            format!("{:?}", statement.identity().descriptor().support).to_ascii_lowercase();
        member.constructs.push(construct(
            statement.source_operation(),
            "jcl",
            Some(identity.row_id()),
            &support,
            None,
            None,
        ));
        for parameter in statement.parameters() {
            let generated = parameter.identity().generated();
            let support = format!("{:?}", parameter.identity().support()).to_ascii_lowercase();
            member.constructs.push(construct(
                parameter.source_keyword(),
                "jcl",
                Some(generated.row_id()),
                &support,
                None,
                None,
            ));
        }
        if statement.source_operation() == "EXEC"
            && let Some(pgm) = statement
                .operands()
                .to_ascii_uppercase()
                .split("PGM=")
                .nth(1)
                .and_then(|s| {
                    s.split(|c: char| {
                        !c.is_ascii_alphanumeric() && c != '@' && c != '$' && c != '#'
                    })
                    .next()
                })
        {
            idcams = pgm == "IDCAMS";
            if let Some(disposition) = utility_disposition(pgm) {
                let mut registered = construct(
                    pgm,
                    "program",
                    None,
                    &format!("{disposition:?}").to_ascii_lowercase(),
                    None,
                    None,
                );
                registered.registry = "matched".into();
                registered.reason = None;
                member.constructs.push(registered);
            } else {
                member.constructs.push(construct(
                    pgm,
                    "program",
                    None,
                    "unknown",
                    Some("no-program-registry-entry"),
                    None,
                ));
            }
        }
        if idcams && statement.name() == Some("SYSIN") && !statement.inline_data().is_empty() {
            match parse_idcams_control(statement.inline_data()) {
                Ok(commands) => {
                    for command in commands {
                        if let AmsStatement::Command(command) = command {
                            member.constructs.push(catalog.construct(
                                "ams",
                                command.label(),
                                "unknown",
                                None,
                                None,
                            ));
                        }
                    }
                }
                Err(e) => {
                    member.diagnostics.push(local_error(
                        "MEINTAKE003",
                        &format!("IDCAMS control: {e:?}"),
                    ));
                    // A malformed SYSIN block must not hide commands the same
                    // parser recognizes independently. Keep the block error.
                    for line in String::from_utf8_lossy(statement.inline_data()).lines() {
                        let words = line
                            .split_whitespace()
                            .take(2)
                            .map(|word| word.split('(').next().unwrap_or(word))
                            .collect::<Vec<_>>();
                        for count in (1..=words.len()).rev() {
                            let candidate = words[..count].join(" ");
                            if let Ok(commands) = parse_idcams_control(candidate.as_bytes())
                                && let Some(AmsStatement::Command(command)) = commands.first()
                            {
                                member.constructs.push(catalog.construct(
                                    "ams",
                                    command.label(),
                                    "unknown",
                                    None,
                                    None,
                                ));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    match convert_jcl(&bundle, JclConversionLimits::default()) {
        Ok(conversion) => {
            member.stages.insert(
                "planner".into(),
                if conversion.is_valid() {
                    "complete"
                } else {
                    "failed"
                }
                .into(),
            );
            member
                .diagnostics
                .extend(conversion.diagnostics().iter().map(|d| {
                    let mut result = diagnostic(d);
                    result.cause = jcl_environment_cause(&result, &text).map(str::to_string);
                    result
                }));
            member.status = if conversion.is_valid() && member.diagnostics.is_empty() {
                "complete"
            } else if conversion.is_valid() {
                "unsupported"
            } else {
                "failed"
            }
            .into();
        }
        Err(e) => {
            member.stages.insert("planner".into(), "failed".into());
            member
                .diagnostics
                .push(local_error("MEINTAKE004", &e.to_string()));
        }
    }
    member
}

fn analyze_bms(path: &str, bytes: &[u8]) -> Member {
    let mut member = simple_member(path, "bms", "available", "failed");
    let Ok(text) = std::str::from_utf8(bytes) else {
        member
            .diagnostics
            .push(local_error("MEINTAKE005", "BMS is not UTF-8"));
        return member;
    };
    match parse_bms(text) {
        Ok(map) => {
            member.stages.insert("parse".into(), "complete".into());
            member.status = "complete".into();
            member.constructs.push(no_catalog_unit(construct(
                "DFHMSD",
                "bms",
                None,
                "unknown",
                Some("no-bms-catalog"),
                None,
            )));
            member.constructs.push(no_catalog_unit(construct(
                "DFHMDI",
                "bms",
                None,
                "unknown",
                Some("no-bms-catalog"),
                None,
            )));
            for _ in &map.fields {
                member.constructs.push(no_catalog_unit(construct(
                    "DFHMDF",
                    "bms",
                    None,
                    "unknown",
                    Some("no-bms-catalog"),
                    None,
                )));
            }
        }
        Err(e) => {
            member.stages.insert("parse".into(), "failed".into());
            member
                .diagnostics
                .push(local_error("MEINTAKE005", &format!("BMS parse: {e:?}")));
        }
    }
    member
}

fn no_catalog_unit(mut item: Construct) -> Construct {
    item.registry = "no-catalog-unit".into();
    item.reason = Some("no-bms-catalog".into());
    item
}

fn jcl_environment_cause(diagnostic: &IntakeDiagnostic, text: &str) -> Option<&'static str> {
    if diagnostic.code == "MEJCL0734" {
        return Some("external-procedure");
    }
    if !["MEJCL0745", "MEJCL0755", "MEJCL0760"].contains(&diagnostic.code.as_str()) {
        return None;
    }
    let span = diagnostic.span.as_ref()?;
    let source = text.get(span.start..span.end)?;
    let open = source.find('<')?;
    source[open + 1..]
        .contains('>')
        .then_some("environment-placeholder")
}

fn local_error(code: &str, message: &str) -> IntakeDiagnostic {
    IntakeDiagnostic {
        code: code.into(),
        message: message.into(),
        span: None,
        cause: None,
    }
}

fn summarize(members: &[Member]) -> Summary {
    let mut by_kind: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut gaps: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut environment: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut failing_members = Vec::new();
    for member in members {
        *by_kind
            .entry(member.kind.clone())
            .or_default()
            .entry(member.status.clone())
            .or_default() += 1;
        if (member.status == "failed" || member.status == "unsupported")
            && let Some(first) = member.diagnostics.first()
        {
            failing_members.push(Failure {
                path: member.path.clone(),
                first_diagnostic: first.clone(),
            });
        }
        for diagnostic in &member.diagnostics {
            if let Some(cause) = &diagnostic.cause {
                environment
                    .entry((cause.clone(), diagnostic.code.clone()))
                    .or_default()
                    .insert(member.path.clone());
            }
        }
        for item in &member.constructs {
            if item.registry == "unregistered" || item.product_support == "deferred" {
                gaps.entry((item.family.clone(), item.name.clone()))
                    .or_default()
                    .insert(member.path.clone());
            }
        }
    }
    let mut top_gaps = gaps
        .into_iter()
        .map(|((family, name), paths)| Gap {
            family,
            name,
            member_count: paths.len(),
        })
        .collect::<Vec<_>>();
    top_gaps.sort_by(|a, b| {
        b.member_count
            .cmp(&a.member_count)
            .then(a.family.cmp(&b.family))
            .then(a.name.cmp(&b.name))
    });
    Summary {
        by_kind,
        top_gaps,
        environment: environment
            .into_iter()
            .map(|((cause, code), paths)| EnvironmentGap {
                cause,
                code,
                member_count: paths.len(),
            })
            .collect(),
        failing_members,
    }
}

impl Report {
    pub fn markdown(&self) -> String {
        let mut out = format!(
            "# Profile intake\n\n{}\n\nLicense: {} ({})\n\n## Members\n\n",
            NOTICE, self.corpus.license_id, self.corpus.license_sha256
        );
        for member in &self.members {
            out.push_str(&format!(
                "- `{}`: {} / {}\n",
                member.path, member.kind, member.status
            ));
        }
        out.push_str("\n## Top gaps\n\n");
        for gap in self.summary.top_gaps.iter().take(15) {
            out.push_str(&format!(
                "- {} {}: {} members\n",
                gap.family, gap.name, gap.member_count
            ));
        }
        out.push_str("\n## Environment\n\n");
        for entry in &self.summary.environment {
            out.push_str(&format!(
                "- {} {}: {} members\n",
                entry.cause, entry.code, entry.member_count
            ));
        }
        out.push_str("\n## Failing members\n\n");
        for failure in &self.summary.failing_members {
            out.push_str(&format!(
                "- `{}`: {} {}\n",
                failure.path, failure.first_diagnostic.code, failure.first_diagnostic.message
            ));
        }
        out
    }
}
