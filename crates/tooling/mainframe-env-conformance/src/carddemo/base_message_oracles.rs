//! Exact selected first-party sign-on/report messages and JOBS request bytes.
//! Offered FUNCTION CURRENT-DATE is independent of the live CICS SystemClockProvider.
//! The bill key uses existing local low-values compatibility only; no IBM numeric credit.
use super::*;
use mainframe_env_cics::CicsTraceEntry;

pub(super) const REPORT_CURRENT_DATE: &str = "2022070600000000+0000";
const SIGNON_INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COSGN00C.cbl",
        "4f901ae6b113eebae60ae1d94b30f8cb9906d031a4419770bf56980bdf97e377",
    ),
    (
        "app/bms/COSGN00.bms",
        "6747ba57e45fed7e52af728a1f8688ed8fab90b93e1234ab0a0bcae32fa39056",
    ),
    (
        "app/cpy-bms/COSGN00.CPY",
        "e2a2f8f6e76e1613cbd8f0baaca87d571a49a7bc219d16a732e4e4ecd7485407",
    ),
    (
        "app/cpy/CSUSR01Y.cpy",
        "37c34094dc4124bc6255e0b06a0387442f4f0dd9d51dd6623b559d370aa0d2cb",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS",
        "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8",
    ),
];
const REPORT_INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/CORPT00C.cbl",
        "0b06bd5d4f38f17871de8985c439dfce7ed30d28e02b6ffeb8459e0cd8db4fc2",
    ),
    (
        "app/bms/CORPT00.bms",
        "3ae6f0a2db62f64ca45ac6d975efefcc0bf635295ce4f6bea202c09d93deefc7",
    ),
    (
        "app/cpy-bms/CORPT00.CPY",
        "7da2980799bcc3df642d02d09fd00fafb890cdb487e034f2f9b5b0e0767ce03b",
    ),
    (
        "app/jcl/TRANREPT.jcl",
        "7d8fc0777e6b9fb1c62aee6b4b10a67d127057c84b92203c7152f230b3db9571",
    ),
    (
        "app/cbl/COBIL00C.cbl",
        "b5c46039eb8fd2f7ed7d3ca379172e56fa87385a369023bef706b04986fe1cfb",
    ),
    (
        "app/bms/COBIL00.bms",
        "f2101ff2143cd4746a5ffe0c4eeec977dfdad8410dee19236e0bdfb5ad58657f",
    ),
    (
        "app/cpy-bms/COBIL00.CPY",
        "a8d1a371182c73b41b4effd2d2a9c3b005f503cb0c8e88f5b2f5b5062e739e85",
    ),
    (
        "app/cbl/COTRN02C.cbl",
        "dc33b4c797a98d2732070a223f43cc78992c2846681163a032e376630742b1ed",
    ),
    (
        "app/cpy/CVTRA05Y.cpy",
        "d7bde0e78ff608497087b9909c889ed39e347269f964bda767b92b547fbb5fec",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
        "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    ),
];
const ACCOUNT_READ_INPUTS: &[(&str, &str)] = &[
    (
        "app/cbl/COACTVWC.cbl",
        "4f1e55176f69edfb55b7af7a161aceb8c3bc3bd1b2924a704a76256e0e751b54",
    ),
    (
        "app/bms/COACTVW.bms",
        "e229c1b4c9d3f52a5b8ebbb5266c9e47502ac90577b46f5f146bdb34b05f7f46",
    ),
    (
        "app/cpy-bms/COACTVW.CPY",
        "f3bf70a636aecb5874518ccc53f9826bcc69070b8307bdc551b2c57b9dd61dce",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CVCUS01Y.cpy",
        "944fd9a8eb10a683d0db97b9a87f2151982f649970287eb74358826056af25f8",
    ),
];
const NOT_FOUND: &[u8] =
    b"User not found. Try again ...                                                 ";
const WRONG_PASSWORD: &[u8] =
    b"Wrong Password. Try again ...                                                 ";
const REPORT_REVIEW: &[u8] =
    b"Please confirm to print the Monthly report...                                 ";
const REPORT_SUBMITTED: &[u8] =
    b"Monthly report submitted for printing ...                                     ";
const BILL_REVIEW: &[u8] =
    b"Confirm to make a bill payment...                                             ";
