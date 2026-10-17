//! Synthetic refusal controls only; no utility/application acceptance credit.
use super::*;
use mainframe_env_batch::{CancellationState, JesCancellation, JesJobKind, StepExecution};

fn reject_state(change: impl FnOnce(&mut LocalTables), phase: RowsPhase) {
    let mut actual = expected::tables(phase);
    change(&mut actual);
    assert!(require_tables(&actual, phase).is_err());
}

#[test]
fn all_named_definitions_and_three_independent_row_phases_accept_locally() {
    for phase in [RowsPhase::Empty, RowsPhase::Loaded, RowsPhase::Maintained] {
        require_tables(&expected::tables(phase), phase).unwrap();
    }
    assert!(require_tables(&expected::tables(RowsPhase::Loaded), RowsPhase::Empty).is_err());
    assert!(require_tables(&expected::tables(RowsPhase::Loaded), RowsPhase::Maintained).is_err());
}

#[test]
fn complete_ordered_metadata_shape_and_every_column_property_refuse_drift() {
    reject_state(
        |tables| {
            tables.0.pop();
        },
        RowsPhase::Loaded,
    );
    reject_state(|tables| tables.0.swap(0, 1), RowsPhase::Loaded);
    for table in 0..3 {
        reject_state(|tables| tables.0[table].0.name.push('X'), RowsPhase::Loaded);
        reject_state(
            |tables| {
                tables.0[table].0.columns.pop();
            },
            RowsPhase::Loaded,
        );
        reject_state(
            |tables| tables.0[table].0.columns.swap(0, 1),
            RowsPhase::Loaded,
        );
        for column in 0..expected::tables(RowsPhase::Loaded).0[table].0.columns.len() {
            reject_state(
                |tables| tables.0[table].0.columns[column].name.push('X'),
                RowsPhase::Loaded,
            );
            reject_state(
                |tables| {
                    let value = &mut tables.0[table].0.columns[column];
                    value.nullable = !value.nullable;
                },
                RowsPhase::Loaded,
            );
            reject_state(
                |tables| tables.0[table].0.columns[column].max_bytes += 1,
                RowsPhase::Loaded,
            );
            reject_state(
                |tables| {
                    let value = &mut tables.0[table].0.columns[column];
                    value.result_encoding = match value.result_encoding {
                        Db2ResultEncoding::Raw => Db2ResultEncoding::Varchar,
                        Db2ResultEncoding::Varchar => Db2ResultEncoding::Raw,
                    };
                },
                RowsPhase::Loaded,
            );
            reject_state(
                |tables| tables.0[table].0.columns[column].default_value = Some(vec![]),
                RowsPhase::Loaded,
            );
        }
    }
}

#[test]
fn primary_foreign_keys_and_extraction_layout_refuse_drift() {
    for table in 0..3 {
        reject_state(
            |tables| tables.0[table].0.primary_key.push("EXTRA".into()),
            RowsPhase::Loaded,
        );
        reject_state(
            |tables| {
                tables.0[table].0.primary_key.pop();
            },
            RowsPhase::Loaded,
        );
    }
    reject_state(
        |tables| tables.0[1].0.primary_key.swap(0, 1),
        RowsPhase::Loaded,
    );
    reject_state(
        |tables| tables.0[1].0.foreign_keys.clear(),
        RowsPhase::Loaded,
    );
    reject_state(
        |tables| tables.0[1].0.foreign_keys[0].delete_restrict = false,
        RowsPhase::Loaded,
    );
    reject_state(
        |tables| tables.0[1].0.foreign_keys[0].columns[0] = "TRC_TYPE_CATEGORY".into(),
        RowsPhase::Loaded,
    );
    reject_state(
        |tables| tables.0[1].0.foreign_keys[0].referenced_table.push('X'),
        RowsPhase::Loaded,
    );
    reject_state(
        |tables| tables.0[1].0.foreign_keys[0].referenced_columns[0].push('X'),
        RowsPhase::Loaded,
    );
    for table in 0..2 {
        reject_state(|tables| tables.0[table].0.extract = None, RowsPhase::Loaded);
        reject_state(
            |tables| {
                tables.0[table]
                    .0
                    .extract
                    .as_mut()
                    .unwrap()
                    .fields
                    .swap(0, 1)
            },
            RowsPhase::Loaded,
        );
        reject_state(
            |tables| tables.0[table].0.extract.as_mut().unwrap().fields[0].width += 1,
            RowsPhase::Loaded,
        );
        reject_state(
            |tables| {
                tables.0[table].0.extract.as_mut().unwrap().fields[0]
                    .column
                    .push('X')
            },
            RowsPhase::Loaded,
        );
        reject_state(
            |tables| tables.0[table].0.extract.as_mut().unwrap().trailer[0] ^= 1,
            RowsPhase::Loaded,
        );
    }
}

