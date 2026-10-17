//! Grounded first-party COBTUPDT comparison and zero-credit rollback admission.
// Pin: CardDemo59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e. Complete raw
// expected SQL columns came only from DB2LTTYP/DB2LTCAT literals. Before
// maintenance, the existing successful LEFT control sets type02=LEFT; the
// three fixed53 controlled inputs require final type02=BATCH PAYMENT.
// Source DISPLAY processing echoes/success literals bind each real command.
// The pending SELECT oracle uses DCLTRTYP's binary-halfword length and X(50)
// capacity plus the owned declared big-endian VARCHAR ABI: literal length8,
// ROLLBACK and42 spaces. No runtime result/provider encoder supplies expected
// bytes. This tests local profile ABI, withholding IBM/COBOL licensed credit.
use super::*;
use mainframe_env_batch::JobSnapshot;
use mainframe_env_host_api::Db2Result;

const SOURCE_INPUTS: &[(&str, &str)] = &[
    (
        "app/app-transaction-type-db2/cbl/COBTUPDT.cbl",
        "0213fd5718c6aadd3fba6bbfb1818456d75d10843181bff63aca9ff57d39e87c",
    ),
    (
        "app/app-transaction-type-db2/jcl/MNTTRDB2.jcl",
        "9e8a11746c47ea762258e8b5d96a6852f35e7dce4ea456a31a2e9f76f127040e",
    ),
    (
        "app/app-transaction-type-db2/dcl/DCLTRTYP.dcl",
        "4c61f60bdf03ba3cdca9a45eeaae21b645640df270751c884f350db3a8ad2677",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTTYP.ctl",
        "65d7c9b86b82d6b85bf50bfa18740c40353104148d1c3ce33dafc1a1a114fd6f",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTCAT.ctl",
        "1e0d56c17d52a71b36e7e52c261cb992394ae6f66be35e0f223b030ae5354602",
    ),
];

const SEED_TYPES: [[&[u8]; 2]; 7] = [
    [b"\x30\x31", b"\x50\x55\x52\x43\x48\x41\x53\x45"],
    [b"\x30\x32", b"\x50\x41\x59\x4d\x45\x4e\x54"],
    [b"\x30\x33", b"\x43\x52\x45\x44\x49\x54"],
    [
        b"\x30\x34",
        b"\x41\x55\x54\x48\x4f\x52\x49\x5a\x41\x54\x49\x4f\x4e",
    ],
    [b"\x30\x35", b"\x52\x45\x46\x55\x4e\x44"],
    [b"\x30\x36", b"\x52\x45\x56\x45\x52\x41\x4c"],
    [b"\x30\x37", b"\x41\x44\x4a\x55\x53\x54\x4d\x45\x4e\x54"],
];

