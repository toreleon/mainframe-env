use super::*;

impl BatchService {
    pub(super) fn dataset_attributes(
        &self,
        invocation: &(impl RunInput + ?Sized),
        dataset: &DatasetName,
        effect_sequence: &mut u64,
    ) -> Result<DatasetAttributes, HostProblem> {
        invocation.check()?;
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let result = self.invoke_host(
            invocation,
            invocation.original().deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.original().run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.original().deadline_tick,
                idempotency_key: None,
                request: HostRequest::Dataset(DatasetRequest::Attributes {
                    dataset: dataset.clone(),
                }),
            },
        );
        let HostResult::Dataset(DatasetResult::Attributes { attributes, .. }) = result.outcome?
        else {
            return Err(HostProblem::ProviderFailure);
        };
        invocation.check()?;
        Ok(attributes)
    }

    pub(super) fn effective_dd_attributes(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        dd_index: usize,
        effect_sequence: &mut u64,
    ) -> Result<DatasetAttributes, HostProblem> {
        invocation.check()?;
        let step_index = job
            .plan
            .steps
            .iter()
            .position(|candidate| candidate.name == step.name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.resolve_dcb_attributes(
            invocation,
            job,
            step_index,
            dd_index,
            false,
            0,
            effect_sequence,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_dcb_attributes(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step_index: usize,
        dd_index: usize,
        source: bool,
        depth: usize,
        effect_sequence: &mut u64,
    ) -> Result<DatasetAttributes, HostProblem> {
        invocation.check()?;
        if depth > 64 {
            return Err(HostProblem::ResourceExhausted);
        }
        let step = job
            .plan
            .steps
            .get(step_index)
            .ok_or(HostProblem::Malformed)?;
        let dd = step.dds.get(dd_index).ok_or(HostProblem::Malformed)?;
        let base = if let Some(reference) = dcb_reference(dd) {
            if let Some(suffix) = reference.strip_prefix("*.") {
                let (target_step, target_dd) = suffix
                    .rsplit_once('.')
                    .map_or((step.name.as_str(), suffix), |(step, dd)| (step, dd));
                let target_step_index = job
                    .plan
                    .steps
                    .iter()
                    .position(|candidate| candidate.name == target_step)
                    .ok_or(HostProblem::Malformed)?;
                if target_step_index > step_index {
                    return Err(HostProblem::Malformed);
                }
                let target_dd_index = job.plan.steps[target_step_index]
                    .dds
                    .iter()
                    .position(|candidate| candidate.name == target_dd)
                    .ok_or(HostProblem::Malformed)?;
                if target_step_index == step_index && target_dd_index >= dd_index {
                    return Err(HostProblem::Malformed);
                }
                Some(self.resolve_dcb_attributes(
                    invocation,
                    job,
                    target_step_index,
                    target_dd_index,
                    true,
                    depth + 1,
                    effect_sequence,
                )?)
            } else {
                let dataset =
                    DatasetName::new(reference, 128).map_err(|_| HostProblem::Malformed)?;
                self.authorize(
                    invocation,
                    "DATASET",
                    dataset.as_str(),
                    AccessIntent::Read,
                    next_effect_sequence(invocation, effect_sequence)?,
                )?;
                Some(self.dataset_attributes(invocation, &dataset, effect_sequence)?)
            }
        } else if source {
            let raw = dd.dataset.as_ref().ok_or(HostProblem::Malformed)?;
            let name = resolved_dataset(job, dd, raw, &job.dataset_resolutions);
            let dataset = DatasetName::new(name, 128).map_err(|_| HostProblem::Malformed)?;
            match self.dataset_attributes(invocation, &dataset, effect_sequence) {
                Ok(attributes) => Some(attributes),
                Err(HostProblem::NotFound)
                    if dd.disposition.first() == Some(&crate::Disposition::New)
                        && (dd.organization.is_some()
                            || dd.record_format.is_some()
                            || dd.logical_record_length.is_some()) =>
                {
                    None
                }
                Err(problem) => return Err(problem),
            }
        } else {
            None
        };
        let mut attributes = base.unwrap_or(dataset_attributes_for_dd(dd)?);
        if let Some(organization) = dd.organization.as_deref() {
            attributes.organization = dd_organization(organization)?;
        }
        if let Some(record_format) = dd.record_format.as_deref() {
            attributes.record_format = dd_record_format(record_format)?;
        }
        if let Some(length) = dd.logical_record_length {
            attributes.logical_record_length = length;
        }
        if let Some(ccsid) = dd.ccsid {
            attributes.ccsid = Some(ccsid);
        }
        invocation.check()?;
        Ok(attributes)
    }

    pub(super) fn resolve_dd_access_path(
        &self,
        invocation: &(impl RunInput + ?Sized),
        dataset: &DatasetName,
        effect_sequence: &mut u64,
    ) -> Result<DatasetName, HostProblem> {
        invocation.check()?;
        let mut current = dataset.clone();
        for _ in 0..2 {
            invocation.check()?;
            let DatasetResult::CatalogEntries { entries, .. } = self.ams_dataset_read(
                invocation,
                effect_sequence,
                DatasetRequest::ListCatalog {
                    pattern: current.as_str().to_string(),
                    start: None,
                    max_items: 2,
                },
            )?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            let entry = entries
                .into_iter()
                .find(|entry| entry.name == current)
                .ok_or(HostProblem::NotFound)?;
            if !matches!(
                entry.kind,
                mainframe_env_host_api::CatalogEntryKind::Path
                    | mainframe_env_host_api::CatalogEntryKind::AlternateIndex
            ) {
                return Err(HostProblem::NotFound);
            }
            current = entry.related.ok_or(HostProblem::InfrastructureFailure)?;
            invocation.check()?;
        }
        Ok(current)
    }
}

fn dcb_reference(dd: &crate::DdPlan) -> Option<&str> {
    let value = dd
        .parameters
        .iter()
        .find(|parameter| parameter.identity().keyword() == "DCB")?
        .normalized_value();
    let first = value.trim_matches(['(', ')']).split(',').next()?.trim();
    (!first.is_empty() && !first.contains('=')).then_some(first)
}

fn dataset_attributes_for_dd(dd: &crate::DdPlan) -> Result<DatasetAttributes, HostProblem> {
    let organization = dd_organization(dd.organization.as_deref().unwrap_or("PS"))?;
    let record_format = dd_record_format(dd.record_format.as_deref().unwrap_or("V"))?;
    let logical_record_length = dd.logical_record_length.unwrap_or({
        if matches!(
            record_format,
            RecordFormat::Fixed | RecordFormat::FixedBlocked
        ) {
            80
        } else {
            32_760
        }
    });
    Ok(DatasetAttributes {
        organization,
        record_format,
        logical_record_length,
        key_offset: None,
        key_length: None,
        ccsid: dd.ccsid.or(Some(37)),
    })
}

fn dd_organization(value: &str) -> Result<DatasetOrganization, HostProblem> {
    Ok(match value {
        "PS" => DatasetOrganization::Sequential,
        "PO" => DatasetOrganization::Partitioned,
        _ => return Err(HostProblem::Unsupported),
    })
}

fn dd_record_format(value: &str) -> Result<RecordFormat, HostProblem> {
    Ok(match value {
        "F" => RecordFormat::Fixed,
        "FB" => RecordFormat::FixedBlocked,
        "V" => RecordFormat::Variable,
        "VB" => RecordFormat::VariableBlocked,
        "U" => RecordFormat::Undefined,
        _ => return Err(HostProblem::Unsupported),
    })
}