#[test]
fn complete_independent_rows_reject_tail_category_order_column_and_auth_drift() {
    reject_state(
        |tables| {
            tables.0[0].1.pop();
        },
        RowsPhase::Maintained,
    );
    reject_state(
        |tables| {
            let duplicate = tables.0[0].1[0].clone();
            tables.0[0].1.push(duplicate);
        },
        RowsPhase::Maintained,
    );
    reject_state(
        |tables| tables.0[0].1[1][1] = b"LEFT".to_vec(),
        RowsPhase::Maintained,
    );
    reject_state(|tables| tables.0[0].1[6][1][0] ^= 1, RowsPhase::Maintained);
    reject_state(|tables| tables.0[0].1.swap(0, 1), RowsPhase::Maintained);
    reject_state(
        |tables| {
            tables.0[1].1[17].pop();
        },
        RowsPhase::Maintained,
    );
    reject_state(|tables| tables.0[1].1[17][2][0] ^= 1, RowsPhase::Maintained);
    reject_state(
        |tables| tables.0[1].1[0].push(vec![]),
        RowsPhase::Maintained,
    );
    reject_state(
        |tables| tables.0[2].1.push(vec![b"unexpected".to_vec()]),
        RowsPhase::Maintained,
    );
    // Two matching retained snapshots do not substitute for the source oracle.
    let mut wrong = expected::tables(RowsPhase::Maintained);
    wrong.0[1].1[17][2][0] ^= 1;
    let reopened = wrong.clone();
    assert_eq!(wrong, reopened);
    assert!(require_tables(&reopened, RowsPhase::Maintained).is_err());
}

fn job(selected: UtilityJob) -> JobSnapshot {
    let names: &[&str] = match selected {
        UtilityJob::Create => &["FREEPLN", "CRCRDDB", "LDTTYPE", "RUNTEP2", "LDTCCAT"],
        UtilityJob::Extract => &["STEP10", "STEP20", "STEP30", "STEP40", "STEP50"],
    };
    JobSnapshot {
        id: "JOB00001".into(),
        name: match selected {
            UtilityJob::Create => "CREADB2",
            UtilityJob::Extract => "TRANEXTR",
        }
        .into(),
        owner: "IBMUSER".into(),
        class: 'A',
        priority: 1,
        state: JobState::Completed,
        return_code: Some(0),
        abend_code: None,
        active_step: None,
        initiator: None,
        steps: names
            .iter()
            .map(|name| StepExecution {
                name: (*name).into(),
                state: StepState::Completed,
                attempt: 1,
                termination: Some(StepTermination::ReturnCode { code: 0 }),
            })
            .collect(),
        attempt: 1,
        version: 2,
        kind: Default::default(),
        origin: Default::default(),
        route: Default::default(),
        cancellation: None,
    }
}

fn reject_job(change: impl FnOnce(&mut JobSnapshot), selected: UtilityJob) {
    let mut actual = job(selected);
    change(&mut actual);
    assert!(require_job("JOB00001", &actual, selected).is_err());
}

#[test]
fn both_actual_job_shapes_require_original_identity_and_terminal_success() {
    for selected in [UtilityJob::Create, UtilityJob::Extract] {
        require_job("JOB00001", &job(selected), selected).unwrap();
        assert!(require_job("", &job(selected), selected).is_err());
        reject_job(|job| job.id.push('X'), selected);
        reject_job(|job| job.name.push('X'), selected);
        reject_job(|job| job.owner = "WEBUSER".into(), selected);
        reject_job(|job| job.attempt += 1, selected);
        reject_job(|job| job.version = 0, selected);
        reject_job(|job| job.return_code = None, selected);
        reject_job(|job| job.return_code = Some(4), selected);
        reject_job(|job| job.abend_code = Some("S0C7".into()), selected);
        reject_job(|job| job.active_step = Some("RUNNING".into()), selected);
        reject_job(|job| job.kind = JesJobKind::StartedTask, selected);
        reject_job(
            |job| {
                job.cancellation = Some(JesCancellation {
                    id: "cancelled".into(),
                    requested_by: "IBMUSER".into(),
                    reason: "synthetic refusal control".into(),
                    requested_tick: 1,
                    state: CancellationState::Completed,
                })
            },
            selected,
        );
        for state in [
            JobState::Submitted,
            JobState::Held,
            JobState::Queued,
            JobState::Selected,
            JobState::Running,
            JobState::Output,
            JobState::Failed,
            JobState::Cancelled,
        ] {
            reject_job(|job| job.state = state, selected);
        }
    }
}