const SEED_CATEGORIES: [[&[u8]; 3]; 18] = [
    [b"\x30\x31", b"\x30\x30\x30\x31", b"\x52\x45\x47\x55\x4c\x41\x52\x20\x53\x41\x4c\x45\x53\x20\x44\x52\x41\x46\x54"],
    [b"\x30\x31", b"\x30\x30\x30\x32", b"\x52\x45\x47\x55\x4c\x41\x52\x20\x43\x41\x53\x48\x20\x41\x44\x56\x41\x4e\x43\x45"],
    [b"\x30\x31", b"\x30\x30\x30\x33", b"\x43\x4f\x4e\x56\x45\x4e\x49\x45\x4e\x43\x45\x20\x43\x48\x45\x43\x4b\x20\x44\x45\x42\x49\x54"],
    [b"\x30\x31", b"\x30\x30\x30\x34", b"\x41\x54\x4d\x20\x43\x41\x53\x48\x20\x41\x44\x56\x41\x4e\x43\x45"],
    [b"\x30\x31", b"\x30\x30\x30\x35", b"\x49\x4e\x54\x45\x52\x45\x53\x54\x20\x41\x4d\x4f\x55\x4e\x54"],
    [b"\x30\x32", b"\x30\x30\x30\x31", b"\x43\x41\x53\x48\x20\x50\x41\x59\x4d\x45\x4e\x54"],
    [b"\x30\x32", b"\x30\x30\x30\x32", b"\x45\x4c\x45\x43\x54\x52\x4f\x4e\x49\x43\x20\x50\x41\x59\x4d\x45\x4e\x54"],
    [b"\x30\x32", b"\x30\x30\x30\x33", b"\x43\x48\x45\x43\x4b\x20\x50\x41\x59\x4d\x45\x4e\x54"],
    [b"\x30\x33", b"\x30\x30\x30\x31", b"\x43\x52\x45\x44\x49\x54\x20\x54\x4f\x20\x41\x43\x43\x4f\x55\x4e\x54"],
    [b"\x30\x33", b"\x30\x30\x30\x32", b"\x43\x52\x45\x44\x49\x54\x20\x54\x4f\x20\x50\x55\x52\x43\x48\x41\x53\x45\x20\x42\x41\x4c\x41\x4e\x43\x45"],
    [b"\x30\x33", b"\x30\x30\x30\x33", b"\x43\x52\x45\x44\x49\x54\x20\x54\x4f\x20\x43\x41\x53\x48\x20\x42\x41\x4c\x41\x4e\x43\x45"],
    [b"\x30\x34", b"\x30\x30\x30\x31", b"\x5a\x45\x52\x4f\x20\x44\x4f\x4c\x4c\x41\x52\x20\x41\x55\x54\x48\x4f\x52\x49\x5a\x41\x54\x49\x4f\x4e"],
    [b"\x30\x34", b"\x30\x30\x30\x32", b"\x4f\x4e\x4c\x49\x4e\x45\x20\x50\x55\x52\x43\x48\x41\x53\x45\x20\x41\x55\x54\x48\x4f\x52\x49\x5a\x41\x54\x49\x4f\x4e"],
    [b"\x30\x34", b"\x30\x30\x30\x33", b"\x54\x52\x41\x56\x45\x4c\x20\x42\x4f\x4f\x4b\x49\x4e\x47\x20\x41\x55\x54\x48\x4f\x52\x49\x5a\x41\x54\x49\x4f\x4e"],
    [b"\x30\x35", b"\x30\x30\x30\x31", b"\x52\x45\x46\x55\x4e\x44\x20\x43\x52\x45\x44\x49\x54"],
    [b"\x30\x36", b"\x30\x30\x30\x31", b"\x46\x52\x41\x55\x44\x20\x52\x45\x56\x45\x52\x53\x41\x4c"],
    [b"\x30\x36", b"\x30\x30\x30\x32", b"\x4e\x4f\x4e\x20\x46\x52\x41\x55\x44\x20\x52\x45\x56\x45\x52\x53\x41\x4c"],
    [b"\x30\x37", b"\x30\x30\x30\x31", b"\x53\x41\x4c\x45\x53\x20\x44\x52\x41\x46\x54\x20\x43\x52\x45\x44\x49\x54\x20\x41\x44\x4a\x55\x53\x54\x4d\x45\x4e\x54"],
];

const CONTROLLED_INPUT: [&[u8]; 3] = [
    b"\x41\x39\x38\x42\x41\x54\x43\x48\x20\x49\x4e\x53\x45\x52\x54\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
    b"\x55\x30\x32\x42\x41\x54\x43\x48\x20\x50\x41\x59\x4d\x45\x4e\x54\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
    b"\x44\x39\x38\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
];

