//! Genuine installed point composition; provider setup data grants no authority.
use super::setup::*;
use super::*;
use mainframe_env_execution_api::{AuditDecision, AuditSubjectRecord, ExecutionOutcome};
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_mq::MqTrustedBatchProducerSource;
use mainframe_env_store_api::{EffectState, ExecutionState, ExecutionStore, ProviderStateStore};
use std::sync::atomic::Ordering;
mod refusals;

pub(super) fn native(sqlite: bool, version: i32) -> Fixture {
    Fixture::native_points(
        sqlite,
        if version == 1 {
            include_bytes!("native_rich_md1.json")
        } else {
            include_bytes!("native_rich_md2.json")
        },
    )
}

pub(super) fn assert_receipts(f: &Fixture, actor: &Invocation, expected: usize) {
    let rows = f
        .store
        .list_provider_state("mq-selected-v1-occurrence", 128)
        .unwrap();
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| {
            serde_json::from_slice::<serde_json::Value>(&r.payload).unwrap()["value"]["execution"]
                == actor.execution_id.as_str()
        })
        .collect();
    assert_eq!(rows.len(), expected);
    let audits: Vec<_> = f
        .store
        .audit_subject_records(&actor.execution_id, 128)
        .unwrap()
        .into_iter()
        .filter_map(|a| match a {
            AuditSubjectRecord::Effect(value) if value.capability.as_str() == "host.mq.write" => {
                Some(value)
            }
            _ => None,
        })
        .collect();
    assert_eq!(audits.len(), expected * 2);
    for row in rows {
        let receipt: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let value = &receipt["value"];
        assert_eq!(value["execution"], actor.execution_id.as_str());
        let key = mainframe_env_execution_api::IdempotencyKey::new(
            value["key"].as_str().unwrap(),
            Default::default(),
        )
        .unwrap();
        let core = f.store.effect(&key).unwrap().unwrap();
        assert_eq!(core.state, EffectState::Completed);
        assert_eq!(core.execution_id, actor.execution_id);
        assert_eq!(
            serde_json::to_value(core.request_digest).unwrap(),
            value["request_digest"]
        );
        let pair: Vec<_> = audits
            .iter()
            .filter(|a| a.effect_sequence == core.sequence)
            .collect();
        assert_eq!(pair.len(), 2);
        assert!(
            pair.iter()
                .all(|a| a.principal == *actor.principal.id()
                    && a.decision == AuditDecision::Success)
        );
        assert_eq!(pair[0].resource, pair[1].resource);
    }
}

fn linked_close() -> &'static str {
    "IDENTIFICATION DIVISION. PROGRAM-ID. MQLEAF. DATA DIVISION. WORKING-STORAGE SECTION. 01 CO PIC S9(9) BINARY VALUE 0. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. LINKAGE SECTION. 01 HC PIC S9(9) BINARY. 01 HO PIC S9(9) BINARY. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING HC HO OUTCOME. MOVE 'DONE' TO OUTCOME. CALL 'MQCLOSE' USING HC HO CO CC RC. IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME END-IF. GOBACK."
}
fn cross_frame_source() -> String {
    include_str!("point.cbl")
        .replace("01 CC PIC", "01 OUTCOME PIC X(8).\n01 CC PIC")
        .replace("CALL 'MQCLOSE' USING HC HO CO CC RC.", "CALL 'MQLEAF' USING HC HO OUTCOME. IF OUTCOME NOT = 'DONE' DISPLAY 'BAD-CHILD' END-IF.")
}
#[test]
fn separately_compiled_child_closes_real_parent_object_without_retiring_parent_connection() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install("MQLEAF", linked_close());
        f.install("MQROOT", &cross_frame_source());
        let original = super::native_root::original(&f);
        let result = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        let ExecutionOutcome::Completed(done) = result else {
            panic!("{result:?}")
        };
        assert_eq!(done.output.bytes(), b"POINT-DONE\n");
        let child = f.factory.children.lock().unwrap()[0].clone();
        assert_receipts(&f, &original, 3);
        assert_receipts(&f, &child, 1);
        assert_eq!(
            f.store
                .get_execution(&child.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .abi_scope(&child)
                .is_err()
        );
    }
}

#[test]
fn genuine_compiled_native_root_open_close_disc_on_memory_owned_sqlite() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for access in [16, 2] {
                let f = native(sqlite, version);
                let source =
                    include_str!("point.cbl").replace("VALUE 16.", &format!("VALUE {access}."));
                f.install("MQROOT", &source);
                let original = super::native_root::original(&f);
                let before = original.clone();
                let outcome = f
                    .router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap();
                let ExecutionOutcome::Completed(done) = outcome else {
                    panic!("{outcome:?}")
                };
                assert_eq!(done.output.bytes(), b"POINT-DONE\n");
                assert_eq!(original, before);
                assert!(original.parent_execution_id.is_none());
                assert!(original.bindings.get("mq.host-context").is_none());
                assert_receipts(&f, &original, 4);
                let execution = f
                    .store
                    .get_execution(&original.execution_id)
                    .unwrap()
                    .unwrap();
                assert_eq!(execution.state, ExecutionState::Completed);
                if sqlite {
                    let reopened =
                        mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536)
                            .unwrap();
                    assert_eq!(
                        reopened.get_execution(&original.execution_id).unwrap(),
                        Some(execution)
                    );
                    assert_eq!(
                        reopened.list_provider_state_prefix("mq-", 4096).unwrap(),
                        f.rows()
                    );
                }
            }
        }
    }
}

