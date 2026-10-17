//! Explicit initial local publication under the actual shared publication and provider owners.
use super::*;

const MODE_PATH: &str = "resource/local-complete-publication";
const MODE_BYTES: &[u8] = b"mainframe-env.application-local-complete-initial@1";

pub(super) fn bind_mode(
    package: &ApplicationPackageV2,
    used: &mut BTreeSet<String>,
) -> Result<(), HostProblem> {
    verify_mode(package)?;
    bind_entry(package, EntryKind::Resource, MODE_PATH, MODE_BYTES, used)
}

pub(super) fn verify_mode(package: &ApplicationPackageV2) -> Result<(), HostProblem> {
    let entry = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == MODE_PATH && entry.kind == EntryKind::Resource)
        .ok_or(HostProblem::NotFound)?;
    if package.generation != 1
        || entry.bytes != MODE_BYTES.len()
        || entry.sha256 != format!("sha256:{:x}", Sha256::digest(MODE_BYTES))
        || package.base.blobs.get(&entry.sha256).map(Vec::as_slice) != Some(MODE_BYTES)
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

pub(super) fn prepared(
    expected: &ApplicationGenerationRecord,
    controllers: bool,
    db2: bool,
    ims: bool,
) -> Result<ProviderStateWrite, HostProblem> {
    if expected.generation != 1 {
        return Err(HostProblem::Unsupported);
    }
    let terminal = |present| {
        if present {
            PublicationSectionState::Applied
        } else {
            PublicationSectionState::NotApplicable
        }
    };
    let state = ApplicationPublicationState {
        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
        package: expected.package.to_ascii_uppercase(),
        generation: expected.generation,
        identity: expected.identity.clone(),
        action: PublicationAction::Install,
        controllers: terminal(controllers),
        db2: terminal(db2),
        ims: terminal(ims),
        complete: true,
    };
    let payload = serde_json::to_vec(&state).map_err(|_| HostProblem::InfrastructureFailure)?;
    if payload.len() > MAX_APPLICATION_PUBLICATION_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
            key: state.package.clone(),
            version: 1,
            payload,
        },
        expected_version: None,
    })
}

pub(super) fn selection_row(
    selected: &LocalSelection,
) -> Result<Option<&ProviderStateRecord>, HostProblem> {
    let rows = selected
        .rows
        .iter()
        .filter(|row| row.namespace == APPLICATION_PUBLICATION_NAMESPACE)
        .collect::<Vec<_>>();
    if rows.len() > 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let Some(row) = rows.first().copied() else {
        return Ok(None);
    };
    let state = ApplicationPublicationState::from_payload(&row.payload)?;
    let terminal = |present| {
        if present {
            PublicationSectionState::Applied
        } else {
            PublicationSectionState::NotApplicable
        }
    };
    if row.version != 1
        || row.key != selected.package
        || selected.generation != 1
        || !state.matches_complete(&selected.package, selected.generation, &selected.identity)
        || state.action != PublicationAction::Install
        || state.controllers != terminal(selected.batch.is_some())
        || state.db2 != terminal(selected.db2.is_some())
        || state.ims != terminal(selected.ims.is_some())
        || serde_json::to_vec(&state).map_err(|_| HostProblem::InfrastructureFailure)?
            != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Some(row))
}

