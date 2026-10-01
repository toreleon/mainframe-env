//! Durable CICS SIGNAL EVENT capture-point authority.

use super::super::{CicsService, Run};
use super::store_error;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "cics-signal-event-v1";
const KEY: &str = "region";
const MAX_SPECS: usize = 64;
const MAX_CHANNELS: usize = 256;
const MAX_EMISSIONS: usize = 256;
const MAX_REPLAYS: usize = 512;
const MAX_PAYLOAD: usize = 65_536;
const MAX_STATE_BYTES: usize = 1_048_576;
const MAX_CAS_ATTEMPTS: usize = 8;

/// One enabled business-event capture rule at a SIGNAL EVENT point.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsSignalCaptureSpec {
    /// Stable registration identity.
    pub capture_id: String,
    /// Business event identity carried in captured records.
    pub business_event: String,
    /// SIGNAL EVENT primary predicate, from the 1–32 byte EVENT identifier.
    pub event: String,
    /// Whether this individual capture specification can match.
    pub enabled: bool,
    /// Optional byte-prefix predicate on FROM data.
    pub from_prefix: Option<Vec<u8>>,
    /// Optional exact FROMCHANNEL predicate.
    pub from_channel: Option<String>,
    /// Exact byte predicates on named containers in FROMCHANNEL.
    pub container_equals: BTreeMap<String, Vec<u8>>,
}

/// One durable event emitted by a matching enabled capture specification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsSignalEmission {
    /// Stable sequence within this region's capture journal.
    pub sequence: u64,
    /// Matching capture specification identity.
    pub capture_id: String,
    /// Business event identity configured by that specification.
    pub business_event: String,
    /// SIGNAL EVENT primary identifier.
    pub event: String,
    /// Exact bounded FROM bytes, when supplied.
    pub from: Option<Vec<u8>>,
    /// FROMCHANNEL identity, when supplied.
    pub channel: Option<String>,
    /// Snapshot of the named channel's containers at the capture point.
    pub containers: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignalState {
    active: bool,
    #[serde(default)]
    channels: BTreeSet<String>,
    specs: BTreeMap<String, CicsSignalCaptureSpec>,
    emissions: Vec<CicsSignalEmission>,
    replays: BTreeMap<String, [u8; 32]>,
    next_sequence: u64,
    #[serde(skip)]
    version: u64,
}

impl CicsService {
    /// Register one existing, possibly empty channel for SIGNAL EVENT lookup.
    ///
    /// The CICS channel adapter owns this trusted ingress. Channels holding
    /// transform containers are also recognized from their durable contents.
    pub fn register_signal_channel(&self, channel: &str) -> Result<(), HostProblem> {
        let name = channel_name(channel)?;
        change(self, |state| {
            if state.channels.len() == MAX_CHANNELS && !state.channels.contains(&name) {
                return Err(HostProblem::ResourceExhausted);
            }
            state.channels.insert(name.clone());
            Ok(())
        })
    }

    /// Enable or disable matching of installed SIGNAL EVENT capture rules.
    ///
    /// The region event adapter owns this trusted configuration ingress.
    pub fn set_signal_event_processing(&self, active: bool) -> Result<(), HostProblem> {
        change(self, |state| {
            state.active = active;
            Ok(())
        })
    }

