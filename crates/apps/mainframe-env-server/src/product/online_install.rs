//! Existing online admission body; local selection overlap and uncertainty refuse before writes.
use super::*;
impl ProductServer {
    pub fn install_online_application(
        &self,
        definition: OnlineApplicationDefinition,
    ) -> Result<OnlineInstallReceipt, HostProblem> {
        let _publication = self.publication_write()?;
        self.refuse_local_online_definition(&definition)?;
        if definition.programs.is_empty()
            || definition.transactions.is_empty()
            || definition.maps.is_empty()
            || definition.programs.len() > 4096
            || definition.transactions.len() > 4096
        {
            return Err(HostProblem::Malformed);
        }
        let mut programs = BTreeMap::new();
        let mut artifact_records = Vec::with_capacity(definition.programs.len());
        let mut identity = Sha256::new();
        for definition in &definition.programs {
            let name = normalize_online_name(&definition.name, 128)?;
            if programs
                .insert(name.clone(), definition.artifact.clone())
                .is_some()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let record = artifact::admitted_record(
                &definition.artifact,
                &definition.payload,
                &definition.manifest,
                &definition.semantic_identity,
            )?;
            if let Some(existing) = artifact_records
                .iter()
                .find(|existing: &&ArtifactRecord| existing.artifact == record.artifact)
            {
                if existing != &record {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                artifact_records.push(record);
            }
            digest_online_field(&mut identity, name.as_bytes());
            digest_online_field(&mut identity, definition.artifact.as_str().as_bytes());
        }
        for record in artifact_records {
            self.artifacts.put_artifact(record).map_err(store_error)?;
        }
        let mut transactions = BTreeMap::new();
        for (transaction, program) in &definition.transactions {
            let transaction = normalize_online_name(transaction, 16)?;
            let program = normalize_online_name(program, 128)?;
            if !programs.contains_key(&program)
                || transactions
                    .insert(transaction.clone(), program.clone())
                    .is_some()
            {
                return Err(HostProblem::NotFound);
            }
            digest_online_field(&mut identity, transaction.as_bytes());
            digest_online_field(&mut identity, program.as_bytes());
        }
        let program_names = programs.keys().cloned().collect::<BTreeSet<_>>();
        self.cics.register_programs(&program_names)?;
        for map in &definition.maps {
            self.cics.register_map(map.clone())?;
            digest_online_field(&mut identity, map.mapset.as_bytes());
            digest_online_field(&mut identity, map.map.as_bytes());
            digest_online_field(&mut identity, &(map.fields.len() as u64).to_be_bytes());
        }
        let mut current_programs = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut current_transactions = self
            .online_transactions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut writes = Vec::new();
        for (name, artifact) in &programs {
            if let Some(existing) = current_programs.get(name) {
                if existing != artifact {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                writes.push(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "online-program".into(),
                        key: name.clone(),
                        version: 1,
                        payload: artifact.as_str().as_bytes().to_vec(),
                    },
                    expected_version: None,
                });
            }
        }
        for (transaction, program) in &transactions {
            if let Some(existing) = current_transactions.get(transaction) {
                if existing != program {
                    return Err(HostProblem::IdempotencyConflict);
                }
            } else {
                writes.push(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "online-transaction".into(),
                        key: transaction.clone(),
                        version: 1,
                        payload: program.as_bytes().to_vec(),
                    },
                    expected_version: None,
                });
            }
        }
        let replayed = writes.is_empty();
        if !replayed {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        current_programs.extend(programs);
        current_transactions.extend(transactions);
        Ok(OnlineInstallReceipt {
            programs: definition.programs.len(),
            transactions: definition.transactions.len(),
            maps: definition.maps.len(),
            identity: format!("sha256:{:x}", identity.finalize()),
            replayed,
        })
    }
}
