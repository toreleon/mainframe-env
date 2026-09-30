use mainframe_env_conformance::profile_intake::{IntakeError, run};
use std::{fs, path::PathBuf, process::Command};

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn profile_intake_maps_gaps_and_verifies_pin() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = std::env::temp_dir().join(format!("profile-intake-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("base/src")).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    git(&dir, &["config", "user.name", "Fixture"]);
    git(
        &dir,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/fixture.git",
        ],
    );
    fs::write(dir.join("LICENSE"), "self-authored fixture license\n").unwrap();
    fs::write(dir.join("base/src/TEST.cbl"), "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. TEST.\n       PROCEDURE DIVISION.\n      *    EXEC SQL DELETE FROM T END-EXEC.\n      /    EXEC SQL DROP TABLE T END-EXEC.\n                                                                        EXEC SQL ALTER TABLE T END-EXEC.\n           EXEC CICS RETURN END-EXEC.\n           EXEC SQL SELECT X FROM T END-EXEC.\n           EXEC SQL DECLARE C1 CURSOR FOR SELECT X FROM T END-EXEC.\n           EXEC SQL SET :HV = 1 END-EXEC.\n           START TEST-FILE BOGUS.\n           GOBACK.\n").unwrap();
    fs::write(dir.join("base/src/SQLINC.cbl"), "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. SQLINC.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n       01 ROW-GROUP.\n           EXEC SQL INCLUDE DCLROW END-EXEC.\n       PROCEDURE DIVISION.\n           GOBACK.\n").unwrap();
    fs::write(
        dir.join("base/src/DCLROW.cpy"),
        "       05 ROW-KEY PIC X(2).\n",
    )
    .unwrap();
    fs::write(dir.join("base/src/SQLCA.cbl"), "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. SQLCA.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n           EXEC SQL INCLUDE SQLCA END-EXEC.\n       PROCEDURE DIVISION.\n           GOBACK.\n").unwrap();
    fs::write(dir.join("base/src/TEST.jcl"), "//TEST JOB 1\n//STEP EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE CLUSTER (NAME(TEST))\n/*\n//OTHER EXEC PGM=UNKNOWN\n//COPY EXEC PGM=IEBGENER\n//OUT DD DSNAME=<HLQ>.X,DISP=SHR\n//LIB JCLLIB ORDER=(<HLQ>.PROC)\n//PROC EXEC EXTERNAL\n").unwrap();
    fs::write(
        dir.join("base/src/TEST.bms"),
        "TEST DFHMSD TYPE=MAP\nMAP1 DFHMDI SIZE=(24,80)\nF1 DFHMDF POS=(1,1),LENGTH=1\n",
    )
    .unwrap();
    fs::write(dir.join("base/src/mystery.zzz"), "unknown\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "fixture"]);
    let commit = git(&dir, &["rev-parse", "HEAD"]);
    let manifest = dir.with_extension("json");
    let manifest_value = serde_json::json!({"schema_version":"mainframe-env.profile-corpus@1","origin":"https://example.invalid/fixture.git","commit":commit,"license_id":"EPL-2.0","license_file":"LICENSE","layout":[{"glob":"base/src/*.cbl","kind":"cobol"},{"glob":"base/src/*.cpy","kind":"copybook"},{"glob":"base/src/*.jcl","kind":"jcl"},{"glob":"base/src/*.bms","kind":"bms"},{"glob":"LICENSE","kind":"other"}]});
    fs::write(&manifest, serde_json::to_vec(&manifest_value).unwrap()).unwrap();
    let first = run(&manifest, &dir, &root).unwrap();
    let second = run(&manifest, &dir, &root).unwrap();
    let bytes = serde_json::to_vec_pretty(&first).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&second).unwrap());
    let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("conformance/profiles/schemas/profile-intake.schema.json")).unwrap(),
    )
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&report)
    );
    let members = report["members"].as_array().unwrap();
    assert!(members.iter().any(|m| m["kind"] == "unclassified"));
    let constructs = members
        .iter()
        .flat_map(|m| m["constructs"].as_array().unwrap());
    let rows: Vec<_> = constructs.collect();
    assert!(
        rows.iter()
            .any(|c| c["name"] == "RETURN" && c["row_id"].is_string())
    );
    assert!(
        rows.iter()
            .any(|c| c["name"] == "SELECT" && c["row_id"].is_string())
    );
    assert!(
        rows.iter()
            .any(|c| c["name"] == "DEFINE CLUSTER" && c["row_id"].is_string())
    );
    assert!(
        rows.iter()
            .any(|c| c["name"] == "UNKNOWN" && c["registry"] == "unregistered")
    );
    assert!(
        rows.iter()
            .any(|c| c["name"] == "START" && c["row_id"].is_string())
    );
    assert!(members.iter().any(|m| {
        m["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"].as_str().unwrap_or("").starts_with("MECOB"))
    }));
    let mut missing = Vec::new();
    let check = |condition: bool, name: &str, missing: &mut Vec<String>| {
        if !condition {
            missing.push(name.to_string());
        }
    };
    check(
        rows.iter().any(|c| {
            c["name"] == "IEBGENER"
                && c["registry"] == "matched"
                && c["product_support"] == "implemented"
        }),
        "registered IEBGENER",
        &mut missing,
    );
    check(
        rows.iter()
            .any(|c| c["name"] == "UNKNOWN" && c["reason"] == "no-program-registry-entry"),
        "unknown program",
        &mut missing,
    );
    check(
        !rows.iter().any(|c| c["name"] == "DELETE"),
        "commented SQL ignored",
        &mut missing,
    );
    check(
        !rows
            .iter()
            .any(|c| c["name"] == "DROP" || c["name"] == "ALTER"),
        "slash comment and column 73 ignored",
        &mut missing,
    );
    check(
        !rows.iter().any(|c| c["name"] == "EXEC SQL"),
        "SQL wrapper ignored",
        &mut missing,
    );
    check(
        rows.iter()
            .any(|c| c["name"] == "DECLARE CURSOR" && c["row_id"].is_string()),
        "DECLARE CURSOR row",
        &mut missing,
    );
    check(
        rows.iter()
            .any(|c| c["name"] == "SET ASSIGNMENT-STATEMENT" && c["row_id"].is_string()),
        "SET assignment row",
        &mut missing,
    );
    check(
        rows.iter()
            .any(|c| c["name"] == "INCLUDE" && c["row_id"].is_string()),
        "INCLUDE row",
        &mut missing,
    );
    check(
        members
            .iter()
            .find(|m| m["path"] == "base/src/SQLINC.cbl")
            .is_some_and(|m| {
                m["stages"]["semantic"] == "complete"
                    && !m["diagnostics"].as_array().unwrap().iter().any(|d| {
                        d["message"]
                            .as_str()
                            .unwrap_or("")
                            .contains("elementary item requires PICTURE")
                    })
            }),
        "SQL include under group",
        &mut missing,
    );
    check(
        members
            .iter()
            .find(|m| m["path"] == "base/src/SQLCA.cbl")
            .is_some_and(|m| {
                m["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["message"].as_str().unwrap_or("").contains("SQLCA"))
            }),
        "missing SQLCA explicit",
        &mut missing,
    );
    check(
        rows.iter()
            .filter(|c| c["family"] == "bms")
            .all(|c| c["registry"] == "no-catalog-unit"),
        "BMS no catalog unit",
        &mut missing,
    );
    check(
        !report["summary"]["top_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["family"] == "bms"),
        "BMS excluded from gaps",
        &mut missing,
    );
    check(
        members
            .iter()
            .any(|m| m["path"] == "LICENSE" && m["kind"] == "other" && m["status"] == "skipped"),
        "other layout kind",
        &mut missing,
    );
    check(
        members
            .iter()
            .find(|m| m["path"] == "base/src/TEST.jcl")
            .is_some_and(|m| {
                m["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["code"] == "MEJCL0745" && d["cause"] == "environment-placeholder")
            }),
        "placeholder diagnostic cause",
        &mut missing,
    );
    check(
        members
            .iter()
            .find(|m| m["path"] == "base/src/TEST.jcl")
            .is_some_and(|m| {
                m["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d["cause"] == "external-procedure")
            }),
        "external procedure diagnostic cause",
        &mut missing,
    );
    check(
        report["summary"]["environment"].is_array(),
        "environment aggregate",
        &mut missing,
    );
    assert!(missing.is_empty(), "fail-first: {missing:?}");
    let mut wrong = manifest_value.clone();
    wrong["commit"] = "0000000000000000000000000000000000000000".into();
    fs::write(&manifest, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert!(matches!(
        run(&manifest, &dir, &root),
        Err(IntakeError::Pin(_))
    ));
    wrong["commit"] = commit.into();
    wrong["origin"] = "https://example.invalid/wrong.git".into();
    fs::write(&manifest, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert!(matches!(
        run(&manifest, &dir, &root),
        Err(IntakeError::Pin(_))
    ));
    wrong["origin"] = "https://example.invalid/fixture.git".into();
    fs::write(&manifest, serde_json::to_vec(&wrong).unwrap()).unwrap();
    fs::write(dir.join("dirty"), "dirty").unwrap();
    assert!(matches!(
        run(&manifest, &dir, &root),
        Err(IntakeError::Pin(_))
    ));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_file(&manifest);
}
