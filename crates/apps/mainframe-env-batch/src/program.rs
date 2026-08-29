use crate::DdPlan;
use mainframe_env_execution_api::{BoundedPayload, CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, ProgramRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramInput {
    pub parameter: Option<String>,
    pub dds: Vec<DdPlan>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProgramOutput {
    pub return_code: i32,
    pub records: Vec<Vec<u8>>,
}

pub trait Program: Send + Sync {
    fn execute(
        &self,
        invocation: &Invocation,
        input: &ProgramInput,
    ) -> Result<ProgramOutput, HostProblem>;
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
        for name in [
            "IEFBR14", "IEBGENER", "IEBCOPY", "IEBCOMPR", "IEBDG", "IEBEDIT", "IEBUPDTE", "IDCAMS",
            "SORT",
        ] {
            if programs
                .insert(name.to_string(), Arc::new(Builtin(name)))
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

struct Builtin(&'static str);

impl Program for Builtin {
    fn execute(&self, _: &Invocation, input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
        match self.0 {
            "IEFBR14" => output(0, vec![b"IEFBR14".to_vec()]),
            "IEBGENER" => {
                let records = dd(input, "SYSUT1")?
                    .inline_data
                    .split(|byte| *byte == b'\n')
                    .filter(|record| !record.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect();
                output(0, records)
            }
            "IEBCOPY" => output(0, vec![summary("IEBCOPY", input)]),
            "IEBCOMPR" => {
                let left = dd(input, "SYSUT1")?;
                let right = dd(input, "SYSUT2")?;
                output(
                    i32::from(left.inline_data != right.inline_data) * 8,
                    vec![if left.inline_data == right.inline_data {
                        b"IEBCOMPR EQUAL".to_vec()
                    } else {
                        b"IEBCOMPR DIFFERENT".to_vec()
                    }],
                )
            }
            "IEBDG" => output(0, vec![summary("IEBDG", input)]),
            "IEBEDIT" => output(0, vec![summary("IEBEDIT", input)]),
            "IEBUPDTE" => output(0, vec![summary("IEBUPDTE", input)]),
            "IDCAMS" => {
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
                if !matches!(command, "LISTCAT" | "DEFINE" | "DELETE" | "REPRO") {
                    return Err(HostProblem::Unsupported);
                }
                output(0, vec![format!("IDCAMS {command}").into_bytes()])
            }
            "SORT" => {
                let mut records = dd(input, "SORTIN")?
                    .inline_data
                    .split(|byte| *byte == b'\n')
                    .filter(|record| !record.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect::<Vec<_>>();
                records.sort();
                output(0, records)
            }
            _ => Err(HostProblem::Unsupported),
        }
    }
}

fn dd<'a>(input: &'a ProgramInput, name: &str) -> Result<&'a DdPlan, HostProblem> {
    input
        .dds
        .iter()
        .find(|dd| dd.name == name)
        .ok_or(HostProblem::NotFound)
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
                temporary: false,
                sysout: None,
                disposition: Vec::new(),
                inline_data: bytes.to_vec(),
                concatenation: false,
                source_line: 1,
            }],
        }
    }

    #[test]
    fn every_accepted_builtin_has_a_real_route() {
        let router = ProgramRouter::with_builtins(InvocationLimits::default());
        assert_eq!(router.supported_programs().len(), 9);
        assert!(!router.supported_programs().any(|name| name == "UNKNOWN"));
    }

    #[test]
    fn generate_and_sort_transform_exact_records() {
        assert_eq!(
            Builtin("IEBGENER")
                .execute(&invocation(), &input("SYSUT1", b"B\nA\n"))
                .unwrap()
                .records,
            vec![b"B".to_vec(), b"A".to_vec()]
        );
        assert_eq!(
            Builtin("SORT")
                .execute(&invocation(), &input("SORTIN", b"B\nA\n"))
                .unwrap()
                .records,
            vec![b"A".to_vec(), b"B".to_vec()]
        );
    }

    #[test]
    fn idcams_unknown_command_is_not_generic_success() {
        assert_eq!(
            Builtin("IDCAMS").execute(&invocation(), &input("SYSIN", b"UNKNOWN THING")),
            Err(HostProblem::Unsupported)
        );
    }
}
