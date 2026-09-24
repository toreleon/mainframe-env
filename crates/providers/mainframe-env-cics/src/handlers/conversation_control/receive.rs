//! Mapped APPC/MRO RECEIVE from the one durable peer exchange ledger.

use super::{
    ConversationDataReply, ConversationKind, ConversationLedger, ConversationOwner,
    ConversationProblem, ConversationReplay, ConversationReply, ConversationState,
    ConversationTransmitOutcome, DataCondition, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const PAYLOAD_SCHEMA: &str = "mainframe-env.cics.payload@1";
const DECIMAL_SCHEMA: &str = "mainframe-env.cics.decimal@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let principal = run.invocation.principal.id().as_str().to_owned();
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let saved = load_conversation_replay(service.store.as_ref(), mutation.idempotency_key.as_str())
        .map_err(store_error)?;
    let token = if let Some(saved) = &saved {
        saved
            .matches_request(
                &owner.execution,
                &owner.run_unit,
                &principal,
                owner.lease_epoch,
                mutation.sequence,
                digest,
            )
            .map_err(store_error)?
            .token
            .ok_or(HostProblem::InfrastructureFailure)?
    } else {
        select_token(&initial, &owner, request)?
    };
    let system = initial
        .conversation(token)
        .ok_or_else(|| condition("NOTALLOC", 61))?
        .system
        .clone();
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Read,
    )?;
    let max_length = maximum(request)?;
    let retain_remainder = request.arguments.contains_key("OPTION.NOTRUNCATE");
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), mutation.idempotency_key.as_str())
                .map_err(store_error)?
        {
            let reply = saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    &principal,
                    owner.lease_epoch,
                    mutation.sequence,
                    digest,
                )
                .map_err(store_error)?;
            return response(service, run, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = current
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61))?;
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        if record.kind == ConversationKind::AppcBasic {
            return Err(condition("INVREQ", 16));
        }
        if record.kind == ConversationKind::Mro && request.arguments.contains_key("CONVID") {
            return Err(condition("INVREQ", 16));
        }
        if record.data.terminal_error() {
            return Err(condition("TERMERR", 81));
        }
        if current
            .exchanges
            .get(&u32::from_be_bytes(token).to_string())
            .is_some_and(|exchange| exchange.pending_outbound() != 0)
        {
            match service.flush_conversation_send(run, token)? {
                ConversationTransmitOutcome::Pending => {
                    return service.response(
                        run,
                        CicsDisposition::Suspended,
                        "NORMAL",
                        0,
                        0,
                        None,
                        None,
                        Vec::new(),
                    );
                }
                ConversationTransmitOutcome::Confirmed
                | ConversationTransmitOutcome::Rejected(_) => continue,
            }
        }
        if record.state == ConversationState::PendReceive {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut next = current.clone();
        let Some(received) = next
            .receive_mapped_peer_frame(
                token,
                &owner,
                super::context(run)?,
                max_length,
                retain_remainder,
            )
            .map_err(map_problem)?
        else {
            return service.response(
                run,
                CicsDisposition::Suspended,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            );
        };
        let mut reply = reply(token, &received, retain_remainder);
        capture_disposition(service, run, request, &mut reply)?;
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: mutation.idempotency_key.as_str().into(),
            owner_execution: owner.execution.clone(),
            owner_run_unit: owner.run_unit.clone(),
            owner_principal: principal.clone(),
            owner_epoch: owner.lease_epoch,
            mutation_sequence: mutation.sequence,
            request_digest: digest,
            deadline_tick: retention_tick,
            retain_until_tick: retention_tick,
            reply: reply.clone(),
        };
        if current
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return response(service, run, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::ReceiveConversation
        || request.arguments.contains_key("CONVID") && request.arguments.contains_key("SESSION")
        || request.arguments.contains_key("INTO") == request.arguments.contains_key("SET")
        || request.arguments.contains_key("MAXLENGTH")
            && request.arguments.contains_key("MAXFLENGTH")
        || request.arguments.contains_key("LENGTH") && request.arguments.contains_key("FLENGTH")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "SESSION"
                    | "LENGTH"
                    | "FLENGTH"
                    | "MAXLENGTH"
                    | "MAXFLENGTH"
                    | "INTO.MAXLENGTH"
                    | "SET.MAXLENGTH"
                    | "INTO"
                    | "SET"
                    | "STATE"
                    | "OPTION.NOTRUNCATE"
                    | "OPTION.NOHANDLE"
                    | "RESP"
                    | "RESP2"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(super) fn select_token(
    ledger: &ConversationLedger,
    owner: &ConversationOwner,
    request: &CicsRequest,
) -> Result<[u8; 4], HostProblem> {
    if let Some(value) = request.arguments.get("CONVID") {
        return value
            .bytes()
            .try_into()
            .map_err(|_| condition("NOTALLOC", 61));
    }
    if let Some(value) = request.arguments.get("SESSION") {
        if let Ok(token) = <[u8; 4]>::try_from(value.bytes())
            && ledger.conversation(token).is_some_and(|record| {
                record.kind == ConversationKind::AppcMapped
                    && !record.released
                    && record.owner.execution == owner.execution
                    && record.owner.run_unit == owner.run_unit
            })
        {
            return Ok(token);
        }
        let name = std::str::from_utf8(value.bytes())
            .map_err(|_| condition("NOTALLOC", 61))?
            .trim_end_matches(' ');
        return ledger
            .conversations
            .values()
            .find(|record| {
                record.kind == ConversationKind::Mro
                    && !record.released
                    && record.mro_session_name.as_deref() == Some(name)
                    && record.owner.execution == owner.execution
                    && record.owner.run_unit == owner.run_unit
            })
            .map(|record| record.token)
            .ok_or_else(|| condition("NOTALLOC", 61));
    }
    ledger
        .conversations
        .values()
        .find(|record| {
            record.principal_facility
                && !record.released
                && record.owner.execution == owner.execution
                && record.owner.run_unit == owner.run_unit
        })
        .map(|record| record.token)
        .ok_or_else(|| condition("NOTALLOC", 61))
}

fn maximum(request: &CicsRequest) -> Result<usize, HostProblem> {
    let mut maximum = 32_767usize;
    let data_limit = ["MAXLENGTH", "MAXFLENGTH", "LENGTH", "FLENGTH"]
        .into_iter()
        .find(|name| request.arguments.contains_key(*name));
    for name in data_limit
        .into_iter()
        .chain(["INTO.MAXLENGTH", "SET.MAXLENGTH"])
    {
        if let Some(value) = request.arguments.get(name) {
            if value.schema() == "mainframe-env.cics.argument@1" {
                continue;
            }
            let width = if matches!(name, "LENGTH" | "MAXLENGTH") {
                2
            } else {
                4
            };
            let parsed = parse_signed(value, width)?;
            if !matches!(name, "INTO.MAXLENGTH" | "SET.MAXLENGTH") && parsed > 32_767 {
                return Err(condition("LENGERR", 22));
            }
            maximum = maximum.min(usize::try_from(parsed.max(0)).unwrap_or(0));
        }
    }
    Ok(maximum)
}

pub(super) fn parse_signed(value: &BoundedPayload, width: usize) -> Result<i32, HostProblem> {
    match value.schema() {
        DECIMAL_SCHEMA => std::str::from_utf8(value.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<i32>()
            .map_err(|_| HostProblem::Malformed),
        "mainframe-env.cics.storage-value@1" if value.bytes().len() == width => Ok(if width == 2 {
            i32::from(i16::from_be_bytes(value.bytes().try_into().unwrap()))
        } else {
            i32::from_be_bytes(value.bytes().try_into().unwrap())
        }),
        _ => Err(HostProblem::Malformed),
    }
}

fn reply(token: [u8; 4], received: &ConversationDataReply, retain: bool) -> ConversationReply {
    let (name, code) = match received.condition {
        DataCondition::Normal => ("NORMAL", 0),
        DataCondition::EndOfChain => ("EOC", 6),
        DataCondition::InboundFmh => ("INBFMH", 7),
        DataCondition::LengthError => ("LENGERR", 22),
        DataCondition::Signal => ("SIGNAL", 24),
    };
    let length = if received.condition == DataCondition::LengthError && !retain {
        received.original_length
    } else {
        received.returned_length
    };
    ConversationReply {
        condition: name.into(),
        response: code,
        response2: 0,
        state: Some(received.state),
        token: Some(token),
        outputs: BTreeMap::from([
            ("INTO".into(), received.bytes.clone()),
            ("SET".into(), received.bytes.clone()),
            ("LENGTH".into(), length.to_string().into_bytes()),
            ("FLENGTH".into(), length.to_string().into_bytes()),
            (
                "EIBEOC".into(),
                vec![u8::from(received.end_of_chain) * 0xff],
            ),
            ("EIBFMH".into(), vec![u8::from(received.inbound_fmh) * 0xff]),
            (
                "STATE".into(),
                received.state.cvda().to_string().into_bytes(),
            ),
        ]),
    }
}

pub(super) fn capture_disposition(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    reply: &mut ConversationReply,
) -> Result<(), HostProblem> {
    let delivered = if reply.condition == "NORMAL" {
        None
    } else {
        match super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: reply.condition.clone(),
                response: reply.response,
                response2: reply.response2,
            },
        ) {
            Ok(response) => Some(response),
            Err(HostProblem::Condition { .. }) => {
                reply.outputs.insert("DISPOSITION".into(), b"A".to_vec());
                return Ok(());
            }
            Err(other) => return Err(other),
        }
    };
    let (code, target) = match delivered {
        None => (b"C".to_vec(), None),
        Some(response) => {
            let code = match response.disposition {
                CicsDisposition::Complete => b"C".to_vec(),
                CicsDisposition::Ignored => b"I".to_vec(),
                CicsDisposition::Handler => b"H".to_vec(),
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            (code, response.target)
        }
    };
    reply.outputs.insert("DISPOSITION".into(), code);
    if let Some(target) = target {
        reply.outputs.insert("TARGET".into(), target.into_bytes());
    }
    Ok(())
}

pub(super) fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    let disposition = match reply.outputs.get("DISPOSITION").map(Vec::as_slice) {
        Some(b"C") => CicsDisposition::Complete,
        Some(b"I") => CicsDisposition::Ignored,
        Some(b"H") => CicsDisposition::Handler,
        Some(b"A") => CicsDisposition::Abended,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let target = reply
        .outputs
        .get("TARGET")
        .map(|bytes| {
            String::from_utf8(bytes.clone()).map_err(|_| HostProblem::InfrastructureFailure)
        })
        .transpose()?;
    if (disposition == CicsDisposition::Handler) != target.is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut response = service.response(
        run,
        disposition,
        &reply.condition,
        reply.response,
        reply.response2,
        target,
        None,
        Vec::new(),
    )?;
    for (name, bytes) in &reply.outputs {
        if matches!(name.as_str(), "DISPOSITION" | "TARGET") {
            continue;
        }
        let schema = if matches!(name.as_str(), "LENGTH" | "FLENGTH" | "STATE") {
            DECIMAL_SCHEMA
        } else {
            PAYLOAD_SCHEMA
        };
        response.outputs.insert(
            name.clone(),
            BoundedPayload::new(schema, bytes.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}

fn map_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned | ConversationProblem::Malformed => condition("NOTALLOC", 61),
        ConversationProblem::DplPrincipal => HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 200,
        },
        ConversationProblem::WrongKind | ConversationProblem::WrongState => condition("INVREQ", 16),
        ConversationProblem::Length => condition("LENGERR", 22),
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
    }
}

fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}

