//! Bounded, zero-credit semantic differential exploration against GnuCOBOL.

use crate::cobol_reference::run_bounded;
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SCHEMA: &str =
    include_str!("../../../../conformance/spec/schemas/cobol-differential-receipt.schema.json");
const MAX_OUTPUT: usize = 65_536;
static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// GnuCOBOL flags for the reference build. `-fbinary-truncate` makes ordinary
/// binary items truncate to their PICTURE digits, emulating IBM's default
/// TRUNC(STD) that the product models; without it GnuCOBOL wraps binary storage.
const REFERENCE_FLAGS: [&str; 4] = ["-x", "-std=ibm", "-free", "-fbinary-truncate"];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CobolObservation {
    pub stdout_records: Vec<String>,
    pub return_code: Option<i32>,
    pub abend_code: Option<String>,
    pub abend_reason: Option<String>,
}

/// Compile and drive a single source without losing records produced before an abend.
pub fn observe_cobol_source(source: &str) -> Result<CobolObservation, String> {
    let artifact = crate::compile(source)?;
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        crate::invocation(&artifact, MAX_OUTPUT as u64),
        CodecLimits::default(),
    )
    .map_err(|error| format!("create product machine: {error:?}"))?;
    for _ in 0..1000 {
        match machine.drive(
            MachineResume::Start,
            Quantum::new(8, 4096).expect("valid quantum"),
        ) {
            MachineDrive::Continue => {}
            MachineDrive::Completed(done) => {
                return Ok(CobolObservation {
                    stdout_records: records(done.output.bytes())?,
                    return_code: Some(done.return_code),
                    abend_code: None,
                    abend_reason: None,
                });
            }
            MachineDrive::Abend(abend) => {
                return Ok(CobolObservation {
                    stdout_records: records(machine.output())?,
                    return_code: None,
                    abend_code: Some(abend.code),
                    abend_reason: abend.reason,
                });
            }
            MachineDrive::Condition(condition) => {
                return Ok(CobolObservation {
                    stdout_records: records(machine.output())?,
                    return_code: None,
                    abend_code: Some(condition.name),
                    abend_reason: Some(format!(
                        "response={} response2={}",
                        condition.response, condition.response2
                    )),
                });
            }
            other => return Err(format!("product execution did not complete: {other:?}")),
        }
    }
    Err("product execution exceeded 8000 steps".into())
}