const EXPECTED_COMMAND_RESULTS: [&[u8]; 6] = [
    b"\x50\x52\x4f\x43\x45\x53\x53\x49\x4e\x47\x20\x20\x20\x41\x39\x38\x42\x41\x54\x43\x48\x20\x49\x4e\x53\x45\x52\x54\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
    b"\x52\x45\x43\x4f\x52\x44\x20\x49\x4e\x53\x45\x52\x54\x45\x44\x20\x53\x55\x43\x43\x45\x53\x53\x46\x55\x4c\x4c\x59",
    b"\x50\x52\x4f\x43\x45\x53\x53\x49\x4e\x47\x20\x20\x20\x55\x30\x32\x42\x41\x54\x43\x48\x20\x50\x41\x59\x4d\x45\x4e\x54\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
    b"\x52\x45\x43\x4f\x52\x44\x20\x55\x50\x44\x41\x54\x45\x44\x20\x53\x55\x43\x43\x45\x53\x53\x46\x55\x4c\x4c\x59",
    b"\x50\x52\x4f\x43\x45\x53\x53\x49\x4e\x47\x20\x20\x20\x44\x39\x38\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20",
    b"\x52\x45\x43\x4f\x52\x44\x20\x44\x45\x4c\x45\x54\x45\x44\x20\x53\x55\x43\x43\x45\x53\x53\x46\x55\x4c\x4c\x59",
];

const EXPECTED_PENDING_DESCRIPTION: &[u8] = b"\x00\x08\x52\x4f\x4c\x4c\x42\x41\x43\x4b\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20";

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Tables {
    types: Vec<Vec<Vec<u8>>>,
    categories: Vec<Vec<Vec<u8>>>,
}

fn drift(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.control_comparison_drift", detail)
}

pub(super) fn check_inputs(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, expected) in SOURCE_INPUTS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *expected {
            return Err(CorpusProblem::new(
                "carddemo.db2.control_source_drift",
                *path,
            ));
        }
    }
    Ok(())
}

pub(super) fn capture(server: &ProductServer) -> Result<Tables, CorpusProblem> {
    Ok(Tables {
        types: server
            .db2_service()
            .table_rows("CARDDEMO.TRANSACTION_TYPE")
            .map_err(terminal_problem)?,
        categories: server
            .db2_service()
            .table_rows("CARDDEMO.TRANSACTION_TYPE_CATEGORY")
            .map_err(terminal_problem)?,
    })
}

fn tables_match(tables: &Tables, type02: &[u8]) -> bool {
    tables
        .types
        .iter()
        .map(|row| row.iter().map(Vec::as_slice).collect::<Vec<_>>())
        .eq(SEED_TYPES.into_iter().map(|mut row| {
            if row[0] == b"02" {
                row[1] = type02;
            }
            row.to_vec()
        }))
        && tables
            .categories
            .iter()
            .map(|row| row.iter().map(Vec::as_slice).collect::<Vec<_>>())
            .eq(SEED_CATEGORIES.into_iter().map(|row| row.to_vec()))
}

pub(super) fn require_rollback_before(before: &Tables) -> Result<(), CorpusProblem> {
    if !tables_match(before, b"PAYMENT") {
        return Err(drift(
            "complete pre-rollback tables differ from the pinned loads after type99 deletion",
        ));
    }
    Ok(())
}

pub(super) fn rollback_probe() -> Db2Request {
    Db2Request {
        operation: Db2Operation::Select,
        statement: "SELECT TR_TYPE,TR_DESCRIPTION FROM CARDDEMO.TRANSACTION_TYPE WHERE TR_TYPE = :DCL-TR-TYPE".into(),
        cursor: None,
        inputs: BTreeMap::from([("DCL-TR-TYPE".into(), db2_variable("97"))]),
        outputs: Vec::new(),
        max_rows: 1,
        mutation: None,
    }
}

fn selected_pending_match(result: &Db2Result) -> bool {
    result.sqlcode == 0
        && result.sqlstate == "00000"
        && result.affected_rows == 0
        && result.rows.len() == 1
        && result.rows[0]
            .columns
            .iter()
            .map(Vec::as_slice)
            .eq([b"97".as_slice(), EXPECTED_PENDING_DESCRIPTION])
}

pub(super) fn require_admitted_insert(
    inserted: &Db2Result,
    pending: &Db2Result,
    before: &Tables,
    committed: &Tables,
) -> Result<(), CorpusProblem> {
    if inserted.sqlcode != 0
        || inserted.sqlstate != "00000"
        || inserted.affected_rows != 1
        || !inserted.rows.is_empty()
        || !selected_pending_match(pending)
        || before != committed
        || !tables_match(before, b"PAYMENT")
    {
        return Err(drift(
            "INSERT97 was not one exact pending row with unchanged complete committed tables",
        ));
    }
    Ok(())
}