impl ProductServer {
    fn recover_complete_local_publication(
        &self,
        record: &ProviderStateRecord,
    ) -> Result<bool, HostProblem> {
        let Some((_, selected)) = self.local_selection(&record.key)? else {
            return Ok(false);
        };
        let Some(expected) = selection_row(&selected)? else {
            return Err(HostProblem::IdempotencyConflict);
        };
        if expected != record
            || self
                .selected_local_application_mode_locked(&record.key, true)?
                .is_none()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        // Full fixed-catalog projections were checked by their real owners above.
        // Legacy IMS metadata must not be re-published over the owned catalog versions.
        Ok(true)
    }
    pub(in crate::product) fn recover_application_publications_locked(
        &self,
        writer: &BatchPublicationWrite<'_>,
    ) -> Result<(), HostProblem> {
        for record in self
            .store
            .list_provider_state(APPLICATION_PUBLICATION_NAMESPACE, 1_024)
            .map_err(store_error)?
        {
            if record.payload.len() > MAX_APPLICATION_PUBLICATION_BYTES {
                return Err(HostProblem::ResourceExhausted);
            }
            if self.recover_complete_local_publication(&record)? {
                continue;
            }
            let state = self.decode_application_publication(&record)?;
            if state.schema_version != APPLICATION_PUBLICATION_CONTRACT
                || state.package.to_ascii_uppercase() != record.key
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let retained = self
                .applications_v2
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .installer
                .generation(&state.package, state.generation, &state.identity)
                .map_err(application_install_problem)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let expected = retained.record().clone();
            drop(retained);
            if !state.complete {
                match state.action {
                    PublicationAction::Install => {
                        self.publish_application_generation_locked(&expected, writer)?;
                    }
                    PublicationAction::Rollback => {
                        self.rollback_application_generation_locked(&expected, writer)?;
                    }
                }
            } else {
                if !state.matches_complete(
                    &expected.package,
                    expected.generation,
                    &expected.identity,
                ) {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let selected = self.selected_application_v2(&expected)?;
                let plan =
                    self.prevalidate_application_publication(&selected, state.action, true)?;
                if plan.db2.is_some() && state.db2 != PublicationSectionState::Applied
                    || !plan.controllers.controllers.is_empty()
                        && state.controllers != PublicationSectionState::Applied
                    || (selected.package().sections.ims_metadata.is_some()
                        || selected.package().sections.ims_tm.is_some())
                        && state.ims != PublicationSectionState::Applied
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                self.apply_application_ims_metadata(&selected)?;
            }
        }
        Ok(())
    }
}

pub(super) fn local_binding(
    identity: &str,
    seed: &str,
    snapshot: &str,
    sources: &BTreeMap<String, String>,
    profiles: &[RacfProfileProjection],
    participants: (
        Option<&Db2CatalogProjection>,
        Option<&ImsCatalogProjection>,
        Option<&MqCatalogProjection>,
        Option<&BatchControllerProjection>,
    ),
    rows: &[ProviderStateRecord],
) -> Result<String, HostProblem> {
    let (catalog, ims_catalog, mq_catalog, controllers) = participants;
    let mut digest = Sha256::new();
    for field in [
        LOCAL_CONTRACT.as_bytes(),
        identity.as_bytes(),
        seed.as_bytes(),
        snapshot.as_bytes(),
    ] {
        digest_online_field(&mut digest, field);
    }
    for (program, source) in sources {
        digest_online_field(&mut digest, program.as_bytes());
        digest_online_field(&mut digest, source.as_bytes());
    }
    for projection in profiles {
        digest_online_field(&mut digest, &projection.content_payload()?);
    }
    if let Some(catalog) = catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(catalog) = ims_catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(catalog) = mq_catalog {
        digest_online_field(&mut digest, &catalog.content_payload()?);
    }
    if let Some(controllers) = controllers {
        digest_online_field(&mut digest, &controllers.content_payload()?);
    }
    for row in rows {
        for field in [
            row.namespace.as_bytes(),
            row.key.as_bytes(),
            row.payload.as_slice(),
        ] {
            digest_online_field(&mut digest, field);
        }
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub(super) fn decode_local_selection(
    row: &ProviderStateRecord,
) -> Result<LocalSelection, HostProblem> {
    if row.version == 0 || row.payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 2 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let value: LocalSelection =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if !matches!(row.namespace.as_str(), LOCAL_SELECTION | LOCAL_GENERATION)
        || value.schema_version != LOCAL_CONTRACT
        || value.package.is_empty()
        || value.generation == 0
        || value.rows.is_empty()
        || value.rows.len() > 16384
        || value.compiled_sources.is_empty()
        || value.compiled_sources.len() > 4096
        || value.compiled_sources.iter().any(|(name, identity)| {
            normalize_online_name(name, 128).as_deref() != Ok(name.as_str())
                || identity.len() != 64
                || !identity
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        || value
            .compiled_sources
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != value
                .rows
                .iter()
                .filter(|row| row.namespace == "online-program")
                .map(|row| row.key.as_str())
                .collect::<BTreeSet<_>>()
        || value
            .db2
            .as_ref()
            .is_some_and(|catalog| !db2::valid_projection(catalog, &value))
        || value
            .ims
            .as_ref()
            .is_some_and(|catalog| !ims::valid_projection(catalog, &value))
        || value
            .mq
            .as_ref()
            .is_some_and(|catalog| !mq::valid_projection(catalog, &value))
        || value
            .batch
            .as_ref()
            .is_some_and(|controllers| !batch::valid_projection(controllers, &value))
        || value.racf.len() > 4096
        || security::projection_keys(&value.racf).is_err()
        || value
            .racf
            .windows(2)
            .any(|pair| pair[0].key() >= pair[1].key())
        || value.binding
            != local_binding(
                &value.identity,
                &value.seed_identity,
                &value.seed_snapshot,
                &value.compiled_sources,
                &value.racf,
                (
                    value.db2.as_ref(),
                    value.ims.as_ref(),
                    value.mq.as_ref(),
                    value.batch.as_ref(),
                ),
                &value.rows,
            )?
        || value
            .rows
            .windows(2)
            .any(|pair| (&pair[0].namespace, &pair[0].key) >= (&pair[1].namespace, &pair[1].key))
        || (row.namespace == LOCAL_SELECTION && row.key != value.package)
        || (row.namespace == LOCAL_GENERATION
            && row.key != format!("{}:{}", value.package, value.generation))
        || value
            .rows
            .iter()
            .any(|row| row.validate_write(MAX_PROVIDER_COMPOSITION_BYTES).is_err())
        || value.rows.iter().any(|row| {
            !matches!(
                row.namespace.as_str(),
                "cics-program"
                    | "cics-map"
                    | "cics-file-alias"
                    | "online-program"
                    | "online-transaction"
                    | "batch-program"
                    | APPLICATION_PUBLICATION_NAMESPACE
            )
        })
        || runtime_publication::selection_row(&value).is_err()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(value)
}
