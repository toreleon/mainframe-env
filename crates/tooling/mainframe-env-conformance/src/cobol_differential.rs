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

// This grammar is deliberately small. Template choices come from the seed, not
// from issue-specific replay inputs; the anchors below are a separate corpus.
fn generated(seed: u64) -> Program {
    let value = if seed & 1 == 0 { "95.85" } else { "-95.85" };
    let storage = ["DISPLAY", "COMP-3", "COMP", "COMP-5"][(seed as usize / 7) % 4];
    let sign = if seed & 2 == 0 { "S" } else { "" };
    let rounded = if seed & 4 == 0 { " ROUNDED" } else { "" };
    match seed % 9 {
        0 => Program {
            class: "#229".into(),
            data: vec![
                format!("01 A PIC S9(9)V99 VALUE {value}."),
                "01 R PIC +ZZZ,ZZZ,ZZZ.ZZ.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        1 => Program {
            class: "#230".into(),
            data: vec![
                "01 A PIC S9(7)V99 VALUE 95.85.".into(),
                "01 R PIC $$$,$$9.99.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        2 => Program {
            class: "#231".into(),
            data: vec![
                format!("01 A PIC S9(5)V99 VALUE {value}."),
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
        3 => Program {
            class: "#158".into(),
            data: vec![
                "01 A PIC 9(18) VALUE 999999999999999999.".into(),
                "01 R PIC 9(18).".into(),
            ],
            statements: vec!["COMPUTE R = (A + 2) - A.".into(), "DISPLAY R.".into()],
        },
        4 => Program {
            class: "#159".into(),
            data: vec!["01 Q PIC 9V9(12).".into()],
            statements: vec!["COMPUTE Q = 1 / 3.".into(), "DISPLAY Q.".into()],
        },
        5 => {
            let verb_index = (seed as usize / 9) % 5;
            let verb = [
                "ADD 2 TO R",
                "SUBTRACT 2 FROM R",
                "MULTIPLY 2 BY R",
                "DIVIDE 2 INTO R",
                "COMPUTE",
            ][verb_index];
            let statement = if verb == "COMPUTE" {
                format!("COMPUTE R{rounded} = A + 2.")
            } else {
                format!("{verb}{rounded}.")
            };
            Program {
                class: format!(
                    "arithmetic/{verb_index}/{storage}/{sign}/{}",
                    rounded == " ROUNDED"
                ),
                data: vec![
                    format!("01 A PIC {sign}9(7)V99 USAGE {storage} VALUE 95.85."),
                    format!("01 R PIC {sign}9(7)V99 USAGE {storage} VALUE 95.85."),
                    "01 OUT-NUM PIC 9(7)V99.".into(),
                ],
                statements: vec![
                    statement,
                    "MOVE R TO OUT-NUM.".into(),
                    "DISPLAY OUT-NUM.".into(),
                ],
            }
        }
        6 => Program {
            class: "numeric-edited".into(),
            data: vec![
                format!("01 A PIC {sign}9(7)V99 USAGE {storage} VALUE 95.85."),
                "01 R PIC Z,ZZZ,ZZ9.99.".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        7 => Program {
            class: "#251".into(),
            data: vec![
                "01 A PIC X(6) VALUE '123456'.".into(),
                "01 R PIC 9(6).".into(),
            ],
            statements: vec!["MOVE A TO R.".into(), "DISPLAY R.".into()],
        },
        _ => Program {
            class: "literal-to-display".into(),
            data: vec!["01 R PIC 9(6).".into()],
            statements: vec!["MOVE '123456' TO R.".into(), "DISPLAY R.".into()],
        },
    }
}

fn anchors() -> Vec<Program> {
    let mut result = Vec::new();
    for (class, seed) in [
        ("#229", 0),
        ("#230", 1),
        ("#231", 2),
        ("#158", 3),
        ("#159", 4),
    ] {
        let mut program = generated(seed);
        program.class = class.into();
        result.push(program);
    }
    // The issue reports both sign variants and both trailing sign pictures.
    let mut negative = generated(9);
    negative.class = "#229".into();
    negative.data[1] = "01 R PIC -ZZZ,ZZZ,ZZZ.ZZ.".into();
    result.push(negative);
    let mut db = generated(11);
    db.class = "#231".into();
    db.data[0] = "01 A PIC S9(5)V99 VALUE -95.85.".into();
    result.push(db);
    result
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

struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "cobol-differential-{}-{}",
            std::process::id(),
            DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)
            .map_err(|error| format!("create campaign work directory: {error}"))?;
        Ok(Self(path))
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
    let arguments: Vec<OsString> = ["-x", "-std=ibm", "-free", "-o", "reference", "input.cbl"]
        .into_iter()
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
        schema_version: "mainframe-env.cobol-differential-receipt@1".into(),
        reference: "GnuCOBOL".into(),
        licensed_credit: 0,
        cobc_path: resolved.display().to_string(),
        cobc_version: version,
        cobc_sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
        compiler_flags: vec!["-x".into(), "-std=ibm".into(), "-free".into()],
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
        let issue = program.class.to_owned();
        let result = run_one(
            &program,
            Some(seed),
            &resolved,
            &work.0,
            &mut receipt,
            &mut seen,
        )?;
        receipt.seeds_run += 1;
        if issue.starts_with('#') && issue != "#251" {
            receipt
                .generated_reached
                .entry(issue)
                .and_modify(|v| *v |= result)
                .or_insert(result);
        }
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
            receipt.rejected_reference += 1;
            return Ok(false);
        }
        Trial::RejectedProduct => {
            receipt.rejected_product += 1;
            return Ok(false);
        }
        Trial::Observed(product, reference) => (product, reference),
    };
    if product == reference {
        return Ok(false);
    }
    let class = program.class.to_owned();
    if seen.insert(class.clone()) {
        let reduced = minimize(program.clone(), |candidate| {
            candidate
                .statements
                .iter()
                .any(|statement| !statement.starts_with("DISPLAY"))
                && matches!(trial(candidate, cobc, work), Ok(Trial::Observed(a, b)) if a == product && b == reference)
        });
        let (product, reference) = match trial(&reduced, cobc, work)? {
            Trial::Observed(a, b) => (a, b),
            _ => (product, reference),
        };
        let known = class.starts_with('#') || class == "numeric-edited";
        receipt.divergences.push(DifferentialCase {
            origin: if seed.is_some() {
                "generated"
            } else {
                "anchor"
            }
            .into(),
            seed,
            class: class.clone(),
            count: 1,
            classification: if known {
                "known-difference"
            } else {
                "product-defect-candidate"
            }
            .into(),
            reason: known.then(|| {
                let issue = if class == "numeric-edited" {
                    "229"
                } else {
                    &class[1..]
                };
                format!("https://github.com/toreleon/mainframe-env/issues/{issue}")
            }),
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
            "schema_version":"mainframe-env.cobol-differential-receipt@1",
            "reference":"GnuCOBOL", "licensed_credit":0, "cobc_path":"/bin/cobc",
            "cobc_version":"cobc (GnuCOBOL) 3.2.0", "cobc_sha256":format!("sha256:{}", "0".repeat(64)),
            "compiler_flags":["-x","-std=ibm","-free"], "seed_start":1,"seed_end":1,
            "seeds_run":1,"rejected_reference":0,"rejected_product":0,
            "anchors":{},"anchor_programs":{},"generated_reached":{},"divergences":[]
        });
        assert!(validator.is_valid(&value));
        value["licensed_credit"] = 1.into();
        assert!(!validator.is_valid(&value));
    }
}
