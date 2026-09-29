use crate::{DdPlan, UtilityHandler};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{BoundedPayload, CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, ProgramRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "generated/common_programs.rs"]
mod common_programs;
pub use common_programs::SystemServiceProgram;
pub(crate) use common_programs::TsoProgramExecution;
use common_programs::{
    BuiltinProgram, COMMON_PROGRAM_CATALOG_SHA256, COMMON_PROGRAMS, SYSTEM_SERVICES, TSO_PROGRAMS,
};
mod sort_control;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramExecutionContext {
    pub job_name: String,
    pub step_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramInput {
    pub parameter: Option<String>,
    pub dds: Vec<DdPlan>,
    /// Exact records hydrated for each effective DD name.
    ///
    /// This is separate from `DdPlan::inline_data`: flattening dataset records
    /// into delimiter-separated bytes cannot represent empty records or data
    /// bytes that happen to equal the delimiter. The default preserves wire
    /// compatibility with callers that still provide inline-only input.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dd_records: BTreeMap<String, Vec<Vec<u8>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ProgramExecutionContext>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramOutput {
    pub return_code: i32,
    pub records: Vec<Vec<u8>>,
    #[serde(default)]
    pub dd_outputs: BTreeMap<String, Vec<Vec<u8>>>,
}

pub trait Program: Send + Sync {
    fn execute(
        &self,
        invocation: &Invocation,
        input: &ProgramInput,
    ) -> Result<ProgramOutput, HostProblem>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UtilityDisposition {
    Implemented,
    CicsFileControl,
    NetworkFtp,
    ReportRexx,
    Db2Tso,
    ImsController,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProgramExecution {
    ProgramService,
    Idcams,
    Sdsf,
    Db2Tso,
    ImsController,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "handler")]
pub enum RegisteredProgramHandler {
    ProgramService,
    Utility(UtilityHandler),
    Sdsf,
    Db2Tso,
    ImsController,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramRegistration {
    pub schema_version: String,
    pub program: String,
    pub disposition: Option<UtilityDisposition>,
    pub handler: RegisteredProgramHandler,
}

impl ProgramRegistration {
    pub fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != crate::JES_UTILITY_REGISTRY_CONTRACT
            || self.program.is_empty()
            || self.program.len() > 128
            || !self.program.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#' | b'_' | b'-')
            })
            || matches!(self.handler, RegisteredProgramHandler::Utility(_))
                != self
                    .disposition
                    .is_some_and(|value| value == UtilityDisposition::Implemented)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CommonProgramEntry {
    pub name: &'static str,
    pub disposition: UtilityDisposition,
    pub execution: ProgramExecution,
    pub builtin: Option<BuiltinProgram>,
}

#[must_use]
pub const fn common_program_catalog_sha256() -> &'static str {
    COMMON_PROGRAM_CATALOG_SHA256
}

fn common_program(program: &str) -> Option<&'static CommonProgramEntry> {
    COMMON_PROGRAMS
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(program))
}

pub fn resolve_program_registration(program: &str) -> Result<ProgramRegistration, HostProblem> {
    let program = program.to_ascii_uppercase();
    let entry = common_program(&program);
    let handler = if let Some(builtin) = entry.and_then(|entry| entry.builtin) {
        RegisteredProgramHandler::Utility(builtin.utility_handler())
    } else {
        match entry.map_or(ProgramExecution::ProgramService, |entry| entry.execution) {
            ProgramExecution::ProgramService => RegisteredProgramHandler::ProgramService,
            ProgramExecution::Idcams => RegisteredProgramHandler::Utility(UtilityHandler::Idcams),
            ProgramExecution::Sdsf => RegisteredProgramHandler::Sdsf,
            ProgramExecution::Db2Tso => RegisteredProgramHandler::Db2Tso,
            ProgramExecution::ImsController => RegisteredProgramHandler::ImsController,
            ProgramExecution::Unsupported => RegisteredProgramHandler::Unsupported,
        }
    };
    let registration = ProgramRegistration {
        schema_version: crate::JES_UTILITY_REGISTRY_CONTRACT.into(),
        program,
        disposition: entry.map(|entry| entry.disposition),
        handler,
    };
    registration.validate()?;
    Ok(registration)
}

pub(crate) fn tso_program_execution(program: &str) -> Option<TsoProgramExecution> {
    TSO_PROGRAMS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(program))
        .map(|(_, execution)| *execution)
}

#[must_use]
pub fn system_service_program(program: &str) -> Option<SystemServiceProgram> {
    SYSTEM_SERVICES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(program))
        .map(|(_, service)| *service)
}

#[must_use]
pub fn utility_disposition(program: &str) -> Option<UtilityDisposition> {
    common_program(program).map(|entry| entry.disposition)
}

pub struct ProgramRouter {
    descriptor: CapabilityDescriptor,
    programs: BTreeMap<String, Arc<dyn Program>>,
}

