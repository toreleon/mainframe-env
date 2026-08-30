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
            max_lines: 65536,
            max_steps: 4096,
            max_dds_per_step: 1024,
            max_procedures: 1024,
            max_symbols: 4096,
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
    pub temporary: bool,
    pub sysout: Option<String>,
    pub disposition: Vec<Disposition>,
    pub inline_data: Vec<u8>,
    pub concatenation: bool,
    pub source_line: usize,
    #[serde(default)]
    pub source_end_line: usize,
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
    pub source: String,
}

#[derive(Clone, Debug)]
struct Statement {
    name: String,
    operation: String,
    operands: String,
    inline: Vec<u8>,
    line: usize,
    end_line: usize,
}

#[derive(Clone, Debug)]
struct ProcedureDefinition {
    defaults: BTreeMap<String, String>,
    body: Vec<Statement>,
}

pub fn parse_jcl(bundle: &JclBundle, limits: JclLimits) -> Result<JobPlan, HostProblem> {
    if bundle.primary.is_empty()
        || bundle.primary.len() > limits.max_source_bytes
        || bundle.primary.lines().count() > limits.max_lines
        || bundle.includes.len() > limits.max_procedures
        || bundle.cataloged_procedures.len() > limits.max_procedures
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut source = bundle.primary.clone();
    for _ in 0..limits.max_expansion_depth {
        let mut expanded = String::new();
        let mut changed = false;
        for line in source.lines() {
            if let Some(member) = include_member(line) {
                let include = bundle.includes.get(&member).ok_or(HostProblem::NotFound)?;
                expanded.push_str(include);
                if !include.ends_with('\n') {
                    expanded.push('\n');
                }
                changed = true;
            } else {
                expanded.push_str(line);
                expanded.push('\n');
            }
        }
        source = expanded;
        if !changed {
            break;
        }
        if source.len() > limits.max_source_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    if source.lines().any(|line| include_member(line).is_some()) {
        return Err(HostProblem::ResourceExhausted);
    }
    let statements = statements(&source, limits)?;
    build_plan(&source, statements, bundle, limits, 0)
}

fn build_plan(
    source: &str,
    parsed_statements: Vec<Statement>,
    bundle: &JclBundle,
    limits: JclLimits,
    depth: usize,
) -> Result<JobPlan, HostProblem> {
    if depth > limits.max_expansion_depth {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut symbols = BTreeMap::new();
    let mut procedures: BTreeMap<String, ProcedureDefinition> = BTreeMap::new();
    let mut current_proc: Option<(String, ProcedureDefinition)> = None;
    let mut retained = Vec::new();
    for statement in parsed_statements {
        match statement.operation.as_str() {
            "SET" => {
                for (name, value) in assignments(&statement.operands) {
                    if symbols.len() >= limits.max_symbols {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    symbols.insert(name, substitute(&value, &symbols)?);
                }
            }
            "PROC" => {
                if current_proc.is_some() {
                    return Err(HostProblem::Malformed);
                }
                current_proc = Some((
                    statement.name.clone(),
                    ProcedureDefinition {
                        defaults: assignments(&statement.operands),
                        body: Vec::new(),
                    },
                ));
            }
            "PEND" => {
                let (name, body) = current_proc.take().ok_or(HostProblem::Malformed)?;
                if procedures.insert(name, body).is_some()
                    || procedures.len() > limits.max_procedures
                {
                    return Err(HostProblem::ResourceExhausted);
                }
            }
            _ if current_proc.is_some() => current_proc.as_mut().unwrap().1.body.push(statement),
            _ => retained.push(statement),
        }
    }
    if current_proc.is_some() {
        return Err(HostProblem::Malformed);
    }
    for (name, body) in &bundle.cataloged_procedures {
        let parsed = statements(body, limits)?;
        let (declared_name, definition) = cataloged_procedure(parsed)?;
        procedures
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| definition.clone());
        procedures.entry(declared_name).or_insert(definition);
    }
    let mut job_name = None;
    let mut class = 'A';
    let mut priority = 0u8;
    let mut restart_step = None;
    let mut procedure_libraries = Vec::new();
    let mut job_dds = Vec::new();
    let mut steps = Vec::new();
    let mut active_if: Option<StepCondition> = None;
    for mut statement in retained {
        statement.operands = substitute(&statement.operands, &symbols)?;
        match statement.operation.as_str() {
            "JOB" => {
                if job_name.is_some() {
                    return Err(HostProblem::Malformed);
                }
                job_name = Some(statement.name.clone());
                let values = assignments(&statement.operands);
                class = values
                    .get("CLASS")
                    .and_then(|value| value.chars().next())
                    .unwrap_or('A');
                priority = values
                    .get("PRTY")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                restart_step = values.get("RESTART").cloned();
            }
            "IF" => active_if = Some(parse_if(&statement.operands)?),
            "ELSE" => active_if = active_if.take().map(invert_condition),
            "ENDIF" => active_if = None,
            "JCLLIB" => {
                procedure_libraries.extend(
                    assignments(&statement.operands)
                        .get("ORDER")
                        .into_iter()
                        .flat_map(|value| list_values(value)),
                );
            }
            "EXEC" => {
                if steps.len() >= limits.max_steps {
                    return Err(HostProblem::ResourceExhausted);
                }
                let values = assignments(&statement.operands);
                if let Some(program) = values.get("PGM") {
                    steps.push(StepPlan {
                        name: statement.name,
                        program: program.to_ascii_uppercase(),
                        parameter: values.get("PARM").cloned(),
                        condition: values
                            .get("COND")
                            .map(|value| parse_cond(value))
                            .transpose()?
                            .or_else(|| active_if.clone())
                            .unwrap_or(StepCondition::Always),
                        dds: Vec::new(),
                        source_line: statement.line,
                        source_end_line: statement.end_line,
                        procedure: None,
                        invocation_line: None,
                    });
                } else {
                    let proc_name = values
                        .get("PROC")
                        .cloned()
                        .or_else(|| first_operand(&statement.operands))
                        .ok_or(HostProblem::Malformed)?
                        .to_ascii_uppercase();
                    let definition = procedures.get(&proc_name).ok_or(HostProblem::NotFound)?;
                    let mut procedure_symbols = symbols.clone();
                    procedure_symbols.extend(definition.defaults.clone());
                    procedure_symbols.extend(
                        values
                            .into_iter()
                            .filter(|(name, _)| !matches!(name.as_str(), "PGM" | "PROC")),
                    );
                    for mut nested in procedure_steps(
                        &definition.body,
                        &procedure_symbols,
                        &proc_name,
                        limits,
                        depth + 1,
                    )? {
                        if steps.len() >= limits.max_steps {
                            return Err(HostProblem::ResourceExhausted);
                        }
                        nested.invocation_line = Some(statement.line);
                        steps.push(nested);
                    }
                }
            }
            "DD" => {
                if override_procedure_dd(&mut steps, &statement, limits)? {
                    continue;
                }
                let target = steps
                    .last_mut()
                    .map(|step| &mut step.dds)
                    .unwrap_or(&mut job_dds);
                if target.len() >= limits.max_dds_per_step {
                    return Err(HostProblem::ResourceExhausted);
                }
                push_dd(target, &statement, limits)?;
            }
            "OUTPUT" => {}
            _ => return Err(HostProblem::Unsupported),
        }
    }
    let name = job_name.ok_or(HostProblem::Malformed)?;
    if steps.is_empty() {
        return Err(HostProblem::Malformed);
    }
    Ok(JobPlan {
        name,
        class,
        priority,
        restart_step,
        symbols,
        procedure_libraries,
        job_dds,
        steps,
        source: source.into(),
    })
}

fn cataloged_procedure(
    statements: Vec<Statement>,
) -> Result<(String, ProcedureDefinition), HostProblem> {
    let mut current: Option<(String, ProcedureDefinition)> = None;
    let mut completed = None;
    for statement in statements {
        match statement.operation.as_str() {
            "PROC" => {
                if current.is_some() || completed.is_some() {
                    return Err(HostProblem::Malformed);
                }
                current = Some((
                    statement.name,
                    ProcedureDefinition {
                        defaults: assignments(&statement.operands),
                        body: Vec::new(),
                    },
                ));
            }
            "PEND" => {
                completed = Some(current.take().ok_or(HostProblem::Malformed)?);
            }
            _ => current
                .as_mut()
                .ok_or(HostProblem::Malformed)?
                .1
                .body
                .push(statement),
        }
    }
    if current.is_some() {
        return Err(HostProblem::Malformed);
    }
    completed.ok_or(HostProblem::Malformed)
}

fn override_procedure_dd(
    steps: &mut [StepPlan],
    statement: &Statement,
    limits: JclLimits,
) -> Result<bool, HostProblem> {
    let Some((procedure_step, dd_name)) = statement.name.split_once('.') else {
        return Ok(false);
    };
    let step = steps
        .iter_mut()
        .rev()
        .find(|step| {
            step.procedure.is_some()
                && step
                    .name
                    .rsplit('.')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(procedure_step))
        })
        .ok_or(HostProblem::NotFound)?;
    step.dds.retain(|dd| !dd.name.eq_ignore_ascii_case(dd_name));
    let mut override_statement = statement.clone();
    override_statement.name = dd_name.to_ascii_uppercase();
    push_dd(&mut step.dds, &override_statement, limits)?;
    Ok(true)
}

fn procedure_steps(
    statements_in: &[Statement],
    symbols: &BTreeMap<String, String>,
    procedure: &str,
    limits: JclLimits,
    depth: usize,
) -> Result<Vec<StepPlan>, HostProblem> {
    if depth > limits.max_expansion_depth {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut steps: Vec<StepPlan> = Vec::new();
    for statement in statements_in {
        let mut statement = statement.clone();
        statement.operands = substitute(&statement.operands, symbols)?;
        match statement.operation.as_str() {
            "EXEC" => {
                let values = assignments(&statement.operands);
                let program = values.get("PGM").ok_or(HostProblem::Unsupported)?;
                steps.push(StepPlan {
                    name: statement.name,
                    program: program.to_ascii_uppercase(),
                    parameter: values.get("PARM").cloned(),
                    condition: values
                        .get("COND")
                        .map(|value| parse_cond(value))
                        .transpose()?
                        .unwrap_or(StepCondition::Always),
                    dds: Vec::new(),
                    source_line: statement.line,
                    source_end_line: statement.end_line,
                    procedure: Some(procedure.into()),
                    invocation_line: None,
                });
            }
            "DD" => {
                let step = steps.last_mut().ok_or(HostProblem::Malformed)?;
                if step.dds.len() >= limits.max_dds_per_step {
                    return Err(HostProblem::ResourceExhausted);
                }
                push_dd(&mut step.dds, &statement, limits)?;
            }
            _ => return Err(HostProblem::Unsupported),
        }
    }
    Ok(steps)
}

fn statements(source: &str, limits: JclLimits) -> Result<Vec<Statement>, HostProblem> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line_number = index + 1;
        let line = lines[index];
        index += 1;
        if line.starts_with("//*") || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with("//") {
            return Err(HostProblem::Malformed);
        }
        let content = line
            .get(2..72.min(line.len()))
            .ok_or(HostProblem::Malformed)?;
        if content.trim().is_empty() {
            break;
        }
        let no_name = content.starts_with(char::is_whitespace);
        let parts = content
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let (name, operation, mut operands) = if no_name {
            (
                "*".to_string(),
                parts
                    .first()
                    .cloned()
                    .unwrap_or_default()
                    .to_ascii_uppercase(),
                parts.get(1..).unwrap_or_default().join(" "),
            )
        } else {
            (
                parts
                    .first()
                    .cloned()
                    .unwrap_or_default()
                    .to_ascii_uppercase(),
                parts
                    .get(1)
                    .cloned()
                    .unwrap_or_default()
                    .to_ascii_uppercase(),
                parts.get(2..).unwrap_or_default().join(" "),
            )
        };
        let mut end_line = line_number;
        while operands.ends_with(',') && index < lines.len() {
            let continuation = lines[index];
            if !continuation.starts_with("//") {
                break;
            }
            operands.push_str(
                continuation
                    .get(2..72.min(continuation.len()))
                    .unwrap_or("")
                    .trim(),
            );
            index += 1;
            end_line = index;
        }
        let mut inline = Vec::new();
        let inline_operand = split_operands(&operands).into_iter().next();
        if operation == "DD"
            && inline_operand
                .as_deref()
                .is_some_and(|value| value == "*" || value.starts_with("DATA"))
        {
            let delimiter = assignments(&operands)
                .get("DLM")
                .cloned()
                .unwrap_or_else(|| "/*".into());
            while index < lines.len()
                && jcl_control_columns(lines[index]).trim_end() != delimiter
                && !(delimiter == "/*" && lines[index].starts_with("//"))
            {
                inline.extend_from_slice(lines[index].as_bytes());
                inline.push(b'\n');
                if inline.len() > limits.max_inline_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                index += 1;
            }
            if index < lines.len() && jcl_control_columns(lines[index]).trim_end() == delimiter {
                index += 1;
            }
        }
        if name.is_empty() || operation.is_empty() {
            return Err(HostProblem::Malformed);
        }
        out.push(Statement {
            name,
            operation,
            operands,
            inline,
            line: line_number,
            end_line,
        });
        if out.len() > limits.max_lines {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(out)
}

fn dd_plan(
    statement: &Statement,
    concatenation: bool,
    limits: JclLimits,
) -> Result<DdPlan, HostProblem> {
    let values = assignments(&statement.operands);
    let (dataset, member, generation) = values
        .get("DSN")
        .or_else(|| values.get("DSNAME"))
        .map(|value| dataset_and_member(value))
        .unwrap_or((None, None, None));
    let temporary = dataset.as_ref().is_some_and(|name| name.starts_with("&&"));
    let sysout = values.get("SYSOUT").cloned();
    let dcb_values = values
        .get("DCB")
        .map(|value| assignments(value.trim().trim_matches(['(', ')'])))
        .unwrap_or_default();
    let parameter = |name: &str| values.get(name).or_else(|| dcb_values.get(name));
    let organization = parameter("DSORG")
        .map(|value| match value.to_ascii_uppercase().as_str() {
            "PS" => Ok("PS".into()),
            "PO" | "PO-E" => Ok("PO".into()),
            _ => Err(HostProblem::Unsupported),
        })
        .transpose()?
        .or_else(|| {
            parameter("DSNTYPE")
                .is_some_and(|value| value.eq_ignore_ascii_case("LIBRARY"))
                .then(|| "PO".into())
        });
    let record_format = parameter("RECFM")
        .map(|value| match value.to_ascii_uppercase().as_str() {
            "F" => Ok("F".into()),
            "FB" | "FBA" => Ok("FB".into()),
            "V" => Ok("V".into()),
            "VB" | "VBA" => Ok("VB".into()),
            "U" => Ok("U".into()),
            _ => Err(HostProblem::Unsupported),
        })
        .transpose()?;
    let logical_record_length = parameter("LRECL")
        .map(|value| value.parse::<u32>().map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let disposition = values
        .get("DISP")
        .map(|value| {
            value
                .trim_matches(|ch| ch == '(' || ch == ')')
                .split(',')
                .filter(|value| !value.is_empty())
                .map(disposition)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if statement.inline.len() > limits.max_inline_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(DdPlan {
        name: statement.name.clone(),
        dataset,
        member,
        generation,
        organization,
        record_format,
        logical_record_length,
        temporary,
        sysout,
        disposition,
        inline_data: statement.inline.clone(),
        concatenation,
        source_line: statement.line,
        source_end_line: statement.end_line,
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

fn push_dd(
    target: &mut Vec<DdPlan>,
    statement: &Statement,
    limits: JclLimits,
) -> Result<(), HostProblem> {
    let concatenation = statement.name == "*";
    let mut plan = dd_plan(statement, concatenation, limits)?;
    if concatenation {
        plan.name = target
            .last()
            .map(|previous| previous.name.clone())
            .ok_or(HostProblem::Malformed)?;
    }
    target.push(plan);
    Ok(())
}

fn jcl_control_columns(line: &str) -> &str {
    line.get(..72.min(line.len())).unwrap_or(line)
}

fn list_values(value: &str) -> Vec<String> {
    value
        .trim_matches(|ch| matches!(ch, '(' | ')'))
        .split(',')
        .map(|item| item.trim().trim_matches(['\'', '"']).to_string())
        .filter(|item| !item.is_empty())
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

fn assignments(value: &str) -> BTreeMap<String, String> {
    split_operands(value)
        .into_iter()
        .filter_map(|item| {
            item.split_once('=').map(|(name, value)| {
                (
                    name.trim().to_ascii_uppercase(),
                    value
                        .trim()
                        .trim_matches('\'')
                        .trim_matches('"')
                        .to_string(),
                )
            })
        })
        .collect()
}

fn split_operands(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    for ch in value.chars() {
        if matches!(ch, '\'' | '"') {
            quote = if quote == Some(ch) {
                None
            } else if quote.is_none() {
                Some(ch)
            } else {
                quote
            };
        }
        if quote.is_none() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth = depth.saturating_sub(1);
            } else if ch == ',' && depth == 0 {
                out.push(current.trim().to_string());
                current.clear();
                continue;
            }
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

fn substitute(value: &str, symbols: &BTreeMap<String, String>) -> Result<String, HostProblem> {
    let mut output = value.to_string();
    for (name, replacement) in symbols {
        output = output.replace(&format!("&{name}."), replacement);
        output = output.replace(&format!("&{name}"), replacement);
    }
    Ok(output)
}

fn parse_cond(value: &str) -> Result<StepCondition, HostProblem> {
    let value = value.trim().trim_matches(|ch| ch == '(' || ch == ')');
    if value.eq_ignore_ascii_case("EVEN") {
        return Ok(StepCondition::Even);
    }
    if value.eq_ignore_ascii_case("ONLY") {
        return Ok(StepCondition::Only);
    }
    let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(HostProblem::Malformed);
    }
    Ok(StepCondition::SkipIfMaxRc {
        code: parts[0].parse().map_err(|_| HostProblem::Malformed)?,
        operator: parts[1].to_ascii_uppercase(),
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

fn first_operand(value: &str) -> Option<String> {
    split_operands(value)
        .first()
        .filter(|value| !value.contains('='))
        .cloned()
}

fn include_member(line: &str) -> Option<String> {
    if !line.starts_with("//") || !line.to_ascii_uppercase().contains(" INCLUDE ") {
        return None;
    }
    let upper = line.to_ascii_uppercase();
    let start = upper.find("MEMBER=")? + "MEMBER=".len();
    let value = upper[start..]
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .next()?
        .trim_matches(|ch| ch == '\'' || ch == '"');
    (!value.is_empty()).then(|| value.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_job_exec_dd_inline_symbols_and_conditions() {
        let source = "//CARDJOB JOB CLASS=A,PRTY=7\n// SET PGM=IEBGENER\n//COPY EXEC PGM=&PGM.,COND=(4,GE)\n//SYSUT1 DD *\nHELLO\n/*\n//SYSUT2 DD SYSOUT=*\n";
        let plan = parse_jcl(
            &JclBundle {
                primary: source.into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.name, "CARDJOB");
        assert_eq!(plan.priority, 7);
        assert_eq!(plan.steps[0].program, "IEBGENER");
        assert_eq!(plan.steps[0].dds[0].inline_data, b"HELLO\n");
        assert!(!plan.steps[0].condition.should_run(4, false));
    }

    #[test]
    fn dd_allocation_attributes_preserve_nested_dcb_and_library_type() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A\n//S EXEC PGM=IEBGENER\n//OUT DD DSN=U.OUT,DISP=(NEW,CATLG,DELETE),\n// DCB=(LRECL=80,RECFM=FBA,DSORG=PS)\n//LIB DD DSN=U.LIB,DISP=(NEW,CATLG,DELETE),\n// DSNTYPE=LIBRARY,RECFM=FB,LRECL=132\n".into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        let output = &plan.steps[0].dds[0];
        assert_eq!(output.organization.as_deref(), Some("PS"));
        assert_eq!(output.record_format.as_deref(), Some("FB"));
        assert_eq!(output.logical_record_length, Some(80));
        let library = &plan.steps[0].dds[1];
        assert_eq!(library.organization.as_deref(), Some("PO"));
        assert_eq!(library.record_format.as_deref(), Some("FB"));
        assert_eq!(library.logical_record_length, Some(132));
    }

    #[test]
    fn expands_include_and_instream_procedure() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=B\n//INC INCLUDE MEMBER=PART\n//P PROC\n//S EXEC PGM=IEFBR14\n// PEND\n//RUN EXEC P\n".into(),
                includes: BTreeMap::from([("PART".into(), "//* INCLUDED\n".into())]),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].procedure.as_deref(), Some("P"));
    }

    #[test]
    fn malformed_and_unbounded_fail_closed() {
        assert_eq!(
            parse_jcl(
                &JclBundle {
                    primary: "NOT JCL".into(),
                    ..Default::default()
                },
                JclLimits::default()
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            parse_jcl(
                &JclBundle {
                    primary: "x".repeat(5),
                    ..Default::default()
                },
                JclLimits {
                    max_source_bytes: 4,
                    ..Default::default()
                }
            ),
            Err(HostProblem::ResourceExhausted)
        );
    }

    #[test]
    fn fixed_width_instream_delimiter_ignores_record_padding() {
        let source = "//CARDJOB JOB CLASS=A                                                       \n//COPY EXEC PGM=IEBGENER                                                   \n//SYSUT1 DD *                                                              \nHELLO                                                                       \n/*                                                                          \n//SYSUT2 DD SYSOUT=*                                                        \n";
        let plan = parse_jcl(
            &JclBundle {
                primary: source.into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(
            plan.steps[0].dds[0].inline_data,
            b"HELLO                                                                       \n"
        );
        assert_eq!(plan.steps[0].dds[1].name, "SYSUT2");
    }

    #[test]
    fn cataloged_procedure_symbols_overrides_and_concatenations_keep_provenance() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A\n// SET ROOT=AWS\n// SET HLQ=&ROOT..DEMO\n//JOBLIB DD DSN=ONE\n// DD DSN=TWO\n//RUN EXEC PROC=REPROC,\n// CNTLLIB=&HLQ..CNTL\n//P.IN DD DSN=&HLQ..INPUT\n".into(),
                cataloged_procedures: BTreeMap::from([(
                    "REPROC".into(),
                    "//REPROC PROC CNTLLIB=NULL\n//P EXEC PGM=IDCAMS\n//IN DD DSN=&CNTLLIB\n// PEND\n".into(),
                )]),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.symbols["HLQ"], "AWS.DEMO");
        assert_eq!(plan.job_dds.len(), 2);
        assert_eq!(plan.job_dds[1].name, "JOBLIB");
        assert!(plan.job_dds[1].concatenation);
        assert_eq!(plan.steps[0].procedure.as_deref(), Some("REPROC"));
        assert_eq!(plan.steps[0].invocation_line, Some(6));
        assert_eq!(plan.steps[0].source_line, 2);
        assert_eq!(plan.steps[0].dds.len(), 1);
        assert_eq!(
            plan.steps[0].dds[0].dataset.as_deref(),
            Some("AWS.DEMO.INPUT")
        );
        assert_eq!(plan.steps[0].dds[0].source_line, 8);
        assert_eq!(plan.steps[0].source_end_line, 2);
    }

    #[test]
    fn if_else_conditions_keep_each_selected_step_line() {
        let plan = parse_jcl(
            &JclBundle {
                primary: "//J JOB CLASS=A\n// IF (RC = 0) THEN\n//YES EXEC PGM=IEFBR14\n// ELSE\n//NO EXEC PGM=IDCAMS\n// ENDIF\n".into(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .unwrap();
        assert_eq!(
            plan.steps[0].condition,
            StepCondition::RunIfMaxRc {
                operator: "=".into(),
                code: 0
            }
        );
        assert_eq!(
            plan.steps[1].condition,
            StepCondition::SkipIfMaxRc {
                operator: "=".into(),
                code: 0
            }
        );
        assert_eq!(plan.steps[0].source_line, 3);
        assert_eq!(plan.steps[1].source_line, 5);
    }

    #[test]
    fn prior_job_plan_json_defaults_new_provenance_fields() {
        let plan: JobPlan = serde_json::from_str(
            r#"{"name":"J","class":"A","priority":0,"restart_step":null,"steps":[{"name":"S","program":"IEFBR14","parameter":null,"condition":"Always","dds":[],"source_line":2,"procedure":null}],"source":"//J JOB CLASS=A\n//S EXEC PGM=IEFBR14\n"}"#,
        )
        .unwrap();
        assert!(plan.symbols.is_empty());
        assert!(plan.procedure_libraries.is_empty());
        assert!(plan.job_dds.is_empty());
        assert_eq!(plan.steps[0].source_end_line, 0);
        assert_eq!(plan.steps[0].invocation_line, None);
    }
}
