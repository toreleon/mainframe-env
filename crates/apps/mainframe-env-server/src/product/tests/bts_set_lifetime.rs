//! Row 0086 root-only selected-route diagnostics; no official or recovery credit.
//!
//! IBM CICS TS 6.x sources-a baseline
//! ibm-cics-ts-6x-application-api-sources-a-2026-09-10:
//! commands-bts/dfhp4_getcontainerbts.html, SHA-256
//! e486de8019489b85dbcf0b0f343849d0cc088a331c03054db7329aa1da58ad16,
//! SET lines 81–88, FLENGTH 41–68, CONTAINERERR 94–97.
//! commands-api/dfhp4_getcontainerchannel.html, SHA-256
//! e699b5c003cedd015c05ec116bd05e5cdd46e8fce2f0bd070559555984386674,
//! lines 162–178 define a different pair/scope lifetime. Every GET below uses
//! explicit PROCESS with no current channel; no cross-variant timing is inferred.
//! Expiry is observed through compiled LINKAGE reads. DATA-EXCEPTION is the
//! existing checked interpreter outcome, not a claimed IBM abend/RESP mapping.

use super::*;
use mainframe_env_cics::bts_lifecycle::{
    BtsProcessTypeDefinition, BtsReply, BtsTransactionDefinition,
};

const FIRST_LENGTH: i32 = 65_536;
const FIRST_PREFIX: &[u8] = b"AAAAAAAA";
const SECOND_DATA: &[u8] = b"BETA456!";

#[test]
fn compiled_bts_set_dereferences_pointer_and_returns_fullword_flength() {
    let route = RootRoute::new("");
    let machine = route.checkpoint();
    assert_first_read(&machine);
    assert!(bytes(&machine, "PTR-A").iter().any(|byte| *byte != 0));
}

#[test]
fn compiled_bts_into_preserves_existing_set_linkage() {
    let route = RootRoute::new(
        "EXEC CICS GET CONTAINER('TWO') PROCESS INTO(INTO-X) FLENGTH(INTO-LEN) \
         RESP(RESP-X) RESP2(RESP2-X) END-EXEC. MOVE LINK-X TO AFTER-X.",
    );
    let machine = route.checkpoint();
    assert_first_read(&machine);
    assert_eq!(bytes(&machine, "INTO-X"), SECOND_DATA);
    assert_fullword(&machine, "INTO-LEN", 8);
    assert_fullword(&machine, "RESP-X", 0);
    assert_fullword(&machine, "RESP2-X", 0);
    assert_eq!(bytes(&machine, "AFTER-X"), FIRST_PREFIX);
    assert!(bytes(&machine, "PTR-B").iter().all(|byte| *byte == 0));
}

#[test]
fn compiled_bts_nodata_preserves_existing_set_linkage() {
    let route = RootRoute::new(
        "EXEC CICS GET CONTAINER('ONE') PROCESS NODATA FLENGTH(NODATA-LEN) \
         RESP(RESP-X) RESP2(RESP2-X) END-EXEC. MOVE LINK-X TO AFTER-X.",
    );
    let machine = route.checkpoint();
    assert_first_read(&machine);
    assert_fullword(&machine, "NODATA-LEN", FIRST_LENGTH);
    assert_fullword(&machine, "RESP-X", 0);
    assert_fullword(&machine, "RESP2-X", 0);
    assert_eq!(bytes(&machine, "AFTER-X"), FIRST_PREFIX);
    assert_eq!(bytes(&machine, "INTO-X"), b"ZZZZZZZZ");
    assert!(bytes(&machine, "PTR-B").iter().all(|byte| *byte == 0));
}

#[test]
fn compiled_bts_missing_set_reports_source_condition_without_fresh_outputs() {
    let route = RootRoute::new(missing_set());
    assert_missing_result(&route.checkpoint());
    // Do not dereference the prior loan in this control: that is the red
    // diagnostic below, not a permitted lifetime after the observed issue.
}

#[test]
fn compiled_bts_set_different_container_expires_previous_linkage() {
    let mut route = RootRoute::new(
        "EXEC CICS GET CONTAINER('TWO') PROCESS SET(PTR-B) FLENGTH(LEN-B) \
         RESP(RESP-X) RESP2(RESP2-X) END-EXEC. \
         SET ADDRESS OF SECOND-LINK TO PTR-B. MOVE SECOND-LINK TO SECOND-X.",
    );
    let machine = route.checkpoint();
    assert_first_read(&machine);
    assert_eq!(bytes(&machine, "SECOND-X"), SECOND_DATA);
    assert_fullword(&machine, "LEN-B", 8);
    assert_fullword(&machine, "RESP-X", 0);
    assert_fullword(&machine, "RESP2-X", 0);
    assert_ne!(bytes(&machine, "PTR-A"), bytes(&machine, "PTR-B"));
    route.assert_prior_linkage_expired();
}

