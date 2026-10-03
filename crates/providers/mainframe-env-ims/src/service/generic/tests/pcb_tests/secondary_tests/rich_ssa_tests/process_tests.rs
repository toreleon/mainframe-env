use super::*;

#[test]
fn secondary_ssa_sqlite_process_worker() {
    let Ok(url) = std::env::var("IMS_SECONDARY_SSA_PROCESS_URL") else {
        return;
    };
    let stage = std::env::var("IMS_SECONDARY_SSA_PROCESS_STAGE").unwrap();
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let run = "ssa-process";
    let service = if stage == "seed" {
        installed(store, run, false)
    } else {
        ImsService::open(store, Default::default()).unwrap()
    };
    let get = nav(
        run,
        2,
        ImsOperation::GetHoldNext,
        2,
        &[b"ROOT    (BYCHILD GEA)"],
    );
    let before = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        public(service.clone(), run, get).unwrap().segments[0].data,
        b"B2AX"
    );
    if stage != "seed" {
        assert_eq!(
            pcb::position(&service.lock().unwrap().state.sessions[run], 2),
            before
        );
    }
    if stage == "reopen" {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 3, ImsOperation::GetNext, 2, &[b"ROOT     "])
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1ZX"
        );
    } else if stage == "verify" {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 3, ImsOperation::GetNext, 2, &[b"ROOT     "])
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1ZX"
        );
        assert_eq!(
            pcb::position(&service.lock().unwrap().state.sessions[run], 2),
            before
        );
        assert_eq!(
            public(service, run, nav(run, 4, ImsOperation::GetNext, 2, &[]))
                .unwrap()
                .segments[0]
                .data,
            b"C1ZA"
        );
    } else {
        assert_eq!(stage, "seed");
    }
}

#[test]
fn secondary_ssa_sqlite_three_independent_processes_retain_pointer_and_replay() {
    let file = std::env::temp_dir().join(format!(
        "ims-rich-process-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    for stage in ["seed", "reopen", "verify"] {
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","service::generic::tests::pcb_tests::secondary_tests::rich_ssa_tests::process_tests::secondary_ssa_sqlite_process_worker","--nocapture"])
            .env("IMS_SECONDARY_SSA_PROCESS_URL",&url).env("IMS_SECONDARY_SSA_PROCESS_STAGE",stage)
            .output().unwrap();
        assert!(
            result.status.success(),
            "stage {stage}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }
    std::fs::remove_file(file).unwrap();
}
