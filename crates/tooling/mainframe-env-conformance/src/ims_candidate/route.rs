//! Public host invocation and bounded row observations, never expected values.
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_ims::{ImsLimits, ImsService, ims_providers};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::ProviderStateStore;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);
const RUN: &str = "ims-ir-preparation";
const DB: &str = "IRDB";

struct Authorizer(Arc<AtomicBool>);
impl EnterpriseAuthorizer for Authorizer {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        if self.0.load(Ordering::SeqCst) {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

pub(super) struct Route {
    store: Arc<dyn ProviderStateStore>,
    service: Arc<ImsService>,
    deny: Arc<AtomicBool>,
    file: Option<std::path::PathBuf>,
}

impl Drop for Route {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            // Delete only this private fixture file; fields close their handles next.
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", file.display()));
            }
        }
    }
}

impl Route {
    pub(super) fn open(
        fixture: &Fixture,
        metadata: &ImsMetadataCatalog,
        seed: &mainframe_env_ims::ImsGenericLoadImage,
    ) -> Result<Self, String> {
        let file = (fixture.backend == Backend::Sqlite).then(|| {
            std::env::temp_dir().join(format!(
                "mainframe-env-ims-ir-{}-{}.sqlite",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ))
        });
        let store = open_store(file.as_deref())?;
        let deny = Arc::new(AtomicBool::new(false));
        let service = ImsService::open_authorized(
            store.clone(),
            ImsLimits::default(),
            Arc::new(Authorizer(deny.clone())),
        )
        .map_err(|e| e.to_string())?;
        service
            .install_metadata(metadata.clone())
            .map_err(|e| e.to_string())?;
        let route = Self {
            store,
            service,
            deny,
            file,
        };
        check_setup(
            route.call(ImsOperation::Schedule, 1, &[], &[], None, &[])?,
            0,
        )?;
        let mut seed = seed.clone();
        if fixture.seed_scope == SeedScope::RootsOnly {
            seed.records.retain(|record| record.segment == "ROOT");
        }
        let image = serde_json::to_vec(&seed).map_err(|e| e.to_string())?;
        check_setup(
            route.call(ImsOperation::Load, 2, &[], &image, None, &[])?,
            seed.records.len() as u64,
        )?;
        check_setup(route.call(ImsOperation::Commit, 3, &[], &[], None, &[])?, 0)?;
        Ok(route)
    }

    pub(super) fn deny(&self) {
        self.deny.store(true, Ordering::SeqCst);
    }

    pub(super) fn reopen(&mut self) -> Result<(), String> {
        // New service and (for SQLite) a fresh independent store connection.
        let store = if self.file.is_none() {
            self.store.clone()
        } else {
            open_store(self.file.as_deref())?
        };
        self.service = ImsService::open_authorized(
            store.clone(),
            ImsLimits::default(),
            Arc::new(Authorizer(self.deny.clone())),
        )
        .map_err(|e| e.to_string())?;
        self.store = store;
        Ok(())
    }

