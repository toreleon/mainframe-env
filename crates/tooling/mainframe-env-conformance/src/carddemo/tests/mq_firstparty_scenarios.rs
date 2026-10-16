//! Independent source-derived logical messages, before CICS/MQI integration.
//!
//! This module performs no I/O, invokes no provider, constructs no Invocation or
//! MQTM/IBM control block, and returns no RouteObservations/application receipt.
//! A projection match is insufficient for any J15/J16 observation. A later real
//! owner must still authenticate the complete actor/effect/result/audit/UOW and
//! lifecycle proof, including the existing CICS ownership fence.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const FIXTURE: &str = include_str!("mq_firstparty_scenarios/fixture.json");
const FIXTURE_SHA: &str = "c47edb82bdfe95caaceabbec4a02195016e09015cb9a551f2d0fb7c3643ef3d3";
const CORPUS: &str = "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e";
const MAX_FIXTURE_BYTES: usize = 65_536;
const MAX_SOURCE_BYTES: usize = 262_144;
const MAX_MESSAGE_BYTES: usize = 1_000;
const CASE_IDS: [&str; 5] = [
    "j15-date",
    "j15-account",
    "j16-approved",
    "j16-insufficient",
    "j16-missing-xref",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Case {
    Date,
    Account,
    Approved,
    InsufficientCredit,
    MissingCrossReference,
}
impl Case {
    fn id(self) -> &'static str {
        CASE_IDS[self as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    Fixture,
    Bounds,
    Source,
    Request,
    Reply,
    Identity,
    Seed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePin {
    path: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedSpec {
    dataset: String,
    key_hex: String,
    record_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseSpec {
    id: String,
    program: String,
    transaction: String,
    input_queue: String,
    reply_queue: String,
    request_hex: String,
    expected_reply_hex: String,
    message_id_hex: String,
    correlation_id_hex: String,
    seed_images: Vec<SeedSpec>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fixtures {
    schema: String,
    corpus_commit: String,
    clock: String,
    source_pins: Vec<SourcePin>,
    cases: Vec<CaseSpec>,
}

/// Full independently authored seed image; not an actual store observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SeedImage {
    pub dataset: String,
    pub key: Vec<u8>,
    pub record: Vec<u8>,
}

/// Logical input for a future genuine trigger producer. It is not a MQTM frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RequestOffer {
    pub program: String,
    pub transaction: String,
    pub input_queue: String,
    pub reply_queue: String,
    pub body: Vec<u8>,
    pub message_id: [u8; 24],
    pub correlation_id: [u8; 24],
    pub required_clock: String,
    pub seeds: Vec<SeedImage>,
}

/// Caller-supplied view, not an authenticated execution receipt. No Boolean
/// supplied here can turn a byte comparison into complete application evidence.
#[derive(Clone, Debug)]
pub(crate) struct MessageProjection {
    pub program: String,
    pub transaction: String,
    pub source_sha256: String,
    pub clock: String,
    pub input_queue: String,
    pub reply_queue: String,
    pub request: Vec<u8>,
    pub request_message_id: [u8; 24],
    pub request_correlation_id: [u8; 24],
    pub reply: Vec<u8>,
    pub reply_message_id: [u8; 24],
    pub reply_correlation_id: [u8; 24],
}

/// Only the supplied source bytes matched the fixed public source pins. Physical
/// path/FD, installed artifact, runtime association and clean-source admission
/// remain the responsibility of the original real owner.
#[derive(Debug)]
pub(crate) struct SourceProjection {
    fixture_sha256: String,
}

/// Deliberately cannot represent an application observation or owning PASS.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ProjectionMatch {
    pub compared_message_bytes: usize,
    pub reply_message_id_checked: bool,
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn decode_hex(text: &str, max: usize) -> Result<Vec<u8>, Refusal> {
    // Charge length before allocation; accept only canonical lowercase hex.
    if !text.len().is_multiple_of(2) || text.len() / 2 > max {
        return Err(Refusal::Bounds);
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(Refusal::Fixture),
    };
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Ok((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect()
}
fn identifier(text: &str, width: usize) -> Result<[u8; 24], Refusal> {
    let value = decode_hex(text, width)?;
    value.try_into().map_err(|_| Refusal::Fixture)
}

impl Fixtures {
    pub(crate) fn load() -> Result<Self, Refusal> {
        if FIXTURE.len() > MAX_FIXTURE_BYTES || hex(FIXTURE.as_bytes()) != FIXTURE_SHA {
            return Err(Refusal::Fixture);
        }
        let value: Self = serde_json::from_str(FIXTURE).map_err(|_| Refusal::Fixture)?;
        if value.schema != "mainframe-env.carddemo-mq-firstparty-source-scenarios@1"
            || value.corpus_commit != CORPUS
            || value.clock != "2026-08-30T12:00:00.000Z"
            || value.cases.len() != CASE_IDS.len()
            || value.source_pins.len() != 12
        {
            return Err(Refusal::Fixture);
        }
        let mut paths = BTreeSet::new();
        for pin in &value.source_pins {
            if !paths.insert(&pin.path)
                || pin.bytes == 0
                || pin.bytes > MAX_SOURCE_BYTES
                || decode_hex(&pin.sha256, 32)?.len() != 32
            {
                return Err(Refusal::Fixture);
            }
        }
        for (case, id) in value.cases.iter().zip(CASE_IDS) {
            if case.id != id || case.seed_images.len() > 3 {
                return Err(Refusal::Fixture);
            }
            identifier(&case.message_id_hex, 24)?;
            identifier(&case.correlation_id_hex, 24)?;
            let request = decode_hex(&case.request_hex, MAX_MESSAGE_BYTES)?;
            let reply = decode_hex(&case.expected_reply_hex, MAX_MESSAGE_BYTES)?;
            let is_base = case.id.starts_with("j15-");
            if request.is_empty()
                || reply.is_empty()
                || (is_base && (request.len() != 1_000 || reply.len() != 1_000))
                || (!is_base && (request.len() > 500 || reply.len() > 200))
            {
                return Err(Refusal::Fixture);
            }
            let mut keys = BTreeSet::new();
            for seed in &case.seed_images {
                let key = decode_hex(&seed.key_hex, 16)?;
                let record = decode_hex(&seed.record_hex, 500)?;
                let expected = match seed.dataset.as_str() {
                    "AWS.M2.CARDDEMO.ACCTDAT" => (11, 300),
                    "AWS.M2.CARDDEMO.CARDXREF" => (16, 50),
                    "AWS.M2.CARDDEMO.CUSTDAT" => (9, 500),
                    _ => return Err(Refusal::Fixture),
                };
                if (key.len(), record.len()) != expected || !keys.insert((&seed.dataset, key)) {
                    return Err(Refusal::Fixture);
                }
            }
        }
        Ok(value)
    }

    pub(crate) fn compare_sources(
        &self,
        inputs: &[(&str, &[u8])],
    ) -> Result<SourceProjection, Refusal> {
        if inputs.len() != self.source_pins.len() {
            return Err(Refusal::Source);
        }
        let mut seen = BTreeSet::new();
        let mut total = 0usize;
        for (path, bytes) in inputs {
            if !seen.insert(*path) {
                return Err(Refusal::Source);
            }
            total = total.checked_add(bytes.len()).ok_or(Refusal::Bounds)?;
            if total > MAX_SOURCE_BYTES {
                return Err(Refusal::Bounds);
            }
            let pin = self
                .source_pins
                .iter()
                .find(|pin| pin.path == *path)
                .ok_or(Refusal::Source)?;
            if bytes.len() != pin.bytes || hex(bytes) != pin.sha256 {
                return Err(Refusal::Source);
            }
        }
        Ok(SourceProjection {
            fixture_sha256: FIXTURE_SHA.into(),
        })
    }

    fn case(&self, case: Case, source: &SourceProjection) -> Result<&CaseSpec, Refusal> {
        if source.fixture_sha256 != FIXTURE_SHA {
            return Err(Refusal::Source);
        }
        self.cases
            .iter()
            .find(|value| value.id == case.id())
            .ok_or(Refusal::Fixture)
    }

    pub(crate) fn offer(
        &self,
        case: Case,
        source: &SourceProjection,
    ) -> Result<RequestOffer, Refusal> {
        let spec = self.case(case, source)?;
        let seeds = spec
            .seed_images
            .iter()
            .map(|seed| {
                Ok(SeedImage {
                    dataset: seed.dataset.clone(),
                    key: decode_hex(&seed.key_hex, 16)?,
                    record: decode_hex(&seed.record_hex, 500)?,
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        Ok(RequestOffer {
            program: spec.program.clone(),
            transaction: spec.transaction.clone(),
            input_queue: spec.input_queue.clone(),
            reply_queue: spec.reply_queue.clone(),
            body: decode_hex(&spec.request_hex, MAX_MESSAGE_BYTES)?,
            message_id: identifier(&spec.message_id_hex, 24)?,
            correlation_id: identifier(&spec.correlation_id_hex, 24)?,
            required_clock: self.clock.clone(),
            seeds,
        })
    }

    /// Checks full supplied input seed bytes only, not a post-application state.
    pub(crate) fn compare_seed_images(
        &self,
        case: Case,
        source: &SourceProjection,
        actual: &[SeedImage],
    ) -> Result<(), Refusal> {
        let expected = self.offer(case, source)?;
        if actual != expected.seeds.as_slice() {
            return Err(Refusal::Seed);
        }
        Ok(())
    }

    pub(crate) fn compare_message_projection(
        &self,
        case: Case,
        source: &SourceProjection,
        actual: &MessageProjection,
    ) -> Result<ProjectionMatch, Refusal> {
        let spec = self.case(case, source)?;
        let expected = self.offer(case, source)?;
        let source_path = match case {
            Case::Date => "app/app-vsam-mq/cbl/CODATE01.cbl",
            Case::Account => "app/app-vsam-mq/cbl/COACCT01.cbl",
            _ => "app/app-authorization-ims-db2-mq/cbl/COPAUA0C.cbl",
        };
        let source_pin = self
            .source_pins
            .iter()
            .find(|pin| pin.path == source_path)
            .ok_or(Refusal::Source)?;
        if actual.program != expected.program || actual.source_sha256 != source_pin.sha256 {
            return Err(Refusal::Source);
        }
        if actual.request.len() > MAX_MESSAGE_BYTES || actual.reply.len() > MAX_MESSAGE_BYTES {
            return Err(Refusal::Bounds);
        }
        if actual.clock != expected.required_clock
            || actual.transaction != expected.transaction
            || actual.input_queue != expected.input_queue
            || actual.reply_queue != expected.reply_queue
            || actual.request_message_id != expected.message_id
            || actual.request_correlation_id != expected.correlation_id
            || actual.reply_correlation_id != expected.correlation_id
        {
            return Err(Refusal::Identity);
        }
        if actual.request != expected.body {
            return Err(Refusal::Request);
        }
        if actual.reply != decode_hex(&spec.expected_reply_hex, MAX_MESSAGE_BYTES)? {
            return Err(Refusal::Reply);
        }
        let echo_message_id = matches!(case, Case::Date | Case::Account);
        if echo_message_id && actual.reply_message_id != expected.message_id {
            return Err(Refusal::Identity);
        }
        // COPAUA0C requests MQMI_NONE. Its issued reply message ID must later be
        // authenticated through the actual MQI result/journal; not guessed here.
        Ok(ProjectionMatch {
            compared_message_bytes: actual.request.len() + actual.reply.len(),
            reply_message_id_checked: echo_message_id,
        })
    }
}

#[cfg(test)]
#[path = "mq_firstparty_scenarios_tests.rs"]
mod tests;

// The installed CICS/legacy MQ owner is test-only, like this projection module.
#[cfg(target_os = "linux")]
#[path = "mq_cics_firstparty_owner.rs"]
mod installed_cics;

// Conditional J16 postimage comparisons remain pure and emit no observations.
#[path = "mq_authorization_postimages.rs"]
mod mq_authorization_postimages;
