//! Selected public navigation_key navigation; native index probes earn no application credit.
use super::transaction_harness::{
    NAVIGATION_PROCESSING, RawRows, TransactionFixture, navigation_key, navigation_padded,
    navigation_refuse, navigation_rows, snapshot, validate_navigation_initial,
};
use super::transaction_harness::{navigation_fields, navigation_screen_identity};
use super::*;
use mainframe_env_cics::CicsTraceEntry;

const FIRST: [u8; 10] = [41, 42, 43, 44, 45, 46, 47, 48, 49, 50];

#[cfg(test)]
mod index_probes;
#[cfg(test)]
pub(super) use index_probes::{IndexCase, compare_index};

pub(super) enum NavigationCase {
    FirstPage,
    Bottom,
    Previous,
    Selection,
    Lookup,
    Back,
    Clear,
    DetailRefusal,
    ListRefusal,
    AddBack,
    #[cfg(test)]
    WrongPrimaryOrder,
}

pub(super) struct Capture {
    pub(super) screens: Vec<serde_json::Value>,
    pub(super) states: Vec<Vec<RawRows>>,
    pub(super) trace: Vec<CicsTraceEntry>,
    #[cfg(test)]
    pub(super) probes: Vec<serde_json::Value>,
}

fn menu(response: &serde_json::Value) -> Result<(), CorpusProblem> {
    navigation_screen_identity(response, "COMEN01", "COMEN1A", "COMEN01C", "CM00")?;
    navigation_fields(
        response,
        BTreeMap::from([
            (
                "OPTN006".into(),
                navigation_padded("06. Transaction List", 40),
            ),
            (
                "OPTN007".into(),
                navigation_padded("07. Transaction View", 40),
            ),
            (
                "OPTN008".into(),
                navigation_padded("08. Transaction Add", 40),
            ),
        ]),
    )
}
fn list(
    response: &serde_json::Value,
    keys: &[u8],
    page: &str,
    error: &str,
) -> Result<(), CorpusProblem> {
    navigation_screen_identity(response, "COTRN00", "COTRN0A", "COTRN00C", "CT00")?;
    let mut fields = BTreeMap::from([
        ("PAGENUM".into(), page.into()),
        ("ERRMSG".into(), navigation_padded(error, 78)),
    ]);
    for slot in 0..10 {
        let suffix = format!("{:02}", slot + 1);
        let values = if let Some(key) = keys.get(slot) {
            [
                navigation_key(*key),
                "02/28/26".into(),
                navigation_padded(&format!("NAV ROW {key}"), 26),
                "+00000001.00".into(),
            ]
        } else {
            [
                navigation_padded("", 16),
                navigation_padded("", 8),
                navigation_padded("", 26),
                navigation_padded("", 12),
            ]
        };
        for (name, value) in ["TRNID", "TDATE", "TDESC", "TAMT0"].into_iter().zip(values) {
            fields.insert(format!("{name}{suffix}"), value);
        }
    }
    navigation_fields(response, fields)
}
fn detail(
    response: &serde_json::Value,
    key: Option<u8>,
    input: &str,
    error: &str,
    blank: char,
) -> Result<(), CorpusProblem> {
    navigation_screen_identity(response, "COTRN01", "COTRN1A", "COTRN01C", "CT01")?;
    let mut fields = BTreeMap::from([
        ("TRNIDIN".into(), navigation_padded(input, 16)),
        ("ERRMSG".into(), navigation_padded(error, 78)),
    ]);
    let values = if let Some(key) = key {
        [
            navigation_key(key),
            "0500024453765740".into(),
            "01".into(),
            "0001".into(),
            navigation_padded("ONLINE", 10),
            navigation_padded(&format!("NAV ROW {key}"), 60),
            "+00000001.00".into(),
            "2026-02-28".into(),
            NAVIGATION_PROCESSING[usize::from(key - 41)].into(),
            "123456789".into(),
            navigation_padded("NAVIGATION SHOP", 30),
            navigation_padded("BOSTON", 25),
            navigation_padded("02110", 10),
        ]
    } else {
        [16, 16, 2, 4, 10, 60, 12, 10, 10, 9, 30, 25, 10].map(|n| blank.to_string().repeat(n))
    };
    for (name, value) in [
        "TRNID", "CARDNUM", "TTYPCD", "TCATCD", "TRNSRC", "TDESC", "TRNAMT", "TORIGDT", "TPROCDT",
        "MID", "MNAME", "MCITY", "MZIP",
    ]
    .into_iter()
    .zip(values)
    {
        fields.insert(name.into(), value);
    }
    navigation_fields(response, fields)
}