fn records(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| format!("non-UTF-8 output: {error}"))?;
    Ok(text
        .replace("\r\n", "\n")
        .split_terminator('\n')
        .map(str::to_owned)
        .collect())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DifferentialCase {
    pub origin: String,
    pub seed: Option<u64>,
    pub class: String,
    pub signature: String,
    pub count: u64,
    pub classification: String,
    pub reason: Option<String>,
    pub minimized_program: String,
    pub product: CobolObservation,
    pub reference: CobolObservation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CobolDifferentialReceipt {
    pub schema_version: String,
    pub reference: String,
    pub licensed_credit: u8,
    pub cobc_path: String,
    pub cobc_version: String,
    pub cobc_sha256: String,
    pub compiler_flags: Vec<String>,
    pub seed_start: u64,
    pub seed_end: u64,
    pub seeds_run: u64,
    pub rejected_reference: u64,
    pub rejected_product: u64,
    pub anchors: BTreeMap<String, bool>,
    pub anchor_programs: BTreeMap<String, Vec<String>>,
    pub generated_reached: BTreeMap<String, bool>,
    pub divergences: Vec<DifferentialCase>,
}

#[derive(Clone, Debug)]
struct Program {
    class: String,
    data: Vec<String>,
    statements: Vec<String>,
}

impl Program {
    fn source(&self) -> String {
        format!(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n{}\nPROCEDURE DIVISION.\n{}\nSTOP RUN.\n",
            self.data.join("\n"),
            self.statements.join("\n")
        )
    }
}

mod generator;
use generator::generated;

fn anchors() -> Vec<Program> {
    vec![
        Program {
            class: "#229".into(),
            data: vec![
                "01 A PIC S9(9)V99 VALUE 95.85.".into(),
                "01 R PIC +ZZZ,ZZZ,ZZZ.ZZ.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        Program {
            class: "#229".into(),
            data: vec![
                "01 A PIC S9(9)V99 VALUE -95.85.".into(),
                "01 R PIC -ZZZ,ZZZ,ZZZ.ZZ.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        Program {
            class: "#230".into(),
            data: vec![
                "01 A PIC S9(7)V99 VALUE 95.85.".into(),
                "01 R PIC $$$,$$9.99.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        Program {
            class: "#231".into(),
            data: vec![
                "01 A PIC S9(5)V99 VALUE -95.85.".into(),
                "01 R PIC 99999.99CR.".into(),
                "01 Q PIC 99999.99DB.".into(),
            ],
            statements: vec![
                "MOVE A TO R.".into(),
                "MOVE A TO Q.".into(),
                "DISPLAY R.".into(),
                "DISPLAY Q.".into(),
            ],
        },
        Program {
            class: "#158".into(),
            data: vec![
                "01 A PIC 9(18) VALUE 999999999999999999.".into(),
                "01 R PIC 9(18).".into(),
            ],
            statements: vec!["COMPUTE R = (A + 2) - A.".into(), "DISPLAY R.".into()],
        },
        Program {
            class: "#159".into(),
            data: vec!["01 Q PIC 9V9(12).".into()],
            statements: vec!["COMPUTE Q = 1 / 3.".into(), "DISPLAY Q.".into()],
        },
        Program {
            class: "#251".into(),
            data: vec![
                "01 A PIC X(6) VALUE '123456'.".into(),
                "01 R PIC 9(6).".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        Program {
            class: "#264".into(),
            data: vec![
                "01 R PIC 9(7)V99 USAGE COMP VALUE 95.85.".into(),
                "01 OUT-NUM PIC 9(7)V99.".into(),
            ],
            statements: vec![
                "SUBTRACT 2 FROM R.",
                "MOVE R TO OUT-NUM.",
                "DISPLAY OUT-NUM.",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        },
        Program {
            class: "#265".into(),
            data: vec![
                "01 R PIC S9(7)V99 USAGE DISPLAY VALUE 95.85.".into(),
                "01 OUT-NUM PIC 9(7)V99.".into(),
            ],
            statements: vec![
                "DIVIDE 2 INTO R ROUNDED.".into(),
                "MOVE R TO OUT-NUM.".into(),
                "DISPLAY OUT-NUM.".into(),
            ],
        },
        Program {
            class: "#286".into(),
            data: vec![
                "01 N0 PIC S9(1)V9(2) USAGE DISPLAY VALUE 2.20.".into(),
                "01 N0-O PIC S9(1)V9(2) USAGE DISPLAY SIGN IS TRAILING SEPARATE.".into(),
            ],
            statements: vec![
                "MOVE 73 TO N0.".into(),
                "MOVE N0 TO N0-O.".into(),
                "DISPLAY N0-O.".into(),
            ],
        },
    ]
}

fn minimize<F>(mut program: Program, mut divergent: F) -> Program
where
    F: FnMut(&Program) -> bool,
{
    for index_kind in 0..2 {
        let mut index = 0;
        loop {
            let len = if index_kind == 0 {
                program.statements.len()
            } else {
                program.data.len()
            };
            if index >= len {
                break;
            }
            let mut candidate = program.clone();
            if index_kind == 0 {
                candidate.statements.remove(index);
            } else {
                candidate.data.remove(index);
            }
            if divergent(&candidate) {
                program = candidate;
            } else {
                index += 1;
            }
        }
    }
    program
}

fn displays_initialized(program: &Program) -> bool {
    program
        .statements
        .iter()
        .enumerate()
        .all(|(index, statement)| {
            let Some(target) = statement
                .strip_prefix("DISPLAY ")
                .and_then(|s| s.strip_suffix('.'))
            else {
                return true;
            };
            let Some(previous) = index.checked_sub(1).and_then(|i| program.statements.get(i))
            else {
                return false;
            };
            previous.starts_with("MOVE ") && previous.ends_with(&format!(" TO {target}."))
        })
}

struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self, String> {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "cobol-differential-{}-{}",
                std::process::id(),
                DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("create campaign work directory: {error}")),
            }
        }
        Err("create campaign work directory: exhausted unique names".into())
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

enum Trial {
    RejectedReference,
    RejectedProduct,
    Observed(CobolObservation, CobolObservation),
}

fn trial(program: &Program, cobc: &Path, work: &Path) -> Result<Trial, String> {
    let source = program.source();
    if source.len() > 16_384 {
        return Err("generated source exceeded bound".into());
    }
    fs::write(work.join("input.cbl"), &source).map_err(|error| error.to_string())?;
    let arguments: Vec<OsString> = REFERENCE_FLAGS
        .into_iter()
        .chain(["-o", "reference", "input.cbl"])
        .map(OsString::from)
        .collect();
    let compiled = run_bounded(cobc, &arguments, work, &[])?;
    if !compiled.status.success() {
        return Ok(Trial::RejectedReference);
    }
    let artifact = match crate::compile(&source) {
        Ok(artifact) => artifact,
        Err(_) => return Ok(Trial::RejectedProduct),
    };
    // Use the same source helper for the observation. Compilation above is the
    // eligibility test; execution remains a single in-process product run.
    drop(artifact);
    let product = observe_cobol_source(&source)?;
    let output = run_bounded(&work.join("reference"), &[], work, &[])?;
    let status = output
        .status
        .code()
        .ok_or("GnuCOBOL process ended without exit code")?;
    let reference = CobolObservation {
        stdout_records: records(&output.stdout)?,
        return_code: Some(status),
        abend_code: None,
        abend_reason: None,
    };
    Ok(Trial::Observed(product, reference))
}

fn allowlisted(signature: &str) -> Result<Option<String>, String> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../conformance/spec/cobol-differential-allowlist.json"
    ))
    .map_err(|e| e.to_string())?;
    Ok(value
        .get("entries")
        .and_then(|v| v.as_array())
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.get("signature").and_then(|v| v.as_str()) == Some(signature))
        })
        .and_then(|entry| entry.get("reason").and_then(|v| v.as_str()))
        .map(str::to_owned))
}