#[test]
fn compiled_bts_missing_set_issue_expires_previous_linkage() {
    let mut route = RootRoute::new(missing_set());
    assert_missing_result(&route.checkpoint());
    route.assert_prior_linkage_expired();
}

fn missing_set() -> &'static str {
    "MOVE PTR-A TO PTR-B. \
     EXEC CICS GET CONTAINER('MISSING') PROCESS SET(PTR-B) FLENGTH(LEN-B) \
     RESP(RESP-X) RESP2(RESP2-X) END-EXEC."
}

fn assert_missing_result(machine: &ReferenceMachine) {
    assert_first_read(machine);
    // This proves that a real, handled command condition was observed before
    // the expiry probe. It says nothing about rejected/unissued/cancelled calls.
    assert_fullword(machine, "RESP-X", 110);
    assert_fullword(machine, "RESP2-X", 10);
    assert_fullword(machine, "GET-EIBRESP", 110);
    assert_fullword(machine, "GET-EIBRESP2", 10);
    assert_eq!(bytes(machine, "GET-FN"), [0x34, 0x14]);
    assert_eq!(bytes(machine, "PTR-B"), bytes(machine, "PTR-A"));
    assert_fullword(machine, "LEN-B", 77);
    assert_eq!(bytes(machine, "SECOND-X"), b"SSSSSSSS");
}

fn assert_first_read(machine: &ReferenceMachine) {
    assert_eq!(bytes(machine, "FIRST-X"), FIRST_PREFIX);
    assert_fullword(machine, "LEN-A", FIRST_LENGTH);
    assert_fullword(machine, "FIRST-RESP", 0);
    assert_fullword(machine, "FIRST-RESP2", 0);
}

fn bytes(machine: &ReferenceMachine, name: &str) -> Vec<u8> {
    machine.variable(name).unwrap().bytes().to_vec()
}

fn assert_fullword(machine: &ReferenceMachine, name: &str, expected: i32) {
    assert_eq!(bytes(machine, name), expected.to_be_bytes(), "{name}");
}

struct RootRoute {
    server: Arc<ProductServer>,
    artifact: PublishedArtifact,
    session: SessionId,
    principal: PrincipalId,
    now: u64,
    artifact_root: std::path::PathBuf,
}