#[test]
fn genuine_separately_compiled_same_task_child_uses_exact_root_abi_allocation() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install(
            "MQLEAF",
            &include_str!("point.cbl").replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
        );
        f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. DISPLAY 'ROOT-DONE'. GOBACK.");
        let original = super::native_root::original(&f);
        let before = original.clone();
        let outcome = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}; child executions={:?}; receipts={:?}",
            f.factory
                .children
                .lock()
                .unwrap()
                .iter()
                .map(|child| f.store.get_execution(&child.execution_id).unwrap())
                .collect::<Vec<_>>(),
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .iter()
                .map(
                    |r| serde_json::from_slice::<serde_json::Value>(&r.payload).unwrap()["value"]
                        .clone()
                )
                .collect::<Vec<_>>()
        );
        assert_eq!(original, before);
        let child = f.factory.children.lock().unwrap()[0].clone();
        assert_eq!(
            child.parent_execution_id.as_ref(),
            Some(&original.execution_id)
        );
        assert_receipts(&f, &child, 4);
        let map = f.mq.topology.lock().unwrap();
        let RootEntry::Retained { frame: root, .. } =
            map.roots.get(&original.execution_id).unwrap()
        else {
            panic!()
        };
        let FrameEntry::Retained(child_frame) = map.frames.get(&child.execution_id).unwrap() else {
            panic!()
        };
        assert_eq!(child_frame.original(), &child);
        assert!(Arc::ptr_eq(
            &root.abi.as_ref().unwrap().scope,
            &child_frame.abi.as_ref().unwrap().scope
        ));
        assert_eq!(
            root.abi.as_ref().unwrap().context,
            child_frame.abi.as_ref().unwrap().context
        );
        assert!(child_frame.check_original(&child).is_err());
    }
}

#[test]
fn actual_compiler_publishes_point_specimen_without_claiming_point_dispatch() {
    let f = Fixture::new(false);
    let before = f.rows();
    f.install("MQROOT", include_str!("point.cbl"));
    let admitted = f
        .router
        .cobol
        .preflight_installed_program("MQROOT", true)
        .unwrap();
    assert_eq!(admitted.name, "MQROOT");
    assert_eq!(f.rows(), before);
    assert!(f.saf.resources.lock().unwrap().is_empty());
    assert!(
        f.store
            .list_provider_state("mq-selected-v1-occurrence", 128)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn actual_admitted_compiled_metadata_is_charged_before_retained_root_clone() {
    let f = native(false, 1);
    f.install("MQROOT", include_str!("point.cbl"));
    let admitted = f
        .router
        .cobol
        .preflight_installed_program("MQROOT", true)
        .unwrap();
    let crate::cobol::artifact::AdmittedProgramProvenance::Catalog(catalog) = &admitted.provenance
    else {
        panic!()
    };
    let charge = super::super::frame::CompiledFrame::charge_parts(
        catalog,
        &admitted.metadata,
        &admitted.artifact,
        usize::MAX,
    )
    .unwrap();
    assert!(charge > std::mem::size_of_val(&admitted.metadata));
    assert_eq!(
        super::super::frame::CompiledFrame::charge_parts(
            catalog,
            &admitted.metadata,
            &admitted.artifact,
            charge
        ),
        Ok(charge)
    );
    assert_eq!(
        super::super::frame::CompiledFrame::charge_parts(
            catalog,
            &admitted.metadata,
            &admitted.artifact,
            charge - 1
        ),
        Err(HostProblem::ResourceExhausted)
    );
    let mut foreign = catalog.clone();
    foreign.namespace = "foreign-program".into();
    assert_eq!(
        super::super::frame::CompiledFrame::charge_parts(
            &foreign,
            &admitted.metadata,
            &admitted.artifact,
            usize::MAX
        ),
        Err(HostProblem::Unauthorized)
    );
    assert!(f.saf.resources.lock().unwrap().is_empty());
}

#[test]
fn native_setup_refuses_partial_fixture_without_rows_or_saf_changes() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let before = f.rows();
        assert!(
            ConfiguredInstalledMqHost::open_native_points(
                f.store.clone(),
                f.saf.clone(),
                f.clock.clone(),
                descriptor(),
                Default::default(),
                Default::default(),
                Default::default(),
                3,
                5,
                InstalledMqHostBounds {
                    max_roots: 8,
                    max_frames: 16
                },
            )
            .is_err()
        );
        assert_eq!(f.rows(), before);
        assert!(f.saf.resources.lock().unwrap().is_empty());
    }
}

#[test]
fn no_context_source_uses_bounded_character_authority_and_requires_live_host() {
    let store: Arc<dyn PlatformStore> =
        Arc::new(mainframe_env_store::MemoryStore::new(Default::default()));
    let source = super::super::native_point::Source::new(store, 4);
    assert_eq!(
        source
            .encode_structure("AB", MqMdCharacterEncoding::AsciiCompatible)
            .unwrap(),
        b"AB"
    );
    assert_eq!(
        source
            .encode_structure("AB", MqMdCharacterEncoding::OwnedCp037)
            .unwrap(),
        mainframe_env_encoding::CodePage::Cp037
            .encode("AB", 4)
            .unwrap()
    );
    assert_eq!(
        source.encode_structure("ABCDE", MqMdCharacterEncoding::AsciiCompatible),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(
        source.encode_structure("é", MqMdCharacterEncoding::AsciiCompatible),
        Err(HostProblem::Unsupported)
    );
    let original = crate::cobol::hardening::parent();
    assert_eq!(source.check_live(&original), Err(HostProblem::Unauthorized));
    assert!(matches!(
        source.physical_gmt(&original),
        Err(HostProblem::Unsupported)
    ));
    assert!(matches!(
        source.batch_context(&original),
        Err(HostProblem::Unsupported)
    ));
}