fn signature(program: &Program, product: &CobolObservation) -> String {
    let mut operations: Vec<&str> = program
        .statements
        .iter()
        .filter(|s| !s.starts_with("DISPLAY"))
        .map(|s| s.split_whitespace().next().unwrap_or(""))
        .collect();
    operations.sort_unstable();
    operations.dedup();
    let mut usages = Vec::new();
    let mut scales = Vec::new();
    let mut edit = Vec::new();
    for data in &program.data {
        let pic = data
            .split("PIC ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("")
            .trim_end_matches('.');
        if pic.starts_with('X') {
            usages.push("X");
        } else if pic.contains('Z')
            || pic.contains('*')
            || pic.contains('$')
            || pic.contains("CR")
            || pic.contains("DB")
            || pic.contains('.')
            || pic.contains(',')
            || pic.contains('B')
            || pic.contains('/')
            || pic.starts_with('+')
            || pic.starts_with('-')
        {
            if pic.contains(',') {
                edit.push("comma");
            }
            if pic.contains('$') {
                edit.push("currency");
            }
            if pic.contains('Z') {
                edit.push("Z");
            }
            if pic.contains('*') {
                edit.push("star");
            }
            if pic.contains('+') || pic.contains('-') {
                edit.push("sign");
            }
            if pic.contains("CR") {
                edit.push("CR");
            }
            if pic.contains("DB") {
                edit.push("DB");
            }
            if pic.trim_end_matches("DB").contains('B') || pic.contains('0') || pic.contains('/') {
                edit.push("insertion");
            }
        } else {
            let usage = ["COMP-5", "COMP-4", "COMP-3", "COMP", "BINARY"]
                .into_iter()
                .find(|u| data.contains(u))
                .unwrap_or("DISPLAY");
            usages.push(usage);
            let scale = pic
                .split('V')
                .nth(1)
                .map(|v| {
                    v.strip_prefix("9(")
                        .and_then(|s| s.strip_suffix(')'))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or_else(|| v.chars().filter(|c| *c == '9').count())
                })
                .unwrap_or(0);
            scales.push(match scale {
                0 => "0",
                1..=9 => "1-9",
                _ => "10+",
            });
        }
    }
    usages.sort();
    usages.dedup();
    scales.sort();
    scales.dedup();
    edit.sort();
    edit.dedup();
    let text = program.statements.join(" ");
    format!(
        "verbs={};usages={};scales={};edit={};division={};rounded={};outcome={}",
        operations.join("+"),
        usages.join("+"),
        scales.join("+"),
        edit.join("+"),
        text.contains("DIVIDE") || text.contains(" / "),
        text.contains("ROUNDED"),
        product.abend_code.as_ref().map_or_else(
            || "value-mismatch".to_owned(),
            |code| format!("abend:{code}")
        )
    )
}

fn classify(
    program: &Program,
    sig: &str,
    product: &CobolObservation,
    reference: &CobolObservation,
) -> Option<&'static str> {
    if product.abend_code.as_deref() == Some("SIZE-ERROR")
        && reference.abend_code.is_none()
        && reference.return_code.is_some()
        && !program.source().contains("ON SIZE ERROR")
    {
        return Some("#286");
    }
    let edited_move = program
        .data
        .iter()
        .filter_map(|d| {
            let (name, pic) = (
                d.split_whitespace().nth(1)?,
                d.split("PIC ").nth(1)?.split_whitespace().next()?,
            );
            program
                .statements
                .iter()
                .any(|s| s.starts_with("MOVE ") && s.contains(&format!(" TO {name}.")))
                .then_some(pic)
        })
        .collect::<Vec<_>>();
    let alpha_names: Vec<_> = program
        .data
        .iter()
        .filter(|d| d.contains(" PIC X"))
        .filter_map(|d| d.split_whitespace().nth(1))
        .collect();
    let display_names: Vec<_> = program
        .data
        .iter()
        .filter(|d| {
            (d.contains("PIC 9") || d.contains("PIC S9"))
                && !d.contains("COMP")
                && !d.contains("BINARY")
        })
        .filter_map(|d| d.split_whitespace().nth(1))
        .collect();
    if program.statements.iter().any(|s| {
        s.starts_with("MOVE ")
            && display_names
                .iter()
                .any(|name| s.ends_with(&format!("TO {name}.")))
            && (alpha_names
                .iter()
                .any(|name| s.contains(&format!("MOVE {name} TO")))
                || s.contains("MOVE '"))
    }) {
        return Some("#251");
    }
    if edited_move
        .iter()
        .any(|p| p.contains("CR") || p.contains("DB"))
    {
        return Some("#231");
    }
    if edited_move.iter().any(|p| p.starts_with("$$")) {
        return Some("#230");
    }
    if edited_move
        .iter()
        .any(|p| p.contains(',') && (p.contains('Z') || p.contains('+') || p.contains('-')))
    {
        return Some("#229");
    }
    if program
        .statements
        .iter()
        .any(|s| s.starts_with("DIVIDE ") && s.contains(" ROUNDED"))
    {
        return Some("#265");
    }
    if sig.contains("division=true")
        && program
            .data
            .iter()
            .any(|d| (10..=18).any(|n| d.contains(&format!("V9({n})"))))
    {
        return Some("#159");
    }
    if program.data.iter().any(|d| d.contains("9(18)"))
        && program.statements.iter().any(|s| {
            ["COMPUTE", "ADD", "SUBTRACT"]
                .iter()
                .any(|v| s.starts_with(v))
        })
    {
        return Some("#158");
    }
    if program.data.iter().any(|d| {
        let name = d.split_whitespace().nth(1).unwrap_or("");
        d.split("PIC ")
            .nth(1)
            .and_then(|picture| picture.split_whitespace().next())
            .is_some_and(|picture| picture.contains('V'))
            && d.contains(" VALUE ")
            && ["COMP", "COMP-4", "COMP-5", "BINARY"]
                .iter()
                .any(|usage| d.contains(&format!(" USAGE {usage} ")))
            && program.statements.iter().any(|s| {
                s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .any(|word| word == name)
            })
    }) {
        return Some("#264");
    }
    None
}