pub(super) fn require_rolled_back(
    rolled_back: &Db2Result,
    selected: &Db2Result,
    before: &Tables,
    after: &Tables,
) -> Result<(), CorpusProblem> {
    if rolled_back.sqlcode != 0
        || rolled_back.sqlstate != "00000"
        || rolled_back.affected_rows != 0
        || !rolled_back.rows.is_empty()
        || selected.sqlcode != 100
        || selected.sqlstate != "02000"
        || selected.affected_rows != 0
        || !selected.rows.is_empty()
        || before != after
        || !tables_match(after, b"PAYMENT")
    {
        return Err(drift(
            "ROLLBACK did not remove the admitted pending row and preserve complete committed tables",
        ));
    }
    Ok(())
}

pub(super) fn require_maintenance_before(server: &ProductServer) -> Result<Tables, CorpusProblem> {
    let before = capture(server)?;
    if !tables_match(&before, b"LEFT")
        || !utility_records(server, "INPFILE", None)?
            .iter()
            .map(Vec::as_slice)
            .eq(CONTROLLED_INPUT)
    {
        return Err(drift(
            "complete pre-maintenance tables or the three fixed53 input records differ",
        ));
    }
    Ok(before)
}

fn selected_spool_matches(records: &[Vec<u8>]) -> bool {
    records
        .iter()
        .map(Vec::as_slice)
        .filter(|record| record.starts_with(b"PROCESSING   ") || record.starts_with(b"RECORD "))
        .eq(EXPECTED_COMMAND_RESULTS)
}

fn compare_maintenance(
    observations: &mut RouteObservations,
    before: &Tables,
    after: &Tables,
    spool: &[Vec<u8>],
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J14",
        "COBTUPDT",
        !tables_match(before, b"LEFT")
            || !tables_match(after, b"BATCH PAYMENT")
            || !selected_spool_matches(spool),
        || {
            Ok(drift(
                "complete maintenance transition or exact command/success spool sequence differs",
            ))
        },
    )
}

fn require_completed_maintenance_job(id: &str, job: &JobSnapshot) -> Result<(), CorpusProblem> {
    if id.is_empty()
        || job.id != id
        || job.name != "MNTTRDB2"
        || job.owner != "IBMUSER"
        || job.state != JobState::Completed
        || job.return_code != Some(0)
        || job.abend_code.is_some()
    {
        return Err(drift(
            "maintenance job identity or successful terminal result differs",
        ));
    }
    Ok(())
}

