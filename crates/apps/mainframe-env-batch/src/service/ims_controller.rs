//! Package-owned IMS batch controllers and their participant completion.
use super::*;

impl BatchService {
    pub(super) fn execute_ims_controller(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        invocation.check()?;
        let selector = ims_controller_selector(input.parameter.as_deref().unwrap_or_default())?;
        let (_publication, controller) = self.admit_controller(&selector)?;
        let controller = controller.ok_or(HostProblem::Unsupported)?;
        if crate::program::control_source_has_data(input, "SYSIN")
            && !matches!(&controller.plan, BatchControllerPlan::ImsPurge { control_dd, .. } if control_dd == "SYSIN")
        {
            return Err(HostProblem::Unsupported);
        }
        let program = controller
            .program
            .path
            .rsplit('/')
            .next()
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_string();
        let mode = controller.selector.mode().unwrap_or_default().to_string();
        match controller.plan {
            BatchControllerPlan::ProgramCall => Err(HostProblem::ProviderFailure),
            BatchControllerPlan::ImsLoad {
                database,
                root_dd,
                child_dd,
                root_record_bytes,
                child_record_bytes,
                parent_key_bytes,
            } => {
                let roots = input_dd_records(input, &root_dd)?;
                let children = input_dd_records(input, &child_dd)?;
                let mut hierarchy = BTreeMap::new();
                for data in roots {
                    if data.len() != root_record_bytes || data.len() < parent_key_bytes {
                        return Err(HostProblem::Malformed);
                    }
                    let key = data[..parent_key_bytes].to_vec();
                    match hierarchy.entry(key) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert((data, Vec::<Vec<u8>>::new()));
                        }
                        std::collections::btree_map::Entry::Occupied(_) => {
                            return Err(HostProblem::Malformed);
                        }
                    }
                }
                for record in children {
                    if record.len() != child_record_bytes || record.len() < parent_key_bytes {
                        return Err(HostProblem::Malformed);
                    }
                    hierarchy
                        .get_mut(&record[..parent_key_bytes])
                        .ok_or(HostProblem::Malformed)?
                        .1
                        .push(record[parent_key_bytes..].to_vec());
                }
                let image = serde_json::json!({
                    "database": database,
                    "roots": hierarchy.into_values().map(|(data, children)| {
                        serde_json::json!({"data": data, "children": children})
                    }).collect::<Vec<_>>()
                });
                let result = self.ims_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Load,
                    Some(database),
                    serde_json::to_vec(&image).map_err(|_| HostProblem::ProviderFailure)?,
                    1,
                )?;
                if result.status == "  " {
                    // Successful completion of the owned batch loader ends its
                    // unit of work. Publish that decision before returning a
                    // successful JES step; foreign runs must not see pending data.
                    let committed = self.ims_call(
                        invocation,
                        job,
                        step,
                        effect_sequence,
                        ImsOperation::Commit,
                        None,
                        Vec::new(),
                        1,
                    )?;
                    if committed.status != "  " {
                        return Err(HostProblem::ProviderFailure);
                    }
                }
                Ok(crate::ProgramOutput {
                    return_code: i32::from(result.status != "  ") * 8,
                    records: vec![
                        format!("DFSRRC00 LOAD SEGMENTS={}", result.affected_segments).into_bytes(),
                    ],
                    dd_outputs: BTreeMap::new(),
                    termination: None,
                })
            }
            BatchControllerPlan::ImsUnload {
                database,
                root_segment,
                child_segment,
                root_output_dd,
                child_output_dd,
                combined_output_dd,
            } => {
                let result = self.ims_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Unload,
                    Some(database),
                    Vec::new(),
                    4_096,
                )?;
                let mut roots = Vec::new();
                let mut children = Vec::new();
                for segment in &result.segments {
                    if segment.name == root_segment {
                        roots.push(segment.data.clone());
                    } else if segment.name == child_segment {
                        let mut record = segment
                            .parent_key
                            .clone()
                            .ok_or(HostProblem::ProviderFailure)?;
                        record.extend_from_slice(&segment.data);
                        children.push(record);
                    } else {
                        return Err(HostProblem::ProviderFailure);
                    }
                }
                let dd_outputs = if let Some(combined) = combined_output_dd {
                    BTreeMap::from([(combined, roots.into_iter().chain(children).collect())])
                } else {
                    BTreeMap::from([
                        (root_output_dd.ok_or(HostProblem::ProviderFailure)?, roots),
                        (
                            child_output_dd.ok_or(HostProblem::ProviderFailure)?,
                            children,
                        ),
                    ])
                };
                Ok(crate::ProgramOutput {
                    return_code: i32::from(result.status != "  ") * 8,
                    records: vec![
                        format!("DFSRRC00 UNLOAD SEGMENTS={}", result.segments.len()).into_bytes(),
                    ],
                    dd_outputs,
                    termination: None,
                })
            }
            BatchControllerPlan::ImsPurge {
                psb,
                root_segment,
                child_segment,
                control_dd,
                required_expiry_days,
                checkpoint_prefix,
                summary_field,
            } => {
                let control = input_dd_records(input, &control_dd)?;
                if control.len() != 1 {
                    return Err(HostProblem::Malformed);
                }
                let range = control
                    .first()
                    .ok_or(HostProblem::Malformed)
                    .and_then(|record| {
                        std::str::from_utf8(record).map_err(|_| HostProblem::Malformed)
                    })?;
                let fields = range.split(',').map(str::trim).collect::<Vec<_>>();
                let expiry_days = fields.first().ok_or(HostProblem::Malformed)?;
                if fields.len() != 4
                    || expiry_days.len() != 2
                    || fields[1].len() != 5
                    || fields[2].len() != 5
                    || !fields[..3]
                        .iter()
                        .all(|field| field.bytes().all(|byte| byte.is_ascii_digit()))
                    || !matches!(fields[3], "Y" | "N")
                {
                    return Err(HostProblem::Malformed);
                }
                // This owned utility implements the pinned single-checkpoint
                // demonstration, not arbitrary checkpoint/debug policies.
                if *expiry_days != required_expiry_days
                    || fields[1] != "00001"
                    || fields[2] != "00001"
                    || fields[3] != "Y"
                {
                    return Err(HostProblem::Unsupported);
                }
                self.ims_dli_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Schedule,
                    Some(psb),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    None,
                    1,
                )?;
                let mut deleted_roots = 0u64;
                let mut deleted_children = 0u64;
                loop {
                    let root = self.ims_dli_call(
                        invocation,
                        job,
                        step,
                        effect_sequence,
                        ImsOperation::GetNext,
                        None,
                        vec![root_segment.clone()],
                        Vec::new(),
                        Vec::new(),
                        None,
                        1,
                    )?;
                    if root.status == "GB" {
                        break;
                    }
                    if !root.status.trim().is_empty() {
                        return Err(HostProblem::ProviderFailure);
                    }
                    loop {
                        let child = self.ims_dli_call(
                            invocation,
                            job,
                            step,
                            effect_sequence,
                            ImsOperation::GetNextParent,
                            None,
                            vec![child_segment.clone()],
                            Vec::new(),
                            Vec::new(),
                            None,
                            1,
                        )?;
                        if child.status == "GE" {
                            break;
                        }
                        if !child.status.trim().is_empty() {
                            return Err(HostProblem::ProviderFailure);
                        }
                        let deleted = self.ims_dli_call(
                            invocation,
                            job,
                            step,
                            effect_sequence,
                            ImsOperation::Delete,
                            None,
                            vec![child_segment.clone()],
                            Vec::new(),
                            Vec::new(),
                            None,
                            1,
                        )?;
                        deleted_children =
                            deleted_children.saturating_add(deleted.affected_segments);
                    }
                    let deleted = self.ims_dli_call(
                        invocation,
                        job,
                        step,
                        effect_sequence,
                        ImsOperation::Delete,
                        None,
                        vec![root_segment.clone()],
                        Vec::new(),
                        Vec::new(),
                        None,
                        1,
                    )?;
                    deleted_roots = deleted_roots.saturating_add(deleted.affected_segments);
                }
                let checkpoint = format!(
                    "{checkpoint_prefix}{:0>3}",
                    job.id.trim_start_matches("JOB")
                );
                self.ims_dli_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Checkpoint,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Some(checkpoint.clone()),
                    1,
                )?;
                Ok(crate::ProgramOutput {
                    return_code: 0,
                    records: vec![format!(
                        "DFSRRC00 {mode} PROGRAM={program} EXPIRY-DAYS={expiry_days} ROOTS={deleted_roots} CHILDREN={deleted_children} CHECKPOINT={checkpoint} {summary_field}={deleted_roots}"
                    )
                    .into_bytes()],
                    dd_outputs: BTreeMap::new(),
                    termination: None,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ims_call(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        operation: ImsOperation,
        psb: Option<String>,
        data: Vec<u8>,
        max_segments: u32,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        invocation.check()?;
        self.ims_dli_call(
            invocation,
            job,
            step,
            effect_sequence,
            operation,
            psb,
            Vec::new(),
            data,
            Vec::new(),
            None,
            max_segments,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn ims_dli_call(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        operation: ImsOperation,
        psb: Option<String>,
        segments: Vec<String>,
        data: Vec<u8>,
        qualifiers: Vec<ImsQualifier>,
        checkpoint_id: Option<String>,
        max_segments: u32,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        invocation.check()?;
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let mutation = if operation.is_mutating() {
            let key = effect_key(job, step, sequence)?;
            Some(Mutation {
                sequence,
                idempotency_key: key,
                transaction: Some(job.id.clone()),
            })
        } else {
            None
        };
        let result = self.invoke_host(
            invocation,
            invocation.original().deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.original().run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.original().deadline_tick,
                idempotency_key: mutation
                    .as_ref()
                    .map(|mutation| mutation.idempotency_key.clone()),
                request: HostRequest::Ims(ImsRequest {
                    operation,
                    psb,
                    pcb: 1,
                    segments,
                    data,
                    qualifiers,
                    checkpoint_id,
                    max_segments,
                    mutation,
                    system: None,
                    q_class: None,
                }),
            },
        );
        match result.outcome? {
            HostResult::Ims(result) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }
}