pub fn run_cobol_differential(
    cobc: &Path,
    start: u64,
    end: u64,
) -> Result<CobolDifferentialReceipt, String> {
    if start > end || end - start >= 300 {
        return Err("seed range must contain 1..300 seeds".into());
    }
    let resolved = fs::canonicalize(cobc)
        .map_err(|error| format!("cobc unavailable at {}: {error}", cobc.display()))?;
    if !resolved.is_file() {
        return Err(format!("cobc is not a file: {}", resolved.display()));
    }
    let bytes = fs::read(&resolved).map_err(|error| format!("read cobc binary: {error}"))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("cobc binary exceeds identity bound".into());
    }
    let work = WorkDir::new()?;
    let version = run_bounded(&resolved, &[OsString::from("--version")], &work.0, &[])?;
    if !version.status.success() {
        return Err("cobc --version failed".into());
    }
    let version = std::str::from_utf8(&version.stdout)
        .map_err(|_| "cobc --version is not UTF-8")?
        .lines()
        .next()
        .ok_or("cobc --version returned no version")?
        .to_owned();
    let mut receipt = CobolDifferentialReceipt {
        schema_version: "mainframe-env.cobol-differential-receipt@2".into(),
        reference: "GnuCOBOL".into(),
        licensed_credit: 0,
        cobc_path: resolved.display().to_string(),
        cobc_version: version,
        cobc_sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        compiler_flags: REFERENCE_FLAGS.map(String::from).to_vec(),
        seed_start: start,
        seed_end: end,
        seeds_run: 0,
        rejected_reference: 0,
        rejected_product: 0,
        anchors: BTreeMap::new(),
        anchor_programs: BTreeMap::new(),
        generated_reached: BTreeMap::new(),
        divergences: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    for issue in [
        "#158", "#159", "#229", "#230", "#231", "#251", "#264", "#265", "#286",
    ] {
        receipt.generated_reached.insert(issue.into(), false);
    }
    for program in anchors() {
        let issue = program.class.to_owned();
        receipt
            .anchor_programs
            .entry(issue.clone())
            .or_default()
            .push(program.source());
        let result = run_one(&program, None, &resolved, &work.0, &mut receipt, &mut seen)?;
        receipt
            .anchors
            .entry(issue)
            .and_modify(|v| *v |= result)
            .or_insert(result);
    }
    for seed in start..=end {
        let program = generated(seed);
        run_one(
            &program,
            Some(seed),
            &resolved,
            &work.0,
            &mut receipt,
            &mut seen,
        )?;
        receipt.seeds_run += 1;
    }
    let value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let schema: serde_json::Value =
        serde_json::from_str(SCHEMA).map_err(|error| error.to_string())?;
    let validator = jsonschema::draft202012::new(&schema).map_err(|error| error.to_string())?;
    if !validator.is_valid(&value) {
        return Err("differential receipt failed schema validation".into());
    }
    Ok(receipt)
}