#[test]
fn all_steps_require_complete_source_order_attempt_and_exact_return_code() {
    for selected in [UtilityJob::Create, UtilityJob::Extract] {
        reject_job(
            |job| {
                job.steps.pop();
            },
            selected,
        );
        reject_job(
            |job| {
                let duplicate = job.steps[0].clone();
                job.steps.push(duplicate);
            },
            selected,
        );
        reject_job(|job| job.steps.swap(0, 1), selected);
        for index in 0..5 {
            reject_job(|job| job.steps[index].name.push('X'), selected);
            reject_job(|job| job.steps[index].attempt = 0, selected);
            reject_job(|job| job.steps[index].attempt = 2, selected);
            reject_job(|job| job.steps[index].termination = None, selected);
            reject_job(
                |job| job.steps[index].termination = Some(StepTermination::ReturnCode { code: 4 }),
                selected,
            );
            for state in [
                StepState::Pending,
                StepState::BypassedRestart,
                StepState::SkippedCondition,
                StepState::Allocating,
                StepState::Running,
                StepState::Disposing,
                StepState::Abended,
                StepState::Failed,
                StepState::Cancelled,
            ] {
                reject_job(|job| job.steps[index].state = state, selected);
            }
        }
    }
}

#[test]
fn selected_spool_descriptor_refuses_missing_duplicate_record_and_byte_drift() {
    for selected in [UtilityJob::Create, UtilityJob::Extract] {
        for (file, records) in selected.spool() {
            let descriptor = (
                0,
                (*file).into(),
                records.len(),
                byte_count(records).unwrap(),
            );
            require_descriptor(std::slice::from_ref(&descriptor), file, records).unwrap();
            assert!(require_descriptor(&[], file, records).is_err());
            assert!(
                require_descriptor(&[descriptor.clone(), descriptor.clone()], file, records)
                    .is_err()
            );
            let mut wrong = descriptor.clone();
            wrong.1 = "SYSPRINT".into();
            assert!(require_descriptor(&[wrong], file, records).is_err());
            let mut wrong = descriptor.clone();
            wrong.2 += 1;
            assert!(require_descriptor(&[wrong], file, records).is_err());
            let mut wrong = descriptor;
            wrong.3 += 1;
            assert!(require_descriptor(&[wrong], file, records).is_err());
        }
    }
}

#[test]
fn every_selected_spool_refuses_incomplete_missing_extra_or_changed_bytes() {
    for selected in [UtilityJob::Create, UtilityJob::Extract] {
        for (_, expected) in selected.spool() {
            let records = expected
                .iter()
                .map(|record| record.to_vec())
                .collect::<Vec<_>>();
            require_records(&records, false, expected).unwrap();
            assert!(require_records(&records, true, expected).is_err());
            let mut wrong = records.clone();
            wrong.pop();
            assert!(require_records(&wrong, false, expected).is_err());
            let mut wrong = records.clone();
            wrong.push(b"extra".to_vec());
            assert!(require_records(&wrong, false, expected).is_err());
            let mut wrong = records;
            wrong[0][0] ^= 1;
            assert!(require_records(&wrong, false, expected).is_err());
        }
    }
    let wrong_count = vec![b"IKJEFT01 DSNTEP4 SQLCODE=0 ROWS=7".to_vec()];
    assert!(require_records(&wrong_count, false, &[b"IKJEFT01 DSNTEP4 SQLCODE=0 ROWS=0"]).is_err());
}

#[test]
fn control_member_tail_and_source_identity_changes_refuse() {
    let source = b"DSN SYSTEM(DAZ1)\nRUN PROGRAM(DSNTEP4)\n";
    let records = vec![
        b"DSN SYSTEM(DAZ1)".to_vec(),
        b"RUN PROGRAM(DSNTEP4)".to_vec(),
    ];
    require_control_records(&records, source).unwrap();
    let mut wrong = records.clone();
    wrong.pop();
    assert!(require_control_records(&wrong, source).is_err());
    let mut wrong = records.clone();
    wrong[1].push(b'X');
    assert!(require_control_records(&wrong, source).is_err());
    let mut wrong = records;
    wrong.swap(0, 1);
    assert!(require_control_records(&wrong, source).is_err());
    assert!(require_control_records(&[], b"\xff").is_err());
    let identity = format!("{:x}", Sha256::digest(source));
    require_source_identity(source, &identity).unwrap();
    assert!(require_source_identity(b"changed", &identity).is_err());
    assert!(require_source_identity(source, &"0".repeat(64)).is_err());
}

#[test]
fn changed_terminal_owner_snapshot_refuses_even_with_exact_spool() {
    let before = job(UtilityJob::Create);
    require_stable_job(&before, &before).unwrap();
    let mut after = before.clone();
    after.version += 1;
    require_job("JOB00001", &after, UtilityJob::Create).unwrap();
    assert!(require_stable_job(&before, &after).is_err());
    let mut after = before.clone();
    after.route.output_node = "REMOTE".into();
    assert!(require_stable_job(&before, &after).is_err());
}
