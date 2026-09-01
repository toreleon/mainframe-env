use super::*;

const GENERATED_CATALOG: &str = "conformance/0.7/generated/jcl-catalog.json";
const SEEDS_PATH: &str = "conformance/0.7/fixtures/jcl-fixture-seeds.json";
const FIXTURES_PATH: &str = "conformance/0.7/fixtures/jcl-conformance-fixtures.json";
const FIXTURE_SCHEMA: &str = "conformance/0.7/schemas/jcl-conformance-fixtures.schema.json";
const SPEC_PATH: &str = "conformance/spec/v1/spec.json";
const JCL_BASELINE: &str = "ibm-zos-3.2-jcl-jes2-2026-06";

struct Rendered {
    fixtures: Vec<u8>,
    spec: Vec<u8>,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    let rendered = render(root)?;
    fs::write(root.join(FIXTURES_PATH), rendered.fixtures)
        .map_err(|error| format!("{FIXTURES_PATH}: {error}"))?;
    fs::write(root.join(SPEC_PATH), rendered.spec).map_err(|error| format!("{SPEC_PATH}: {error}"))
}

pub(super) fn check(root: &Path) -> TaskResult {
    let rendered = render(root)?;
    for (relative, expected) in [
        (FIXTURES_PATH, rendered.fixtures),
        (SPEC_PATH, rendered.spec),
    ] {
        let actual =
            fs::read(root.join(relative)).map_err(|error| format!("{relative}: {error}"))?;
        require(
            actual == expected,
            &format!("{relative} is stale; run cargo xtask jcl-conformance"),
        )?;
    }
    Ok(())
}

fn render(root: &Path) -> TaskResult<Rendered> {
    let catalog_path = root.join(GENERATED_CATALOG);
    let catalog = json(&catalog_path)?;
    let seeds_path = root.join(SEEDS_PATH);
    let seeds = json(&seeds_path)?;
    require(
        seeds["schema_version"] == Value::String("mainframe-env.jcl-fixture-seeds@1".into())
            && seeds["target_version"] == Value::String("0.7.0".into()),
        "JCL fixture seed identity is invalid",
    )?;
    let families = array(&catalog, "families", &catalog_path)?;
    let mut fixtures = Vec::new();
    let mut expected_errors = BTreeMap::new();
    for family in families {
        let family_id = text(family, "id", &catalog_path)?;
        for row in array(family, "rows", &catalog_path)? {
            let ordinal = row["ordinal"]
                .as_u64()
                .ok_or("generated JCL row ordinal is invalid")?;
            let keyword = text(row, "keyword", &catalog_path)?;
            let valid_id = fixture_id(family_id, ordinal, "valid");
            let invalid_id = fixture_id(family_id, ordinal, "invalid");
            let (valid, invalid, expected_error) = if family_id == "jcl-statements" {
                let valid = statement_seed(&seeds, keyword)?;
                let mut invalid = valid.clone();
                invalid["id"] = Value::String(invalid_id.clone());
                invalid["primary"] =
                    Value::String(format!("BROKEN\n{}", text(&valid, "primary", &seeds_path)?));
                (with_id(valid, &valid_id), invalid, "MEJCL0703")
            } else if family_id == "jes2-jecl-statements" {
                let (valid_line, invalid_line) = jecl_seed(&seeds, keyword)?;
                (
                    fixture(
                        &valid_id,
                        jecl_source(keyword, &valid_line),
                        BTreeMap::new(),
                        BTreeMap::new(),
                    ),
                    fixture(
                        &invalid_id,
                        jecl_source(keyword, &invalid_line),
                        BTreeMap::new(),
                        BTreeMap::new(),
                    ),
                    "MEJCL0763",
                )
            } else {
                let validation = text(row, "validation", &catalog_path)?;
                let (valid_value, invalid_value) = value_seeds(&seeds, row, validation)?;
                let (valid, invalid) = parameter_fixtures(
                    family_id,
                    keyword,
                    validation,
                    &valid_value,
                    &invalid_value,
                    &valid_id,
                    &invalid_id,
                )?;
                (
                    valid,
                    invalid,
                    if invalid_value == "(" {
                        "MEJCL0743"
                    } else {
                        "MEJCL0745"
                    },
                )
            };
            fixtures.push(valid);
            fixtures.push(invalid);
            expected_errors.insert((family_id.to_string(), ordinal), expected_error.to_string());
        }
    }
    fixtures.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    require(
        fixtures.len() == 474,
        "JCL conformance fixture generator must emit 474 cases",
    )?;
    let fixture_value = json!({
        "schema_version": "mainframe-env.jcl-conformance-fixtures@1",
        "target_version": "0.7.0",
        "source_catalog_sha256": catalog["source_catalog_sha256"],
        "fixtures": fixtures,
    });
    validate_schema_instance(
        &json(&root.join(FIXTURE_SCHEMA))?,
        &fixture_value,
        &root.join(FIXTURE_SCHEMA),
    )?;
    let fixtures_bytes = pretty_json(&fixture_value)?;
    let spec = render_spec(root, &catalog, &fixture_value, &expected_errors)?;
    Ok(Rendered {
        fixtures: fixtures_bytes,
        spec: pretty_json(&spec)?,
    })
}