const BILL_PAID: &[u8] =
    b"Payment successful.  Your Transaction ID is 0000000000000002.                 ";
const JOB_ROWS: &[&[u8]] = &[
    b"//TRNRPT00 JOB 'TRAN REPORT',CLASS=A,MSGCLASS=0,                                ",
    b"// NOTIFY=&SYSUID                                                               ",
    b"//*                                                                             ",
    b"//JOBLIB JCLLIB ORDER=('AWS.M2.CARDDEMO.PROC')                                  ",
    b"//*                                                                             ",
    b"//STEP10 EXEC PROC=TRANREPT                                                     ",
    b"//*                                                                             ",
    b"//STEP05R.SYMNAMES DD *                                                         ",
    b"TRAN-CARD-NUM,263,16,ZD                                                         ",
    b"TRAN-PROC-DT,305,10,CH                                                          ",
    b"PARM-START-DATE,C'2022-07-01'                                                   ",
    b"PARM-END-DATE,C'2022-07-31'                                                     ",
    b"/*                                                                              ",
    b"//STEP10R.DATEPARM DD *                                                         ",
    b"2022-07-01 2022-07-31                                                           ",
    b"/*                                                                              ",
    b"/*EOF                                                                           ",
];

pub(super) struct NotFoundMessageProof {
    _private: (),
}
pub(super) struct ReportReviewProof {
    _private: (),
}
pub(super) struct ReportMessagesProof {
    _private: (),
}
pub(super) struct BillReviewProof {
    _private: (),
}
pub(super) struct PurchaseKeyProof {
    _private: (),
}

fn pins(corpus: &Path, expected: &[(&str, &str)]) -> Result<(), CorpusProblem> {
    for (path, digest) in expected {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *digest {
            return Err(CorpusProblem::new(
                "carddemo.online.message_source_drift",
                *path,
            ));
        }
    }
    Ok(())
}
fn refuse(refused: bool, diagnostic: &str) -> Result<(), CorpusProblem> {
    if refused {
        Err(CorpusProblem::new(
            diagnostic,
            "first-party fixed comparison refused",
        ))
    } else {
        Ok(())
    }
}
fn message_matches(terminal: &serde_json::Value, mapset: &str, map: &str, expected: &[u8]) -> bool {
    terminal["mapset"] == mapset
        && terminal["map"] == map
        && online_screen_fields(terminal)
            .is_ok_and(|fields| fields.get("ERRMSG").map(Vec::as_slice) == Some(expected))
}
fn signon_matches(terminal: &serde_json::Value, trace: &[CicsTraceEntry], not_found: bool) -> bool {
    let reads = trace
        .iter()
        .filter(|e| e.operation == CicsOperation::Read)
        .collect::<Vec<_>>();
    let expected = if not_found { NOT_FOUND } else { WRONG_PASSWORD };
    let (tag, response, response2, bytes) = if not_found {
        ("NOTFND", 13, 80, 0)
    } else {
        ("NORMAL", 0, 0, 80)
    };
    message_matches(terminal, "COSGN00", "COSGN0A", expected)
        && reads.len() == 1
        && reads[0].outcome == tag
        && reads[0].response == response
        && reads[0].response2 == response2
        && reads[0].payload_bytes == bytes
}
// Bind only the existing original-session account-detail ENTER's fresh trace.
// CicsTraceEntry has no program/file actor field: map identity and this owned
// exchange select the stage; the pinned COACTVWC path declares XREF/account/customer.
fn account_detail_reads_match(terminal: &serde_json::Value, trace: &[CicsTraceEntry]) -> bool {
    let reads = trace
        .iter()
        .filter(|entry| entry.operation == CicsOperation::Read)
        .collect::<Vec<_>>();
    terminal["mapset"] == "COACTVW"
        && terminal["map"] == "CACTVWA"
        && reads.len() == 3
        && reads.iter().zip([50, 300, 500]).all(|(entry, bytes)| {
            entry.outcome == "NORMAL"
                && entry.response == 0
                && entry.response2 == 0
                && entry.payload_bytes == bytes
        })
}
pub(super) fn compare_account_detail_reads(
    corpus: &Path,
    terminal: &serde_json::Value,
    trace: &[CicsTraceEntry],
) -> Result<(), CorpusProblem> {
    pins(corpus, ACCOUNT_READ_INPUTS)?;
    refuse(
        !account_detail_reads_match(terminal, trace),
        "carddemo.online.account_detail_reads",
    )
}

