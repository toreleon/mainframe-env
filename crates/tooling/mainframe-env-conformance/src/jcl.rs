use mainframe_env_batch::{
    JclBundle, JclConversionLimits, JclExpansionLimits, JclSyntaxLimits, analyze_jcl_syntax,
    convert_jcl, parse_jcl_statements, parse_jes2_statements,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const FIXTURES: &str =
    include_str!("../../../../conformance/0.7/fixtures/jcl-conformance-fixtures.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    source_catalog_sha256: String,
    fixtures: Vec<JclFixture>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JclFixture {
    id: String,
    primary: String,
    #[serde(default)]
    includes: BTreeMap<String, String>,
    #[serde(default)]
    cataloged_procedures: BTreeMap<String, String>,
    #[serde(default)]
    procedure_libraries: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    symbols: BTreeMap<String, String>,
}

impl JclFixture {
    fn bundle(&self) -> JclBundle {
        JclBundle {
            primary: self.primary.clone(),
            includes: self.includes.clone(),
            cataloged_procedures: self.cataloged_procedures.clone(),
            procedure_libraries: self.procedure_libraries.clone(),
            symbols: self.symbols.clone(),
        }
    }
}

#[must_use]
pub fn jcl_fixture_runtime() -> JclFixtureRuntime {
    let catalog = fixture_catalog();
    let fixtures = catalog
        .fixtures
        .into_iter()
        .map(|fixture| (fixture.id.clone(), fixture))
        .collect::<BTreeMap<_, _>>();
    JclFixtureRuntime { fixtures }
}

pub struct JclFixtureRuntime {
    fixtures: BTreeMap<String, JclFixture>,
}

impl JclFixtureRuntime {
    #[must_use]
    pub fn contains(&self, fixture_id: &str) -> bool {
        self.fixtures.contains_key(fixture_id)
    }

    pub fn execute(&self, fixture_id: &str) -> Result<Vec<u8>, String> {
        let fixture = self
            .fixtures
            .get(fixture_id)
            .ok_or_else(|| format!("unknown JCL fixture {fixture_id}"))?;
        let bundle = fixture.bundle();
        let syntax = analyze_jcl_syntax(&bundle, JclConversionLimits::default().syntax)
            .map_err(|problem| problem.to_string())?;
        let jcl = parse_jcl_statements(&syntax);
        let jecl = parse_jes2_statements(&syntax);
        let conversion = convert_jcl(&bundle, JclConversionLimits::default())
            .map_err(|problem| problem.to_string())?;
        serde_json::to_vec(&serde_json::json!({
            "recognized": !jcl.statements().is_empty() || !jecl.statements().is_empty(),
            "plan": conversion.plan().is_some(),
            "diagnostic_codes": conversion
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<Vec<_>>(),
            "plan_identity": conversion.plan().map(|plan| plan.plan_identity()),
        }))
        .map_err(|error| error.to_string())
    }
}

fn fixture_catalog() -> FixtureCatalog {
    let catalog: FixtureCatalog =
        serde_json::from_str(FIXTURES).expect("generated JCL fixture catalog is valid JSON");
    assert_eq!(
        catalog.schema_version, "mainframe-env.jcl-conformance-fixtures@1",
        "generated JCL fixture catalog contract"
    );
    assert_eq!(catalog.target_version, "0.7.0");
    assert_eq!(
        catalog.source_catalog_sha256,
        mainframe_env_batch::JCL_OFFICIAL_CATALOG_SHA256
    );
    catalog
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JclExitReceipt {
    pub schema_version: &'static str,
    pub status: &'static str,
    pub official_rows: usize,
    pub valid_fixture_plans: usize,
    pub invalid_fixture_rejections: usize,
    pub deterministic_plan_pairs: usize,
    pub forbidden_mutation_cases: usize,
    pub malformed_recovery_cases: usize,
    pub scale_boundary_cases: usize,
    pub compatibility_plans: usize,
    pub carddemo_representative_plans: usize,
    pub plan_set_sha256: String,
    pub historical_differential_receipt_sha256: String,
    pub licensed_differential: &'static str,
}

pub fn verify_jcl_exit() -> Result<JclExitReceipt, String> {
    let catalog = fixture_catalog();
    let mut valid_fixture_plans = 0usize;
    let mut invalid_fixture_rejections = 0usize;
    let mut deterministic_plan_pairs = 0usize;
    let mut forbidden_mutation_cases = 0usize;
    let mut plan_set = Sha256::new();
    for fixture in &catalog.fixtures {
        let bundle = fixture.bundle();
        let unchanged = bundle.clone();
        let first = convert_jcl(&bundle, JclConversionLimits::default())
            .map_err(|problem| problem.to_string())?;
        if bundle != unchanged {
            return Err(format!(
                "JCL planning mutated its input fixture: {}",
                fixture.id
            ));
        }
        forbidden_mutation_cases += 1;
        if fixture.id.ends_with(".valid") {
            let plan = first
                .plan()
                .ok_or_else(|| format!("valid JCL fixture has no plan: {}", fixture.id))?;
            valid_fixture_plans += 1;
            plan_set.update((plan.plan_identity().len() as u64).to_be_bytes());
            plan_set.update(plan.plan_identity().as_bytes());
            let second = convert_jcl(&bundle, JclConversionLimits::default())
                .map_err(|problem| problem.to_string())?;
            if second.plan().map(|plan| plan.plan_identity()) != Some(plan.plan_identity()) {
                return Err(format!("JCL plan is nondeterministic: {}", fixture.id));
            }
            deterministic_plan_pairs += 1;
        } else if first.plan().is_none() {
            invalid_fixture_rejections += 1;
        } else {
            return Err(format!(
                "invalid JCL fixture produced a plan: {}",
                fixture.id
            ));
        }
    }
    let malformed_recovery_cases = verify_malformed_recovery()?;
    let scale_boundary_cases = verify_scale_boundaries()?;
    let (compatibility_plans, carddemo_representative_plans) = verify_compatibility_plans()?;
    let historical =
        include_bytes!("../../../../conformance/0.1/evidence/differential/jcl-jes.json");
    Ok(JclExitReceipt {
        schema_version: "mainframe-env.jcl-exit@1",
        status: "pass-with-licensed-differential-pending",
        official_rows: 237,
        valid_fixture_plans,
        invalid_fixture_rejections,
        deterministic_plan_pairs,
        forbidden_mutation_cases,
        malformed_recovery_cases,
        scale_boundary_cases,
        compatibility_plans,
        carddemo_representative_plans,
        plan_set_sha256: format!("sha256:{:x}", plan_set.finalize()),
        historical_differential_receipt_sha256: format!("sha256:{:x}", Sha256::digest(historical)),
        licensed_differential: "pending-no-pinned-licensed-oracle-receipt",
    })
}

fn verify_malformed_recovery() -> Result<usize, String> {
    let cases = [
        "BROKEN\n//J JOB\n//S EXEC PGM=IEFBR14\n",
        "//J JOB CLASS=AB\n//S EXEC PGM=IEFBR14\n",
        "//J JOB\n//S EXEC PGM=TOO-LONG9\n//T EXEC PGM=IEFBR14\n",
        "//J JOB\n// IF (RC = 0) THEN\n//S EXEC PGM=IEFBR14\n",
        "//J JOB\n//S EXEC PGM=IEFBR14\n/*PRIORITY 16\n",
        "//J JOB\n//S EXEC PGM=IEFBR14\n//D DD DATA,DLM=@@\nNO DELIMITER\n",
    ];
    for source in cases {
        let bundle = JclBundle {
            primary: source.into(),
            ..JclBundle::default()
        };
        let syntax = analyze_jcl_syntax(&bundle, JclConversionLimits::default().syntax)
            .map_err(|problem| problem.to_string())?;
        let parsed = parse_jcl_statements(&syntax);
        if parsed.statements().is_empty() {
            return Err("malformed JCL did not recover at a later statement boundary".into());
        }
        let conversion = convert_jcl(&bundle, JclConversionLimits::default())
            .map_err(|problem| problem.to_string())?;
        if conversion.plan().is_some() || conversion.diagnostics().is_empty() {
            return Err("malformed JCL did not fail closed after recovery".into());
        }
    }
    Ok(cases.len())
}

fn verify_scale_boundaries() -> Result<usize, String> {
    let source = JclBundle {
        primary: "//J JOB\n//S EXEC PGM=IEFBR14\n".into(),
        ..JclBundle::default()
    };
    let mut cases = 0usize;
    let limits = JclConversionLimits {
        syntax: JclSyntaxLimits {
            max_file_bytes: 8,
            ..JclSyntaxLimits::default()
        },
        ..JclConversionLimits::default()
    };
    if convert_jcl(&source, limits).is_err() {
        cases += 1;
    }
    let limits = JclConversionLimits {
        syntax: JclSyntaxLimits {
            max_lines: 1,
            ..JclSyntaxLimits::default()
        },
        ..JclConversionLimits::default()
    };
    if convert_jcl(&source, limits).is_err() {
        cases += 1;
    }
    let limits = JclConversionLimits {
        max_plan_nodes: 1,
        ..JclConversionLimits::default()
    };
    if convert_jcl(&source, limits).is_err() {
        cases += 1;
    }
    let limits = JclConversionLimits {
        expansion: JclExpansionLimits {
            max_expanded_statements: 1,
            ..JclExpansionLimits::default()
        },
        ..JclConversionLimits::default()
    };
    if convert_jcl(&source, limits).is_err() {
        cases += 1;
    }
    if cases != 4 {
        return Err("one or more JCL scale limits did not fail closed".into());
    }
    Ok(cases)
}

fn verify_compatibility_plans() -> Result<(usize, usize), String> {
    let fixtures = [
        (
            "//LINKJOB JOB CLASS=A\n//RUN EXEC PGM=LINKMAIN\n//TRNXFILE DD DSN=IBMUSER.TRNX,DISP=SHR\n",
            1,
            1,
        ),
        ("//ABENDJOB JOB CLASS=A\n//FAIL EXEC PGM=ABENDCHK\n", 1, 0),
        (
            "//COPYJOB JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.INPUT,DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.OUTPUT,DISP=OLD\n",
            1,
            2,
        ),
        (
            "//SORTJOB JOB CLASS=A\n//SORT EXEC PGM=SORT\n//SORTIN DD DSN=IBMUSER.INPUT,DISP=SHR\n//SORTOUT DD DSN=IBMUSER.SORTOUT,DISP=OLD\n//SYSIN DD *\n SORT FIELDS=(1,6,CH,A)\n/*\n",
            1,
            3,
        ),
        (
            "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.TARGET,DISP=OLD\n//SYSIN DD *\n DELETE IBMUSER.TARGET\n/*\n",
            1,
            3,
        ),
        (
            "//TEMPJOB JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//WORK DD DSN=&&WORK,DISP=(NEW,PASS,DELETE)\n//USE EXEC PGM=IEFBR14\n//INPUT DD DSN=&&WORK,DISP=(OLD,DELETE,DELETE)\n",
            2,
            2,
        ),
        (
            "//GDGJOB JOB CLASS=A\n//DEFINE EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE GENERATIONDATAGROUP (NAME(IBMUSER.HISTORY) LIMIT(3))\n/*\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD *\nGENERATION\n/*\n//SYSUT2 DD DSN=IBMUSER.HISTORY(+1),DISP=(NEW,CATLG,DELETE)\n",
            2,
            3,
        ),
        (
            "//AIXJOB JOB CLASS=A\n//AIX EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE ALTERNATEINDEX (NAME(IBMUSER.BASE.AIX))\n/*\n",
            1,
            1,
        ),
        (
            "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.JCL(CHILD),DISP=SHR\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n",
            1,
            2,
        ),
    ];
    for (source, expected_steps, expected_dds) in fixtures {
        let bundle = JclBundle {
            primary: source.into(),
            ..JclBundle::default()
        };
        let conversion = convert_jcl(&bundle, JclConversionLimits::default())
            .map_err(|problem| problem.to_string())?;
        let plan = conversion
            .legacy_plan()
            .ok_or("compatibility fixture did not produce a legacy adapter plan")?;
        if plan.steps.len() != expected_steps
            || plan.steps.iter().map(|step| step.dds.len()).sum::<usize>() != expected_dds
        {
            return Err("compatibility plan shape differs from its reviewed expectation".into());
        }
    }
    Ok((fixtures.len(), fixtures.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_registry_is_exact_and_bounded() {
        let catalog = fixture_catalog();
        assert_eq!(catalog.fixtures.len(), 474);
        assert_eq!(
            catalog
                .fixtures
                .iter()
                .map(|fixture| fixture.id.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            474
        );
    }
}