fn render_spec(
    root: &Path,
    generated_catalog: &Value,
    fixture_catalog: &Value,
    expected_errors: &BTreeMap<(String, u64), String>,
) -> TaskResult<Value> {
    let spec_path = root.join(SPEC_PATH);
    let mut spec = json(&spec_path)?;
    let official = json(&root.join("conformance/0.2/catalogs/jcl-jes2.json"))?;
    let mut rows = spec["rows"].as_array().cloned().unwrap_or_default();
    rows.retain(|row| {
        !row["row_id"]
            .as_str()
            .is_some_and(|row_id| row_id.starts_with(JCL_BASELINE))
    });
    let mut obligations = spec["obligations"].as_array().cloned().unwrap_or_default();
    obligations.retain(|row| {
        !row["row_id"]
            .as_str()
            .is_some_and(|row_id| row_id.starts_with(JCL_BASELINE))
    });
    let mut cases = spec["cases"].as_array().cloned().unwrap_or_default();
    cases.retain(|row| {
        !row["row_id"]
            .as_str()
            .is_some_and(|row_id| row_id.starts_with(JCL_BASELINE))
    });
    let fixture_digests = fixture_catalog["fixtures"]
        .as_array()
        .ok_or("generated JCL fixtures are missing")?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "id", Path::new(FIXTURES_PATH))?.to_string(),
                format!(
                    "sha256:{:x}",
                    Sha256::digest(serde_json::to_vec(fixture).map_err(|error| error.to_string())?)
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    for unit in array(
        &official,
        "units",
        &root.join("conformance/0.2/catalogs/jcl-jes2.json"),
    )? {
        let family = text(unit, "id", &spec_path)?;
        for (index, official_row) in array(unit, "rows", &spec_path)?.iter().enumerate() {
            let ordinal = (index + 1) as u64;
            let row_id = text(official_row, "id", &spec_path)?;
            let slug = format!("{}-{ordinal:04}", family.replace('-', "."));
            let valid_obligation = format!("{slug}.valid");
            let malformed_obligation = format!("{slug}.malformed");
            rows.push(json!({
                "row_id": row_id,
                "operation": format!("jcl.convert.{family}"),
                "input": "jcl.input.bundle",
                "preconditions": ["jcl.fixture.exists"],
                "transition": "jcl.transition.convert",
                "postconditions": [
                    "jcl.observation.recognized",
                    "jcl.observation.plan-present",
                    "jcl.observation.error.MEJCL0703",
                    "jcl.observation.error.MEJCL0743",
                    "jcl.observation.error.MEJCL0745",
                    "jcl.observation.error.MEJCL0763"
                ],
                "conditions": ["jcl.condition.diagnostic"],
                "recovery": "jcl.recovery.record-boundary",
                "oracle": Value::Null,
                "applicable_gates": ["recognized", "validated", "executed", "conditioned", "recovered", "differential"],
                "obligations": [valid_obligation, malformed_obligation],
            }));
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": valid_obligation,
                "applicable_gates": ["recognized", "validated"],
            }));
            obligations.push(json!({
                "row_id": row_id,
                "obligation_id": malformed_obligation,
                "applicable_gates": ["validated"],
            }));
            let valid_fixture = fixture_id(family, ordinal, "valid");
            let invalid_fixture = fixture_id(family, ordinal, "invalid");
            cases.push(case(
                row_id,
                &valid_obligation,
                "recognized",
                &format!("jcl.{slug}.recognized"),
                &valid_fixture,
                "jcl.observation.recognized",
                true,
            ));
            cases.push(case(
                row_id,
                &valid_obligation,
                "validated",
                &format!("jcl.{slug}.validated"),
                &valid_fixture,
                "jcl.observation.plan-present",
                true,
            ));
            let error = expected_errors
                .get(&(family.to_string(), ordinal))
                .ok_or("JCL malformed fixture has no expected error")?;
            cases.push(case(
                row_id,
                &malformed_obligation,
                "validated",
                &format!("jcl.{slug}.malformed"),
                &invalid_fixture,
                &format!("jcl.observation.error.{error}"),
                true,
            ));
        }
    }
    rows.sort_by(|left, right| left["row_id"].as_str().cmp(&right["row_id"].as_str()));
    obligations.sort_by(|left, right| {
        (left["row_id"].as_str(), left["obligation_id"].as_str())
            .cmp(&(right["row_id"].as_str(), right["obligation_id"].as_str()))
    });
    cases.sort_by(|left, right| {
        (
            left["row_id"].as_str(),
            left["obligation_id"].as_str(),
            left["gate"].as_str(),
        )
            .cmp(&(
                right["row_id"].as_str(),
                right["obligation_id"].as_str(),
                right["gate"].as_str(),
            ))
    });
    spec["rows"] = Value::Array(rows);
    spec["obligations"] = Value::Array(obligations);
    spec["cases"] = Value::Array(cases);
    merge_registry_strings(
        &mut spec,
        "operations",
        &[
            "jcl.convert.jcl-statements",
            "jcl.convert.jes2-jecl-statements",
            "jcl.convert.dd-parameters",
            "jcl.convert.exec-parameters",
            "jcl.convert.job-parameters",
            "jcl.convert.output-parameters",
        ],
    )?;
    merge_registry_strings(&mut spec, "input_shapes", &["jcl.input.bundle"])?;
    merge_registry_strings(&mut spec, "predicates", &["jcl.fixture.exists"])?;
    merge_registry_strings(&mut spec, "transitions", &["jcl.transition.convert"])?;
    merge_registry_strings(
        &mut spec,
        "observations",
        &[
            "jcl.observation.recognized",
            "jcl.observation.plan-present",
            "jcl.observation.error.MEJCL0703",
            "jcl.observation.error.MEJCL0743",
            "jcl.observation.error.MEJCL0745",
            "jcl.observation.error.MEJCL0763",
        ],
    )?;
    merge_registry_strings(&mut spec, "conditions", &["jcl.condition.diagnostic"])?;
    merge_registry_strings(&mut spec, "recoveries", &["jcl.recovery.record-boundary"])?;
    merge_registry_strings(&mut spec, "drivers", &["jcl.driver.convert"])?;
    let registry_fixtures = spec["registries"]["fixtures"]
        .as_array_mut()
        .ok_or("shared spec fixture registry is not an array")?;
    registry_fixtures.retain(|fixture| {
        !fixture["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("jcl.fixture."))
    });
    registry_fixtures.extend(
        fixture_digests
            .into_iter()
            .map(|(id, digest)| json!({"id": id, "digest": digest})),
    );
    registry_fixtures.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    require(
        generated_catalog["source_catalog_sha256"] == official_catalog_digest(root)?,
        "JCL generated catalog and official source identity differ",
    )?;
    Ok(spec)
}

