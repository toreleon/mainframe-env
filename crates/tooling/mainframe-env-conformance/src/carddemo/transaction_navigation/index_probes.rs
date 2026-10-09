//! Physical alternate-index controls; compiled only for existing tests.
use super::super::transaction_harness::{navigation_encoded, navigation_record};
use super::*;
use mainframe_env_host_api::{HostProblem, KeyRelation};

const AIX: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH";
const FORWARD: [u8; 12] = [42, 43, 50, 44, 47, 52, 41, 49, 45, 46, 48, 51];
const REVERSE: [u8; 12] = [51, 48, 46, 45, 49, 41, 52, 47, 44, 50, 43, 42];

pub(in super::super) enum IndexCase {
    Forward,
    ReversePositioned,
    Ended,
}
fn index_request(
    fixture: &TransactionFixture,
    request: DatasetRequest,
    capture: &mut Capture,
) -> Result<DatasetResult, HostProblem> {
    let result = fixture.server.dataset_service().invoke(request);
    capture.probes.push(match &result {
        Ok(DatasetResult::Browse {
            cursor,
            record,
            identity,
            key,
        }) => serde_json::json!([cursor, record, identity, key]),
        other => serde_json::json!(format!("{other:?}")),
    });
    result
}
fn cursor_start(
    fixture: &TransactionFixture,
    key: Vec<u8>,
    relation: KeyRelation,
    capture: &mut Capture,
) -> Result<String, CorpusProblem> {
    match index_request(
        fixture,
        DatasetRequest::StartBrowse {
            dataset: DatasetName::new(AIX, 128).expect("literal dataset"),
            key,
            relation,
        },
        capture,
    )
    .map_err(terminal_problem)?
    {
        DatasetResult::Browse {
            cursor,
            record: None,
            identity: None,
            key: None,
        } if !cursor.is_empty() => Ok(cursor),
        actual => Err(CorpusProblem::new(
            "carddemo.navigation.cursor_start",
            format!("{actual:?}"),
        )),
    }
}
fn cursor_read(
    fixture: &TransactionFixture,
    cursor: &str,
    reverse: bool,
    capture: &mut Capture,
) -> Result<DatasetResult, HostProblem> {
    index_request(
        fixture,
        DatasetRequest::ReadNext {
            dataset: DatasetName::new(AIX, 128).expect("literal dataset"),
            cursor: cursor.into(),
            reverse,
            control: Default::default(),
        },
        capture,
    )
}
fn cursor_end(
    fixture: &TransactionFixture,
    cursor: &str,
    capture: &mut Capture,
) -> Result<(), CorpusProblem> {
    index_request(
        fixture,
        DatasetRequest::EndBrowse {
            dataset: DatasetName::new(AIX, 128).expect("literal dataset"),
            cursor: cursor.into(),
        },
        capture,
    )
    .map_err(terminal_problem)?;
    Ok(())
}
fn compare_reply(reply: DatasetResult, cursor: &str, key: Option<u8>) -> Result<(), CorpusProblem> {
    let expected = if let Some(key) = key {
        DatasetResult::Browse {
            cursor: cursor.into(),
            record: Some(navigation_record(key)?),
            identity: Some(navigation_encoded(&navigation_key(key))?),
            key: Some(navigation_encoded(&navigation_padded(
                NAVIGATION_PROCESSING[usize::from(key - 41)],
                26,
            ))?),
        }
    } else {
        DatasetResult::Browse {
            cursor: cursor.into(),
            record: None,
            identity: None,
            key: None,
        }
    };
    navigation_refuse(
        reply != expected,
        format!("native AIX tuple differs: expected {expected:?}; actual {reply:?}"),
    )
}
fn index_state(
    fixture: &TransactionFixture,
    before: &[RawRows],
    capture: &mut Capture,
) -> Result<(), CorpusProblem> {
    let state = snapshot(&fixture.server)?;
    capture.states.push(state.clone());
    navigation_refuse(state != before, "native AIX probe mutated complete tuples")
}
fn traverse(
    fixture: &TransactionFixture,
    before: &[RawRows],
    capture: &mut Capture,
    key: Vec<u8>,
    relation: KeyRelation,
    order: &[u8],
    reverse: bool,
    eof: bool,
) -> Result<(), CorpusProblem> {
    let cursor = cursor_start(fixture, key, relation, capture)?;
    let result = (|| {
        index_state(fixture, before, capture)?;
        for key in order {
            compare_reply(
                cursor_read(fixture, &cursor, reverse, capture).map_err(terminal_problem)?,
                &cursor,
                Some(*key),
            )?;
            index_state(fixture, before, capture)?;
        }
        if eof {
            compare_reply(
                cursor_read(fixture, &cursor, reverse, capture).map_err(terminal_problem)?,
                &cursor,
                None,
            )?;
            index_state(fixture, before, capture)?;
        }
        Ok(())
    })();
    let ended = cursor_end(fixture, &cursor, capture);
    result.and(ended)?;
    index_state(fixture, before, capture)
}
pub(in super::super) async fn compare_index(case: IndexCase) -> Result<Capture, CorpusProblem> {
    let fixture = TransactionFixture::open_navigation_rows(navigation_rows()?).await?;
    let before = match snapshot(&fixture.server) {
        Ok(before) => before,
        Err(problem) => return fixture.finish(Err(problem)).await,
    };
    let mut capture = Capture {
        screens: vec![],
        states: vec![],
        trace: vec![],
        probes: vec![],
    };
    let result = (|| {
        validate_navigation_initial(&before)?;
        match case {
            IndexCase::Forward => traverse(
                &fixture,
                &before,
                &mut capture,
                vec![0; 26],
                KeyRelation::GreaterOrEqual,
                &FORWARD,
                false,
                true,
            )?,
            IndexCase::ReversePositioned => {
                traverse(
                    &fixture,
                    &before,
                    &mut capture,
                    vec![255; 26],
                    KeyRelation::GreaterOrEqual,
                    &REVERSE,
                    true,
                    true,
                )?;
                traverse(
                    &fixture,
                    &before,
                    &mut capture,
                    navigation_encoded("2026-03-01                ")?,
                    KeyRelation::Equal,
                    &[42, 43, 50, 44],
                    false,
                    false,
                )?;
            }
            IndexCase::Ended => {
                let cursor = cursor_start(
                    &fixture,
                    vec![0; 26],
                    KeyRelation::GreaterOrEqual,
                    &mut capture,
                )?;
                let read = cursor_read(&fixture, &cursor, false, &mut capture)
                    .map_err(terminal_problem)
                    .and_then(|reply| compare_reply(reply, &cursor, Some(42)));
                let ended = cursor_end(&fixture, &cursor, &mut capture);
                read.and(ended)?;
                let reply = cursor_read(&fixture, &cursor, false, &mut capture);
                navigation_refuse(
                    !matches!(reply,Err(HostProblem::Condition {ref name,response:16,..}) if name=="INVREQ"),
                    format!("ended cursor did not refuse INVREQ 16: {reply:?}"),
                )?;
                index_state(&fixture, &before, &mut capture)?;
            }
        }
        Ok(())
    })();
    let exported = fixture
        .export_json("native-probes.json", &capture.probes)
        .and_then(|()| fixture.export_json("native-states.json", &capture.states))
        .and_then(|()| snapshot(&fixture.server))
        .and_then(|after| fixture.export_case(&serde_json::json!([]), &before, &after, &[]));
    fixture.finish(result.and(exported).map(|()| capture)).await
}
