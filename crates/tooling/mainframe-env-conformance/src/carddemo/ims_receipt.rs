//! Local controller/provider results do not establish first-party IMS actors.
use super::*;

// The owner has two admitted Rust controller jobs and one handwritten IMSROUTE
// program. Pinned first-party EXEC DLI sources are compiled/package inputs but
// are not dispatched by this owner. Counts and self-digests cannot promote them.
pub(super) fn provider_control_receipt(
    corpus_commit: String,
    definitions_checked: usize,
    programs_compiled: usize,
    dli_operations: BTreeMap<String, usize>,
    exercise: ims_routes::ImsExercise,
    ims_shape_sha256: String,
) -> (CardDemoImsReceipt, RouteObservations) {
    (
        CardDemoImsReceipt {
            schema_version: "mainframe-env.carddemo-ims-receipt@1".into(),
            status: "pass".into(),
            corpus_commit,
            definitions_checked,
            databases_installed: exercise.databases_installed,
            psbs_installed: exercise.psbs_installed,
            pcbs_installed: exercise.pcbs_installed,
            programs_compiled,
            dli_operations,
            application_routes: 0,
            selected_job_routes: exercise.selected_job_routes,
            roots: exercise.roots,
            children: exercise.children,
            secondary_index_entries: exercise.secondary_index_entries,
            load_unload_routes: 2,
            checkpoint_controls: 1,
            restart_controls: 1,
            authorization_controls: 1,
            malformed_controls: 1,
            resource_controls: 1,
            provider_failure_controls: 1,
            hierarchy_sha256: exercise.hierarchy_sha256,
            spool_sha256: exercise.spool_sha256,
            ims_shape_sha256,
        },
        RouteObservations::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_digests_do_not_establish_firstparty_application_routes() {
        for selected_job_routes in [0, 2, 1_000_000] {
            let dli_operations =
                BTreeMap::from([("GU".into(), 1_000_000), ("ISRT".into(), 1_000_000)]);
            let spool = BTreeMap::from([
                ("LOADPADB".into(), "a".repeat(64)),
                ("UNLDPADB".into(), "b".repeat(64)),
            ]);
            let exercise = ims_routes::ImsExercise {
                databases_installed: 2,
                psbs_installed: 3,
                pcbs_installed: 3,
                roots: 2,
                children: 2,
                secondary_index_entries: 2,
                hierarchy_sha256: "c".repeat(64),
                selected_job_routes,
                spool_sha256: spool.clone(),
            };
            let (receipt, observations) = provider_control_receipt(
                "synthetic-not-executed".into(),
                8,
                99,
                dli_operations.clone(),
                exercise,
                "d".repeat(64),
            );
            // Even complete-looking metadata, operation counts, controller
            // counts and digests cannot supply an executed first-party actor.
            assert_eq!(receipt.application_routes, 0);
            assert!(observations.journeys().is_empty());
            assert!(observations.issues().is_empty());
            assert_eq!(receipt.status, "pass");
            assert_eq!(receipt.selected_job_routes, selected_job_routes);
            assert_eq!(receipt.programs_compiled, 99);
            assert_eq!(receipt.dli_operations, dli_operations);
            assert_eq!(receipt.spool_sha256, spool);
            assert_eq!(
                (
                    receipt.load_unload_routes,
                    receipt.checkpoint_controls,
                    receipt.restart_controls,
                    receipt.authorization_controls,
                    receipt.malformed_controls,
                    receipt.resource_controls,
                    receipt.provider_failure_controls,
                ),
                (2, 1, 1, 1, 1, 1, 1),
            );
        }
    }
}