fn case(
    row_id: &str,
    obligation_id: &str,
    gate: &str,
    test_id: &str,
    fixture: &str,
    observation: &str,
    recovery: bool,
) -> Value {
    json!({
        "spec_version": "mainframe-env.conformance-ir@1",
        "row_id": row_id,
        "obligation_id": obligation_id,
        "gate": gate,
        "test_id": test_id,
        "driver": "jcl.driver.convert",
        "input": fixture,
        "preconditions": ["jcl.fixture.exists"],
        "expected": [observation],
        "recovery": recovery.then_some("jcl.recovery.record-boundary"),
        "oracle": Value::Null,
    })
}

fn merge_registry_strings(spec: &mut Value, name: &str, values: &[&str]) -> TaskResult {
    let registry = spec["registries"][name]
        .as_array_mut()
        .ok_or_else(|| format!("shared spec registry {name} is not an array"))?;
    registry.retain(|value| {
        !value
            .as_str()
            .is_some_and(|value| value.starts_with("jcl."))
    });
    registry.extend(values.iter().map(|value| Value::String((*value).into())));
    registry.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    registry.dedup();
    Ok(())
}

fn statement_seed(seeds: &Value, keyword: &str) -> TaskResult<Value> {
    let value = seeds["statements"]
        .get(keyword)
        .cloned()
        .ok_or_else(|| format!("JCL fixture seeds omit statement {keyword}"))?;
    let mut fixture = fixture(
        "placeholder",
        text(&value, "primary", Path::new(SEEDS_PATH))?.into(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    for field in [
        "includes",
        "cataloged_procedures",
        "procedure_libraries",
        "symbols",
    ] {
        if let Some(value) = value.get(field) {
            fixture[field] = value.clone();
        }
    }
    Ok(fixture)
}

fn jecl_seed(seeds: &Value, keyword: &str) -> TaskResult<(String, String)> {
    let value = seeds["jecl"]
        .get(keyword)
        .ok_or_else(|| format!("JCL fixture seeds omit JECL {keyword}"))?;
    Ok((
        text(value, "valid", Path::new(SEEDS_PATH))?.into(),
        text(value, "invalid", Path::new(SEEDS_PATH))?.into(),
    ))
}

fn value_seeds(seeds: &Value, row: &Value, validation: &str) -> TaskResult<(String, String)> {
    let seed = seeds["value_shapes"]
        .get(validation)
        .ok_or_else(|| format!("JCL fixture seeds omit value shape {validation}"))?;
    let mut valid = text(seed, "valid", Path::new(SEEDS_PATH))?.to_string();
    if valid == "$FIRST_CHOICE" {
        valid = row["choices"]
            .as_array()
            .and_then(|choices| choices.first())
            .and_then(Value::as_str)
            .ok_or("enum JCL row has no first choice")?
            .into();
    } else if validation == "integer" {
        valid = row["minimum"].as_u64().unwrap_or(1).to_string();
    }
    let mut invalid = text(seed, "invalid", Path::new(SEEDS_PATH))?.to_string();
    if validation == "integer"
        && let Some(maximum) = row["maximum"].as_u64()
    {
        invalid = maximum.saturating_add(1).to_string();
    }
    Ok((valid, invalid))
}

fn parameter_fixtures(
    family: &str,
    keyword: &str,
    validation: &str,
    valid: &str,
    invalid: &str,
    valid_id: &str,
    invalid_id: &str,
) -> TaskResult<(Value, Value)> {
    let procedure = BTreeMap::from([(
        "PROC1".into(),
        "//PROC1 PROC\n//PS EXEC PGM=IEFBR14\n// PEND\n".into(),
    )]);
    let (valid_source, invalid_source, procedures) = match family {
        "job-parameters" if keyword == "POSITIONAL-ACCOUNTING" => (
            "//J JOB (A)\n//S EXEC PGM=IEFBR14\n".into(),
            "//J JOB ''\n//S EXEC PGM=IEFBR14\n".into(),
            BTreeMap::new(),
        ),
        "job-parameters" if keyword == "POSITIONAL-PROGRAMMER" => (
            "//J JOB (A),'PROGRAMMER'\n//S EXEC PGM=IEFBR14\n".into(),
            "//J JOB (A),''\n//S EXEC PGM=IEFBR14\n".into(),
            BTreeMap::new(),
        ),
        "job-parameters" => (
            format!("//J JOB {keyword}={valid}\n//S EXEC PGM=IEFBR14\n"),
            format!("//J JOB {keyword}={invalid}\n//S EXEC PGM=IEFBR14\n"),
            BTreeMap::new(),
        ),
        "exec-parameters" if keyword == "PROC" => (
            "//J JOB\n//S EXEC PROC=PROC1\n".into(),
            format!("//J JOB\n//S EXEC PROC={invalid}\n"),
            procedure,
        ),
        "exec-parameters" if keyword == "PGM" => (
            format!("//J JOB\n//S EXEC PGM={valid}\n"),
            format!("//J JOB\n//S EXEC PGM={invalid}\n"),
            BTreeMap::new(),
        ),
        "exec-parameters" => (
            format!("//J JOB\n//S EXEC PGM=IEFBR14,{keyword}={valid}\n"),
            format!("//J JOB\n//S EXEC PGM=IEFBR14,{keyword}={invalid}\n"),
            BTreeMap::new(),
        ),
        "dd-parameters" if matches!(keyword, "*" | "DATA" | "DUMMY") => {
            let valid_source = if keyword == "*" {
                "//J JOB\n//S EXEC PGM=IEFBR14\n//D DD *\nDATA\n/*\n".into()
            } else if keyword == "DATA" {
                "//J JOB\n//S EXEC PGM=IEFBR14\n//D DD DATA\nDATA\n/*\n".into()
            } else {
                "//J JOB\n//S EXEC PGM=IEFBR14\n//D DD DUMMY\n".into()
            };
            (
                valid_source,
                format!("//J JOB\n//S EXEC PGM=IEFBR14\n//D DD {keyword}=BAD\n"),
                BTreeMap::new(),
            )
        }
        "dd-parameters" if matches!(keyword, "DDNAME" | "REFDD") => (
            format!(
                "//J JOB\n//S EXEC PGM=IEFBR14\n//BASE DD DSNAME=USER.BASE\n//D DD {keyword}=*.BASE\n"
            ),
            format!("//J JOB\n//S EXEC PGM=IEFBR14\n//D DD {keyword}={invalid}\n"),
            BTreeMap::new(),
        ),
        "dd-parameters" => (
            format!("//J JOB\n//S EXEC PGM=IEFBR14\n//D DD {keyword}={valid}\n"),
            format!("//J JOB\n//S EXEC PGM=IEFBR14\n//D DD {keyword}={invalid}\n"),
            BTreeMap::new(),
        ),
        "output-parameters" => (
            format!("//J JOB\n//S EXEC PGM=IEFBR14\n//O OUTPUT {keyword}={valid}\n"),
            format!("//J JOB\n//S EXEC PGM=IEFBR14\n//O OUTPUT {keyword}={invalid}\n"),
            BTreeMap::new(),
        ),
        _ => {
            return Err(format!(
                "unknown JCL parameter family {family}/{validation}"
            ));
        }
    };
    Ok((
        fixture(valid_id, valid_source, BTreeMap::new(), procedures.clone()),
        fixture(invalid_id, invalid_source, BTreeMap::new(), procedures),
    ))
}

fn jecl_source(keyword: &str, line: &str) -> String {
    if keyword == "SIGNON" {
        format!("{line}\n//J JOB\n//S EXEC PGM=IEFBR14\n")
    } else if matches!(keyword, "JOBPARM" | "PRIORITY" | "XEQ") {
        format!("//J JOB\n{line}\n//S EXEC PGM=IEFBR14\n")
    } else {
        format!("//J JOB\n//S EXEC PGM=IEFBR14\n{line}\n")
    }
}

fn fixture(
    id: &str,
    primary: String,
    includes: BTreeMap<String, String>,
    procedures: BTreeMap<String, String>,
) -> Value {
    json!({
        "id": id,
        "primary": primary,
        "includes": includes,
        "cataloged_procedures": procedures,
        "procedure_libraries": {},
        "symbols": {},
    })
}

fn with_id(mut fixture: Value, id: &str) -> Value {
    fixture["id"] = Value::String(id.into());
    fixture
}

fn fixture_id(family: &str, ordinal: u64, disposition: &str) -> String {
    format!("jcl.fixture.{family}.{ordinal:04}.{disposition}")
}

fn official_catalog_digest(root: &Path) -> TaskResult<Value> {
    let index = json(&root.join("conformance/0.2/catalogs/index.json"))?;
    array(
        &index,
        "baselines",
        &root.join("conformance/0.2/catalogs/index.json"),
    )?
    .iter()
    .find(|baseline| baseline["id"] == Value::String(JCL_BASELINE.into()))
    .map(|baseline| baseline["catalog_sha256"].clone())
    .ok_or_else(|| "official JCL catalog receipt is missing".into())
}

pub(super) struct JclConformanceRuntime {
    fixture: JclRuntimeFixture,
    recognized: Recognized,
    plan_present: PlanPresent,
    lexical_error: ErrorCode,
    operand_error: ErrorCode,
    parameter_error: ErrorCode,
    jecl_error: ErrorCode,
}

pub(super) fn runtime() -> JclConformanceRuntime {
    JclConformanceRuntime {
        fixture: JclRuntimeFixture(mainframe_env_conformance::jcl_fixture_runtime()),
        recognized: Recognized,
        plan_present: PlanPresent,
        lexical_error: ErrorCode("MEJCL0703"),
        operand_error: ErrorCode("MEJCL0743"),
        parameter_error: ErrorCode("MEJCL0745"),
        jecl_error: ErrorCode("MEJCL0763"),
    }
}

impl JclConformanceRuntime {
    pub(super) fn registry<'a>(
        &'a self,
        spec: &CompiledSpec,
        limits: ConformanceLimits,
    ) -> Result<RuntimeRegistry<'a>, SpecProblem> {
        RuntimeRegistry::new(
            spec,
            vec![(
                DriverRef::new("jcl.driver.convert", limits)?,
                &self.fixture as &dyn ConformanceDriver,
            )],
            vec![(
                PredicateRef::new("jcl.fixture.exists", limits)?,
                &self.fixture as &dyn ConformancePredicate,
            )],
            vec![
                (
                    ObservationRef::new("jcl.observation.recognized", limits)?,
                    &self.recognized as &dyn ConformanceObservation,
                ),
                (
                    ObservationRef::new("jcl.observation.plan-present", limits)?,
                    &self.plan_present,
                ),
                (
                    ObservationRef::new("jcl.observation.error.MEJCL0703", limits)?,
                    &self.lexical_error,
                ),
                (
                    ObservationRef::new("jcl.observation.error.MEJCL0743", limits)?,
                    &self.operand_error,
                ),
                (
                    ObservationRef::new("jcl.observation.error.MEJCL0745", limits)?,
                    &self.parameter_error,
                ),
                (
                    ObservationRef::new("jcl.observation.error.MEJCL0763", limits)?,
                    &self.jecl_error,
                ),
            ],
            limits,
        )
    }
}

