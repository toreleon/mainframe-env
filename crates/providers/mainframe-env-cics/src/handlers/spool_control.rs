//! Shared bounded durable state for CICS JES spool-control commands.

mod ingress;

use super::{field, store_error};
use crate::service::{CicsLimits, CicsService};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::BTreeMap;

pub(in crate::service) fn load_spool_state(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<SpoolState, HostProblem> {
    match store
        .get_provider_state(SPOOL_NAMESPACE, SPOOL_KEY)
        .map_err(store_error)?
    {
        Some(row) => decode_spool_state(&row.payload, row.version, limits),
        None => Ok(SpoolState::default()),
    }
}

pub(crate) const SPOOL_NAMESPACE: &str = "cics-spool-control";
pub(crate) const SPOOL_KEY: &str = "state";
const SPOOL_MAGIC: &[u8; 8] = b"MECSP001";
const MAX_TEXT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpoolReportState {
    AvailableInput,
    OpenInput,
    OpenOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpoolRecordMode {
    Line,
    Page,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpoolRecord {
    pub(crate) mode: SpoolRecordMode,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpoolReport {
    pub(crate) token: String,
    pub(crate) state: SpoolReportState,
    pub(crate) user_id: String,
    pub(crate) node: String,
    pub(crate) class: u8,
    pub(crate) record_length: u32,
    pub(crate) owner_run_unit: Option<String>,
    pub(crate) owner_principal: Option<String>,
    pub(crate) records: Vec<SpoolRecord>,
    pub(crate) next_record: usize,
    pub(crate) eof_seen: bool,
    pub(crate) carriage_control: u8,
    pub(crate) punch: bool,
    pub(crate) out_descriptor: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpoolReply {
    pub(crate) condition: String,
    pub(crate) response: i32,
    pub(crate) response2: i32,
    pub(crate) payload: Vec<u8>,
    pub(crate) token: Option<Vec<u8>>,
    pub(crate) toflength: Option<i64>,
}

impl SpoolReply {
    #[allow(dead_code)] // Activated by the first command slice.
    pub(crate) fn normal() -> Self {
        Self {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            payload: Vec::new(),
            token: None,
            toflength: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpoolReplay {
    pub(crate) request_digest: [u8; 32],
    pub(crate) reply: SpoolReply,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpoolState {
    pub(crate) version: u64,
    pub(crate) next_token: u64,
    pub(crate) reports: BTreeMap<String, SpoolReport>,
    pub(crate) replays: BTreeMap<String, SpoolReplay>,
}

impl Default for SpoolState {
    fn default() -> Self {
        Self {
            version: 0,
            next_token: 1,
            reports: BTreeMap::new(),
            replays: BTreeMap::new(),
        }
    }
}

impl SpoolState {
    pub(crate) fn allocate_token(&mut self) -> Result<String, HostProblem> {
        if self.next_token > 999_999 {
            return Err(HostProblem::ResourceExhausted);
        }
        let token = format!("SP{:06}", self.next_token);
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        if self.reports.contains_key(&token) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(token)
    }

    #[allow(dead_code)] // Activated by the first command slice.
    pub(crate) fn replay(
        &self,
        key: &str,
        request_digest: [u8; 32],
    ) -> Result<Option<SpoolReply>, HostProblem> {
        let Some(replay) = self.replays.get(key) else {
            return Ok(None);
        };
        if replay.request_digest != request_digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(Some(replay.reply.clone()))
    }

    #[allow(dead_code)] // Activated by the first command slice.
    pub(crate) fn record_replay(
        &mut self,
        key: &str,
        request_digest: [u8; 32],
        reply: SpoolReply,
        limits: CicsLimits,
    ) -> Result<(), HostProblem> {
        if let Some(existing) = self.replays.get(key) {
            if existing.request_digest == request_digest && existing.reply == reply {
                return Ok(());
            }
            return Err(HostProblem::IdempotencyConflict);
        }
        if self.replays.len() >= limits.max_spool_replays {
            return Err(HostProblem::ResourceExhausted);
        }
        self.replays.insert(
            key.into(),
            SpoolReplay {
                request_digest,
                reply,
            },
        );
        Ok(())
    }
}

pub(crate) fn persist_spool_state(
    service: &CicsService,
    current_version: u64,
    next: &mut SpoolState,
) -> Result<(), HostProblem> {
    next.version = current_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let payload = encode_spool_state(next, service.limits)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: SPOOL_NAMESPACE.into(),
                key: SPOOL_KEY.into(),
                version: next.version,
                payload,
            },
            (current_version != 0).then_some(current_version),
        )
        .map_err(store_error)
}

pub(crate) fn encode_spool_state(
    state: &SpoolState,
    limits: CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    validate_spool_state(state, limits)?;
    let mut out = SPOOL_MAGIC.to_vec();
    out.extend_from_slice(&state.next_token.to_be_bytes());
    out.extend_from_slice(&count(state.reports.len())?);
    for (token, report) in &state.reports {
        field(&mut out, token.as_bytes())?;
        out.push(match report.state {
            SpoolReportState::AvailableInput => 0,
            SpoolReportState::OpenInput => 1,
            SpoolReportState::OpenOutput => 2,
        });
        field(&mut out, report.user_id.as_bytes())?;
        field(&mut out, report.node.as_bytes())?;
        out.push(report.class);
        out.extend_from_slice(&report.record_length.to_be_bytes());
        optional_text(&mut out, report.owner_run_unit.as_deref())?;
        optional_text(&mut out, report.owner_principal.as_deref())?;
        out.extend_from_slice(&count(report.next_record)?);
        out.push(u8::from(report.eof_seen));
        out.push(report.carriage_control);
        out.push(u8::from(report.punch));
        field(&mut out, &report.out_descriptor)?;
        out.extend_from_slice(&count(report.records.len())?);
        for record in &report.records {
            out.push(match record.mode {
                SpoolRecordMode::Line => 0,
                SpoolRecordMode::Page => 1,
            });
            field(&mut out, &record.bytes)?;
        }
    }
    out.extend_from_slice(&count(state.replays.len())?);
    for (key, replay) in &state.replays {
        field(&mut out, key.as_bytes())?;
        out.extend_from_slice(&replay.request_digest);
        encode_reply(&mut out, &replay.reply)?;
    }
    if out.len() > limits.max_spool_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(out)
}

pub(crate) fn decode_spool_state(
    payload: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<SpoolState, HostProblem> {
    if payload.len() > limits.max_spool_bytes || payload.get(..8) != Some(SPOOL_MAGIC) {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut reader = Reader {
        bytes: payload,
        at: 8,
    };
    let next_token = reader.u64()?;
    let report_count = reader.count(limits.max_spool_reports)?;
    let mut reports = BTreeMap::new();
    for _ in 0..report_count {
        let token = reader.text(8)?;
        let state = match reader.byte()? {
            0 => SpoolReportState::AvailableInput,
            1 => SpoolReportState::OpenInput,
            2 => SpoolReportState::OpenOutput,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let user_id = reader.text(8)?;
        let node = reader.text(8)?;
        let class = reader.byte()?;
        let record_length = reader.u32()?;
        let owner_run_unit = reader.optional_text(MAX_TEXT_BYTES)?;
        let owner_principal = reader.optional_text(MAX_TEXT_BYTES)?;
        let next_record = reader.count(limits.max_spool_records)?;
        let eof_seen = reader.boolean()?;
        let carriage_control = reader.byte()?;
        let punch = reader.boolean()?;
        let out_descriptor = reader.field(limits.max_spool_outdescr_bytes)?;
        let record_count = reader.count(limits.max_spool_records)?;
        let mut records = Vec::with_capacity(record_count);
        for _ in 0..record_count {
            let mode = match reader.byte()? {
                0 => SpoolRecordMode::Line,
                1 => SpoolRecordMode::Page,
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            records.push(SpoolRecord {
                mode,
                bytes: reader.field(32_760)?,
            });
        }
        let report = SpoolReport {
            token: token.clone(),
            state,
            user_id,
            node,
            class,
            record_length,
            owner_run_unit,
            owner_principal,
            records,
            next_record,
            eof_seen,
            carriage_control,
            punch,
            out_descriptor,
        };
        if reports.insert(token, report).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let replay_count = reader.count(limits.max_spool_replays)?;
    let mut replays = BTreeMap::new();
    for _ in 0..replay_count {
        let key = reader.text(MAX_TEXT_BYTES)?;
        let request_digest = reader.array::<32>()?;
        let reply = decode_reply(&mut reader, limits)?;
        if replays
            .insert(
                key,
                SpoolReplay {
                    request_digest,
                    reply,
                },
            )
            .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if reader.at != payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let state = SpoolState {
        version,
        next_token,
        reports,
        replays,
    };
    validate_spool_state(&state, limits)?;
    if encode_spool_state(&state, limits)? != payload {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(state)
}

fn validate_spool_state(state: &SpoolState, limits: CicsLimits) -> Result<(), HostProblem> {
    if state.next_token == 0
        || state.next_token > 1_000_000
        || state.reports.len() > limits.max_spool_reports
        || state.replays.len() > limits.max_spool_replays
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut record_count = 0usize;
    let mut input_owner = None;
    for (token, report) in &state.reports {
        record_count = record_count
            .checked_add(report.records.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if token != &report.token
            || token.len() != 8
            || !token.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || report.user_id.is_empty()
            || report.user_id.len() > 8
            || report.node.is_empty()
            || report.node.len() > 8
            || !report.class.is_ascii_alphanumeric()
            || report.record_length > 32_760
            || report.next_record > report.records.len()
            || report.out_descriptor.len() > limits.max_spool_outdescr_bytes
            || report
                .records
                .iter()
                .any(|record| record.bytes.len() > usize::try_from(report.record_length).unwrap())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let open = matches!(
            report.state,
            SpoolReportState::OpenInput | SpoolReportState::OpenOutput
        );
        if open != report.owner_run_unit.is_some() || open != report.owner_principal.is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
        if report.state == SpoolReportState::OpenInput
            && input_owner.replace(token.as_str()).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if record_count > limits.max_spool_records {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}

fn encode_reply(out: &mut Vec<u8>, reply: &SpoolReply) -> Result<(), HostProblem> {
    field(out, reply.condition.as_bytes())?;
    out.extend_from_slice(&reply.response.to_be_bytes());
    out.extend_from_slice(&reply.response2.to_be_bytes());
    field(out, &reply.payload)?;
    optional_bytes(out, reply.token.as_deref())?;
    match reply.toflength {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        None => out.push(0),
    }
    Ok(())
}

fn decode_reply(reader: &mut Reader<'_>, limits: CicsLimits) -> Result<SpoolReply, HostProblem> {
    let condition = reader.text(32)?;
    let response = reader.i32()?;
    let response2 = reader.i32()?;
    let payload = reader.field(32_760)?;
    let token = reader.optional_bytes(8)?;
    let toflength = match reader.byte()? {
        0 => None,
        1 => Some(reader.i64()?),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if token.as_ref().is_some_and(|token| token.len() != 8)
        || payload.len() > limits.max_spool_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(SpoolReply {
        condition,
        response,
        response2,
        payload,
        token,
        toflength,
    })
}

fn count(value: usize) -> Result<[u8; 4], HostProblem> {
    u32::try_from(value)
        .map(u32::to_be_bytes)
        .map_err(|_| HostProblem::ResourceExhausted)
}

fn optional_text(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), HostProblem> {
    optional_bytes(out, value.map(str::as_bytes))
}

fn optional_bytes(out: &mut Vec<u8>, value: Option<&[u8]>) -> Result<(), HostProblem> {
    match value {
        Some(value) => {
            out.push(1);
            field(out, value)?;
        }
        None => out.push(0),
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Result<[u8; N], HostProblem> {
        let end = self
            .at
            .checked_add(N)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self.bytes[self.at..end]
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.array::<1>()?[0])
    }

    fn boolean(&mut self) -> Result<bool, HostProblem> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn u32(&mut self) -> Result<u32, HostProblem> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, HostProblem> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, HostProblem> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    fn i64(&mut self) -> Result<i64, HostProblem> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    fn count(&mut self, maximum: usize) -> Result<usize, HostProblem> {
        usize::try_from(self.u32()?)
            .ok()
            .filter(|value| *value <= maximum)
            .ok_or(HostProblem::InfrastructureFailure)
    }

    fn field(&mut self, maximum: usize) -> Result<Vec<u8>, HostProblem> {
        let length = self.count(maximum)?;
        let end = self
            .at
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self.bytes[self.at..end].to_vec();
        self.at = end;
        Ok(value)
    }

    fn text(&mut self, maximum: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.field(maximum)?).map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn optional_bytes(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, HostProblem> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.field(maximum).map(Some),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn optional_text(&mut self, maximum: usize) -> Result<Option<String>, HostProblem> {
        self.optional_bytes(maximum)?
            .map(|value| String::from_utf8(value).map_err(|_| HostProblem::InfrastructureFailure))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(token: &str) -> SpoolReport {
        SpoolReport {
            token: token.into(),
            state: SpoolReportState::AvailableInput,
            user_id: "MEAPUSER".into(),
            node: "LOCAL".into(),
            class: b'A',
            record_length: 32_760,
            owner_run_unit: None,
            owner_principal: None,
            records: vec![SpoolRecord {
                mode: SpoolRecordMode::Line,
                bytes: b"RECORD".to_vec(),
            }],
            next_record: 0,
            eof_seen: false,
            carriage_control: 0,
            punch: false,
            out_descriptor: Vec::new(),
        }
    }

    #[test]
    fn durable_spool_codec_is_canonical_and_bounded() {
        let limits = CicsLimits::default();
        let mut state = SpoolState::default();
        let token = state.allocate_token().unwrap();
        state.reports.insert(token.clone(), report(&token));
        state
            .record_replay(
                "effect-1",
                [7; 32],
                SpoolReply {
                    token: Some(token.as_bytes().to_vec()),
                    ..SpoolReply::normal()
                },
                limits,
            )
            .unwrap();
        let encoded = encode_spool_state(&state, limits).unwrap();
        assert_eq!(decode_spool_state(&encoded, 4, limits).unwrap().version, 4);
        let mut corrupt = encoded;
        corrupt.push(0);
        assert_eq!(
            decode_spool_state(&corrupt, 4, limits),
            Err(HostProblem::InfrastructureFailure)
        );
    }
}