impl CicsService {
    /// Trusted terminal ingress binds the symbolic MRO TCTTE name to the
    /// existing allocated token; RECEIVE never invents a session alias.
    pub fn bind_conversation_mro_session(
        &self,
        run_unit: &mainframe_env_execution_api::RunUnitId,
        token: [u8; 4],
        name: &str,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let run = state.runs.get_mut(run_unit).ok_or(HostProblem::NotFound)?;
        super::deadline(self, run)?;
        let owner = ConversationOwner {
            execution: run.invocation.execution_id.as_str().into(),
            run_unit: run.invocation.run_unit_id.as_str().into(),
            lease_epoch: u64::from(run.invocation.attempt),
        };
        let context = super::context(run)?;
        let initial = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
        let system = initial
            .conversation(token)
            .ok_or(HostProblem::NotFound)?
            .system
            .clone();
        self.authorize(
            run,
            "CONNECTION",
            &format!("CICS.CONNECTION.{system}"),
            AccessIntent::Execute,
        )?;
        for _ in 0..MAX_CAS_RETRIES {
            super::deadline(self, run)?;
            let current = ConversationLedger::load(self.store.as_ref()).map_err(store_error)?;
            let mut next = current.clone();
            next.bind_mro_session(token, &owner, context, name)
                .map_err(map_problem)?;
            if next == current {
                return Ok(());
            }
            if current
                .persist(&mut next, self.store.as_ref())
                .map_err(|error| mutation_problem(store_error(error)))?
            {
                if super::deadline(self, run).is_err() {
                    return Err(HostProblem::UnknownOutcome);
                }
                return Ok(());
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}