    /// Install one bounded capture specification for the region.
    ///
    /// Re-registering the same identity and definition is idempotent. The
    /// region event adapter owns this trusted configuration ingress.
    pub fn register_signal_capture(&self, spec: CicsSignalCaptureSpec) -> Result<(), HostProblem> {
        validate_spec(&spec)?;
        change(self, |state| {
            if let Some(existing) = state.specs.get(&spec.capture_id) {
                return if existing == &spec {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            if state.specs.len() == MAX_SPECS {
                return Err(HostProblem::ResourceExhausted);
            }
            state.specs.insert(spec.capture_id.clone(), spec.clone());
            Ok(())
        })
    }

    /// Enable or disable one installed capture specification by identity.
    pub fn set_signal_capture_enabled(
        &self,
        capture_id: &str,
        enabled: bool,
    ) -> Result<(), HostProblem> {
        if capture_id.is_empty() || capture_id.len() > 64 {
            return Err(HostProblem::Malformed);
        }
        change(self, |state| {
            let spec = state
                .specs
                .get_mut(capture_id)
                .ok_or(HostProblem::NotFound)?;
            spec.enabled = enabled;
            Ok(())
        })
    }

    /// Read the durable region event journal in sequence order.
    pub fn captured_signal_events(&self) -> Result<Vec<CicsSignalEmission>, HostProblem> {
        Ok(load(self)?.emissions)
    }
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let event = text(request, "EVENT", eventerr)?;
    let event = signal_name(&event).map_err(|_| eventerr())?;
    let from = request
        .arguments
        .get("FROM")
        .map(|value| value.bytes().to_vec());
    let from_length = request
        .arguments
        .get("FROMLENGTH")
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .ok()
                .and_then(|text| text.parse::<i64>().ok())
                .ok_or_else(lengerr)
        })
        .transpose()?;
    if from_length.is_some_and(|length| length <= 0) {
        return Err(lengerr());
    }
    let from = if let Some(bytes) = from {
        let length = from_length
            .map(|value| usize::try_from(value).map_err(|_| HostProblem::ResourceExhausted))
            .transpose()?
            .unwrap_or(bytes.len());
        if length > bytes.len() || length > MAX_PAYLOAD {
            return Err(HostProblem::Malformed);
        }
        Some(bytes[..length].to_vec())
    } else {
        None
    };
    let channel = request
        .arguments
        .get("FROMCHANNEL")
        .map(|_| text(request, "FROMCHANNEL", || HostProblem::Malformed))
        .transpose()?
        .map(|name| channel_name(&name))
        .transpose()?;

    service.authorize(
        run,
        "EVENT",
        &format!(
            "CICS.SIGNAL.{}",
            event
                .bytes()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join("")
        ),
        AccessIntent::Update,
    )?;
    let containers = channel
        .as_deref()
        .map(|name| channel_snapshot(service, name))
        .transpose()?
        .unwrap_or_default();
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for _ in 0..MAX_CAS_ATTEMPTS {
        let mut state = load(service)?;
        if let Some(prior) = state.replays.get(mutation.idempotency_key.as_str()) {
            return if *prior == digest {
                Ok(response)
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if state.replays.len() == MAX_REPLAYS {
            return Err(HostProblem::ResourceExhausted);
        }
        if state.active {
            let matches = state
                .specs
                .values()
                .filter(|spec| spec.enabled && spec.event == event)
                .filter(|spec| match (&spec.from_prefix, &from) {
                    (Some(prefix), Some(bytes)) => bytes.starts_with(prefix),
                    (Some(_), None) => false,
                    (None, _) => true,
                })
                .filter(|spec| {
                    spec.from_channel
                        .as_deref()
                        .is_none_or(|name| channel.as_deref() == Some(name))
                })
                .filter(|spec| {
                    spec.container_equals
                        .iter()
                        .all(|(name, bytes)| containers.get(name) == Some(bytes))
                })
                .cloned()
                .collect::<Vec<_>>();
            if state.emissions.len() + matches.len() > MAX_EMISSIONS {
                return Err(HostProblem::ResourceExhausted);
            }
            for spec in matches {
                state.next_sequence = state
                    .next_sequence
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                state.emissions.push(CicsSignalEmission {
                    sequence: state.next_sequence,
                    capture_id: spec.capture_id,
                    business_event: spec.business_event,
                    event: event.clone(),
                    from: from.clone(),
                    channel: channel.clone(),
                    containers: containers.clone(),
                });
            }
        }
        state
            .replays
            .insert(mutation.idempotency_key.as_str().into(), digest);
        match persist(service, &mut state) {
            Ok(()) => return Ok(response),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("EVENT")
        || request.arguments.contains_key("FROM") && request.arguments.contains_key("FROMCHANNEL")
        || request.arguments.contains_key("FROMLENGTH") && !request.arguments.contains_key("FROM")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "EVENT" | "FROMCHANNEL" => matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ),
            "FROM" => value.schema() == "mainframe-env.cics.storage-value@1",
            "FROMLENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn text(
    request: &CicsRequest,
    name: &str,
    problem: impl Fn() -> HostProblem,
) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    std::str::from_utf8(value.bytes())
        .map(str::to_string)
        .map_err(|_| problem())
}

fn signal_name(value: &str) -> Result<String, HostProblem> {
    let name = value.trim_end_matches(' ');
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"$@#/%&?!:|\"=,;<>.-_".contains(&byte))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name.into())
}

