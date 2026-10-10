//! Prospective catalog checks shared by read-only admission and existing mutations.
use super::*;

impl Db2Service {
    pub(super) fn prospective_catalog_install(
        &self,
        state: &State,
        catalog: Db2CatalogGeneration,
    ) -> Result<Option<State>, HostProblem> {
        if !state.pending.is_empty() || !state.cursors.is_empty() {
            return Err(HostProblem::Condition {
                name: "DB2-CATALOG-BUSY".into(),
                response: -904,
                response2: 0,
            });
        }
        let application = catalog.application.to_ascii_uppercase();
        let generations = state.catalog_generations.get(&application);
        let retained = generations.and_then(|generations| generations.get(&catalog.generation));
        let selected = state.installations.get(&application);
        if retained.is_some_and(|retained| retained.identity != catalog.identity)
            || selected.is_some_and(|selected| {
                selected.generation > catalog.generation
                    || selected.generation == catalog.generation
                        && selected.identity != catalog.identity
            })
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if selected.is_some_and(|selected| selected.generation == catalog.generation) {
            return Ok(None);
        }
        let mut next = state.scoped_snapshot();
        apply_catalog_generation(&mut next, catalog, self.limits)?;
        validate_state(&next, self.limits)?;
        Ok(Some(next))
    }

    pub(super) fn prospective_catalog_rollback(
        &self,
        state: &State,
        application: &str,
        generation: u64,
    ) -> Result<State, HostProblem> {
        if !state.pending.is_empty() || !state.cursors.is_empty() {
            return Err(HostProblem::Condition {
                name: "DB2-CATALOG-BUSY".into(),
                response: -904,
                response2: 0,
            });
        }
        let application = application.to_ascii_uppercase();
        let target = state
            .catalog_generations
            .get(&application)
            .and_then(|generations| generations.get(&generation))
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = state.scoped_snapshot();
        snapshot_selected_catalog(&mut next, &application)?;
        let current_tables = next
            .installations
            .get(&application)
            .map(|installation| installation.tables.clone())
            .unwrap_or_default();
        let target_tables = target.schemas.keys().cloned().collect::<BTreeSet<_>>();
        for name in current_tables.difference(&target_tables) {
            match next.table_provenance.get(name).cloned() {
                Some(TableProvenance::Legacy) => {
                    let legacy = next
                        .legacy_snapshots
                        .get(name)
                        .cloned()
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    next.schemas.insert(name.clone(), legacy.schema.clone());
                    next.tables.insert(name.clone(), legacy.table.clone());
                }
                Some(TableProvenance::Application { owner, .. }) if owner == application => {
                    next.schemas.remove(name);
                    next.tables.remove(name);
                    next.table_provenance.remove(name);
                    next.legacy_snapshots.remove(name);
                }
                _ => return Err(HostProblem::InfrastructureFailure),
            }
        }
        for (name, schema) in &target.schemas {
            next.schemas.insert(name.clone(), schema.clone());
            if !next.table_provenance.contains_key(name) {
                let provenance = if next.legacy_snapshots.contains_key(name) {
                    TableProvenance::Legacy
                } else {
                    TableProvenance::Application {
                        owner: application.clone(),
                        generation: target.generation,
                    }
                };
                next.table_provenance.insert(name.clone(), provenance);
            }
        }
        for (name, table) in &target.tables {
            next.tables.insert(name.clone(), table.clone());
        }
        next.installations.insert(
            application,
            CatalogInstallation {
                generation: target.generation,
                identity: target.identity.clone(),
                tables: target.schemas.keys().cloned().collect(),
            },
        );
        next.catalog_version = next
            .catalog_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        validate_foreign_keys(&next)?;
        validate_state(&next, self.limits)?;
        Ok(next)
    }
}
