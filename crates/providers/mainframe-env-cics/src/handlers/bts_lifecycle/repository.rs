//! Repository-scoped process-name reservation, separate from the process tree.

use super::*;

const NAMESPACE: &str = "cics-bts-repository-name-v1";
const SCHEMA: &str = "mainframe-env.cics.bts-repository-name@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    schema_version: String,
    repository_resource: String,
    process_name: String,
    process_type: String,
    root_id: String,
    pending_uow: Option<String>,
    #[serde(skip)]
    row_version: u64,
}

impl Reservation {
    fn key(repository: &str, name: &str) -> Result<String, HostProblem> {
        validate_repository(repository)?;
        validate_name(name, 36, true)?;
        let mut key = String::with_capacity((repository.len() + name.len()) * 2 + 1);
        for byte in repository.bytes() {
            key.push_str(&format!("{byte:02x}"));
        }
        key.push('/');
        for byte in name.bytes() {
            key.push_str(&format!("{byte:02x}"));
        }
        Ok(key)
    }

    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != SCHEMA
            || validate_repository(&self.repository_resource).is_err()
            || validate_name(&self.process_name, 36, true).is_err()
            || validate_name(&self.process_type, 8, true).is_err()
            || validate_activity_id(&self.root_id).is_err()
            || self
                .pending_uow
                .as_deref()
                .is_some_and(|uow| validate_identifier(uow, 256).is_err())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }

    fn write(&self, expected_version: Option<u64>) -> Result<ProviderStateMutation, HostProblem> {
        self.validate()?;
        let payload = serde_json::to_vec(self).map_err(|_| HostProblem::ResourceExhausted)?;
        if payload.len() > 1024 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: Self::key(&self.repository_resource, &self.process_name)?,
                version: expected_version
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?,
                payload,
            },
            expected_version,
        }))
    }
}

fn validate_repository(resource: &str) -> Result<(), HostProblem> {
    validate_identifier(resource, 44)?;
    if !resource.bytes().all(|byte| {
        byte.is_ascii_uppercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'@' | b'$' | b'#' | b'_')
    }) {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn duplicate_name() -> HostProblem {
    HostProblem::Condition {
        name: "PROCESSERR".into(),
        response: 108,
        response2: 2,
    }
}

impl<'a> BtsLifecycleStore<'a> {
    fn load_reservation(
        &self,
        repository: &str,
        name: &str,
    ) -> Result<Option<Reservation>, HostProblem> {
        let key = Reservation::key(repository, name)?;
        let Some(row) = self
            .store
            .get_provider_state(NAMESPACE, &key)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > 1024 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut reservation: Reservation =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        reservation.row_version = row.version;
        reservation.validate()?;
        if reservation.repository_resource != repository || reservation.process_name != name {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(reservation))
    }

    pub(super) fn reserve_process_name(
        &self,
        repository: &str,
        process: &BtsProcess,
        run_unit: &str,
    ) -> Result<ProviderStateMutation, HostProblem> {
        if self.load_reservation(repository, &process.name)?.is_some() {
            return Err(duplicate_name());
        }
        Reservation {
            schema_version: SCHEMA.into(),
            repository_resource: repository.into(),
            process_name: process.name.clone(),
            process_type: process.process_type.clone(),
            root_id: process.root_id.clone(),
            pending_uow: Some(run_unit.into()),
            row_version: 0,
        }
        .write(None)
    }

    pub(super) fn process_name_reserved(
        &self,
        repository: &str,
        name: &str,
    ) -> Result<bool, HostProblem> {
        Ok(self.load_reservation(repository, name)?.is_some())
    }

    pub(super) fn settle_process_name(
        &self,
        repository: &str,
        process: &BtsProcess,
        run_unit: &str,
        commit: bool,
    ) -> Result<ProviderStateMutation, HostProblem> {
        let mut reservation = self
            .load_reservation(repository, &process.name)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if reservation.process_type != process.process_type
            || reservation.root_id != process.root_id
            || reservation.pending_uow.as_deref() != Some(run_unit)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if commit {
            reservation.pending_uow = None;
            reservation.write(Some(reservation.row_version))
        } else {
            Ok(ProviderStateMutation::Delete {
                namespace: NAMESPACE.into(),
                key: Reservation::key(repository, &process.name)?,
                expected_version: reservation.row_version,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    fn process(process_type: &str, uow: &str) -> BtsProcess {
        let root = BtsLifecycleStore::root_id(process_type, "ORDER", uow).unwrap();
        BtsProcess::new(process_type, "ORDER", &root, "MAIN", "BTS1", "USER", uow).unwrap()
    }

    fn define(
        authority: &BtsLifecycleStore<'_>,
        process_type: &str,
        repository: &str,
        uow: &str,
    ) -> Result<(), HostProblem> {
        authority.define_process_in_repository_exact(
            process(process_type, uow),
            repository,
            uow,
            uow,
            "USER",
            "define",
            [1; 32],
        )
    }

    #[test]
    fn process_name_is_unique_across_types_sharing_a_repository() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        define(&authority, "TYPE1", "BTS.REPO", "UOW1").unwrap();
        assert_eq!(
            define(&authority, "TYPE2", "BTS.REPO", "UOW2"),
            Err(duplicate_name())
        );
        assert!(authority.load_process("TYPE2", "ORDER").unwrap().is_none());
        authority.finish_uow("UOW1", "UOW1", "USER", false).unwrap();
        define(&authority, "TYPE2", "BTS.REPO", "UOW2").unwrap();
        authority.finish_uow("UOW2", "UOW2", "USER", true).unwrap();
        assert_eq!(
            define(&authority, "TYPE3", "BTS.REPO", "UOW3"),
            Err(duplicate_name())
        );
        define(&authority, "TYPE3", "OTHER.REPO", "UOW3").unwrap();
        assert!(authority.load_process("TYPE3", "ORDER").unwrap().is_some());
    }

    #[test]
    fn repository_reservation_survives_sqlite_reopen_and_replays_exactly() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-repository-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            define(&authority, "TYPE1", "BTS.REPO", "UOW1").unwrap();
        }
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            define(&authority, "TYPE1", "BTS.REPO", "UOW1").unwrap();
            assert_eq!(
                define(&authority, "TYPE2", "BTS.REPO", "UOW2"),
                Err(duplicate_name())
            );
            authority.finish_uow("UOW1", "UOW1", "USER", true).unwrap();
        }
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            assert_eq!(
                define(&authority, "TYPE2", "BTS.REPO", "UOW2"),
                Err(duplicate_name())
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