impl ProgramRouter {
    pub fn new(
        programs: BTreeMap<String, Arc<dyn Program>>,
        limits: InvocationLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        if programs.len() > limits.max_capabilities
            || programs.keys().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || !name.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(Arc::new(Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.program.invoke", limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                provider_id: "mainframe-env-program-router".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.program.request@1".into(),
                result_schema: "mainframe-env.program.output@1".into(),
                max_request_bytes: 4 * 1024 * 1024,
                max_result_bytes: 4 * 1024 * 1024,
                ready: true,
            },
            programs,
        }))
    }

    #[must_use]
    pub fn with_builtins(limits: InvocationLimits) -> Arc<Self> {
        Self::with_builtins_and(BTreeMap::new(), limits).expect("built-in program catalog is valid")
    }

    pub fn with_builtins_and(
        mut programs: BTreeMap<String, Arc<dyn Program>>,
        limits: InvocationLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        for entry in COMMON_PROGRAMS
            .iter()
            .filter(|entry| entry.builtin.is_some())
        {
            if programs
                .insert(entry.name.to_string(), Arc::new(Builtin(entry)))
                .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        Self::new(programs, limits)
    }

    #[must_use]
    pub fn supported_programs(&self) -> impl ExactSizeIterator<Item = &String> {
        self.programs.keys()
    }
}

impl HostProvider for ProgramRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let sequence = effect.sequence;
        let outcome = (|| {
            if let HostRequest::Program(ProgramRequest::Inquire { program }) = &effect.request {
                if !self
                    .programs
                    .contains_key(&program.as_str().to_ascii_uppercase())
                {
                    return Err(HostProblem::NotFound);
                }
                return BoundedPayload::new(
                    "mainframe-env.program.inquire@1",
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .map(HostResult::Program)
                .map_err(|_| HostProblem::ResourceExhausted);
            }
            let (program, payload) = match effect.request {
                HostRequest::Program(ProgramRequest::Call {
                    service: Some(_), ..
                }) => return Err(HostProblem::Unsupported),
                HostRequest::Program(ProgramRequest::Call {
                    program,
                    payload,
                    service: None,
                })
                | HostRequest::Program(ProgramRequest::Link {
                    program,
                    payload,
                    selection: None,
                })
                | HostRequest::Program(ProgramRequest::Xctl { program, payload }) => {
                    (program, payload)
                }
                HostRequest::Program(_) => return Err(HostProblem::Unsupported),
                _ => return Err(HostProblem::Malformed),
            };
            let input: ProgramInput =
                serde_json::from_slice(payload.bytes()).map_err(|_| HostProblem::Malformed)?;
            let implementation = self
                .programs
                .get(program.as_str())
                .ok_or(HostProblem::NotFound)?;
            let output = implementation.execute(invocation, &input)?;
            let bytes = serde_json::to_vec(&output).map_err(|_| HostProblem::ProviderFailure)?;
            Ok(HostResult::Program(
                BoundedPayload::new(
                    "mainframe-env.program.output@1",
                    bytes,
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            ))
        })();
        EffectResult { sequence, outcome }
    }
}

pub fn decode_program_output(payload: &BoundedPayload) -> Result<ProgramOutput, HostProblem> {
    if payload.schema() != "mainframe-env.program.output@1" {
        return Err(HostProblem::Malformed);
    }
    serde_json::from_slice(payload.bytes()).map_err(|_| HostProblem::Malformed)
}

struct Builtin(&'static CommonProgramEntry);

impl BuiltinProgram {
    const fn utility_handler(self) -> UtilityHandler {
        match self {
            Self::Iefbr14 => UtilityHandler::Iefbr14,
            Self::Iebgener => UtilityHandler::Iebgener,
            Self::Iebcopy => UtilityHandler::Iebcopy,
            Self::Iebcompr => UtilityHandler::Iebcompr,
            Self::Iebdg => UtilityHandler::Iebdg,
            Self::Iebedit => UtilityHandler::Iebedit,
            Self::Iebupdte => UtilityHandler::Iebupdte,
            Self::Idcams => UtilityHandler::Idcams,
            Self::Sort => UtilityHandler::Sort,
        }
    }
}

impl Program for Builtin {
    fn execute(
        &self,
        invocation: &Invocation,
        input: &ProgramInput,
    ) -> Result<ProgramOutput, HostProblem> {
        match self.0.builtin.ok_or(HostProblem::InfrastructureFailure)? {
            BuiltinProgram::Iefbr14 => output(0, vec![self.0.name.as_bytes().to_vec()]),
            BuiltinProgram::Iebgener => {
                let records = dd_records(input, "SYSUT1")?;
                output_to(0, records, "SYSUT2")
            }
            BuiltinProgram::Iebcopy => iebcopy(input),
            BuiltinProgram::Iebcompr => {
                let left = dd_records(input, "SYSUT1")?;
                let right = dd_records(input, "SYSUT2")?;
                output(
                    i32::from(left != right) * 8,
                    vec![if left == right {
                        b"IEBCOMPR EQUAL".to_vec()
                    } else {
                        b"IEBCOMPR DIFFERENT".to_vec()
                    }],
                )
            }
            BuiltinProgram::Iebdg => iebdg(input, invocation.limits.max_output_bytes),
            BuiltinProgram::Iebedit => iebedit(input),
            BuiltinProgram::Iebupdte => iebupdte(input),
            BuiltinProgram::Idcams => {
                let control = input
                    .dds
                    .iter()
                    .find(|dd| dd.name == "SYSIN")
                    .map(|dd| String::from_utf8_lossy(&dd.inline_data).to_ascii_uppercase())
                    .or_else(|| {
                        input
                            .parameter
                            .as_ref()
                            .map(|value| value.to_ascii_uppercase())
                    })
                    .ok_or(HostProblem::Malformed)?;
                let command = control
                    .split_whitespace()
                    .next()
                    .ok_or(HostProblem::Malformed)?;
                crate::ams::validate_idcams_control(control.as_bytes())?;
                output(0, vec![format!("IDCAMS {command}").into_bytes()])
            }
            BuiltinProgram::Sort => {
                let mut records = sort_control::execute(input, dd_records(input, "SORTIN")?)?;
                fit_sortout_records(input, &mut records)?;
                output_to(0, records, "SORTOUT")
            }
        }
    }
}

fn dd_records(input: &ProgramInput, name: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
    if let Some(records) = input.dd_records.get(&name.to_ascii_uppercase()) {
        return Ok(records.clone());
    }
    let dds = input
        .dds
        .iter()
        .filter(|dd| dd.name.eq_ignore_ascii_case(name))
        .collect::<Vec<_>>();
    if dds.is_empty() {
        return Err(HostProblem::NotFound);
    }
    Ok(dds
        .into_iter()
        .flat_map(|dd| inline_records(&dd.inline_data))
        .collect())
}

fn inline_records(bytes: &[u8]) -> Vec<Vec<u8>> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut records = bytes
        .split(|byte| *byte == b'\n')
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    if bytes.ends_with(b"\n") {
        records.pop();
    }
    records
}