struct JclRuntimeFixture(mainframe_env_conformance::JclFixtureRuntime);

impl ConformanceDriver for JclRuntimeFixture {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        DriverOutput::new(
            self.0.execute(fixture.as_str())?,
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

impl ConformancePredicate for JclRuntimeFixture {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        Ok(self.0.contains(fixture.as_str()))
    }
}

struct Recognized;

impl ConformanceObservation for Recognized {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let value = output_value(output)?;
        observation(
            value["recognized"].as_bool() == Some(true),
            "recognized=true",
            &format!("recognized={}", value["recognized"]),
        )
    }
}

struct PlanPresent;

impl ConformanceObservation for PlanPresent {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let value = output_value(output)?;
        observation(
            value["plan"].as_bool() == Some(true),
            "plan=true",
            &format!("plan={}", value["plan"]),
        )
    }
}

struct ErrorCode(&'static str);

impl ConformanceObservation for ErrorCode {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let value = output_value(output)?;
        let codes = value["diagnostic_codes"]
            .as_array()
            .ok_or("driver diagnostic_codes is not an array")?;
        observation(
            value["plan"].as_bool() == Some(false)
                && codes.iter().any(|code| code.as_str() == Some(self.0)),
            &format!("plan=false,error={}", self.0),
            &format!(
                "plan={},errors={}",
                value["plan"], value["diagnostic_codes"]
            ),
        )
    }
}

fn output_value(output: &DriverOutput) -> Result<Value, String> {
    serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())
}

fn observation(matched: bool, expected: &str, actual: &str) -> Result<ObservationCheck, String> {
    ObservationCheck::new(matched, expected, actual, ConformanceLimits::default())
        .map_err(|problem| problem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_success_and_validation_bypass_mutants_are_rejected() {
        let generic_success = DriverOutput::new(
            serde_json::to_vec(&serde_json::json!({
                "recognized": true,
                "plan": true,
                "diagnostic_codes": [],
                "plan_identity": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }))
            .unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(
            !ErrorCode("MEJCL0745")
                .evaluate(&generic_success)
                .unwrap()
                .matched
        );

        let omitted_transition = DriverOutput::new(
            serde_json::to_vec(&serde_json::json!({
                "recognized": true,
                "plan": false,
                "diagnostic_codes": [],
                "plan_identity": null
            }))
            .unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(!PlanPresent.evaluate(&omitted_transition).unwrap().matched);
    }
}