impl RootRoute {
    fn new(after_first_set: &str) -> Self {
        // Independent seed values are encoded in the compiled fixture. The
        // expected length crosses both signed and unsigned halfword bounds;
        // it is never derived from the provider response or restored storage.
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. BTSSET. \
             DATA DIVISION. WORKING-STORAGE SECTION. \
             01 DATA-A PIC X(65536) VALUE ALL 'A'. \
             01 DATA-B PIC X(8) VALUE 'BETA456!'. \
             01 PTR-A POINTER. 01 PTR-B POINTER. \
             01 LEN-A PIC S9(9) COMP VALUE -999. \
             01 LEN-B PIC S9(9) COMP VALUE 77. \
             01 INTO-LEN PIC S9(9) COMP VALUE 8. \
             01 NODATA-LEN PIC S9(9) COMP VALUE -999. \
             01 RESP-X PIC S9(9) COMP VALUE -1. \
             01 RESP2-X PIC S9(9) COMP VALUE -1. \
             01 FIRST-RESP PIC S9(9) COMP VALUE -1. \
             01 FIRST-RESP2 PIC S9(9) COMP VALUE -1. \
             01 FIRST-X PIC X(8) VALUE ALL 'F'. \
             01 SECOND-X PIC X(8) VALUE ALL 'S'. \
             01 AFTER-X PIC X(8) VALUE ALL 'Q'. \
             01 INTO-X PIC X(8) VALUE ALL 'Z'. \
             01 GET-FN PIC X(2). \
             01 GET-EIBRESP PIC S9(9) COMP. 01 GET-EIBRESP2 PIC S9(9) COMP. \
             LINKAGE SECTION. 01 LINK-X PIC X(8). 01 SECOND-LINK PIC X(8). \
             PROCEDURE DIVISION. \
             EXEC CICS DEFINE PROCESS('SETROOT') PROCESSTYPE('TYPE') \
             TRANSID('BS01') NOCHECK END-EXEC. EXEC CICS SUSPEND END-EXEC. \
             EXEC CICS PUT CONTAINER('ONE') PROCESS FROM(DATA-A) END-EXEC. \
             EXEC CICS PUT CONTAINER('TWO') PROCESS FROM(DATA-B) END-EXEC. \
             EXEC CICS GET CONTAINER('ONE') PROCESS SET(PTR-A) FLENGTH(LEN-A) \
             RESP(FIRST-RESP) RESP2(FIRST-RESP2) END-EXEC. \
             SET ADDRESS OF LINK-X TO PTR-A. MOVE LINK-X TO FIRST-X. \
             {after_first_set} MOVE EIBFN TO GET-FN. \
             MOVE EIBRESP TO GET-EIBRESP. MOVE EIBRESP2 TO GET-EIBRESP2. \
             EXEC CICS SUSPEND END-EXEC. \
             MOVE LINK-X TO AFTER-X. EXEC CICS SUSPEND END-EXEC. STOP RUN."
        );
        let artifact = published_source_fixture("BTSSET", &source);
        let mut settings = config();
        settings.artifact_root = std::env::temp_dir().join(format!(
            "mainframe-bts-set-root-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let artifact_root = settings.artifact_root.clone();
        let server = ProductServer::memory(settings).unwrap();
        server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
        server
            .install_online_application(OnlineApplicationDefinition {
                programs: vec![OnlineProgramDefinition::current("BTSSET", &artifact)],
                transactions: BTreeMap::from([("BS01".into(), "BTSSET".into())]),
                maps: vec![BmsMapDefinition {
                    mapset: "BTSSET".into(),
                    map: "BTSSET".into(),
                    line: 1,
                    column: 1,
                    rows: 24,
                    columns: 80,
                    fields: Vec::new(),
                }],
            })
            .unwrap();
        server
            .cics
            .register_bts_process_type(
                BtsProcessTypeDefinition::new("TYPE", "BTS.REPO", true).unwrap(),
            )
            .unwrap();
        server
            .cics
            .register_bts_transaction(
                BtsTransactionDefinition::new("BS01", "BTSSET", true, false).unwrap(),
            )
            .unwrap();
        let resource = BtsLifecycleStore::saf_resource("TYPE", "SETROOT").unwrap();
        for (class, name) in [("BTSREPO", "BTS.REPO"), ("BTSLIFE", resource.as_str())] {
            server
                .racf
                .define_profile(class, name, "IBMUSER", None)
                .unwrap();
            for intent in [AccessIntent::Read, AccessIntent::Update] {
                server.racf.permit(class, name, "IBMUSER", intent).unwrap();
            }
        }
        let invocation = server
            .cics_invocation(
                "IBMUSER",
                "BS01",
                Some(
                    ArtifactRef::new(
                        artifact.content_id().to_reference(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        assert!(!invocation.bindings.contains_key("cics.channel"));
        let session = SessionId::new("bts-set-root", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "BS01",
                24,
                80,
                "bts-set-csrf",
                1,
                10_000,
            )
            .unwrap();
        server
            .run_online_exchange(&session, &principal, "BTSSET", 2)
            .unwrap();
        let authority = BtsLifecycleStore::new(server.store.as_ref());
        let root_id = authority
            .load_process("TYPE", "SETROOT")
            .unwrap()
            .unwrap()
            .root_id;
        authority
            .mutate_process(
                "TYPE",
                "SETROOT",
                invocation.run_unit_id.as_str(),
                invocation.execution_id.as_str(),
                "IBMUSER",
                "activate-set-root",
                [0x51; 32],
                |process| {
                    process.start(&root_id, None, true)?;
                    process.checkpoint(&root_id, 1, 7, "root-set-checkpoint")?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        server
            .cics
            .bind_bts_activity_context(&invocation.run_unit_id, "TYPE", "SETROOT", &root_id, 1, 7)
            .unwrap();
        let mut route = Self {
            server,
            artifact,
            session,
            principal,
            now: 2,
            artifact_root,
        };
        route.resume().unwrap();
        route
    }

    fn resume(&mut self) -> Result<(), HostProblem> {
        self.now += 1;
        self.server
            .run_online_exchange(&self.session, &self.principal, "BTSSET", self.now)
    }

    fn checkpoint(&self) -> ReferenceMachine {
        let saved = self
            .server
            .online_machine_continuation(&self.session)
            .unwrap()
            .unwrap();
        let exchange = self.server.online_exchange(&self.session).unwrap().unwrap();
        let mut machine = ReferenceMachine::from_binary(
            self.artifact.payload(),
            self.server.online_exchange_invocation(&exchange).unwrap(),
            CodecLimits::default(),
        )
        .unwrap();
        machine.restore_checkpoint(&saved.checkpoint).unwrap();
        machine
    }

    fn assert_prior_linkage_expired(&mut self) {
        let result = self.resume();
        if result.is_ok() {
            eprintln!(
                "baseline unexpectedly read expired BTS LINKAGE: AFTER-X={:?}",
                bytes(&self.checkpoint(), "AFTER-X")
            );
        }
        assert_eq!(
            result,
            Err(HostProblem::Condition {
                name: "DATA-EXCEPTION".into(),
                response: 2,
                response2: 0
            }),
            "ordinary compiled MOVE through the prior SET LINKAGE must be rejected"
        );
    }
}

impl Drop for RootRoute {
    fn drop(&mut self) {
        // This is a task-specific artifact directory, never a Cargo target.
        std::fs::remove_dir_all(&self.artifact_root).unwrap();
    }
}