pub(super) fn trace_since(
    server: &ProductServer,
    session: &str,
    at: usize,
) -> Result<Vec<CicsTraceEntry>, CorpusProblem> {
    server
        .online_trace(session)
        .map_err(terminal_problem)?
        .get(at..)
        .map(<[CicsTraceEntry]>::to_vec)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.message_trace_regressed",
                "trace prefix disappeared",
            )
        })
}
pub(super) fn compare_not_found(
    corpus: &Path,
    terminal: &serde_json::Value,
    trace: &[CicsTraceEntry],
) -> Result<NotFoundMessageProof, CorpusProblem> {
    pins(corpus, SIGNON_INPUTS)?;
    refuse(
        !signon_matches(terminal, trace, true),
        "carddemo.online.not_found_message",
    )?;
    Ok(NotFoundMessageProof { _private: () })
}
fn supplied_password_absent_from_fields(terminal: &serde_json::Value) -> bool {
    online_screen_fields(terminal).is_ok_and(|fields| {
        fields.values().all(|value| {
            // Literal Cp037 bytes for the same supplied password, independently
            // frozen from the admitted CCSID; no product encoder supplies expected.
            let cp037 = [0xc2, 0xc1, 0xc4, 0xd7, 0xc1, 0xe2, 0xe2];
            !value
                .windows(7)
                .any(|part| part == b"BADPASS" || part == cp037.as_slice())
        })
    })
}
fn bind_wrong_password(
    terminal: &serde_json::Value,
    trace: &[CicsTraceEntry],
    _first: NotFoundMessageProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J02",
        "NOTFND and wrong-password messages",
        !signon_matches(terminal, trace, false) || !supplied_password_absent_from_fields(terminal),
        || {
            Ok(CorpusProblem::new(
                "carddemo.online.wrong_password_message",
                "exact wrong-password message/READ or supplied-password absence refused",
            ))
        },
    )
}
pub(super) fn compare_wrong_password(
    corpus: &Path,
    terminal: &serde_json::Value,
    trace: &[CicsTraceEntry],
    first: NotFoundMessageProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    pins(corpus, SIGNON_INPUTS)?;
    bind_wrong_password(terminal, trace, first, observations)
}