fn channel_name(value: &str) -> Result<String, HostProblem> {
    let name = value.trim_end_matches(' ');
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name.into())
}

fn channel_snapshot(
    service: &CicsService,
    channel: &str,
) -> Result<BTreeMap<String, Vec<u8>>, HostProblem> {
    let registered = load(service)?.channels.contains(channel);
    let state = service.lock()?;
    let containers = state
        .transform_containers
        .iter()
        .filter(|((name, _), _)| name == channel)
        .map(|((_, name), value)| (name.clone(), value.bytes.clone()))
        .collect::<BTreeMap<_, _>>();
    if containers.is_empty() && channel != "DFHTRANSACTION" && !registered {
        return Err(HostProblem::Condition {
            name: "CHANNELERR".into(),
            response: 122,
            response2: 2,
        });
    }
    Ok(containers)
}

fn validate_spec(spec: &CicsSignalCaptureSpec) -> Result<(), HostProblem> {
    if spec.capture_id.is_empty()
        || spec.capture_id.len() > 64
        || !spec
            .capture_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || spec.business_event.is_empty()
        || spec.business_event.len() > 64
        || signal_name(&spec.event).ok().as_deref() != Some(&spec.event)
        || spec
            .from_prefix
            .as_ref()
            .is_some_and(|bytes| bytes.len() > MAX_PAYLOAD)
        || spec.from_prefix.is_some() && spec.from_channel.is_some()
        || !spec.container_equals.is_empty() && spec.from_channel.is_none()
        || spec
            .from_channel
            .as_deref()
            .is_some_and(|name| channel_name(name).ok().as_deref() != Some(name))
        || spec.container_equals.iter().any(|(name, bytes)| {
            channel_name(name).ok().as_deref() != Some(name) || bytes.len() > MAX_PAYLOAD
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn load(service: &CicsService) -> Result<SignalState, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, KEY)
        .map_err(store_error)?
    else {
        return Ok(SignalState::default());
    };
    if row.version == 0 || row.payload.len() > MAX_STATE_BYTES {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut state: SignalState =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    state.version = row.version;
    validate_state(&state)?;
    Ok(state)
}

fn validate_state(state: &SignalState) -> Result<(), HostProblem> {
    if state.channels.len() > MAX_CHANNELS
        || state
            .channels
            .iter()
            .any(|name| channel_name(name).ok().as_deref() != Some(name))
        || state.specs.len() > MAX_SPECS
        || state.emissions.len() > MAX_EMISSIONS
        || state.replays.len() > MAX_REPLAYS
    {
        return Err(HostProblem::ResourceExhausted);
    }
    for (name, spec) in &state.specs {
        validate_spec(spec).map_err(|_| HostProblem::InfrastructureFailure)?;
        if name != &spec.capture_id {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let mut previous = 0;
    for emission in &state.emissions {
        if emission.sequence <= previous
            || emission.sequence > state.next_sequence
            || signal_name(&emission.event).ok().as_deref() != Some(&emission.event)
            || emission
                .from
                .as_ref()
                .is_some_and(|bytes| bytes.len() > MAX_PAYLOAD)
            || emission
                .containers
                .values()
                .any(|bytes| bytes.len() > MAX_PAYLOAD)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        previous = emission.sequence;
    }
    Ok(())
}

fn persist(service: &CicsService, state: &mut SignalState) -> Result<(), HostProblem> {
    validate_state(state)?;
    let payload = serde_json::to_vec(state).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_STATE_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    let version = state
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: KEY.into(),
                version,
                payload,
            },
            (state.version != 0).then_some(state.version),
        )
        .map_err(|error| match error {
            StoreError::AlreadyExists | StoreError::Conflict => HostProblem::IdempotencyConflict,
            other => store_error(other),
        })?;
    state.version = version;
    Ok(())
}

fn change(
    service: &CicsService,
    transition: impl Fn(&mut SignalState) -> Result<(), HostProblem>,
) -> Result<(), HostProblem> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let mut state = load(service)?;
        transition(&mut state)?;
        match persist(service, &mut state) {
            Ok(()) => return Ok(()),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn eventerr() -> HostProblem {
    HostProblem::Condition {
        name: "EVENTERR".into(),
        response: 111,
        response2: 6,
    }
}

fn lengerr() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 3,
    }
}
