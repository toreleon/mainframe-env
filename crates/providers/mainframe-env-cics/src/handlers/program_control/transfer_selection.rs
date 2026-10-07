//! Immutable program selection carried by the existing Transfer replay result.
use super::*;
use crate::service::{decode_cics_effect_replay, validate_cics_effect_replay_identity};
use mainframe_env_execution_api::{Invocation, Transfer};
use mainframe_env_host_api::canonical_result_digest;
use mainframe_env_store_api::{EffectDigestFormat, EffectRecord, EffectState};

const OUTPUT: &str = "PROGRAM.SELECTION";
const SCHEMA: &str = "mainframe-env.cics.program-selection@1";

pub(in crate::service) fn freeze(
    service: &CicsService,
    response: &mut CicsResponse,
) -> Result<(), HostProblem> {
    if response.disposition != CicsDisposition::Transfer {
        return Ok(());
    }
    let target = response
        .target
        .as_ref()
        .ok_or(HostProblem::UnknownOutcome)?;
    let definition = service
        .lock()?
        .program_definitions
        .get(target)
        .and_then(|generations| generations.last_key_value())
        .map(|(_, definition)| definition.clone());
    let Some(definition) = definition else {
        return Ok(());
    };
    if response.outputs.len() >= service.limits.max_fields {
        return Err(HostProblem::UnknownOutcome);
    }
    response.outputs.insert(
        OUTPUT.into(),
        BoundedPayload::new(
            SCHEMA,
            encode_program_definition(&definition).map_err(|_| HostProblem::UnknownOutcome)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_selection_output_is_optional_for_legacy_but_strict_when_present() {
        let definition = CicsProgramDefinition {
            name: "EXIT".into(),
            generation: 1,
            artifact: ArtifactRef::new(
                format!("sha256:{}", "1".repeat(64)),
                InvocationLimits::default(),
            )
            .unwrap(),
            semantic_identity: format!("semantic-sha256:{}", "2".repeat(64)),
            entry_offset: 0,
            enabled: true,
            remote: false,
            reload: false,
            java_status: CicsJavaStatus::NotJava,
        };
        let mut response = CicsResponse {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            applid: "TEST".into(),
            sysid: "TEST".into(),
            transaction: "TEST".into(),
            aid: 0,
            disposition: CicsDisposition::Transfer,
            target: Some("EXIT".into()),
            next_transaction: None,
            payload: BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .unwrap(),
            outputs: BTreeMap::new(),
            unit_of_work: None,
        };
        assert!(validate_response(&response).is_ok());
        let encoded = encode_program_definition(&definition).unwrap();
        response.outputs.insert(
            OUTPUT.into(),
            BoundedPayload::new(SCHEMA, encoded.clone(), InvocationLimits::default()).unwrap(),
        );
        assert!(validate_response(&response).is_ok());
        for case in 0..5 {
            let mut corrupt = response.clone();
            match case {
                0 => corrupt.target = Some("OTHER".into()),
                1 => corrupt.disposition = CicsDisposition::Complete,
                2 => {
                    corrupt.outputs.insert(
                        OUTPUT.into(),
                        BoundedPayload::new(
                            "wrong@1",
                            encoded.clone(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    );
                }
                3 => {
                    let mut trailing = encoded.clone();
                    trailing.push(0);
                    corrupt.outputs.insert(
                        OUTPUT.into(),
                        BoundedPayload::new(SCHEMA, trailing, InvocationLimits::default()).unwrap(),
                    );
                }
                _ => {
                    corrupt.outputs.insert(
                        OUTPUT.into(),
                        BoundedPayload::new(SCHEMA, vec![0; 1025], InvocationLimits::default())
                            .unwrap(),
                    );
                }
            }
            assert_eq!(
                validate_response(&corrupt),
                Err(HostProblem::UnknownOutcome),
                "case {case}"
            );
        }
    }
}

fn definition(response: &CicsResponse) -> Result<CicsProgramDefinition, HostProblem> {
    let payload = response
        .outputs
        .get(OUTPUT)
        .ok_or(HostProblem::UnknownOutcome)?;
    if response.disposition != CicsDisposition::Transfer
        || payload.schema() != SCHEMA
        || payload.bytes().len() > 1024
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let definition =
        decode_program_definition(payload.bytes()).map_err(|_| HostProblem::UnknownOutcome)?;
    if response.target.as_deref() != Some(definition.name.as_str())
        || encode_program_definition(&definition).map_err(|_| HostProblem::UnknownOutcome)?
            != payload.bytes()
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(definition)
}

pub(crate) fn validate_response(response: &CicsResponse) -> Result<(), HostProblem> {
    if response.outputs.contains_key(OUTPUT) {
        definition(response)?;
    }
    Ok(())
}

impl CicsService {
    /// Read the immutable local selection retained in a source Transfer result.
    /// The embedding must read `effect` from its trusted core store and attest
    /// the source invocation separately. This does not authorize execution or
    /// advance a frame; name-only and legacy results remain unknown.
    pub fn attested_program_transfer_selection(
        &self,
        source: &Invocation,
        effect: &EffectRecord,
        observed: &Transfer,
    ) -> Result<ProgramLinkSelection, HostProblem> {
        if !observed.replace_frame
            || effect.state != EffectState::Completed
            || effect.digest_format != EffectDigestFormat::CanonicalHostV1
            || effect.execution_id != source.execution_id
            || effect.run_unit_id != source.run_unit_id
            || effect.intent.owner != source.execution_id
            || effect.intent.attempt != source.attempt
            || effect.intent.capability.as_ref().map(|id| id.as_str()) != Some("host.cics.execute")
            || effect.key.as_str() != format!("{}:{}", source.idempotency_key, effect.sequence)
            || effect.resolved_tick.is_none_or(|tick| tick == 0)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let row = self
            .store
            .get_provider_state("cics-effect-replay-v1", effect.key.as_str())
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let replay = decode_cics_effect_replay(&row.payload, self.limits)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        validate_cics_effect_replay_identity(
            &replay,
            effect.key.as_str(),
            source.execution_id.as_str(),
            source.run_unit_id.as_str(),
            effect.sequence,
            effect.request_digest,
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        if row.version != 2
            || replay.effect_key.is_none()
            || replay.deadline_tick != Some(source.deadline_tick)
            || replay.response.target.as_deref() != Some(observed.selector.as_str())
            || replay.response.payload != observed.payload
            || Some(
                canonical_result_digest(&Ok(HostResult::Cics(replay.response.clone())))
                    .map_err(|_| HostProblem::UnknownOutcome)?,
            ) != effect.result_digest
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let selected = definition(&replay.response)?;
        let selection = ProgramLinkSelection {
            artifact: selected.artifact.clone(),
            generation: selected.generation,
            content_identity: format!(
                "sha256:{:x}",
                Sha256::digest(
                    encode_program_definition(&selected)
                        .map_err(|_| HostProblem::UnknownOutcome)?
                )
            ),
        };
        validate_frozen_selection(self, &selected.name, &selection)?;
        Ok(selection)
    }
}

/// Recheck one frozen definition through the existing immutable program owner.
pub(in crate::service) fn validate_frozen_selection(
    service: &CicsService,
    name: &str,
    selection: &ProgramLinkSelection,
) -> Result<(), HostProblem> {
    let definition = service
        .lock()?
        .program_definitions
        .get(name)
        .and_then(|generations| generations.get(&selection.generation))
        .cloned()
        .ok_or(HostProblem::UnknownOutcome)?;
    let payload =
        encode_program_definition(&definition).map_err(|_| HostProblem::UnknownOutcome)?;
    let retained = service
        .store
        .get_provider_state(
            PROGRAM_DEFINITION_NAMESPACE,
            &program_definition_key(name, selection.generation),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?
        .ok_or(HostProblem::UnknownOutcome)?;
    if !definition.enabled
        || definition.remote
        || definition.entry_offset != 0
        || definition.java_status != CicsJavaStatus::NotJava
        || definition.artifact != selection.artifact
        || selection.content_identity != format!("sha256:{:x}", Sha256::digest(&payload))
        || retained.version != 1
        || retained.payload != payload
    {
        return Err(HostProblem::UnknownOutcome);
    }
    validate_program_artifact(
        service
            .artifacts
            .get()
            .ok_or(HostProblem::UnknownOutcome)?
            .as_ref(),
        &definition,
    )
    .map_err(|_| HostProblem::UnknownOutcome)
}