fn purchase_keys_match(before: &[String], after: &[String]) -> bool {
    let mut seed = vec![0; 350];
    seed[342..].copy_from_slice(b"00000100");
    before.len() == 1
        && before[0].as_bytes() == seed
        && after.len() == 2
        && after[0] == before[0]
        && after[1].len() == 350
        && after[1].get(..16) == Some("0000000000000001")
}
pub(super) fn compare_purchase_key(
    corpus: &Path,
    before: &[String],
    after: &[String],
) -> Result<PurchaseKeyProof, CorpusProblem> {
    pins(corpus, REPORT_INPUTS)?;
    refuse(
        !purchase_keys_match(before, after),
        "carddemo.online.bill_key_basis",
    )?;
    Ok(PurchaseKeyProof { _private: () })
}
pub(super) fn compare_report_review(
    corpus: &Path,
    terminal: &serde_json::Value,
    before: &[Vec<u8>],
    after_review: &[Vec<u8>],
) -> Result<ReportReviewProof, CorpusProblem> {
    pins(corpus, REPORT_INPUTS)?;
    refuse(
        !message_matches(terminal, "CORPT00", "CORPT0A", REPORT_REVIEW) || before != after_review,
        "carddemo.online.report_review_message",
    )?;
    Ok(ReportReviewProof { _private: () })
}
fn queue_matches(before: &[Vec<u8>], after: &[Vec<u8>], trace: &[CicsTraceEntry]) -> bool {
    let writes = trace
        .iter()
        .filter(|e| e.operation == CicsOperation::WriteTransientData)
        .collect::<Vec<_>>();
    before.len().checked_add(JOB_ROWS.len()) == Some(after.len())
        && after.get(..before.len()) == Some(before)
        && after
            .get(before.len()..)
            .is_some_and(|rows| rows.iter().map(Vec::as_slice).eq(JOB_ROWS.iter().copied()))
        && writes.len() == JOB_ROWS.len()
        && writes.iter().all(|e| {
            e.outcome == "NORMAL" && e.response == 0 && e.response2 == 0 && e.payload_bytes == 0
        })
}
fn bind_report_request(
    terminal: &serde_json::Value,
    before: &[Vec<u8>],
    after: &[Vec<u8>],
    trace: &[CicsTraceEntry],
    _review: ReportReviewProof,
    observations: &mut RouteObservations,
) -> Result<ReportMessagesProof, CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J07",
        "WRITEQ TD job request",
        !queue_matches(before, after, trace)
            || !message_matches(terminal, "CORPT00", "CORPT0A", REPORT_SUBMITTED),
        || {
            Ok(CorpusProblem::new(
                "carddemo.online.report_job_request",
                "exact ordered JOBS suffix/prefix, response, or message refused",
            ))
        },
    )?;
    Ok(ReportMessagesProof { _private: () })
}
pub(super) fn compare_report_request(
    corpus: &Path,
    terminal: &serde_json::Value,
    before: &[Vec<u8>],
    after: &[Vec<u8>],
    trace: &[CicsTraceEntry],
    review: ReportReviewProof,
    observations: &mut RouteObservations,
) -> Result<ReportMessagesProof, CorpusProblem> {
    pins(corpus, REPORT_INPUTS)?;
    bind_report_request(terminal, before, after, trace, review, observations)
}
pub(super) fn compare_bill_review(
    corpus: &Path,
    terminal: &serde_json::Value,
) -> Result<BillReviewProof, CorpusProblem> {
    pins(corpus, REPORT_INPUTS)?;
    refuse(
        !message_matches(terminal, "COBIL00", "COBIL0A", BILL_REVIEW),
        "carddemo.online.bill_review_message",
    )?;
    Ok(BillReviewProof { _private: () })
}
fn bind_bill_success(
    terminal: &serde_json::Value,
    _report: ReportMessagesProof,
    _review: BillReviewProof,
    _purchase: PurchaseKeyProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J07",
        "reported messages",
        !message_matches(terminal, "COBIL00", "COBIL0A", BILL_PAID),
        || {
            Ok(CorpusProblem::new(
                "carddemo.online.bill_success_message",
                "exact payment message with independently fixed local key refused",
            ))
        },
    )
}
pub(super) fn compare_bill_success(
    corpus: &Path,
    terminal: &serde_json::Value,
    report: ReportMessagesProof,
    review: BillReviewProof,
    purchase: PurchaseKeyProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    pins(corpus, REPORT_INPUTS)?;
    bind_bill_success(terminal, report, review, purchase, observations)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(mapset: &str, map: &str, value: &[u8]) -> serde_json::Value {
        // The public screen is a sequence of u32 length/name/u32 length/value fields.
        let mut screen = (6u32).to_be_bytes().to_vec();
        screen.extend_from_slice(b"ERRMSG");
        screen.extend_from_slice(&(value.len() as u32).to_be_bytes());
        screen.extend_from_slice(value);
        serde_json::json!({"mapset":mapset,"map":map,"screen_base64":base64::engine::general_purpose::STANDARD.encode(screen)})
    }
    fn entry(
        operation: CicsOperation,
        tag: &str,
        response: i32,
        response2: i32,
        bytes: usize,
    ) -> CicsTraceEntry {
        CicsTraceEntry {
            operation,
            outcome: tag.into(),
            response,
            response2,
            payload_bytes: bytes,
        }
    }
    #[test]
    fn fresh_account_detail_reads_cannot_be_replaced_by_prior_signon_or_other_stages() {
        let terminal = serde_json::json!({"mapset":"COACTVW", "map":"CACTVWA"});
        let original = vec![
            entry(CicsOperation::Read, "NORMAL", 0, 0, 50),
            entry(CicsOperation::Read, "NORMAL", 0, 0, 300),
            entry(CicsOperation::Read, "NORMAL", 0, 0, 500),
        ];
        assert!(account_detail_reads_match(&terminal, &original));
        let mut missing = original.clone();
        missing.remove(1);
        assert!(!account_detail_reads_match(&terminal, &missing));
        // The newly added valid-user/wrong-password READ cannot replace a missing account read.
        let mut signon = missing.clone();
        signon.insert(0, entry(CicsOperation::Read, "NORMAL", 0, 0, 80));
        assert!(!account_detail_reads_match(&terminal, &signon));
        let mut reordered = original.clone();
        reordered.swap(0, 1);
        assert!(!account_detail_reads_match(&terminal, &reordered));
        let mut extra = original.clone();
        extra.push(original[2].clone());
        assert!(!account_detail_reads_match(&terminal, &extra));
        for index in 0..3 {
            for change in 0..4 {
                let mut bad = original.clone();
                match change {
                    0 => bad[index].outcome = "NOTFND".into(),
                    1 => bad[index].response = 13,
                    2 => bad[index].response2 = 1,
                    _ => bad[index].payload_bytes -= 1,
                }
                assert!(!account_detail_reads_match(&terminal, &bad));
            }
        }
        assert!(!account_detail_reads_match(
            &serde_json::json!({"mapset":"COSGN00", "map":"COSGN0A"}),
            &original
        ));
        assert!(!account_detail_reads_match(
            &serde_json::json!({"mapset":"COACTVW", "map":"WRONG"}),
            &original
        ));
    }
    #[test]
    fn signon_message_identity_and_read_corruptions_refuse_without_credit() {
        let good = frame("COSGN00", "COSGN0A", WRONG_PASSWORD);
        let trace = vec![entry(CicsOperation::Read, "NORMAL", 0, 0, 80)];
        assert!(signon_matches(&good, &trace, false));
        let mut cases = vec![
            (frame("COSGN00", "COSGN0A", NOT_FOUND), trace.clone()),
            (frame("WRONG", "COSGN0A", WRONG_PASSWORD), trace.clone()),
            (frame("COSGN00", "WRONG", WRONG_PASSWORD), trace.clone()),
            (good.clone(), vec![]),
        ];
        let mut short = WRONG_PASSWORD.to_vec();
        short.pop();
        cases.push((frame("COSGN00", "COSGN0A", &short), trace.clone()));
        for changed in [
            entry(CicsOperation::Read, "NOTFND", 13, 80, 0),
            entry(CicsOperation::Read, "NORMAL", 0, 1, 80),
            entry(CicsOperation::Read, "NORMAL", 0, 0, 79),
        ] {
            cases.push((good.clone(), vec![changed]));
        }
        // The JSON contains base64, so a raw-body search alone would miss this leak.
        let mut leaked = good.clone();
        let mut screen = base64::engine::general_purpose::STANDARD
            .decode(leaked["screen_base64"].as_str().unwrap())
            .unwrap();
        screen.extend_from_slice(&(6u32).to_be_bytes());
        screen.extend_from_slice(b"PASSWD");
        screen.extend_from_slice(&(7u32).to_be_bytes());
        screen.extend_from_slice(b"BADPASS");
        leaked["screen_base64"] =
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(screen));
        cases.push((leaked.clone(), trace.clone()));
        // Opaque screen values can also carry the admitted Cp037 representation.
        let mut screen = base64::engine::general_purpose::STANDARD
            .decode(leaked["screen_base64"].as_str().unwrap())
            .unwrap();
        let end = screen.len();
        screen[end - 7..].copy_from_slice(&[0xc2, 0xc1, 0xc4, 0xd7, 0xc1, 0xe2, 0xe2]);
        leaked["screen_base64"] =
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(screen));
        cases.push((leaked, trace.clone()));
        for (terminal, trace) in cases {
            let mut observed = RouteObservations::default();
            assert!(
                bind_wrong_password(
                    &terminal,
                    &trace,
                    NotFoundMessageProof { _private: () },
                    &mut observed
                )
                .is_err()
            );
            assert!(observed.journeys().is_empty());
        }
        let found = frame("COSGN00", "COSGN0A", NOT_FOUND);
        assert!(signon_matches(
            &found,
            &[entry(CicsOperation::Read, "NOTFND", 13, 80, 0)],
            true
        ));
        assert!(!signon_matches(
            &found,
            &[entry(CicsOperation::Read, "NORMAL", 0, 0, 80)],
            true
        ));
    }
    #[test]
    fn every_job_row_byte_padding_date_order_and_prefix_corruption_refuses_without_credit() {
        let before = vec![b"existing owned prefix".to_vec()];
        let after = before
            .iter()
            .cloned()
            .chain(JOB_ROWS.iter().map(|r| r.to_vec()))
            .collect::<Vec<_>>();
        let trace = vec![entry(CicsOperation::WriteTransientData, "NORMAL", 0, 0, 0); 17];
        assert!(queue_matches(&before, &after, &trace));
        let terminal = frame("CORPT00", "CORPT0A", REPORT_SUBMITTED);
        let mut bad = Vec::new();
        for row in 0..17 {
            let mut changed = after.clone();
            changed[row + 1][0] ^= 1;
            bad.push(changed);
            let mut changed = after.clone();
            changed[row + 1].pop();
            bad.push(changed);
        }
        let mut order = after.clone();
        order.swap(1, 2);
        bad.push(order);
        let mut date = after.clone();
        date[11][18] = b'9';
        bad.push(date);
        let mut prefix = after.clone();
        prefix[0][0] ^= 1;
        bad.push(prefix);
        let mut missing = after.clone();
        missing.pop();
        bad.push(missing);
        let mut extra = after.clone();
        extra.push(vec![b' '; 80]);
        bad.push(extra);
        for changed in bad {
            let mut observed = RouteObservations::default();
            assert!(
                bind_report_request(
                    &terminal,
                    &before,
                    &changed,
                    &trace,
                    ReportReviewProof { _private: () },
                    &mut observed
                )
                .is_err()
            );
            assert!(observed.journeys().is_empty());
        }
        let mut trace = trace;
        trace[0].response2 = 1;
        let mut observed = RouteObservations::default();
        assert!(
            bind_report_request(
                &terminal,
                &before,
                &after,
                &trace,
                ReportReviewProof { _private: () },
                &mut observed
            )
            .is_err()
        );
        assert!(observed.journeys().is_empty());
    }
    #[test]
    fn all_selected_report_and_bill_message_corruptions_are_refused() {
        for (mapset, map, expected) in [
            ("CORPT00", "CORPT0A", REPORT_REVIEW),
            ("CORPT00", "CORPT0A", REPORT_SUBMITTED),
            ("COBIL00", "COBIL0A", BILL_REVIEW),
            ("COBIL00", "COBIL0A", BILL_PAID),
        ] {
            assert!(message_matches(
                &frame(mapset, map, expected),
                mapset,
                map,
                expected
            ));
            let mut corrupt = expected.to_vec();
            corrupt[0] ^= 1;
            assert!(!message_matches(
                &frame(mapset, map, &corrupt),
                mapset,
                map,
                expected
            ));
            assert!(!message_matches(
                &frame("WRONG", map, expected),
                mapset,
                map,
                expected
            ));
            let mut short = expected.to_vec();
            short.pop();
            assert!(!message_matches(
                &frame(mapset, map, &short),
                mapset,
                map,
                expected
            ));
        }
        let mut bad = BILL_PAID.to_vec();
        bad[59] = b'9';
        let mut observed = RouteObservations::default();
        assert!(
            bind_bill_success(
                &frame("COBIL00", "COBIL0A", &bad),
                ReportMessagesProof { _private: () },
                BillReviewProof { _private: () },
                PurchaseKeyProof { _private: () },
                &mut observed
            )
            .is_err()
        );
        assert!(observed.journeys().is_empty());
    }
    #[test]
    fn bootstrap_or_purchase_key_changes_refuse_fixed_bill_key_basis() {
        let mut seed = vec![0; 350];
        seed[342..].copy_from_slice(b"00000100");
        let seed = String::from_utf8(seed).unwrap();
        let row = format!("0000000000000001{}", " ".repeat(334));
        let before = vec![seed.clone()];
        let after = vec![seed.clone(), row];
        assert!(purchase_keys_match(&before, &after));
        let mut wrong = after.clone();
        wrong[1].replace_range(..16, "0000000000000002");
        assert!(!purchase_keys_match(&before, &wrong));
        assert!(!purchase_keys_match(&[], &after));
        assert!(!purchase_keys_match(&before, &after[1..]));
        let mut changed = before.clone();
        changed[0].replace_range(342..343, "1");
        assert!(!purchase_keys_match(&changed, &after));
    }
}