fn run_one(
    program: &Program,
    seed: Option<u64>,
    cobc: &Path,
    work: &Path,
    receipt: &mut CobolDifferentialReceipt,
    seen: &mut BTreeSet<String>,
) -> Result<bool, String> {
    let (product, reference) = match trial(program, cobc, work)? {
        Trial::RejectedReference => {
            if seed.is_some() {
                receipt.rejected_reference += 1;
            }
            return Ok(false);
        }
        Trial::RejectedProduct => {
            if seed.is_some() {
                receipt.rejected_product += 1;
            }
            return Ok(false);
        }
        Trial::Observed(product, reference) => (product, reference),
    };
    if product == reference {
        return Ok(false);
    }
    let reduced = minimize(program.clone(), |candidate| {
        displays_initialized(candidate)
            && candidate
                .statements
                .iter()
                .any(|statement| !statement.starts_with("DISPLAY"))
            && matches!(trial(candidate, cobc, work), Ok(Trial::Observed(a, b))
                if a != b
                    && a.abend_code == product.abend_code
                    && b.abend_code == reference.abend_code)
    });
    let sig = signature(&reduced, &product);
    let issue = classify(&reduced, &sig, &product, &reference);
    if let (Some(_), Some(issue)) = (seed, issue) {
        receipt.generated_reached.insert(issue.into(), true);
    }
    let allowed = allowlisted(&sig)?;
    let classification = if issue.is_some() {
        "known-defect"
    } else if allowed.is_some() {
        "allowed-difference"
    } else {
        "product-defect-candidate"
    };
    let class = format!(
        "{}:{sig}:{}",
        if seed.is_some() {
            "generated"
        } else {
            "anchor"
        },
        issue.unwrap_or(classification)
    );
    if seen.insert(class.clone()) {
        let (product, reference) = match trial(&reduced, cobc, work)? {
            Trial::Observed(a, b) => (a, b),
            _ => (product, reference),
        };
        receipt.divergences.push(DifferentialCase {
            origin: if seed.is_some() {
                "generated"
            } else {
                "anchor"
            }
            .into(),
            seed,
            class: class.clone(),
            signature: sig,
            count: 1,
            classification: classification.into(),
            reason: issue
                .map(|issue| {
                    format!(
                        "https://github.com/toreleon/mainframe-env/issues/{}",
                        &issue[1..]
                    )
                })
                .or(allowed),
            minimized_program: reduced.source(),
            product,
            reference,
        });
    } else if let Some(existing) = receipt
        .divergences
        .iter_mut()
        .find(|case| case.class == class)
    {
        existing.count += 1;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cobol_differential_observes_product_records_and_return_code() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. HELLO. PROCEDURE DIVISION. DISPLAY 'OK'. STOP RUN.";
        let observed = observe_cobol_source(source).unwrap();
        assert_eq!(observed.stdout_records, ["OK"]);
        assert_eq!(observed.return_code, Some(0));
        assert_eq!(observed.abend_code, None);
    }

    #[test]
    fn cobol_differential_generator_is_deterministic() {
        for seed in 1..=300 {
            assert_eq!(generated(seed).source(), generated(seed).source());
        }
        assert_ne!(generated(1).source(), generated(2).source());
    }

    #[test]
    fn cobol_differential_pictures_are_well_formed() {
        let mut high_scale = 0;
        for seed in 1..=1000 {
            let program = generated(seed);
            assert!((4..=12).contains(&program.data.len()));
            assert!((2..=12).contains(&program.statements.len()));
            for item in &program.data {
                let pic = item
                    .split("PIC ")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .trim_end_matches('.');
                if pic.contains('Z')
                    || pic.contains('*')
                    || pic.contains('$')
                    || pic.contains("CR")
                    || pic.contains("DB")
                    || pic.contains(',')
                    || pic.starts_with('+')
                    || pic.starts_with('-')
                {
                    assert!(generator::well_formed_picture(pic), "seed {seed}: {pic}");
                    assert!(pic.len() <= 30);
                } else if item.contains(" USAGE ") && !pic.starts_with('X') {
                    let body = pic.strip_prefix('S').unwrap_or(pic);
                    let (integer, scale) = body.split_once('V').unwrap_or((body, ""));
                    let count = |part: &str| {
                        if part.is_empty() {
                            0
                        } else if part == "9" {
                            1
                        } else {
                            part.strip_prefix("9(")
                                .and_then(|s| s.strip_suffix(')'))
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap()
                        }
                    };
                    let digits = count(integer) + count(scale);
                    assert!((1..=18).contains(&digits), "seed {seed}: {pic}");
                    if count(scale) >= 10 {
                        high_scale += 1;
                    }
                }
            }
        }
        assert!(high_scale > 0);
    }

    #[test]
    fn cobol_differential_numeric_results_use_display_companions() {
        for seed in 1..=1000 {
            let program = generated(seed);
            assert!(displays_initialized(&program), "seed {seed}");
            for statement in &program.statements {
                let Some(name) = statement
                    .strip_prefix("DISPLAY ")
                    .and_then(|s| s.strip_suffix("-O."))
                else {
                    continue;
                };
                let item = program
                    .data
                    .iter()
                    .find(|d| d.starts_with(&format!("01 {name} PIC ")))
                    .unwrap();
                let pic = item
                    .split("PIC ")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap();
                let companion = format!(
                    "01 {name}-O PIC {pic} USAGE DISPLAY{}.",
                    if pic.starts_with('S') {
                        " SIGN IS TRAILING SEPARATE"
                    } else {
                        ""
                    }
                );
                assert!(program.data.contains(&companion), "seed {seed}: {name}");
                assert!(
                    program
                        .statements
                        .contains(&format!("MOVE {name} TO {name}-O."))
                );
            }
        }
    }

    #[test]
    fn cobol_differential_minimizer_rejects_uninitialized_displays() {
        let mut program = generated(1);
        assert!(displays_initialized(&program));
        let output_move = program
            .statements
            .iter()
            .position(|s| s.starts_with("MOVE ") && s.contains("-O."))
            .unwrap();
        program.statements.remove(output_move);
        assert!(!displays_initialized(&program));
    }

    #[test]
    fn cobol_differential_generator_has_no_issue_labels() {
        let source = include_str!("cobol_differential/generator.rs");
        for number in [158, 159, 229, 230, 231, 251, 264, 265, 286] {
            assert!(!source.contains(&number.to_string()));
        }
    }

    #[test]
    fn cobol_differential_anchor_classifier() {
        for anchor in anchors() {
            let mut product = CobolObservation {
                stdout_records: vec![],
                return_code: Some(0),
                abend_code: None,
                abend_reason: None,
            };
            if anchor.class == "#286" {
                product.return_code = None;
                product.abend_code = Some("SIZE-ERROR".into());
            }
            let reference = CobolObservation {
                stdout_records: vec![],
                return_code: Some(0),
                abend_code: None,
                abend_reason: None,
            };
            let sig = signature(&anchor, &product);
            assert_eq!(
                classify(&anchor, &sig, &product, &reference),
                Some(anchor.class.as_str()),
                "{sig}"
            );
        }
    }

    #[test]
    fn cobol_differential_classifier_gap_regressions() {
        let normal = CobolObservation {
            stdout_records: vec![],
            return_code: Some(0),
            abend_code: None,
            abend_reason: None,
        };
        let cases = [
            (
                "#251",
                vec!["01 A PIC X(3).", "01 R PIC S9(3) USAGE DISPLAY."],
                vec!["MOVE A TO R."],
            ),
            (
                "#230",
                vec!["01 A PIC 9(3).", "01 R PIC $$9.9."],
                vec!["MOVE A TO R."],
            ),
            (
                "#264",
                vec!["01 A PIC 9V99 USAGE COMP-4 VALUE 1.23.", "01 R PIC 9V99."],
                vec!["MOVE A TO R."],
            ),
        ];
        for (issue, data, statements) in cases {
            let program = Program {
                class: issue.into(),
                data: data.into_iter().map(str::to_owned).collect(),
                statements: statements.into_iter().map(str::to_owned).collect(),
            };
            let sig = signature(&program, &normal);
            if issue == "#264" {
                assert!(sig.contains("usages=COMP-4+DISPLAY"));
            }
            assert_eq!(classify(&program, &sig, &normal, &normal), Some(issue));
        }
        let packed = Program {
            class: String::new(),
            data: vec!["01 A PIC 9V99 USAGE COMP-3 VALUE 1.23.".into()],
            statements: vec!["DISPLAY A.".into()],
        };
        let sig = signature(&packed, &normal);
        assert_ne!(classify(&packed, &sig, &normal, &normal), Some("#264"));
        let unscaled = Program {
            class: String::new(),
            data: vec!["01 A PIC 9(4) USAGE COMP VALUE 1234.".into()],
            statements: vec!["DISPLAY A.".into()],
        };
        let sig = signature(&unscaled, &normal);
        assert_ne!(classify(&unscaled, &sig, &normal, &normal), Some("#264"));
        let mut abend = normal.clone();
        abend.return_code = None;
        abend.abend_code = Some("SIZE-ERROR".into());
        let mut program = anchors().into_iter().find(|p| p.class == "#286").unwrap();
        let sig = signature(&program, &abend);
        assert_eq!(classify(&program, &sig, &abend, &normal), Some("#286"));
        program
            .statements
            .push("ADD 1 TO N0 ON SIZE ERROR CONTINUE END-ADD.".into());
        assert_ne!(classify(&program, &sig, &abend, &normal), Some("#286"));
    }

    #[test]
    fn cobol_differential_minimizer_removes_statements_and_items() {
        let program = Program {
            class: "synthetic".into(),
            data: vec!["01 UNUSED PIC X.".into(), "01 DIFF PIC X.".into()],
            statements: vec![
                "MOVE 'A' TO DIFF.".into(),
                "DISPLAY DIFF.".into(),
                "DISPLAY 'EXTRA'.".into(),
            ],
        };
        let reduced = minimize(program, |candidate| {
            let text = candidate.source();
            text.contains("01 DIFF") && text.contains("DISPLAY DIFF")
        });
        assert_eq!(reduced.data, vec!["01 DIFF PIC X."]);
        assert_eq!(reduced.statements, vec!["DISPLAY DIFF."]);
    }

    #[test]
    fn cobol_differential_receipt_schema_validation() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let validator = jsonschema::draft202012::new(&schema).unwrap();
        let mut value = serde_json::json!({
            "schema_version":"mainframe-env.cobol-differential-receipt@2",
            "reference":"GnuCOBOL", "licensed_credit":0, "cobc_path":"/bin/cobc",
            "cobc_version":"cobc (GnuCOBOL) 3.2.0", "cobc_sha256":format!("sha256:{}", "0".repeat(64)),
            "compiler_flags":REFERENCE_FLAGS, "seed_start":1,"seed_end":1,
            "seeds_run":1,"rejected_reference":0,"rejected_product":0,
            "anchors":{},"anchor_programs":{},"generated_reached":{},"divergences":[]
        });
        assert!(validator.is_valid(&value));
        for classification in [
            "known-defect",
            "allowed-difference",
            "product-defect-candidate",
            "generator-bug",
        ] {
            value["divergences"] = serde_json::json!([{
                "origin":"generated", "seed":1, "class":"sample", "signature":"verbs=MOVE",
                "count":1, "classification":classification, "reason":null,
                "minimized_program":"PROCEDURE DIVISION.",
                "product":{"stdout_records":[],"return_code":0,"abend_code":null,"abend_reason":null},
                "reference":{"stdout_records":[],"return_code":0,"abend_code":null,"abend_reason":null}
            }]);
            assert!(validator.is_valid(&value), "{classification}");
        }
        value["divergences"][0]["classification"] = "known-difference".into();
        assert!(!validator.is_valid(&value));
        value["divergences"] = serde_json::json!([]);
        value["licensed_credit"] = 1.into();
        assert!(!validator.is_valid(&value));
    }
}