struct Caller<'a> {
    fixture: &'a TransactionFixture,
    baseline: Vec<RawRows>,
    route: Option<CardDemoOnlineSession>,
    trace_at: usize,
    captured: Capture,
}
impl<'a> Caller<'a> {
    fn new(fixture: &'a TransactionFixture, baseline: Vec<RawRows>) -> Self {
        Self {
            fixture,
            baseline,
            route: None,
            trace_at: 0,
            captured: Capture {
                screens: vec![],
                states: vec![],
                trace: vec![],
                #[cfg(test)]
                probes: vec![],
            },
        }
    }
    fn trace(&mut self) -> Result<Vec<CicsTraceEntry>, CorpusProblem> {
        let route = self.route.as_ref().ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.navigation.route_missing",
                "actual session required",
            )
        })?;
        let trace = self
            .fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?;
        let delta = trace[self.trace_at..].to_vec();
        self.trace_at = trace.len();
        self.captured.trace.extend(delta.iter().cloned());
        Ok(delta)
    }
    fn capture(&mut self, response: serde_json::Value) -> Result<serde_json::Value, CorpusProblem> {
        navigation_refuse(
            self.captured.screens.len() >= 16,
            "navigation stage bound exceeded",
        )?;
        let state = snapshot(&self.fixture.server)?;
        let trace = self.trace()?;
        self.fixture.export_json(
            &format!("stage-{:02}.json", self.captured.screens.len()),
            &(
                &response,
                &state,
                trace
                    .iter()
                    .map(|e| {
                        (
                            format!("{:?}", e.operation),
                            &e.outcome,
                            e.response,
                            e.response2,
                            e.payload_bytes,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        )?;
        self.captured.screens.push(response.clone());
        self.captured.states.push(state.clone());
        navigation_refuse(
            state != self.baseline,
            "navigation changed complete dataset/index tuples",
        )?;
        navigation_refuse(
            trace.iter().any(|e| {
                matches!(
                    e.operation,
                    CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
                )
            }),
            "navigation reached a mutating application operation",
        )?;
        Ok(response)
    }
    async fn open_menu(&mut self) -> Result<serde_json::Value, CorpusProblem> {
        self.route = Some(
            open_carddemo_menu(
                &self.fixture.server,
                &self.fixture.app,
                "WEBUSER",
                "transport-password",
                "USER0001",
                "PASSWORD",
                "COMEN01",
            )
            .await?,
        );
        self.trace_at = 0;
        let route = self.route.as_ref().expect("actual route");
        let screen = terminal_http(
            &self.fixture.app,
            Method::GET,
            &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
            route.headers.clone(),
            vec![],
        )
        .await?;
        require_terminal_status(screen.0, StatusCode::OK, "navigation current screen")?;
        let response = serde_json::from_slice(&screen.1).map_err(|error| {
            CorpusProblem::new("carddemo.navigation.response", error.to_string())
        })?;
        let response = self.capture(response)?;
        menu(&response)?;
        Ok(response)
    }
    async fn step(
        &mut self,
        aid: u8,
        fields: BTreeMap<String, String>,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let route = self.route.as_ref().expect("actual route");
        let result = carddemo_terminal_exchange(
            &self.fixture.app,
            &route.session,
            &route.headers,
            aid,
            fields,
        )
        .await;
        match result {
            Ok(response) => self.capture(response),
            Err(problem) => {
                self.trace()?;
                Err(problem)
            }
        }
    }
    async fn option(&mut self, option: u8) -> Result<serde_json::Value, CorpusProblem> {
        self.step(
            0x7d,
            BTreeMap::from([("OPTION".into(), option.to_string())]),
        )
        .await
    }
    async fn aid(&mut self, aid: u8) -> Result<serde_json::Value, CorpusProblem> {
        self.step(aid, BTreeMap::new()).await
    }
    async fn first(&mut self) -> Result<(), CorpusProblem> {
        self.open_menu().await?;
        let response = self.option(6).await?;
        list(&response, &FIRST, "00000001", "")
    }
    fn observed(&self, at: usize) -> &[CicsTraceEntry] {
        &self.captured.trace[at..]
    }
    fn count(&self, at: usize, op: CicsOperation) -> usize {
        self.observed(at)
            .iter()
            .filter(|e| e.operation == op)
            .count()
    }
    async fn compare_first_page(&mut self, keys: [u8; 10]) -> Result<(), CorpusProblem> {
        self.open_menu().await?;
        let at = self.captured.trace.len();
        let response = self.option(6).await?;
        list(&response, &keys, "00000001", "")?;
        navigation_refuse(
            self.count(at, CicsOperation::StartBrowse) != 1
                || self.count(at, CicsOperation::EndBrowse) != 1,
            "first page did not bracket its actual browse",
        )?;
        let reads = self
            .observed(at)
            .iter()
            .filter(|e| e.operation == CicsOperation::ReadNext)
            .collect::<Vec<_>>();
        navigation_refuse(
            reads.len() != 11
                || reads
                    .iter()
                    .any(|e| e.outcome != "NORMAL" || e.response != 0 || e.payload_bytes != 350),
            "first page requires eleven actual normal complete reads",
        )?;
        Ok(())
    }
    async fn exercise(&mut self, case: NavigationCase) -> Result<(), CorpusProblem> {
        match case {
            NavigationCase::FirstPage => self.compare_first_page(FIRST).await?,
            #[cfg(test)]
            NavigationCase::WrongPrimaryOrder => {
                self.compare_first_page([42, 42, 43, 44, 45, 46, 47, 48, 49, 50])
                    .await?;
            }
            NavigationCase::Bottom | NavigationCase::Previous => {
                self.first().await?;
                let at = self.captured.trace.len();
                let response = self.aid(0xf8).await?;
                list(
                    &response,
                    &[51, 52],
                    "00000002",
                    "You have reached the bottom of the page...",
                )?;
                let reads = self
                    .observed(at)
                    .iter()
                    .filter(|e| e.operation == CicsOperation::ReadNext)
                    .collect::<Vec<_>>();
                navigation_refuse(
                    reads.len() != 4
                        || reads[..3]
                            .iter()
                            .any(|e| e.outcome != "NORMAL" || e.payload_bytes != 350)
                        || reads[3].outcome != "ENDFILE"
                        || reads[3].response != 20
                        || self.count(at, CicsOperation::EndBrowse) != 1,
                    "second page requires discard, two rows, actual ENDFILE 20 and ENDBR",
                )?;
                // Actual RESP2 is retained; the pinned 20/90 qualification remains pending.
                let at = self.captured.trace.len();
                if matches!(case, NavigationCase::Previous) {
                    let response = self.aid(0xf7).await?;
                    list(
                        &response,
                        &FIRST,
                        "00000001",
                        "You have reached the top of the page...",
                    )?;
                    let reads = self
                        .observed(at)
                        .iter()
                        .filter(|e| e.operation == CicsOperation::ReadPrev)
                        .collect::<Vec<_>>();
                    navigation_refuse(
                        reads.len() != 12
                            || reads[..11].iter().any(|e| {
                                e.outcome != "NORMAL"
                                    || e.response != 0
                                    || e.response2 != 0
                                    || e.payload_bytes != 350
                            })
                            || reads[11].outcome != "ENDFILE"
                            || reads[11].response != 20
                            || self.count(at, CicsOperation::StartBrowse) != 1
                            || self.count(at, CicsOperation::EndBrowse) != 1,
                        "PF7 requires discard, ten rows, lookbehind ENDFILE 20 and ENDBR",
                    )?;
                    // Actual ENDFILE RESP2 remains retained, not a new secondary golden.
                    let at = self.captured.trace.len();
                    let response = self.aid(0xf7).await?;
                    list(
                        &response,
                        &FIRST,
                        "00000001",
                        "You are already at the top of the page...",
                    )?;
                    navigation_refuse(
                        self.count(at, CicsOperation::ReadNext)
                            + self.count(at, CicsOperation::ReadPrev)
                            != 0,
                        "top refusal read again",
                    )?;
                } else {
                    let response = self.aid(0xf8).await?;
                    list(
                        &response,
                        &[51, 52],
                        "00000002",
                        "You are already at the bottom of the page...",
                    )?;
                    navigation_refuse(
                        self.count(at, CicsOperation::ReadNext)
                            + self.count(at, CicsOperation::ReadPrev)
                            != 0,
                        "bottom refusal read again",
                    )?;
                }
            }
            NavigationCase::Selection => {
                for (field, selection, key) in [("SEL0002", "S", 42), ("SEL0003", "s", 43)] {
                    self.first().await?;
                    let at = self.captured.trace.len();
                    let response = self
                        .step(0x7d, BTreeMap::from([(field.into(), selection.into())]))
                        .await?;
                    detail(&response, Some(key), &navigation_key(key), "", ' ')?;
                    self.require_read(at, "NORMAL", 0, 0)?;
                    navigation_refuse(
                        self.count(at, CicsOperation::Xctl) == 0,
                        "selection omitted actual XCTL",
                    )?;
                }
            }
            NavigationCase::Lookup | NavigationCase::Clear => {
                self.open_menu().await?;
                self.option(7).await?;
                let at = self.captured.trace.len();
                let response = self
                    .step(
                        0x7d,
                        BTreeMap::from([("TRNIDIN".into(), navigation_key(52))]),
                    )
                    .await?;
                detail(&response, Some(52), &navigation_key(52), "", ' ')?;
                self.require_read(at, "NORMAL", 0, 0)?;
                if matches!(case, NavigationCase::Clear) {
                    let response = self.aid(0xf4).await?;
                    detail(&response, None, "", "", ' ')?;
                    let at = self.captured.trace.len();
                    let response = self.aid(0xf5).await?;
                    list(
                        &response,
                        &[42, 43, 44, 45, 46, 47, 48, 49, 50, 51],
                        "00000001",
                        "",
                    )?;
                    navigation_refuse(
                        self.count(at, CicsOperation::Xctl) == 0,
                        "PF5 omitted XCTL",
                    )?;
                    let reads = self
                        .observed(at)
                        .iter()
                        .filter(|e| e.operation == CicsOperation::ReadNext)
                        .collect::<Vec<_>>();
                    navigation_refuse(
                        reads.len() != 12
                            || reads.iter().any(|e| {
                                e.outcome != "NORMAL"
                                    || e.response != 0
                                    || e.response2 != 0
                                    || e.payload_bytes != 350
                            })
                            || self.count(at, CicsOperation::StartBrowse) != 1
                            || self.count(at, CicsOperation::EndBrowse) != 1,
                        "PF5 requires discard, ten rows, lookahead and twelve normal reads",
                    )?;
                }
            }
            NavigationCase::Back => {
                self.first().await?;
                let response = self
                    .step(0x7d, BTreeMap::from([("SEL0002".into(), "S".into())]))
                    .await?;
                detail(&response, Some(42), &navigation_key(42), "", ' ')?;
                let at = self.captured.trace.len();
                let response = self.aid(0xf3).await?;
                list(&response, &FIRST, "00000001", "")?;
                navigation_refuse(
                    self.count(at, CicsOperation::Xctl) == 0,
                    "detail PF3 omitted list XCTL",
                )?;
                self.open_menu().await?;
                self.option(7).await?;
                let at = self.captured.trace.len();
                let response = self.aid(0xf3).await?;
                menu(&response)?;
                navigation_refuse(
                    self.count(at, CicsOperation::Xctl) == 0,
                    "detail PF3 omitted menu XCTL",
                )?;
            }
            NavigationCase::DetailRefusal => {
                for (input, message, missing, blank) in [
                    ("0000000000000099", "Transaction ID NOT found...", true, ' '),
                    ("", "Tran ID can NOT be empty...", false, ' '),
                ] {
                    self.open_menu().await?;
                    self.option(7).await?;
                    let at = self.captured.trace.len();
                    let response = self
                        .step(0x7d, BTreeMap::from([("TRNIDIN".into(), input.into())]))
                        .await?;
                    detail(&response, None, input, message, blank)?;
                    if missing {
                        self.require_read(at, "NOTFND", 13, 80)?;
                    } else {
                        navigation_refuse(
                            self.count(at, CicsOperation::Read) != 0,
                            "empty detail reached READ",
                        )?;
                    }
                }
            }
            NavigationCase::ListRefusal => {
                for (aid, fields, message) in [
                    (
                        0x7d,
                        BTreeMap::from([("TRNIDIN".into(), "NOT-A-NUMBER".into())]),
                        "Tran ID must be Numeric ...",
                    ),
                    (
                        0x7d,
                        BTreeMap::from([("SEL0001".into(), "X".into())]),
                        "Invalid selection. Valid value is S",
                    ),
                    (
                        0xf4,
                        BTreeMap::new(),
                        "Invalid key pressed. Please see below...",
                    ),
                ] {
                    self.first().await?;
                    let at = self.captured.trace.len();
                    let response = self.step(aid, fields).await?;
                    list(&response, &FIRST, "00000001", message)?;
                    navigation_refuse(
                        self.count(at, CicsOperation::Xctl) != 0,
                        "list refusal reached detail XCTL",
                    )?;
                }
            }
            NavigationCase::AddBack => {
                self.open_menu().await?;
                let response = self.option(8).await?;
                navigation_screen_identity(&response, "COTRN02", "COTRN2A", "COTRN02C", "CT02")?;
                let at = self.captured.trace.len();
                let response = self.aid(0xf3).await?;
                menu(&response)?;
                navigation_refuse(
                    self.count(at, CicsOperation::Xctl) == 0,
                    "add PF3 omitted menu XCTL",
                )?;
            }
        }
        Ok(())
    }
    fn require_read(&self, at: usize, tag: &str, resp: i32, rc2: i32) -> Result<(), CorpusProblem> {
        let reads = self
            .observed(at)
            .iter()
            .filter(|e| e.operation == CicsOperation::Read)
            .collect::<Vec<_>>();
        navigation_refuse(
            reads.len() != 1
                || reads[0].outcome != tag
                || reads[0].response != resp
                || reads[0].response2 != rc2
                || (resp == 0 && reads[0].payload_bytes != 350),
            "detail requires the actual exact keyed READ condition/payload",
        )
    }
}

pub(super) async fn compare_case(case: NavigationCase) -> Result<Capture, CorpusProblem> {
    let fixture = TransactionFixture::open_navigation_rows(navigation_rows()?).await?;
    let before = match snapshot(&fixture.server) {
        Ok(before) => before,
        Err(problem) => return fixture.finish(Err(problem)).await,
    };
    let mut caller = Caller::new(&fixture, before.clone());
    let result = async {
        validate_navigation_initial(&before)?;
        caller.exercise(case).await
    }
    .await;
    let after = snapshot(&fixture.server);
    let exported = after.and_then(|after| {
        fixture.export_case(
            &serde_json::json!(caller.captured.screens),
            &before,
            &after,
            &caller.captured.trace,
        )
    });
    let capture = caller.captured;
    fixture.finish(result.and(exported).map(|()| capture)).await
}

pub(super) async fn compare_navigation() -> Result<(), CorpusProblem> {
    // Every call performs its real route comparisons and awaits teardown before advancing.
    compare_case(NavigationCase::FirstPage).await?;
    compare_case(NavigationCase::Bottom).await?;
    compare_case(NavigationCase::Previous).await?;
    compare_case(NavigationCase::Selection).await?;
    compare_case(NavigationCase::Lookup).await?;
    compare_case(NavigationCase::Back).await?;
    compare_case(NavigationCase::Clear).await?;
    compare_case(NavigationCase::DetailRefusal).await?;
    compare_case(NavigationCase::ListRefusal).await?;
    compare_case(NavigationCase::AddBack).await?;
    Ok(())
}
