//! First-party CLOSEFIL/OPENFIL source, named state and ordered CMDOUT proof.
use super::{CicsFileStatus, CorpusProblem, Digest, RouteObservations, Sha256, journey_closure};

const CLOSE_SOURCE_SHA256: &str =
    "6a9adef38b7b67074851ac6263a5f40b9d47e51aa0e03f98ab87a201c3b6032b";
const OPEN_SOURCE_SHA256: &str = "1831990c78749793661ba75f46007810fd1fb0c237e6220623bc4795e2c7e87e";
const FILES: [&str; 5] = ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"];
// Literal expectations from the pinned ISFIN commands, independent of results.
const CLOSED_OUTPUT: [&[u8]; 5] = [
    b"TRANSACT CLOSED",
    b"CCXREF CLOSED",
    b"ACCTDAT CLOSED",
    b"CXACAIX CLOSED",
    b"USRSEC CLOSED",
];
const OPENED_OUTPUT: [&[u8]; 5] = [
    b"TRANSACT OPEN",
    b"CCXREF OPEN",
    b"ACCTDAT OPEN",
    b"CXACAIX OPEN",
    b"USRSEC OPEN",
];

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.base_batch.file_control_oracle_drift",
        "first-party CLOSEFIL/OPENFIL source, named states or exact ordered output differs",
    )
}

#[derive(Default)]
pub(super) struct FileControls {
    closed: bool,
    opened: bool,
}

fn require_shape(
    states: &[(&str, CicsFileStatus)],
    output: &[Vec<u8>],
    output_more: bool,
    closed: bool,
) -> Result<(), CorpusProblem> {
    let state = if closed {
        CicsFileStatus::Closed
    } else {
        CicsFileStatus::Open
    };
    let expected = if closed {
        &CLOSED_OUTPUT
    } else {
        &OPENED_OUTPUT
    };
    if output_more
        || states.len() != FILES.len()
        || states
            .iter()
            .zip(FILES)
            .any(|((name, actual), expected)| *name != expected || *actual != state)
        || output.len() != expected.len()
        || output
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual.as_slice() != *expected)
    {
        return Err(refused());
    }
    Ok(())
}

impl FileControls {
    pub(super) fn compare_closed(
        &mut self,
        source: &[u8],
        states: &[(&str, CicsFileStatus)],
        output: &[Vec<u8>],
        output_more: bool,
    ) -> Result<(), CorpusProblem> {
        if self.closed
            || self.opened
            || format!("{:x}", Sha256::digest(source)) != CLOSE_SOURCE_SHA256
        {
            return Err(refused());
        }
        require_shape(states, output, output_more, true)?;
        self.closed = true;
        Ok(())
    }

    pub(super) fn compare_opened(
        &mut self,
        source: &[u8],
        states: &[(&str, CicsFileStatus)],
        output: &[Vec<u8>],
        output_more: bool,
    ) -> Result<(), CorpusProblem> {
        if !self.closed
            || self.opened
            || format!("{:x}", Sha256::digest(source)) != OPEN_SOURCE_SHA256
        {
            return Err(refused());
        }
        require_shape(states, output, output_more, false)?;
        self.opened = true;
        Ok(())
    }

