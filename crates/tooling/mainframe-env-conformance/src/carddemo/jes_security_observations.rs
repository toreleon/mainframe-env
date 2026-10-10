//! Ownership checks against every actual identity from the bounded mixed load.
use super::*;

#[derive(Debug, Eq, PartialEq)]
struct OwnerJobState {
    status: serde_json::Value,
    files: serde_json::Value,
    spool: BTreeMap<u64, Vec<u8>>,
}

// Fields are private: only the actual route comparison can create this proof.
// Carry full positive owner state through drain/cleanup, rather than an ID count.
pub(super) struct JesJobProof {
    before: BTreeMap<String, OwnerJobState>,
    after: BTreeMap<String, OwnerJobState>,
    list_before: BTreeMap<String, serde_json::Value>,
    list_after: BTreeMap<String, serde_json::Value>,
    denied: bool,
}

impl JesJobProof {
    pub(super) fn principal_refused(&self) -> bool {
        !self.denied || self.before != self.after || self.list_before != self.list_after
    }

    // The caller publishes only after actual memory drain and owned cleanup.
    pub(super) fn observe_job_ids_and_spool(
        self,
        observations: &mut RouteObservations,
    ) -> Result<(), CorpusProblem> {
        let expected = (0..8)
            .map(|index| format!("MX{index:06}"))
            .collect::<BTreeSet<_>>();
        let complete = self.before.keys().cloned().collect::<BTreeSet<_>>() == expected
            && self.list_before.keys().cloned().collect::<BTreeSet<_>>() == expected
            && self.before.values().all(|job| {
                job.files
                    .as_array()
                    .is_some_and(|files| !files.is_empty() && files.len() == job.spool.len())
                    && !job.spool.is_empty()
            });
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J19",
            "job IDs and spool",
            !complete || self.principal_refused(),
            || Ok(CorpusProblem::new(
                "carddemo.full.job_spool_drift",
                "actual eight-job owner status/list/full spool and independent JESJCL comparisons were incomplete or changed across denial controls",
            )),
        )
    }
}

fn complete_spool_page(file: &serde_json::Value, body: &[u8]) -> bool {
    let records = if body.is_empty() {
        0
    } else {
        body.split(|byte| *byte == b'\n').count() as u64
    };
    let bytes = (body.len() as u64).saturating_sub(records.saturating_sub(1));
    // These selected two-line IEFBR14 controls generate unpadded single-line
    // spool records. Binding both counts refuses a silently truncated page.
    file["record-count"].as_u64() == Some(records)
        && file["byte-count"].as_u64() == Some(bytes)
        && records > 0
        && records <= 100
}

fn problem(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.full.principal_leak", detail.into())
}

fn submitted_identities(
    submitted: &BTreeMap<String, serde_json::Value>,
) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let expected = (0..8)
        .map(|index| format!("MX{index:06}"))
        .collect::<BTreeSet<_>>();
    if submitted.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(problem(
            "principal control did not receive all eight actual submitted job names",
        ));
    }
    let mut identities = BTreeMap::new();
    let mut distinct = BTreeSet::new();
    for (name, job) in submitted {
        let id = job["jobid"]
            .as_str()
            .ok_or_else(|| problem("actual submitted job ID missing"))?;
        if job["jobname"] != *name
            || job["owner"] != "IBMUSER"
            || id.len() != 8
            || !id.starts_with("JOB")
            || !id[3..].bytes().all(|byte| byte.is_ascii_digit())
            || !distinct.insert(id.to_string())
        {
            return Err(problem(
                "actual submitted owner/job name/ID tuple invalid or duplicated",
            ));
        }
        identities.insert(name.clone(), id.to_string());
    }
    Ok(identities)
}

