//! Independent source-authored fixture; no expected byte comes from MQ output.
use super::*;

pub(super) const UTC_INPUT: &str = "20260830120000000";
const FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tooling/mainframe-env-conformance/src/carddemo/tests/mq_firstparty_scenarios/fixture.json"
));

fn hex(text: &str, length: usize) -> Result<Vec<u8>, CorpusProblem> {
    need(text.len() == length * 2 && text.is_ascii())?;
    (0..length)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| refused()))
        .collect()
}
pub(super) fn offer() -> Result<RequestOffer, CorpusProblem> {
    need(
        format!("{:x}", Sha256::digest(FIXTURE))
            == "c47edb82bdfe95caaceabbec4a02195016e09015cb9a551f2d0fb7c3643ef3d3",
    )?;
    let fixture: serde_json::Value = serde_json::from_slice(FIXTURE).map_err(|_| refused())?;
    need(
        fixture["corpus_commit"] == "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            && fixture["clock"] == "2026-08-30T12:00:00.000Z",
    )?;
    let cases = fixture["cases"].as_array().ok_or_else(refused)?;
    let selected = cases
        .iter()
        .filter(|row| row["id"] == "j15-date")
        .collect::<Vec<_>>();
    let [row] = selected.as_slice() else {
        return Err(refused());
    };
    need(
        row["program"] == PROGRAM
            && row["transaction"] == TRANSACTION
            && row["input_queue"] == "CARD.DEMO.REQUEST.DATE"
            && row["reply_queue"] == "CARD.DEMO.REPLY.DATE"
            && row["seed_images"].as_array().is_some_and(Vec::is_empty),
    )?;
    let request = hex(row["request_hex"].as_str().ok_or_else(refused)?, 1000)?;
    let reply = hex(
        row["expected_reply_hex"].as_str().ok_or_else(refused)?,
        1000,
    )?;
    let mut expected_request = vec![b' '; 1000];
    expected_request[..15].copy_from_slice(b"DATE00000000000");
    need(request == expected_request)?;
    compare_reply(&reply)?;
    Ok(RequestOffer {
        program: PROGRAM.into(),
        transaction: TRANSACTION.into(),
        input_queue: "CARD.DEMO.REQUEST.DATE".into(),
        reply_queue: "CARD.DEMO.REPLY.DATE".into(),
        body: request,
        message_id: hex(row["message_id_hex"].as_str().ok_or_else(refused)?, 24)?
            .try_into()
            .map_err(|_| refused())?,
        correlation_id: hex(row["correlation_id_hex"].as_str().ok_or_else(refused)?, 24)?
            .try_into()
            .map_err(|_| refused())?,
        reply,
    })
}
pub(super) fn compare_reply(actual: &[u8]) -> Result<(), CorpusProblem> {
    // CODATE01 clears all1000 bytes, uses MMDDYYYY DATESEP('-'), TIME TIMESEP,
    // then STRING concatenates these exact fields with no extra separator.
    let prefix = b"SYSTEM DATE : 08-30-2026SYSTEM TIME : 12:00:00";
    let mut expected = vec![b' '; 1000];
    expected[..prefix.len()].copy_from_slice(prefix);
    need(actual == expected)
}