    // The owner consumes this only after drain, restart guards and checked cleanup.
    pub(super) fn finish(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J11",
            "CLOSEFIL/OPENFIL state",
            !self.closed || !self.opened,
            || Ok(refused()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed() -> (Vec<(&'static str, CicsFileStatus)>, Vec<Vec<u8>>) {
        (
            vec![
                ("TRANSACT", CicsFileStatus::Closed),
                ("CCXREF", CicsFileStatus::Closed),
                ("ACCTDAT", CicsFileStatus::Closed),
                ("CXACAIX", CicsFileStatus::Closed),
                ("USRSEC", CicsFileStatus::Closed),
            ],
            vec![
                b"TRANSACT CLOSED".to_vec(),
                b"CCXREF CLOSED".to_vec(),
                b"ACCTDAT CLOSED".to_vec(),
                b"CXACAIX CLOSED".to_vec(),
                b"USRSEC CLOSED".to_vec(),
            ],
        )
    }

    #[test]
    fn correct_rows_with_unpinned_source_add_no_observation() {
        let (states, output) = closed();
        assert!(require_shape(&states, &output, false, true).is_ok());
        let mut proof = FileControls::default();
        assert!(
            proof
                .compare_closed(b"synthetic-CLOSEFIL", &states, &output, false)
                .is_err()
        );
        let mut observations = RouteObservations::default();
        assert!(proof.finish(&mut observations).is_err());
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn incorrect_named_state_order_cardinality_or_phase_refuses() {
        let (states, output) = closed();
        for case in 0..5 {
            let mut changed = states.clone();
            match case {
                0 => changed[0].0 = "OTHER",
                1 => changed[0].1 = CicsFileStatus::Open,
                2 => changed.swap(0, 1),
                3 => {
                    changed.pop();
                }
                _ => changed.push(changed[0]),
            }
            assert!(require_shape(&changed, &output, false, true).is_err());
        }
        assert!(require_shape(&states, &output, false, false).is_err());
    }

    #[test]
    fn incorrect_output_bytes_order_cardinality_or_padding_refuses() {
        let (states, output) = closed();
        for case in 0..5 {
            let mut changed = output.clone();
            match case {
                0 => changed[0][0] = b'X',
                1 => changed[0].push(b' '),
                2 => changed.swap(0, 1),
                3 => {
                    changed.pop();
                }
                _ => changed.push(changed[0].clone()),
            }
            assert!(require_shape(&states, &changed, false, true).is_err());
        }
    }

    #[test]
    fn either_missing_phase_adds_no_observation() {
        for proof in [
            FileControls::default(),
            FileControls {
                closed: true,
                opened: false,
            },
            FileControls {
                closed: false,
                opened: true,
            },
        ] {
            let mut observations = RouteObservations::default();
            assert!(proof.finish(&mut observations).is_err());
            assert!(observations.journeys().is_empty());
            assert!(observations.issues().is_empty());
        }
    }

    #[test]
    fn complete_looking_cmdout_with_more_records_refuses() {
        let (states, output) = closed();
        assert!(require_shape(&states, &output, true, true).is_err());
    }

    #[test]
    fn all_five_complete_open_rows_are_required() {
        let states = vec![
            ("TRANSACT", CicsFileStatus::Open),
            ("CCXREF", CicsFileStatus::Open),
            ("ACCTDAT", CicsFileStatus::Open),
            ("CXACAIX", CicsFileStatus::Open),
            ("USRSEC", CicsFileStatus::Open),
        ];
        let output = vec![
            b"TRANSACT OPEN".to_vec(),
            b"CCXREF OPEN".to_vec(),
            b"ACCTDAT OPEN".to_vec(),
            b"CXACAIX OPEN".to_vec(),
            b"USRSEC OPEN".to_vec(),
        ];
        assert!(require_shape(&states, &output, false, false).is_ok());
        assert!(require_shape(&states, &output, true, false).is_err());
        for index in 0..states.len() {
            let mut changed = states.clone();
            changed[index].1 = CicsFileStatus::Closed;
            assert!(require_shape(&changed, &output, false, false).is_err());
            let mut changed = output.clone();
            changed[index].push(b' ');
            assert!(require_shape(&states, &changed, false, false).is_err());
        }
    }

    #[test]
    fn control_only_finish_binds_only_the_declared_token() {
        // This synthetic seam control has zero first-party application credit.
        // Actual source/state/output/drain/cleanup requires the ignored owner.
        let proof = FileControls {
            closed: true,
            opened: true,
        };
        let mut observations = RouteObservations::default();
        proof.finish(&mut observations).unwrap();
        assert_eq!(
            observations.journeys(),
            &[("CD.J11".into(), vec!["CLOSEFIL/OPENFIL state".into()])]
        );
        assert!(observations.issues().is_empty());
    }

    #[test]
    #[ignore = "requires the exact clean CARDDEMO_CORPUS_DIR first-party input"]
    fn corpus_close_open_produces_only_the_owned_observation_after_cleanup() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
        let (receipt, observations) =
            super::super::verify_carddemo_base_batch_observed(&inventory).unwrap();
        assert_eq!(
            receipt.schema_version,
            "mainframe-env.carddemo-base-batch-receipt@1"
        );
        assert_eq!(receipt.status, "pass");
        assert_eq!(receipt.journeys_passed, 3);
        assert_eq!(
            receipt.corpus_commit,
            "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
        );
        assert_eq!(
            (
                receipt.initialization_jobs,
                receipt.operational_jobs,
                receipt.cics_file_controls,
                receipt.internal_submissions,
                receipt.warm_restart_controls,
                receipt.rollback_controls,
                receipt.cancellation_controls
            ),
            (12, 9, 2, 1, 1, 1, 1)
        );
        assert_eq!(
            observations.journeys(),
            &[
                (
                    "CD.J11".into(),
                    vec![
                        "CLOSEFIL/OPENFIL state".into(),
                        "POSTTRAN step outcomes".into(),
                        "INTCALC decimal results".into(),
                        "dataset mutations".into(),
                        "return codes and spool".into(),
                    ]
                ),
                (
                    "CD.J12".into(),
                    vec!["SORT ordering".into(), "report records".into()]
                ),
            ]
        );
        assert!(observations.issues().is_empty());
        println!(
            "actual pinned CLOSEFIL/OPENFIL JCL, five named CICS states and exact ordered output plus complete financial/report records compared; seven owned requirements across J11/J12 after drain/restart and checked fresh owner cleanup; no whole-journey, issue or licensed credit"
        );
    }
}