pub(super) fn compare_completed_maintenance(
    server: &ProductServer,
    id: &str,
    before: &Tables,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    let job = server.batch_service().get(id).map_err(terminal_problem)?;
    require_completed_maintenance_job(id, &job)?;
    let invocation = base_batch_control_invocation()?;
    let files = server
        .batch_service()
        .spool_files(&invocation, id)
        .map_err(terminal_problem)?;
    let selected = files
        .iter()
        .filter(|(_, name, _, _)| name == "STEP1:SYSPRINT")
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(drift(
            "maintenance STEP1 program spool is not one owned file",
        ));
    }
    let declared = selected[0].2;
    let (spool, more) = server
        .batch_service()
        .spool(&invocation, id, "STEP1:SYSPRINT", 0, declared.max(1))
        .map_err(terminal_problem)?;
    if more || spool.len() != declared {
        return Err(drift("maintenance program spool was not read completely"));
    }
    compare_maintenance(observations, before, &capture(server)?, &spool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::Db2Row;

    fn tables(type02: &[u8]) -> Tables {
        Tables {
            types: SEED_TYPES
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .enumerate()
                        .map(|(index, value)| {
                            if row[0] == b"02" && index == 1 {
                                type02.to_vec()
                            } else {
                                value.to_vec()
                            }
                        })
                        .collect()
                })
                .collect(),
            categories: SEED_CATEGORIES
                .into_iter()
                .map(|row| row.into_iter().map(<[u8]>::to_vec).collect())
                .collect(),
        }
    }
    fn result(
        code: i32,
        state: &str,
        affected_rows: u64,
        columns: Option<Vec<Vec<u8>>>,
    ) -> Db2Result {
        Db2Result {
            sqlcode: code,
            sqlstate: state.into(),
            affected_rows,
            message: String::new(),
            rows: columns
                .into_iter()
                .map(|columns| Db2Row { columns })
                .collect(),
        }
    }
    fn pending() -> Db2Result {
        result(
            0,
            "00000",
            0,
            Some(vec![b"97".to_vec(), EXPECTED_PENDING_DESCRIPTION.to_vec()]),
        )
    }
    fn refused(before: &Tables, after: &Tables, spool: &[Vec<u8>]) {
        let mut observations = RouteObservations::default();
        assert_eq!(
            compare_maintenance(&mut observations, before, after, spool)
                .unwrap_err()
                .code,
            "carddemo.db2.control_comparison_drift"
        );
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn exact_admission_and_rollback_guards_accept_without_tokens() {
        let before = tables(b"PAYMENT");
        require_rollback_before(&before).unwrap();
        require_admitted_insert(&result(0, "00000", 1, None), &pending(), &before, &before)
            .unwrap();
        require_rolled_back(
            &result(0, "00000", 0, None),
            &result(100, "02000", 0, None),
            &before,
            &before,
        )
        .unwrap();
    }

    #[test]
    fn unadmitted_insert_or_wrong_pending_prefix_padding_identity_refuses() {
        let before = tables(b"PAYMENT");
        assert!(
            require_admitted_insert(
                &result(-803, "23505", 0, None),
                &pending(),
                &before,
                &before
            )
            .is_err()
        );
        for (column, offset) in [(0, 0), (1, 0), (1, 1), (1, 2), (1, 51)] {
            let mut selected = pending();
            selected.rows[0].columns[column][offset] ^= 1;
            assert!(
                require_admitted_insert(&result(0, "00000", 1, None), &selected, &before, &before)
                    .is_err()
            );
        }
        let mut selected = pending();
        selected.rows[0].columns[1].pop();
        assert!(
            require_admitted_insert(&result(0, "00000", 1, None), &selected, &before, &before)
                .is_err()
        );
    }

    #[test]
    fn premature_commit_or_incomplete_rollback_refuses() {
        let before = tables(b"PAYMENT");
        let mut committed = before.clone();
        committed
            .types
            .push(vec![b"97".to_vec(), b"ROLLBACK".to_vec()]);
        assert!(
            require_admitted_insert(
                &result(0, "00000", 1, None),
                &pending(),
                &before,
                &committed
            )
            .is_err()
        );
        assert!(
            require_rolled_back(
                &result(-551, "42501", 0, None),
                &result(100, "02000", 0, None),
                &before,
                &before
            )
            .is_err()
        );
        assert!(
            require_rolled_back(&result(0, "00000", 0, None), &pending(), &before, &before)
                .is_err()
        );
        assert!(
            require_rolled_back(
                &result(0, "00000", 0, None),
                &result(100, "02000", 0, None),
                &before,
                &committed
            )
            .is_err()
        );
    }

    #[test]
    fn exact_complete_maintenance_transition_produces_only_cobtupdt() {
        let mut observations = RouteObservations::default();
        compare_maintenance(
            &mut observations,
            &tables(b"LEFT"),
            &tables(b"BATCH PAYMENT"),
            &EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec),
        )
        .unwrap();
        assert_eq!(
            observations.journeys(),
            &[("CD.J14".into(), vec!["COBTUPDT".into()])]
        );
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn missing_changed_reordered_or_duplicate_success_spool_refuses() {
        let before = tables(b"LEFT");
        let after = tables(b"BATCH PAYMENT");
        for index in 0..6 {
            let mut spool = EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec).to_vec();
            spool.remove(index);
            refused(&before, &after, &spool);
            let mut spool = EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec).to_vec();
            spool[index][0] ^= 1;
            refused(&before, &after, &spool);
        }
        let mut spool = EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec).to_vec();
        spool.swap(1, 3);
        refused(&before, &after, &spool);
        let mut spool = EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec).to_vec();
        spool.push(spool[1].clone());
        refused(&before, &after, &spool);
    }

    #[test]
    fn complete_type_category_and_before_state_drift_refuses() {
        let before = tables(b"LEFT");
        let spool = EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec);
        let mut after = tables(b"BATCH PAYMENT");
        after.types[0][1][0] ^= 1;
        refused(&before, &after, &spool);
        let mut after = tables(b"BATCH PAYMENT");
        after.categories[0][2][0] ^= 1;
        refused(&before, &after, &spool);
        let mut after = tables(b"BATCH PAYMENT");
        after.types.pop();
        refused(&before, &after, &spool);
        let mut after = tables(b"BATCH PAYMENT");
        after.types.swap(0, 1);
        refused(&before, &after, &spool);
        let mut after = tables(b"BATCH PAYMENT");
        after.types.push(after.types[0].clone());
        refused(&before, &after, &spool);
        refused(&tables(b"PAYMENT"), &tables(b"BATCH PAYMENT"), &spool);
    }
    fn completed_maintenance_job() -> JobSnapshot {
        JobSnapshot {
            id: "JOB00001".into(),
            name: "MNTTRDB2".into(),
            owner: "IBMUSER".into(),
            class: 'A',
            priority: 1,
            state: JobState::Completed,
            return_code: Some(0),
            abend_code: None,
            active_step: None,
            initiator: None,
            steps: vec![],
            attempt: 1,
            version: 2,
            kind: Default::default(),
            origin: Default::default(),
            route: Default::default(),
            cancellation: None,
        }
    }

    fn refused_maintenance_job(id: &str, job: &JobSnapshot) {
        let mut observations = RouteObservations::default();
        let result = require_completed_maintenance_job(id, job).and_then(|()| {
            compare_maintenance(
                &mut observations,
                &tables(b"LEFT"),
                &tables(b"BATCH PAYMENT"),
                &EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec),
            )
        });
        assert_eq!(
            result.unwrap_err().code,
            "carddemo.db2.control_comparison_drift"
        );
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn completed_native_maintenance_with_exact_transition_accepts() {
        let job = completed_maintenance_job();
        let mut observations = RouteObservations::default();
        require_completed_maintenance_job("JOB00001", &job).unwrap();
        compare_maintenance(
            &mut observations,
            &tables(b"LEFT"),
            &tables(b"BATCH PAYMENT"),
            &EXPECTED_COMMAND_RESULTS.map(<[u8]>::to_vec),
        )
        .unwrap();
        assert_eq!(
            observations.journeys(),
            &[("CD.J14".into(), vec!["COBTUPDT".into()])]
        );
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn transient_output_and_all_other_noncompleted_states_refuse_maintenance() {
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
            let mut job = completed_maintenance_job();
            job.state = state;
            refused_maintenance_job("JOB00001", &job);
        }
    }

    #[test]
    fn absent_nonzero_or_abended_maintenance_result_refuses() {
        for code in [None, Some(4), Some(-1)] {
            let mut job = completed_maintenance_job();
            job.return_code = code;
            refused_maintenance_job("JOB00001", &job);
        }
        let mut job = completed_maintenance_job();
        job.abend_code = Some("S0C7".into());
        refused_maintenance_job("JOB00001", &job);
    }

    #[test]
    fn wrong_name_owner_or_job_identity_refuses_maintenance() {
        let job = completed_maintenance_job();
        refused_maintenance_job("", &job);
        refused_maintenance_job("JOB00002", &job);
        let mut wrong = job.clone();
        wrong.id.clear();
        refused_maintenance_job("JOB00001", &wrong);
        let mut wrong = job.clone();
        wrong.name = "OTHERJOB".into();
        refused_maintenance_job("JOB00001", &wrong);
        let mut wrong = job;
        wrong.owner = "WEBUSER".into();
        refused_maintenance_job("JOB00001", &wrong);
    }
}
