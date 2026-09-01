use crate::DdPlan;
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramInput {
    pub parameter: Option<String>,
    pub dds: Vec<DdPlan>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtilityDisposition {
    Implemented,
    CicsFileControl,
    NetworkFtp,
    ReportRexx,
    Db2Tso,
    ImsController,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramExecution {
    ProgramService,
    Idcams,
    Sdsf,
    Db2Tso,
    ImsController,
    Unsupported,
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

pub(crate) fn program_execution(program: &str) -> ProgramExecution {
    common_program(program).map_or(ProgramExecution::ProgramService, |entry| entry.execution)
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
                HostRequest::Program(ProgramRequest::Call { program, payload })
                | HostRequest::Program(ProgramRequest::Link { program, payload })
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

impl Program for Builtin {
    fn execute(&self, _: &Invocation, input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
        match self.0.builtin.ok_or(HostProblem::InfrastructureFailure)? {
            BuiltinProgram::Iefbr14 => output(0, vec![self.0.name.as_bytes().to_vec()]),
            BuiltinProgram::Iebgener => {
                let records = dd_records(input, "SYSUT1")?;
                output_to(0, records, "SYSUT2")
            }
            BuiltinProgram::Iebcopy => output(0, vec![summary(self.0.name, input)]),
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
            BuiltinProgram::Iebdg | BuiltinProgram::Iebedit | BuiltinProgram::Iebupdte => {
                output(0, vec![summary(self.0.name, input)])
            }
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
                let mut records = dd_records(input, "SORTIN")?;
                records.sort();
                records = sort_outrec(input, records)?;
                fit_sortout_records(input, &mut records)?;
                output_to(0, records, "SORTOUT")
            }
        }
    }
}

fn dd_records(input: &ProgramInput, name: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
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
        .flat_map(|dd| {
            dd.inline_data
                .split(|byte| *byte == b'\n')
                .filter(|record| !record.is_empty())
                .map(<[u8]>::to_vec)
        })
        .collect())
}

fn sort_outrec(input: &ProgramInput, records: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, HostProblem> {
    let control = match dd_text(input, "SYSIN") {
        Ok(control) => control,
        Err(HostProblem::NotFound) => return Ok(records),
        Err(problem) => return Err(problem),
    };
    let Some(fields) = control_parenthesized(&control, "OUTREC FIELDS=")? else {
        return Ok(records);
    };
    let symbols = dd_text(input, "SYMNAMES")
        .ok()
        .map(|text| sort_symbols(&text))
        .transpose()?
        .unwrap_or_default();
    let tokens = split_control_fields(&fields)?;
    let ccsid = input
        .dds
        .iter()
        .find(|dd| dd.name.eq_ignore_ascii_case("SORTIN"))
        .and_then(|dd| dd.ccsid);
    records
        .into_iter()
        .map(|record| project_sort_record(&record, &tokens, &symbols, ccsid))
        .collect()
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

fn control_parenthesized(control: &str, keyword: &str) -> Result<Option<String>, HostProblem> {
    let Some(start) = control.find(keyword) else {
        return Ok(None);
    };
    let open = control[start + keyword.len()..]
        .find('(')
        .map(|offset| start + keyword.len() + offset)
        .ok_or(HostProblem::Malformed)?;
    let mut depth = 0usize;
    for (offset, character) in control[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1).ok_or(HostProblem::Malformed)?;
                if depth == 0 {
                    return Ok(Some(control[open + 1..open + offset].to_string()));
                }
            }
            _ => {}
        }
    }
    Err(HostProblem::Malformed)
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

fn sort_symbols(control: &str) -> Result<BTreeMap<String, (usize, usize)>, HostProblem> {
    let mut symbols = BTreeMap::new();
    for line in control.lines().filter(|line| !line.trim().is_empty()) {
        let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 3 {
            return Err(HostProblem::Malformed);
        }
        let start = parts[1]
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        let length = parts[2]
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if start == 0
            || length == 0
            || symbols
                .insert(parts[0].to_string(), (start, length))
                .is_some()
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(symbols)
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

fn dataset_space(ccsid: Option<u16>) -> Result<u8, HostProblem> {
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
        edited.replace_range(..1, "-");
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

fn summary(name: &str, input: &ProgramInput) -> Vec<u8> {
    format!(
        "{name} DD={} PARM={}",
        input.dds.len(),
        input.parameter.as_deref().unwrap_or("")
    )
    .into_bytes()
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
        }
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