    pub(super) fn call(
        &self,
        operation: ImsOperation,
        sequence: u64,
        segments: &[&str],
        data: &[u8],
        key: Option<&[u8]>,
        ssas: &[&[u8]],
    ) -> Result<CallOutcome, String> {
        let l = InvocationLimits::default();
        let invocation = Invocation::new(
            RequestId::new("ims-ir-request", l).map_err(err)?,
            ExecutionId::new("ims-ir-execution", l).map_err(err)?,
            RunUnitId::new(RUN, l).map_err(err)?,
            None,
            Selector::new("ims:ir", l).map_err(err)?,
            ArtifactRef::new("ims:ir", l).map_err(err)?,
            Principal::new(
                PrincipalId::new("IBMUSER", l).map_err(err)?,
                ["host.ims.read", "host.ims.write"]
                    .into_iter()
                    .map(|id| CapabilityId::new(id, l).map_err(err))
                    .collect::<Result<_, _>>()?,
                l,
            )
            .map_err(err)?,
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new("ims-ir-trace", l).map_err(err)?,
            IdempotencyKey::new("ims-ir-invocation", l).map_err(err)?,
            1,
            ResourceLimits::default(),
            Default::default(),
            l,
        )
        .map_err(err)?;
        let request = ImsRequest {
            operation,
            psb: (operation == ImsOperation::Schedule).then(|| "IRPSB".into()),
            pcb: 1,
            segments: segments.iter().map(|name| (*name).into()).collect(),
            data: data.to_vec(),
            qualifiers: key
                .map(|value| {
                    vec![ImsQualifier {
                        segment: "ROOT".into(),
                        field: "ROOTKEY".into(),
                        value: value.into(),
                    }]
                })
                .unwrap_or_default(),
            checkpoint_id: None,
            max_segments: 16,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(format!("ims-ir-{sequence}"), l)
                    .map_err(err)?,
                transaction: Some("IMS-IR".into()),
            }),
            system: None,
            q_class: None,
        };
        let navigation = !ssas.is_empty();
        let request = if navigation {
            HostRequest::ImsNavigation(ImsNavigationRequest {
                request,
                context: ImsExecutionContext::DbBatch,
                ssas: ssas.iter().map(|ssa| ssa.to_vec()).collect(),
            })
        } else {
            HostRequest::Ims(request)
        };
        let result = request.validate(HostLimits::default()).and_then(|()| {
            let provider = ims_providers(self.service.clone(), l).remove(usize::from(navigation));
            provider
                .invoke(
                    &invocation,
                    EffectRequest {
                        run_unit: invocation.run_unit_id.clone(),
                        sequence,
                        deadline_tick: 100,
                        idempotency_key: request.mutation().map(|m| m.idempotency_key.clone()),
                        request,
                    },
                )
                .outcome
        });
        match result {
            Ok(HostResult::Ims(result)) => Ok(CallOutcome {
                status: Some(result.status),
                problem: None,
                segments: result
                    .segments
                    .into_iter()
                    .map(|segment| SegmentOutcome {
                        name: segment.name,
                        data: segment.data,
                        parent_key: segment.parent_key,
                    })
                    .collect(),
                affected: result.affected_segments,
            }),
            Err(problem) => Ok(CallOutcome {
                status: None,
                problem: Some(format!("{problem:?}")),
                segments: vec![],
                affected: 0,
            }),
            other => Err(format!("unexpected host output: {other:?}")),
        }
    }

    pub(super) fn state(&self) -> Result<StateOutcome, String> {
        let row = |namespace: &str, key: &str| -> Result<serde_json::Value, String> {
            let record = self
                .store
                .get_provider_state(namespace, key)
                .map_err(err)?
                .ok_or_else(|| format!("missing observation row {namespace}/{key}"))?;
            if record.payload.len() > 64 * 1024 {
                return Err("observation row exceeds bound".into());
            }
            let value: serde_json::Value = serde_json::from_slice(&record.payload).map_err(err)?;
            Ok(value["value"].clone())
        };
        let image = row("ims-v1-generic-database", DB)?;
        let records = image["records"]
            .as_array()
            .ok_or("database records absent")?;
        if records.len() > 16 {
            return Err("record observation exceeds bound".into());
        }
        let session = row("ims-v1-session-index", RUN)?;
        let position = &session["position"];
        let data = |id: &serde_json::Value| -> Result<Option<Vec<u8>>, String> {
            if id.is_null() {
                return Ok(None);
            }
            let record = records
                .iter()
                .find(|record| &record["id"] == id)
                .ok_or("position references an absent occurrence")?;
            Ok(Some(
                serde_json::from_value(record["data"].clone()).map_err(err)?,
            ))
        };
        Ok(StateOutcome {
            current: data(&position["current"])?,
            parentage: data(&position["parentage"])?,
            held: !position["held"].is_null(),
            database: records
                .iter()
                .map(|record| serde_json::from_value(record["data"].clone()).map_err(err))
                .collect::<Result<_, _>>()?,
        })
    }
}

fn open_store(file: Option<&std::path::Path>) -> Result<Arc<dyn ProviderStateStore>, String> {
    match file {
        None => Ok(Arc::new(MemoryStore::new(Default::default()))),
        Some(file) => Ok(Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", file.display()),
                16 * 1024 * 1024,
                2048,
            )
            .map_err(err)?,
        )),
    }
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn check_setup(outcome: CallOutcome, affected: u64) -> Result<(), String> {
    if outcome.status.as_deref() == Some("  ")
        && outcome.problem.is_none()
        && outcome.affected == affected
    {
        Ok(())
    } else {
        Err(format!("fixture setup failed: {outcome:?}"))
    }
}