async fn owner_job_state(
    app: &axum::Router,
    owner: &BTreeMap<String, String>,
    name: &str,
    id: &str,
) -> Result<OwnerJobState, CorpusProblem> {
    let uri = format!("/zosmf/restjobs/jobs/{name}/{id}");
    let (status, body) = terminal_http(app, Method::GET, &uri, owner.clone(), Vec::new()).await?;
    require_terminal_status(status, StatusCode::OK, "owner job snapshot")?;
    let job: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| problem(error.to_string()))?;
    if job["jobname"] != name
        || job["jobid"] != id
        || job["owner"] != "IBMUSER"
        || job["status"] != "OUTPUT"
        || job["retcode"] != "CC 0000"
    {
        return Err(problem(
            "owner job snapshot disagreed with completed actual submission",
        ));
    }
    let (status, body) = terminal_http(
        app,
        Method::GET,
        &format!("{uri}/files"),
        owner.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(status, StatusCode::OK, "owner spool snapshot")?;
    let files: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| problem(error.to_string()))?;
    let entries = files
        .as_array()
        .ok_or_else(|| problem("owner spool list is not an array"))?;
    // Match existing BatchLimits.max_spool_files. These simple IEFBR14 jobs
    // must fit one unchanged 100-record request for each actual spool file.
    if entries.is_empty() || entries.len() > 128 {
        return Err(problem(
            "owner spool file inventory is empty or exceeds the existing bound",
        ));
    }
    // Exact local spool surface of these owned no-DD IEFBR14 controls:
    // initial job records, job retirement, and the successful STEP output/log.
    let expected_files = [
        "JESJCL",
        "JESMSGLG",
        "JOBLOG",
        "STEP:JOBLOG",
        "STEP:SYSPRINT",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let actual_files = entries
        .iter()
        .map(|file| {
            file["ddname"]
                .as_str()
                .ok_or_else(|| problem("owner spool DD name missing"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if actual_files != expected_files || entries.len() != expected_files.len() {
        return Err(problem(
            "owner spool list omitted, duplicated or added a selected actual IEFBR14 file",
        ));
    }
    let mut spool = BTreeMap::new();
    let mut saw_jcl = false;
    for file in entries {
        let file_id = file["id"]
            .as_u64()
            .ok_or_else(|| problem("actual spool file ID missing"))?;
        let record_count = file["record-count"]
            .as_u64()
            .ok_or_else(|| problem("actual spool record count missing"))?;
        if file["jobname"] != name || file["jobid"] != id || record_count > 100 {
            return Err(problem(
                "actual spool identity or single bounded page disagreed",
            ));
        }
        let (status, body) = terminal_http(
            app,
            Method::GET,
            &format!("{uri}/files/{file_id}/records?start=0&max=100"),
            owner.clone(),
            Vec::new(),
        )
        .await?;
        require_terminal_status(status, StatusCode::OK, "owner spool records")?;
        if !complete_spool_page(file, &body) {
            return Err(problem(
                "owner spool page bytes/records disagreed with its complete actual inventory",
            ));
        }
        if file["ddname"] == "JESJCL" {
            let independent =
                format!("//{name} JOB 'CD027',CLASS=A,MSGCLASS=H\n//STEP EXEC PGM=IEFBR14");
            if body != independent.as_bytes() || record_count != 2 || saw_jcl {
                return Err(problem(
                    "owner JESJCL bytes disagreed with the independent offered JCL",
                ));
            }
            saw_jcl = true;
        }
        if spool.insert(file_id, body).is_some() {
            return Err(problem("owner spool file ID duplicated"));
        }
    }
    if !saw_jcl {
        return Err(problem(
            "actual owner spool did not contain the offered JESJCL",
        ));
    }
    Ok(OwnerJobState {
        status: job,
        files,
        spool,
    })
}

async fn owner_list(
    app: &axum::Router,
    owner: &BTreeMap<String, String>,
    expected: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, serde_json::Value>, CorpusProblem> {
    let (status, body) = terminal_http(
        app,
        Method::GET,
        "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=MX*&max=8",
        owner.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(status, StatusCode::OK, "owner list snapshot")?;
    let jobs: serde_json::Value =
        serde_json::from_slice(&body).map_err(|error| problem(error.to_string()))?;
    let items = jobs
        .as_array()
        .ok_or_else(|| problem("owner job list is not an array"))?;
    let mut observed = BTreeMap::new();
    for job in items {
        let name = job["jobname"]
            .as_str()
            .ok_or_else(|| problem("owner list job name missing"))?;
        if expected.get(name).is_none_or(|id| job["jobid"] != *id)
            || job["owner"] != "IBMUSER"
            || observed.insert(name.to_string(), job.clone()).is_some()
        {
            return Err(problem(
                "owner list contains an unexpected/duplicated actual identity",
            ));
        }
    }
    if observed.len() != 8 {
        return Err(problem("owner list omitted an actual submitted identity"));
    }
    Ok(observed)
}

pub(super) async fn exercise_job_principal_denials(
    app: &axum::Router,
    owner: &BTreeMap<String, String>,
    appuser: &str,
    submitted: &BTreeMap<String, serde_json::Value>,
) -> Result<JesJobProof, CorpusProblem> {
    let identities = submitted_identities(submitted)?;
    let list_before = owner_list(app, owner, &identities).await?;
    let mut before = BTreeMap::new();
    for (name, id) in &identities {
        let state = owner_job_state(app, owner, name, id).await?;
        if list_before[name] != state.status {
            return Err(problem(
                "owner status/list routes disagree before principal controls",
            ));
        }
        before.insert(name.clone(), state);
    }
    let other = BTreeMap::from([("authorization".into(), appuser.to_string())]);
    let mut denied = true;
    for uri in [
        "/zosmf/restjobs/jobs",
        "/zosmf/restjobs/jobs?owner=*&prefix=MX*&max=8",
    ] {
        let (status, body) =
            terminal_http(app, Method::GET, uri, other.clone(), Vec::new()).await?;
        denied &= status == StatusCode::OK
            && serde_json::from_slice::<serde_json::Value>(&body)
                .is_ok_and(|jobs| jobs == serde_json::json!([]));
    }
    let (status, body) = terminal_http(
        app,
        Method::GET,
        "/zosmf/restjobs/jobs?owner=IBMUSER&prefix=MX*&max=8",
        other.clone(),
        Vec::new(),
    )
    .await?;
    denied &= security_observations::exact_forbidden(status, &body);
    for (name, id) in &identities {
        let uri = format!("/zosmf/restjobs/jobs/{name}/{id}");
        for path in [uri.clone(), format!("{uri}/files")] {
            let (status, body) =
                terminal_http(app, Method::GET, &path, other.clone(), Vec::new()).await?;
            denied &= security_observations::exact_forbidden(status, &body);
        }
        // Probe every positive owner file, using the actual IDs, including JCL.
        for file_id in before[name].spool.keys() {
            let (status, body) = terminal_http(
                app,
                Method::GET,
                &format!("{uri}/files/{file_id}/records?start=0&max=100"),
                other.clone(),
                Vec::new(),
            )
            .await?;
            denied &= security_observations::exact_forbidden(status, &body);
        }
    }
    let list_after = owner_list(app, owner, &identities).await?;
    denied &= list_after == list_before;
    let mut after = BTreeMap::new();
    for (name, id) in &identities {
        let state = owner_job_state(app, owner, name, id).await?;
        denied &= state == before[name];
        after.insert(name.clone(), state);
    }
    Ok(JesJobProof {
        before,
        after,
        list_before,
        list_after,
        denied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submitted() -> BTreeMap<String, serde_json::Value> {
        (0..8).map(|index| {
            let name = format!("MX{index:06}");
            let job = serde_json::json!({"jobname":name,"jobid":format!("JOB{:05}",index + 1),"owner":"IBMUSER"});
            (name, job)
        }).collect()
    }

    #[test]
    fn empty_owner_proof_cannot_create_job_ids_and_spool_observation() {
        // Synthetic comparison refusal only; this creates no runtime acceptance.
        let proof = JesJobProof {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
            list_before: BTreeMap::new(),
            list_after: BTreeMap::new(),
            denied: true,
        };
        let mut observed = RouteObservations::default();
        assert!(proof.observe_job_ids_and_spool(&mut observed).is_err());
        assert!(observed.journeys().is_empty());
    }

    #[test]
    fn complete_spool_page_refuses_missing_records_and_short_bytes() {
        let file = serde_json::json!({"record-count":2,"byte-count":6});
        assert!(complete_spool_page(&file, b"ABC\nDEF"));
        assert!(!complete_spool_page(&file, b"ABC"));
        assert!(!complete_spool_page(&file, b"ABC\nDE"));
        assert!(!complete_spool_page(
            &serde_json::json!({"record-count":2}),
            b"ABC\nDEF"
        ));
    }

    #[test]
    fn identity_admission_rejects_every_missing_job_and_duplicate_id() {
        // Synthetic boundary controls earn no actual-route acceptance credit.
        assert_eq!(submitted_identities(&submitted()).unwrap().len(), 8);
        for index in 0..8 {
            let mut jobs = submitted();
            jobs.remove(&format!("MX{index:06}"));
            assert!(submitted_identities(&jobs).is_err());
        }
        let mut jobs = submitted();
        jobs.get_mut("MX000007").unwrap()["jobid"] = "JOB00001".into();
        assert!(submitted_identities(&jobs).is_err());
    }

    #[test]
    fn identity_admission_rejects_owner_name_and_route_injection_drift() {
        for (field, wrong) in [
            ("owner", "APPUSER"),
            ("jobname", "MX000008"),
            ("jobid", "JOB0/123"),
        ] {
            let mut jobs = submitted();
            jobs.get_mut("MX000007").unwrap()[field] = wrong.into();
            assert!(submitted_identities(&jobs).is_err());
        }
    }
}
