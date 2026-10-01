use super::*;
use mainframe_env_host_api::ProgramLinkSelection;

pub(super) fn dispatch(
    router: &DefaultProgramRouter,
    invocation: &Invocation,
    effect: EffectRequest,
) -> EffectResult {
    if let HostRequest::Program(ProgramRequest::Link {
        program,
        payload,
        selection: Some(selection),
    }) = &effect.request
    {
        return EffectResult {
            sequence: effect.sequence,
            outcome: router
                .cobol
                .execute_selected_link(invocation, &effect, program.as_str(), payload, selection)
                .map(HostResult::Program),
        };
    }
    router.router.invoke(invocation, effect)
}

impl CobolProgram {
    pub(super) fn execute_selected_link(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
        program: &str,
        payload: &BoundedPayload,
        selection: &ProgramLinkSelection,
    ) -> Result<BoundedPayload, HostProblem> {
        let values = if payload.schema() == "mainframe-env.cics.channel@1" {
            Vec::new()
        } else {
            vec![payload.bytes().to_vec()]
        };
        let call = encode_selected_call(&values)?;
        let result =
            self.execute_installed_effect(parent, effect, program, &call, Some(selection))?;
        let mut returned = decode_selected_result(&result)?;
        let mut bytes = if returned.is_empty() {
            payload.bytes().to_vec()
        } else {
            returned.remove(0)
        };
        if payload.schema() == "mainframe-env.cics.brxa@1" {
            if bytes.len() < payload.bytes().len() {
                return Err(HostProblem::ProviderFailure);
            }
            bytes.truncate(payload.bytes().len());
        }
        BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            bytes,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)
    }
}

fn encode_selected_call(values: &[Vec<u8>]) -> Result<BoundedPayload, HostProblem> {
    let mut bytes = u32::try_from(values.len())
        .map_err(|_| HostProblem::ResourceExhausted)?
        .to_be_bytes()
        .to_vec();
    for value in values {
        let name = b"DFHCOMMAREA";
        bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.push(1);
        bytes.extend_from_slice(
            &u64::try_from(value.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(value);
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn decode_selected_result(payload: &BoundedPayload) -> Result<Vec<Vec<u8>>, HostProblem> {
    if payload.schema() != "mainframe-env.cobol.call-result@1" {
        return Err(HostProblem::ProviderFailure);
    }
    let mut bytes = payload.bytes();
    let mut take = |length: usize| {
        let (head, tail) = bytes
            .split_at_checked(length)
            .ok_or(HostProblem::ProviderFailure)?;
        bytes = tail;
        Ok::<_, HostProblem>(head)
    };
    let count = u32::from_be_bytes(
        take(4)?
            .try_into()
            .map_err(|_| HostProblem::ProviderFailure)?,
    );
    if count > 1 {
        return Err(HostProblem::ProviderFailure);
    }
    let mut values = Vec::new();
    for _ in 0..count {
        let length = usize::try_from(u64::from_be_bytes(
            take(8)?
                .try_into()
                .map_err(|_| HostProblem::ProviderFailure)?,
        ))
        .map_err(|_| HostProblem::ProviderFailure)?;
        values.push(take(length)?.to_vec());
    }
    if !bytes.is_empty() {
        return Err(HostProblem::ProviderFailure);
    }
    Ok(values)
}