fn fit_sortout_records(input: &ProgramInput, records: &mut [Vec<u8>]) -> Result<(), HostProblem> {
    let Some(dd) = input
        .dds
        .iter()
        .find(|dd| dd.name.eq_ignore_ascii_case("SORTOUT"))
    else {
        return Err(HostProblem::NotFound);
    };
    let Some(length) = dd.logical_record_length.map(|length| length as usize) else {
        return Ok(());
    };
    let space = dataset_space(dd.ccsid)?;
    for record in records {
        record.resize(length, space);
    }
    Ok(())
}

fn dd_text(input: &ProgramInput, name: &str) -> Result<String, HostProblem> {
    String::from_utf8(
        dd_records(input, name)?
            .into_iter()
            .flat_map(|mut record| {
                record.push(b'\n');
                record
            })
            .collect(),
    )
    .map(|text| text.to_ascii_uppercase())
    .map_err(|_| HostProblem::Malformed)
}

fn split_control_fields(fields: &str) -> Result<Vec<String>, HostProblem> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote = None;
    for character in fields.chars() {
        if matches!(character, '\'' | '"') {
            quote = if quote == Some(character) {
                None
            } else if quote.is_none() {
                Some(character)
            } else {
                quote
            };
        }
        match character {
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() => {
                depth = depth.checked_sub(1).ok_or(HostProblem::Malformed)?;
            }
            ',' if quote.is_none() && depth == 0 => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    if quote.is_some() || depth != 0 {
        return Err(HostProblem::Malformed);
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    Ok(tokens)
}

fn project_sort_record(
    record: &[u8],
    tokens: &[String],
    symbols: &BTreeMap<String, (usize, usize)>,
    ccsid: Option<u16>,
) -> Result<Vec<u8>, HostProblem> {
    let mut output = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if let Some(spaces) = token.strip_suffix('X')
            && (spaces.is_empty() || spaces.bytes().all(|byte| byte.is_ascii_digit()))
        {
            let count = if spaces.is_empty() {
                1
            } else {
                spaces.parse().map_err(|_| HostProblem::Malformed)?
            };
            output.extend(std::iter::repeat_n(dataset_space(ccsid)?, count));
            index += 1;
            continue;
        }
        let mut output_position = None;
        let source = if let Some((output_at, source)) = token.split_once(':') {
            output_position = Some(
                output_at
                    .parse::<usize>()
                    .map_err(|_| HostProblem::Malformed)?,
            );
            source
        } else {
            token.as_str()
        };
        let (start, length) = if let Some(value) = symbols.get(source) {
            *value
        } else {
            let start = source
                .parse::<usize>()
                .map_err(|_| HostProblem::Unsupported)?;
            let length = tokens
                .get(index + 1)
                .ok_or(HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)?;
            index += 1;
            (start, length)
        };
        if start == 0 || length == 0 {
            return Err(HostProblem::Malformed);
        }
        if let Some(output_at) = output_position {
            if output_at == 0 || output_at - 1 < output.len() {
                return Err(HostProblem::Malformed);
            }
            output.resize(output_at - 1, dataset_space(ccsid)?);
        }
        let value =
            record
                .get(start - 1..start - 1 + length)
                .ok_or_else(|| HostProblem::Condition {
                    name: "LENGERR".into(),
                    response: 22,
                    response2: 0,
                })?;
        if tokens
            .get(index + 1)
            .is_some_and(|token| token.starts_with("EDIT=("))
        {
            output.extend(edit_zoned(value, &tokens[index + 1], ccsid)?);
            index += 1;
        } else {
            output.extend_from_slice(value);
        }
        index += 1;
    }
    Ok(output)
}

pub(crate) fn dataset_space(ccsid: Option<u16>) -> Result<u8, HostProblem> {
    match ccsid {
        None | Some(1208) => Ok(b' '),
        Some(37) => Ok(0x40),
        Some(_) => Err(HostProblem::Unsupported),
    }
}

fn edit_zoned(value: &[u8], edit: &str, ccsid: Option<u16>) -> Result<Vec<u8>, HostProblem> {
    let pattern = edit
        .strip_prefix("EDIT=(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or(HostProblem::Malformed)?;
    let mut digits = match ccsid {
        None | Some(1208) => {
            String::from_utf8(value.to_vec()).map_err(|_| HostProblem::Malformed)?
        }
        Some(37) => CodePage::Cp037
            .decode(value, value.len().saturating_mul(4).max(1))
            .map_err(|_| HostProblem::Malformed)?,
        Some(_) => return Err(HostProblem::Unsupported),
    };
    let last = digits.pop().ok_or(HostProblem::Malformed)?;
    let (digit, negative) = match last {
        '{' => ('0', false),
        'A'..='I' => ((b'1' + (last as u8 - b'A')) as char, false),
        '}' => ('0', true),
        'J'..='R' => ((b'1' + (last as u8 - b'J')) as char, true),
        '0'..='9' => (last, false),
        _ => return Err(HostProblem::Malformed),
    };
    digits.push(digit);
    let mut source = digits.chars();
    let mut edited = pattern
        .chars()
        .map(|character| {
            if character == 'T' {
                source.next().ok_or(HostProblem::Malformed)
            } else {
                Ok(character)
            }
        })
        .collect::<Result<String, HostProblem>>()?;
    if negative {
        edited.insert(0, '-');
    }
    if source.next().is_some() {
        return Err(HostProblem::Malformed);
    }
    match ccsid {
        None | Some(1208) => Ok(edited.into_bytes()),
        Some(37) => CodePage::Cp037
            .encode(&edited, edited.len().saturating_mul(4).max(1))
            .map_err(|_| HostProblem::Malformed),
        Some(_) => Err(HostProblem::Unsupported),
    }
}

fn iebcopy(input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
    let control = optional_dd_text(input, "SYSIN")?;
    if control
        .as_ref()
        .is_some_and(|control| !control.split_whitespace().any(|word| word == "COPY"))
    {
        return Err(HostProblem::Unsupported);
    }
    let input_dd = control
        .as_deref()
        .and_then(|control| control_parameter(control, "INDD"))
        .unwrap_or_else(|| "SYSUT1".into());
    let output_dd = control
        .as_deref()
        .and_then(|control| control_parameter(control, "OUTDD"))
        .unwrap_or_else(|| "SYSUT2".into());
    if input_dd == output_dd {
        return Err(HostProblem::Malformed);
    }
    let records = dd_records(input, &input_dd)?;
    output_to(0, records, &output_dd)
}

fn iebdg(input: &ProgramInput, max_output_bytes: u64) -> Result<ProgramOutput, HostProblem> {
    let control = dd_text(input, "SYSIN")?;
    if !control.contains("DSD") || !control.contains("CREATE") {
        return Err(HostProblem::Unsupported);
    }
    let output_dd = control_parameter(&control, "OUTPUT").unwrap_or_else(|| "SYSUT2".into());
    let quantity = control_parameter(&control, "QUANTITY")
        .or_else(|| control_parameter(&control, "RECORDS"))
        .ok_or(HostProblem::Malformed)?
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if quantity == 0 || quantity > 262_144 {
        return Err(HostProblem::ResourceExhausted);
    }
    let target = input
        .dds
        .iter()
        .find(|dd| dd.name.eq_ignore_ascii_case(&output_dd))
        .ok_or(HostProblem::NotFound)?;
    let length = control_parameter(&control, "LENGTH")
        .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
        .transpose()?
        .or_else(|| target.logical_record_length.map(|value| value as usize))
        .unwrap_or(80);
    if length == 0 || length > 1024 * 1024 {
        return Err(HostProblem::ResourceExhausted);
    }
    let output_bytes = u64::try_from(quantity)
        .ok()
        .and_then(|quantity| {
            u64::try_from(length)
                .ok()
                .and_then(|length| quantity.checked_mul(length))
        })
        .ok_or(HostProblem::ResourceExhausted)?;
    if output_bytes > max_output_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let value = control_parameter(&control, "VALUE").map(String::into_bytes);
    let records = (1..=quantity)
        .map(|ordinal| {
            if let Some(value) = &value {
                if value.is_empty() {
                    return Err(HostProblem::Malformed);
                }
                Ok(value.iter().copied().cycle().take(length).collect())
            } else {
                let digits = format!("{ordinal:0length$}");
                Ok(digits.as_bytes()[digits.len().saturating_sub(length)..].to_vec())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    output_to(0, records, &output_dd)
}

fn iebedit(input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
    let records = dd_records(input, "SYSUT1")?;
    let control = dd_text(input, "SYSIN")?;
    if !control.contains("EDIT") {
        return Err(HostProblem::Unsupported);
    }
    let start = control_parameter(&control, "START").ok_or(HostProblem::Malformed)?;
    let (start_index, end_index) = if let Ok(position) = start.parse::<usize>() {
        if position == 0 || position > records.len() {
            return Err(HostProblem::Malformed);
        }
        let stop = control_parameter(&control, "STOP")
            .or_else(|| control_parameter(&control, "END"))
            .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
            .transpose()?
            .unwrap_or(records.len());
        if stop < position || stop > records.len() {
            return Err(HostProblem::Malformed);
        }
        (position - 1, stop)
    } else {
        let start = records
            .iter()
            .position(|record| jcl_card(record, &start, "JOB"))
            .ok_or(HostProblem::NotFound)?;
        let end = records[start + 1..]
            .iter()
            .position(|record| jcl_job_card(record))
            .map_or(records.len(), |offset| start + 1 + offset);
        (start, end)
    };
    let selected = records[start_index..end_index].to_vec();
    let selected = if let Some(step) = control_parameter(&control, "STEPNAME") {
        let step_start = selected
            .iter()
            .position(|record| jcl_card(record, &step, "EXEC"))
            .ok_or(HostProblem::NotFound)?;
        let step_end = selected[step_start + 1..]
            .iter()
            .position(|record| jcl_exec_or_job_card(record))
            .map_or(selected.len(), |offset| step_start + 1 + offset);
        let mut output = selected
            .first()
            .filter(|record| jcl_job_card(record))
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        output.extend_from_slice(&selected[step_start..step_end]);
        output
    } else {
        selected
    };
    output_to(0, selected, "SYSUT2")
}

fn iebupdte(input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
    let target = input
        .dds
        .iter()
        .find(|dd| dd.name.eq_ignore_ascii_case("SYSUT2"))
        .ok_or(HostProblem::NotFound)?;
    let target_member = target.member.as_deref().ok_or(HostProblem::Unsupported)?;
    let controls = dd_records(input, "SYSIN")?;
    let mut header = None;
    let mut records = Vec::new();
    for record in controls {
        let text = std::str::from_utf8(&record).map_err(|_| HostProblem::Malformed)?;
        let upper = text.trim().to_ascii_uppercase();
        if upper.starts_with("./ ADD ") || upper.starts_with("./ REPL ") {
            if header.is_some() {
                return Err(HostProblem::Unsupported);
            }
            let member = control_parameter(&upper, "NAME").ok_or(HostProblem::Malformed)?;
            if !member.eq_ignore_ascii_case(target_member) {
                return Err(HostProblem::Malformed);
            }
            header = Some(member);
        } else if upper.starts_with("./ ENDUP") {
            break;
        } else if upper.starts_with("./") {
            return Err(HostProblem::Unsupported);
        } else if header.is_some() {
            records.push(record);
        }
    }
    let member = header.ok_or(HostProblem::Malformed)?;
    let count = records.len();
    Ok(ProgramOutput {
        return_code: 0,
        records: vec![format!("IEBUPDTE MEMBER={member} RECORDS={count}").into_bytes()],
        dd_outputs: BTreeMap::from([("SYSUT2".into(), records)]),
    })
}

fn optional_dd_text(input: &ProgramInput, name: &str) -> Result<Option<String>, HostProblem> {
    match dd_text(input, name) {
        Ok(value) => Ok(Some(value)),
        Err(HostProblem::NotFound) => Ok(None),
        Err(problem) => Err(problem),
    }
}

fn control_parameter(control: &str, keyword: &str) -> Option<String> {
    let needle = format!("{keyword}=");
    let start = control.match_indices(&needle).find_map(|(offset, _)| {
        (offset == 0
            || !control.as_bytes()[offset - 1].is_ascii_alphanumeric()
                && control.as_bytes()[offset - 1] != b'_')
            .then_some(offset + needle.len())
    })?;
    let tail = control[start..].trim_start();
    let value = if let Some(inner) = tail.strip_prefix('(') {
        &inner[..inner.find(')')?]
    } else {
        &tail[..tail
            .find(|character: char| character == ',' || character.is_ascii_whitespace())
            .unwrap_or(tail.len())]
    };
    let value = value.trim().trim_matches(['\'', '"']).to_ascii_uppercase();
    (!value.is_empty()).then_some(value)
}

fn jcl_card(record: &[u8], name: &str, operation: &str) -> bool {
    let Ok(text) = std::str::from_utf8(record) else {
        return false;
    };
    let Some(text) = text.strip_prefix("//") else {
        return false;
    };
    let mut fields = text.split_ascii_whitespace();
    fields.next().is_some_and(|value| value == name)
        && fields.next().is_some_and(|value| value == operation)
}

fn jcl_job_card(record: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(record) else {
        return false;
    };
    let Some(text) = text.strip_prefix("//") else {
        return false;
    };
    text.split_ascii_whitespace().nth(1) == Some("JOB")
}

fn jcl_exec_or_job_card(record: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(record) else {
        return false;
    };
    let Some(text) = text.strip_prefix("//") else {
        return false;
    };
    matches!(text.split_ascii_whitespace().nth(1), Some("EXEC" | "JOB"))
}

fn output(return_code: i32, records: Vec<Vec<u8>>) -> Result<ProgramOutput, HostProblem> {
    Ok(ProgramOutput {
        return_code,
        records,
        dd_outputs: BTreeMap::new(),
    })
}

fn output_to(
    return_code: i32,
    records: Vec<Vec<u8>>,
    dd: &str,
) -> Result<ProgramOutput, HostProblem> {
    let count = records.len();
    Ok(ProgramOutput {
        return_code,
        records: vec![format!("{dd} RECORDS={count}").into_bytes()],
        dd_outputs: BTreeMap::from([(dd.into(), records)]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::{ProgramLinkSelection, ProgramName};
    use std::collections::{BTreeMap, BTreeSet};

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
            None,
            Selector::new("program:test", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("USER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", limits).unwrap(),
            IdempotencyKey::new("key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    #[test]
    fn selected_link_cannot_fall_back_to_builtin_name_dispatch() {
        let limits = InvocationLimits::default();
        let invocation = invocation();
        let request = HostRequest::Program(ProgramRequest::Link {
            program: ProgramName::new("IEFBR14", 128).unwrap(),
            payload: BoundedPayload::new("mainframe-env.program.input@1", b"{}".to_vec(), limits)
                .unwrap(),
            selection: Some(ProgramLinkSelection {
                artifact: ArtifactRef::new(format!("sha256:{:064x}", 1), limits).unwrap(),
                generation: 1,
                content_identity: format!("sha256:{:064x}", 2),
            }),
        });
        let router = ProgramRouter::with_builtins(limits);
        let result = router.invoke(
            &invocation,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: 100,
                idempotency_key: None,
                request,
            },
        );
        assert_eq!(result.outcome, Err(HostProblem::Unsupported));
    }

    fn input(name: &str, bytes: &[u8]) -> ProgramInput {
        ProgramInput {
            parameter: None,
            dds: vec![DdPlan {
                name: name.into(),
                dataset: None,
                member: None,
                generation: None,
                organization: None,
                record_format: None,
                logical_record_length: None,
                ccsid: None,
                temporary: false,
                sysout: None,
                disposition: Vec::new(),
                inline_data: bytes.to_vec(),
                concatenation: false,
                source_line: 1,
                source_end_line: 1,
                parameters: Vec::new(),
            }],
            dd_records: BTreeMap::new(),
            execution: None,
        }
    }

    fn add_dd(input: &mut ProgramInput, name: &str, bytes: &[u8]) {
        input.dds.push(DdPlan {
            name: name.into(),
            dataset: None,
            member: None,
            generation: None,
            organization: None,
            record_format: None,
            logical_record_length: None,
            ccsid: None,
            temporary: false,
            sysout: None,
            disposition: Vec::new(),
            inline_data: bytes.to_vec(),
            concatenation: false,
            source_line: 1,
            source_end_line: 1,
            parameters: Vec::new(),
        });
    }

    #[test]
    fn every_accepted_builtin_has_a_real_route() {
        let router = ProgramRouter::with_builtins(InvocationLimits::default());
        assert_eq!(router.supported_programs().len(), 9);
        assert!(!router.supported_programs().any(|name| name == "UNKNOWN"));
        assert_eq!(
            utility_disposition("SDSF"),
            Some(UtilityDisposition::CicsFileControl)
        );
        assert_eq!(
            utility_disposition("FTP"),
            Some(UtilityDisposition::NetworkFtp)
        );
        assert_eq!(
            utility_disposition("IKJEFT1B"),
            Some(UtilityDisposition::ReportRexx)
        );
        assert_eq!(
            tso_program_execution("DSNTIAUL"),
            Some(TsoProgramExecution::Extract)
        );
        assert_eq!(
            system_service_program("ceedays"),
            Some(SystemServiceProgram::Ceedays)
        );
        assert!(common_program_catalog_sha256().starts_with("sha256:"));
        assert_eq!(utility_disposition("UNKNOWN"), None);
        for program in [
            "IEFBR14", "IEBGENER", "IEBCOPY", "IEBCOMPR", "IEBDG", "IEBEDIT", "IEBUPDTE", "IDCAMS",
            "SORT",
        ] {
            assert!(matches!(
                resolve_program_registration(program).unwrap().handler,
                RegisteredProgramHandler::Utility(_)
            ));
        }
        assert_eq!(
            resolve_program_registration("APPLICATION").unwrap().handler,
            RegisteredProgramHandler::ProgramService
        );
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/schemas/jes-program-registration.schema.json"
        ))
        .unwrap();
        let validator = jsonschema::draft202012::options()
            .offline()
            .build(&schema)
            .unwrap();
        for program in ["IEBCOPY", "IDCAMS", "SDSF", "APPLICATION"] {
            validator
                .validate(
                    &serde_json::to_value(resolve_program_registration(program).unwrap()).unwrap(),
                )
                .unwrap();
        }
    }

    #[test]
    fn generate_and_sort_transform_exact_records() {
        assert_eq!(
            Builtin(common_program("IEBGENER").unwrap())
                .execute(&invocation(), &input("SYSUT1", b"B\nA\n"))
                .unwrap()
                .records,
            vec![b"SYSUT2 RECORDS=2".to_vec()]
        );
        let mut sort_input = input("SORTIN", b"B\nA\n");
        let mut sort_output = input("SORTOUT", b"");
        sort_input.dds.append(&mut sort_output.dds);
        assert_eq!(
            Builtin(common_program("SORT").unwrap())
                .execute(&invocation(), &sort_input)
                .unwrap()
                .records,
            vec![b"SORTOUT RECORDS=2".to_vec()]
        );
        assert_eq!(
            Builtin(common_program("SORT").unwrap())
                .execute(&invocation(), &sort_input)
                .unwrap()
                .dd_outputs["SORTOUT"],
            [b"A".to_vec(), b"B".to_vec()]
        );
    }

    fn sort_case(
        records: &[&[u8]],
        sysin: &str,
        symnames: Option<&str>,
        ccsid: Option<u16>,
    ) -> Result<Vec<Vec<u8>>, HostProblem> {
        let mut case = input("SORTIN", b"");
        case.dd_records.insert(
            "SORTIN".into(),
            records.iter().map(|record| record.to_vec()).collect(),
        );
        case.dds[0].ccsid = ccsid;
        add_dd(&mut case, "SORTOUT", b"");
        add_dd(&mut case, "SYSIN", sysin.as_bytes());
        if let Some(symbols) = symnames {
            add_dd(&mut case, "SYMNAMES", symbols.as_bytes());
        }
        Builtin(common_program("SORT").unwrap())
            .execute(&invocation(), &case)
            .map(|out| out.dd_outputs["SORTOUT"].clone())
    }

    #[test]
    fn sort_orders_by_positional_key_not_leading_bytes() {
        assert_eq!(
            sort_case(
                &[b"AAAAAAAAAAB", b"ZZZZZZZZZZA"],
                " SORT FIELDS=(11,1,CH,A)\n",
                None,
                None
            )
            .unwrap(),
            [b"ZZZZZZZZZZA".to_vec(), b"AAAAAAAAAAB".to_vec()]
        );
    }

    #[test]
    fn sort_orders_by_symnames_zd_key_stably() {
        let records = [b"B\xF0\xF2".as_slice(), b"Z\xF0\xD1", b"A\xF0\xF2"];
        assert_eq!(
            sort_case(
                &records,
                " SORT FIELDS=(AMOUNT,A)\n",
                Some("AMOUNT,2,2,ZD\n"),
                Some(37)
            )
            .unwrap(),
            [
                records[1].to_vec(),
                records[0].to_vec(),
                records[2].to_vec()
            ]
        );
    }

    #[test]
    fn sort_accepts_hyphenated_carddemo_symbol() {
        let records = ["Z012345678901234", "A012345678901234"]
            .map(|record| CodePage::Cp037.encode(record, 16).unwrap());
        let refs = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            sort_case(
                &refs,
                " SORT FIELDS=(TRAN-ID,A)\n",
                Some("TRAN-ID,1,16,CH                                                         \n"),
                Some(37),
            )
            .unwrap(),
            [records[1].clone(), records[0].clone()]
        );
    }

    #[test]
    fn sort_orders_by_two_keys_like_creastmt() {
        assert_eq!(
            sort_case(
                &[b"ZAa", b"ABa", b"YAb"],
                " SORT FIELDS=(3,1,CH,A,1,1,CH,A)\n",
                None,
                None
            )
            .unwrap(),
            [b"ABa".to_vec(), b"ZAa".to_vec(), b"YAb".to_vec()]
        );
    }

    #[test]
    fn include_cond_keeps_only_records_in_date_range() {
        let records = ["20240101", "20240615", "20250101"]
            .map(|date| CodePage::Cp037.encode(date, 8).unwrap());
        let refs = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
        let symbols = "DATE,1,8,CH //Date\nFROM,C'20240601' //Date\nTHRU,C'20241231' //Date\n";
        assert_eq!(sort_case(&refs, " INCLUDE COND=(DATE,GE,FROM,\n               AND,DATE,LE,THRU)\n SORT FIELDS=(DATE,A)\n", Some(symbols), Some(37)).unwrap(), [records[1].clone()]);
    }

    #[test]
    fn omit_cond_drops_matching_records() {
        assert_eq!(
            sort_case(
                &[b"A1", b"B2", b"C3"],
                " OMIT COND=(1,1,CH,EQ,C'B')\n SORT FIELDS=(1,1,CH,A)\n",
                None,
                None
            )
            .unwrap(),
            [b"A1".to_vec(), b"C3".to_vec()]
        );
    }

    #[test]
    fn include_cond_applies_and_before_or() {
        assert_eq!(
            sort_case(
                &[b"A0", b"B1", b"B2", b"C2"],
                " INCLUDE COND=(1,1,CH,EQ,C'A',OR,1,1,CH,EQ,C'B',\n AND,2,1,CH,EQ,C'2')\n SORT FIELDS=(1,1,CH,A)\n",
                None,
                None
            )
            .unwrap(),
            [b"A0".to_vec(), b"B2".to_vec()]
        );
    }

    #[test]
    fn sort_ignores_comment_and_columns_after_71() {
        let control = format!(
            "* SUM FIELDS=NONE\n {:<70}OUTFIL FNAMES=X\n",
            "SORT FIELDS=(1,1,CH,A)"
        );
        assert_eq!(
            sort_case(&[b"B", b"A"], &control, None, None).unwrap(),
            [b"A".to_vec(), b"B".to_vec()]
        );
    }

    #[test]
    fn sort_rejects_unsupported_statement() {
        for control in [
            " SUM FIELDS=NONE\n",
            " OUTFIL FNAMES=X\n",
            " SORT FIELDS=COPY\n",
            " OPTION EQUALS\n",
            " SORT FIELDS=(1,1,BI,A)\n",
            " SORT FIELDS=(1,1,CH,A),EQUALS\n",
            " INCLUDE COND=((1,1,CH,EQ,C'A'))\n",
        ] {
            assert_eq!(
                sort_case(&[b"A"], control, None, None),
                Err(HostProblem::Unsupported)
            );
        }
    }

    #[test]
    fn sort_rejects_unknown_symbol_and_malformed_symnames() {
        assert_eq!(
            sort_case(&[b"A"], " SORT FIELDS=(UNKNOWN,A)\n", None, None),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(
            sort_case(
                &[b"A"],
                " SORT FIELDS=(FIELD,A)\n",
                Some("FIELD,BOGUS,1,CH\n"),
                None
            ),
            Err(HostProblem::Unsupported)
        );
    }

    #[test]
    fn copy_generation_edit_and_update_utilities_produce_exact_dd_effects() {
        let mut copy = input("INPUT", b"ONE\nTWO\n");
        add_dd(&mut copy, "OUTPUT", b"");
        add_dd(&mut copy, "SYSIN", b" COPY INDD=INPUT,OUTDD=OUTPUT\n");
        assert_eq!(
            Builtin(common_program("IEBCOPY").unwrap())
                .execute(&invocation(), &copy)
                .unwrap()
                .dd_outputs["OUTPUT"],
            [b"ONE".to_vec(), b"TWO".to_vec()]
        );

        let mut compare = input("SYSUT1", b"SAME\n");
        add_dd(&mut compare, "SYSUT2", b"SAME\n");
        assert_eq!(
            Builtin(common_program("IEBCOMPR").unwrap())
                .execute(&invocation(), &compare)
                .unwrap()
                .return_code,
            0
        );
        compare.dds.last_mut().unwrap().inline_data = b"DIFFERENT\n".to_vec();
        assert_eq!(
            Builtin(common_program("IEBCOMPR").unwrap())
                .execute(&invocation(), &compare)
                .unwrap()
                .return_code,
            8
        );

        let mut generate = input(
            "SYSIN",
            b" DSD OUTPUT=(DATA)\n FD NAME=FIELD,LENGTH=4\n CREATE QUANTITY=3\n END\n",
        );
        add_dd(&mut generate, "DATA", b"");
        assert_eq!(
            Builtin(common_program("IEBDG").unwrap())
                .execute(&invocation(), &generate)
                .unwrap()
                .dd_outputs["DATA"],
            [b"0001".to_vec(), b"0002".to_vec(), b"0003".to_vec()]
        );

        let mut edit = input(
            "SYSUT1",
            b"//JOB1 JOB\n//S1 EXEC PGM=A\n//D1 DD *\n//JOB2 JOB\n//S2 EXEC PGM=B\n//D2 DD *\n",
        );
        add_dd(&mut edit, "SYSIN", b" EDIT START=JOB2\n");
        add_dd(&mut edit, "SYSUT2", b"");
        assert_eq!(
            Builtin(common_program("IEBEDIT").unwrap())
                .execute(&invocation(), &edit)
                .unwrap()
                .dd_outputs["SYSUT2"],
            [
                b"//JOB2 JOB".to_vec(),
                b"//S2 EXEC PGM=B".to_vec(),
                b"//D2 DD *".to_vec()
            ]
        );

        let mut update = input(
            "SYSIN",
            b"./ ADD NAME=MEMBER\nRECORD ONE\nRECORD TWO\n./ ENDUP\n",
        );
        add_dd(&mut update, "SYSUT2", b"");
        update.dds.last_mut().unwrap().member = Some("MEMBER".into());
        assert_eq!(
            Builtin(common_program("IEBUPDTE").unwrap())
                .execute(&invocation(), &update)
                .unwrap()
                .dd_outputs["SYSUT2"],
            [b"RECORD ONE".to_vec(), b"RECORD TWO".to_vec()]
        );
    }

    #[test]
    fn unsupported_utility_controls_fail_without_generic_success() {
        let mut copy = input("SYSUT1", b"ONE\n");
        add_dd(&mut copy, "SYSUT2", b"");
        add_dd(&mut copy, "SYSIN", b" COMPRESS\n");
        assert_eq!(
            Builtin(common_program("IEBCOPY").unwrap()).execute(&invocation(), &copy),
            Err(HostProblem::Unsupported)
        );

        let mut edit = input("SYSUT1", b"//JOB JOB\n");
        add_dd(&mut edit, "SYSUT2", b"");
        add_dd(&mut edit, "SYSIN", b" COPY\n");
        assert_eq!(
            Builtin(common_program("IEBEDIT").unwrap()).execute(&invocation(), &edit),
            Err(HostProblem::Unsupported)
        );

        let mut update = input(
            "SYSIN",
            b"./ ADD NAME=MEMBER\nONE\n./ ADD NAME=SECOND\nTWO\n./ ENDUP\n",
        );
        add_dd(&mut update, "SYSUT2", b"");
        update.dds.last_mut().unwrap().member = Some("MEMBER".into());
        assert_eq!(
            Builtin(common_program("IEBUPDTE").unwrap()).execute(&invocation(), &update),
            Err(HostProblem::Unsupported)
        );
    }

    #[test]
    fn sort_outrec_projects_symbolic_edit_and_positional_fields() {
        let symbols = BTreeMap::from([("ACCOUNT".into(), (1, 3)), ("BALANCE".into(), (4, 11))]);
        assert_eq!(
            project_sort_record(
                b"1230000011648G",
                &[
                    "ACCOUNT".into(),
                    "X".into(),
                    "BALANCE".into(),
                    "EDIT=(TTTTTTTTT.TT)".into(),
                    "2X".into(),
                ],
                &symbols,
                Some(1208),
            )
            .unwrap(),
            b"123 000001164.87  "
        );
        assert_eq!(
            project_sort_record(
                b"ABCDEFGHIJKLMNOPQRST",
                &["1:5".into(), "4".into(), "5:1".into(), "4".into()],
                &BTreeMap::new(),
                Some(1208),
            )
            .unwrap(),
            b"EFGHABCD"
        );
        assert_eq!(
            edit_zoned(b"0000011648P", "EDIT=(TTTTTTTTT.TT)", Some(1208)).unwrap(),
            b"-000001164.87"
        );
    }

    #[test]
    fn idcams_unknown_command_is_not_generic_success() {
        assert_eq!(
            Builtin(common_program("IDCAMS").unwrap())
                .execute(&invocation(), &input("SYSIN", b"UNKNOWN THING")),
            Err(HostProblem::Unsupported)
        );
    }
}
