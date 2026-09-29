use crate::{
    JclCapabilityRequirement, JclConversionLimits, JclParameterNode, JclPlanDocument, JclPlanNode,
    JclSourceOriginKind, convert_jcl,
};
use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JclLimits {
    pub max_source_bytes: usize,
    pub max_lines: usize,
    pub max_steps: usize,
    pub max_dds_per_step: usize,
    pub max_procedures: usize,
    pub max_symbols: usize,
    pub max_inline_bytes: usize,
    pub max_expansion_depth: usize,
}

impl Default for JclLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 4 * 1024 * 1024,
            max_lines: 65_536,
            max_steps: 4_096,
            max_dds_per_step: 1_024,
            max_procedures: 1_024,
            max_symbols: 4_096,
            max_inline_bytes: 64 * 1024 * 1024,
            max_expansion_depth: 16,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JclBundle {
    pub primary: String,
    pub includes: BTreeMap<String, String>,
    pub cataloged_procedures: BTreeMap<String, String>,
    pub procedure_libraries: BTreeMap<String, BTreeMap<String, String>>,
    pub symbols: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Disposition {
    Old,
    Shared,
    New,
    Modify,
    Pass,
    Keep,
    Catalog,
    Delete,
    Uncatalog,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DdPlan {
    pub name: String,
    pub dataset: Option<String>,
    #[serde(default)]
    pub member: Option<String>,
    #[serde(default)]
    pub generation: Option<i32>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub record_format: Option<String>,
    #[serde(default)]
    pub logical_record_length: Option<u32>,
    #[serde(default)]
    pub ccsid: Option<u16>,
    pub temporary: bool,
    pub sysout: Option<String>,
    pub disposition: Vec<Disposition>,
    pub inline_data: Vec<u8>,
    pub concatenation: bool,
    pub source_line: usize,
    #[serde(default)]
    pub source_end_line: usize,
    #[serde(default)]
    pub parameters: Vec<JclParameterNode>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutputPlan {
    pub name: String,
    pub parameters: Vec<JclParameterNode>,
    pub source_line: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StepCondition {
    Always,
    Even,
    Only,
    RunIfMaxRc { operator: String, code: i32 },
    SkipIfMaxRc { operator: String, code: i32 },
}

impl StepCondition {
    #[must_use]
    pub fn should_run(&self, max_rc: i32, abended: bool) -> bool {
        match self {
            Self::Always => !abended,
            Self::Even => true,
            Self::Only => abended,
            Self::RunIfMaxRc { operator, code } => compare(max_rc, operator, *code),
            Self::SkipIfMaxRc { operator, code } => !compare(max_rc, operator, *code),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StepPlan {
    pub name: String,
    pub program: String,
    pub parameter: Option<String>,
    pub condition: StepCondition,
    pub dds: Vec<DdPlan>,
    pub source_line: usize,
    #[serde(default)]
    pub source_end_line: usize,
    pub procedure: Option<String>,
    #[serde(default)]
    pub invocation_line: Option<usize>,
    #[serde(default)]
    pub parameters: Vec<JclParameterNode>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobPlan {
    pub name: String,
    pub class: char,
    pub priority: u8,
    pub restart_step: Option<String>,
    #[serde(default)]
    pub symbols: BTreeMap<String, String>,
    #[serde(default)]
    pub procedure_libraries: Vec<String>,
    #[serde(default)]
    pub job_dds: Vec<DdPlan>,
    pub steps: Vec<StepPlan>,
    #[serde(default)]
    pub outputs: Vec<OutputPlan>,
    #[serde(default)]
    pub parameters: Vec<JclParameterNode>,
    #[serde(default)]
    pub capabilities: Vec<JclCapabilityRequirement>,
    pub source: String,
}

/// Compatibility adapter for the existing JES service. The authoritative
/// implementation is the immutable converter; no legacy parser remains.
pub fn parse_jcl(bundle: &JclBundle, limits: JclLimits) -> Result<JobPlan, HostProblem> {
    if bundle.primary.is_empty() || bundle.primary.len() > limits.max_source_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let conversion = convert_jcl(bundle, conversion_limits(limits)).map_err(|problem| {
        if matches!(problem, crate::JclConversionProblem::ResourceLimit(_)) {
            HostProblem::ResourceExhausted
        } else {
            HostProblem::Malformed
        }
    })?;
    if let Some(plan) = conversion.legacy_plan() {
        return Ok(plan.clone());
    }
    if conversion
        .diagnostics()
        .iter()
        .any(|diagnostic| matches!(diagnostic.code().as_str(), "MEJCL0733" | "MEJCL0734"))
    {
        Err(HostProblem::NotFound)
    } else if conversion
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0720")
    {
        Err(HostProblem::Unsupported)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn conversion_limits(limits: JclLimits) -> JclConversionLimits {
    let mut conversion = JclConversionLimits::default();
    conversion.syntax.max_file_bytes = limits.max_source_bytes;
    conversion.syntax.max_total_source_bytes = limits
        .max_source_bytes
        .saturating_mul(limits.max_procedures.saturating_add(1));
    conversion.syntax.max_lines = limits.max_lines;
    conversion.syntax.max_inline_bytes = limits.max_inline_bytes;
    conversion.expansion.max_expansion_depth = limits.max_expansion_depth;
    conversion.expansion.max_procedures = limits.max_procedures;
    conversion.expansion.max_symbols = limits.max_symbols;
    conversion.expansion.max_expanded_statements = limits
        .max_steps
        .saturating_mul(limits.max_dds_per_step.saturating_add(2));
    conversion.max_plan_nodes = conversion.expansion.max_expanded_statements;
    conversion
}

pub(crate) fn legacy_job_plan(
    document: &JclPlanDocument,
    source: String,
    limits: JclConversionLimits,
) -> Result<JobPlan, HostProblem> {
    let statements = document
        .statements()
        .iter()
        .map(|statement| (statement.id(), statement))
        .collect::<BTreeMap<_, _>>();
    let mut name = None;
    let mut class = 'A';
    let mut priority = 0u8;
    let mut restart_step = None;
    let mut job_parameters = Vec::new();
    let mut job_dds = Vec::new();
    let mut steps = Vec::<StepPlan>::new();
    let mut outputs = Vec::new();
    let mut procedure_libraries = Vec::new();
    let mut active_conditions = Vec::<StepCondition>::new();
    for node in document.nodes() {
        match node {
            JclPlanNode::Job {
                name: job_name,
                statement_id,
            } => {
                name = Some(job_name.clone());
                let statement = statements
                    .get(statement_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                job_parameters = statement.parameters().to_vec();
                let parameters = parameter_map(statement.parameters());
                class = parameters
                    .get("CLASS")
                    .and_then(|value| value.chars().next())
                    .unwrap_or('A');
                priority = parameters
                    .get("PRTY")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                restart_step = parameters.get("RESTART").cloned();
            }
            JclPlanNode::Step {
                name,
                program,
                statement_id,
            } => {
                let statement = statements
                    .get(statement_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let parameters = parameter_map(statement.parameters());
                let condition = parameters
                    .get("COND")
                    .map(|value| parse_cond(value))
                    .transpose()?
                    .or_else(|| active_conditions.last().cloned())
                    .unwrap_or(StepCondition::Always);
                let procedure = statement
                    .source()
                    .origins()
                    .iter()
                    .find(|origin| origin.kind() == JclSourceOriginKind::Procedure)
                    .and_then(|origin| {
                        origin
                            .logical_path()
                            .rsplit('/')
                            .next()
                            .and_then(|leaf| leaf.strip_suffix(".jcl"))
                            .map(str::to_string)
                    });
                let invocation_line = statement
                    .source()
                    .origins()
                    .iter()
                    .find(|origin| origin.kind() == JclSourceOriginKind::Generated)
                    .and_then(|origin| {
                        source_line(document, origin.file_id(), origin.byte_start())
                    });
                steps.push(StepPlan {
                    name: name.clone(),
                    program: program.clone(),
                    parameter: parameters.get("PARM").cloned(),
                    condition,
                    dds: Vec::new(),
                    source_line: statement.source().line(),
                    source_end_line: statement.source().line(),
                    procedure,
                    invocation_line,
                    parameters: statement.parameters().to_vec(),
                });
            }
            JclPlanNode::Dd {
                name,
                step,
                statement_id,
            } => {
                let statement = statements
                    .get(statement_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let target = step
                    .as_ref()
                    .and_then(|step| {
                        steps
                            .iter_mut()
                            .rev()
                            .find(|candidate| candidate.name == *step)
                    })
                    .map(|step| &mut step.dds)
                    .unwrap_or(&mut job_dds);
                if target.len() >= limits.max_parameters_per_statement.saturating_mul(4) {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut dd = dd_plan(name, statement)?;
                if dd.name == "*" {
                    dd.name = target
                        .last()
                        .map(|previous| previous.name.clone())
                        .ok_or(HostProblem::Malformed)?;
                    dd.concatenation = true;
                }
                target.push(dd);
            }
            JclPlanNode::Output { name, statement_id } => {
                let statement = statements
                    .get(statement_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                outputs.push(OutputPlan {
                    name: name.clone(),
                    parameters: statement.parameters().to_vec(),
                    source_line: statement.source().line(),
                });
            }
            JclPlanNode::Annotation {
                operation,
                statement_id,
            } => {
                let statement = statements
                    .get(statement_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                match operation.as_str() {
                    "IF" => active_conditions.push(parse_if(statement.raw_operands())?),
                    "ELSE" => {
                        if let Some(condition) = active_conditions.pop() {
                            active_conditions.push(invert_condition(condition));
                        }
                    }
                    "ENDIF" => {
                        active_conditions.pop();
                    }
                    "JCLLIB" => {
                        procedure_libraries.extend(jcllib_values(statement.raw_operands()));
                    }
                    _ => {}
                }
            }
            JclPlanNode::Jecl { .. } => {}
        }
    }
    Ok(JobPlan {
        name: name.ok_or(HostProblem::Malformed)?,
        class,
        priority,
        restart_step,
        symbols: document
            .symbols()
            .iter()
            .map(|symbol| (symbol.name().to_string(), symbol.value().to_string()))
            .collect(),
        procedure_libraries,
        job_dds,
        steps,
        outputs,
        parameters: job_parameters,
        capabilities: document.capabilities().to_vec(),
        source,
    })
}

fn parameter_map(parameters: &[JclParameterNode]) -> BTreeMap<String, String> {
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.identity().keyword().to_string(),
                parameter.normalized_value().to_string(),
            )
        })
        .collect()
}

fn dd_plan(name: &str, statement: &crate::JclStatementNode) -> Result<DdPlan, HostProblem> {
    let parameters = parameter_map(statement.parameters());
    let (dataset, member, generation) = parameters
        .get("DSNAME")
        .map(|value| dataset_and_member(value))
        .unwrap_or((None, None, None));
    let dcb = parameters
        .get("DCB")
        .map(|value| assignment_map(value.trim_matches(['(', ')'])))
        .unwrap_or_default();
    let parameter = |name: &str| parameters.get(name).or_else(|| dcb.get(name));
    let organization = parameter("DSORG")
        .map(|value| match value.as_str() {
            "PS" => Ok("PS".into()),
            "PO" | "PO-E" => Ok("PO".into()),
            _ => Err(HostProblem::Unsupported),
        })
        .transpose()?
        .or_else(|| {
            parameter("DSNTYPE")
                .is_some_and(|value| value == "LIBRARY")
                .then(|| "PO".into())
        });
    let record_format = parameter("RECFM").map(|value| {
        if value.starts_with('F') {
            "FB".to_string()
        } else if value.starts_with('V') {
            "VB".to_string()
        } else {
            value.clone()
        }
    });
    let logical_record_length = parameter("LRECL").and_then(|value| value.parse::<u32>().ok());
    let ccsid = parameter("CCSID").and_then(|value| value.parse::<u16>().ok());
    let disposition = parameters
        .get("DISP")
        .map(|value| {
            value
                .trim_matches(['(', ')'])
                .split(',')
                .filter(|value| !value.trim().is_empty())
                .map(disposition)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(DdPlan {
        name: name.into(),
        temporary: dataset.as_ref().is_some_and(|name| name.starts_with("&&")),
        dataset,
        member,
        generation,
        organization,
        record_format,
        logical_record_length,
        ccsid,
        sysout: parameters.get("SYSOUT").cloned(),
        disposition,
        inline_data: statement.inline_data().to_vec(),
        concatenation: false,
        source_line: statement.source().line(),
        source_end_line: statement.source().line(),
        parameters: statement.parameters().to_vec(),
    })
}

fn dataset_and_member(value: &str) -> (Option<String>, Option<String>, Option<i32>) {
    let Some(open) = value.rfind('(') else {
        return (Some(value.to_string()), None, None);
    };
    if !value.ends_with(')') || open == 0 {
        return (Some(value.to_string()), None, None);
    }
    let qualifier = &value[open + 1..value.len() - 1];
    if let Ok(relative) = qualifier.parse::<i32>() {
        (Some(value[..open].to_string()), None, Some(relative))
    } else {
        (
            Some(value[..open].to_string()),
            Some(qualifier.to_ascii_uppercase()),
            None,
        )
    }
}

fn assignment_map(value: &str) -> BTreeMap<String, String> {
    value
        .split(',')
        .filter_map(|operand| {
            operand.split_once('=').map(|(name, value)| {
                (
                    name.trim().to_ascii_uppercase(),
                    value.trim().to_ascii_uppercase(),
                )
            })
        })
        .collect()
}

fn jcllib_values(value: &str) -> Vec<String> {
    let Some((name, value)) = value.split_once('=') else {
        return Vec::new();
    };
    if !name.trim().eq_ignore_ascii_case("ORDER") {
        return Vec::new();
    }
    value
        .trim()
        .trim_matches(['(', ')'])
        .split(',')
        .map(|value| value.trim().trim_matches(['\'', '"']).to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn disposition(value: &str) -> Result<Disposition, HostProblem> {
    Ok(match value.trim().to_ascii_uppercase().as_str() {
        "OLD" => Disposition::Old,
        "SHR" => Disposition::Shared,
        "NEW" => Disposition::New,
        "MOD" => Disposition::Modify,
        "PASS" => Disposition::Pass,
        "KEEP" => Disposition::Keep,
        "CATLG" => Disposition::Catalog,
        "DELETE" => Disposition::Delete,
        "UNCATLG" => Disposition::Uncatalog,
        _ => return Err(HostProblem::Unsupported),
    })
}

fn parse_cond(value: &str) -> Result<StepCondition, HostProblem> {
    let value = value.trim().trim_matches(['(', ')']);
    if value.eq_ignore_ascii_case("EVEN") {
        return Ok(StepCondition::Even);
    }
    if value.eq_ignore_ascii_case("ONLY") {
        return Ok(StepCondition::Only);
    }
    let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(HostProblem::Malformed);
    }
    Ok(StepCondition::SkipIfMaxRc {
        code: parts[0].parse().map_err(|_| HostProblem::Malformed)?,
        operator: reverse_cond_operator(parts[1])?.into(),
    })
}

fn parse_if(value: &str) -> Result<StepCondition, HostProblem> {
    let normalized = value.replace(['(', ')'], " ");
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let position = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("RC"))
        .ok_or(HostProblem::Unsupported)?;
    if position + 2 >= words.len() {
        return Err(HostProblem::Malformed);
    }
    Ok(StepCondition::RunIfMaxRc {
        operator: words[position + 1].to_ascii_uppercase(),
        code: words[position + 2]
            .parse()
            .map_err(|_| HostProblem::Malformed)?,
    })
}

fn reverse_cond_operator(operator: &str) -> Result<&'static str, HostProblem> {
    Ok(match operator.trim().to_ascii_uppercase().as_str() {
        "EQ" | "=" => "EQ",
        "NE" | "¬=" => "NE",
        "GT" | ">" => "LT",
        "GE" | ">=" => "LE",
        "LT" | "<" => "GT",
        "LE" | "<=" => "GE",
        _ => return Err(HostProblem::Malformed),
    })
}

fn invert_condition(value: StepCondition) -> StepCondition {
    match value {
        StepCondition::RunIfMaxRc { operator, code } => {
            StepCondition::SkipIfMaxRc { operator, code }
        }
        StepCondition::SkipIfMaxRc { operator, code } => {
            StepCondition::RunIfMaxRc { operator, code }
        }
        other => other,
    }
}

fn compare(left: i32, operator: &str, right: i32) -> bool {
    match operator {
        "EQ" | "=" => left == right,
        "NE" | "¬=" => left != right,
        "GT" | ">" => left > right,
        "GE" | ">=" => left >= right,
        "LT" | "<" => left < right,
        "LE" | "<=" => left <= right,
        _ => false,
    }
}

fn source_line(document: &JclPlanDocument, file_id: u32, byte_start: usize) -> Option<usize> {
    document
        .statements()
        .iter()
        .find(|statement| {
            statement.source().file_id() == file_id && statement.source().byte_start() == byte_start
        })
        .map(|statement| statement.source().line())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_parm_preserves_text_and_absence_for_main_program() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB\n//WITH EXEC PGM=PARMTEST,PARM='2022071800'\n//WITHOUT EXEC PGM=PARMTEST\n".into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.steps[0].parameter.as_deref(), Some("2022071800"));
        assert_eq!(plan.steps[1].parameter.as_deref(), None);
    }

    #[test]
    fn exec_parm_unquotes_doubled_apostrophes_and_accepts_empty() {
        let plan = parse_jcl(
            &JclBundle {
                primary:
                    "//J JOB\n//Q EXEC PGM=PARMTEST,PARM='A''B'\n//E EXEC PGM=PARMTEST,PARM=''\n"
                        .into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.steps[0].parameter.as_deref(), Some("A'B"));
        assert_eq!(plan.steps[1].parameter.as_deref(), Some(""));
    }

    #[test]
    fn compatibility_adapter_uses_the_new_converter_plan() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A,PRTY=7\n//S EXEC PGM=IEBGENER,COND=(4,GE)\n//IN DD *\nONE\n/*\n//OUT DD SYSOUT=*\n".into(),
                ..JclBundle::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.name, "J");
        assert_eq!(plan.priority, 7);
        assert_eq!(plan.steps[0].program, "IEBGENER");
        assert_eq!(plan.steps[0].dds[0].inline_data, b"ONE\n");
        assert!(!plan.steps[0].condition.should_run(4, false));
        assert!(!plan.capabilities.is_empty());
    }

    #[test]
    fn prior_job_plan_json_defaults_additive_typed_fields() {
        let plan: JobPlan = serde_json::from_str(
            r#"{"name":"J","class":"A","priority":0,"restart_step":null,"steps":[{"name":"S","program":"IEFBR14","parameter":null,"condition":"Always","dds":[],"source_line":2,"procedure":null}],"source":"//J JOB\n//S EXEC PGM=IEFBR14\n"}"#,
        )
        .unwrap();
        assert!(plan.outputs.is_empty());
        assert!(plan.parameters.is_empty());
        assert!(plan.capabilities.is_empty());
        assert!(plan.steps[0].parameters.is_empty());
    }

    #[test]
    fn temporary_disposition_plan_preserves_both_step_allocations() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//TEMPJOB JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//WORK DD DSN=&&WORK,DISP=(NEW,PASS,DELETE)\n//USE EXEC PGM=IEFBR14\n//INPUT DD DSN=&&WORK,DISP=(OLD,DELETE,DELETE)\n".into(),
                ..JclBundle::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].dds.len(), 1);
        assert_eq!(plan.steps[1].dds.len(), 1);
        assert!(plan.steps[0].dds[0].temporary);
        assert_eq!(
            plan.steps[0].dds[0].disposition,
            vec![Disposition::New, Disposition::Pass, Disposition::Delete]
        );
        assert_eq!(
            plan.steps[1].dds[0].disposition,
            vec![Disposition::Old, Disposition::Delete, Disposition::Delete]
        );
    }

    #[test]
    fn dcb_referback_to_unknown_or_forward_dd_fails_jcl_conversion() {
        for source in [
            "//J JOB\n//S EXEC PGM=IEFBR14\n//OUT DD DSN=USER.OUT,DISP=NEW,DCB=*.MISSING\n",
            "//J JOB\n//S EXEC PGM=IEFBR14\n//OUT DD DSN=USER.OUT,DISP=NEW,DCB=*.MISSING.IN\n",
            "//J JOB\n//S EXEC PGM=IEFBR14\n//OUT DD DSN=USER.OUT,DISP=NEW,DCB=*.LATER\n//LATER DD DSN=USER.IN,DISP=SHR\n",
        ] {
            let bundle = JclBundle {
                primary: source.into(),
                ..JclBundle::default()
            };
            let conversion = convert_jcl(&bundle, JclConversionLimits::default()).unwrap();
            assert!(conversion.plan().is_none());
            assert!(
                conversion
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0738")
            );
            assert_eq!(
                parse_jcl(&bundle, JclLimits::default()),
                Err(HostProblem::Malformed)
            );
        }
    }
}
