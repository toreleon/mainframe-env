//! Next-command-lifetime virtual pointers for EXTRACT CERTIFICATE.

use super::*;

const MARKER_A: &[u8] = b"\0MEC-CERT-A@1";
const MARKER_B: &[u8] = b"\0MEC-CERT-B@1";

pub(super) fn release_previous(machine: &mut ReferenceMachine) {
    let mut expired = Vec::new();
    for marker in machine.static_base_count..machine.bases.len().saturating_sub(2) {
        if machine.bases[marker] == MARKER_A
            && machine.bases[marker + 1] == MARKER_B
            && machine.freed_allocations.contains(&marker)
            && machine.freed_allocations.contains(&(marker + 1))
            && !machine.freed_allocations.contains(&(marker + 2))
        {
            expired.push(marker + 2);
        }
    }
    for base in expired {
        machine.bases[base].fill(0);
        machine.freed_allocations.insert(base);
    }
}

pub(super) fn apply_outputs(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    outputs: &BTreeMap<String, CicsTarget>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    if operation != CicsOperation::ExtractCertificate {
        return Ok(());
    }
    for name in response.outputs.keys() {
        if CicsCertificateOutput::from_name(name).is_some() && !outputs.contains_key(name) {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    }
    if response.response != 0 {
        return Ok(());
    }
    let mut pointer_targets = Vec::new();
    let mut combined = Vec::new();
    for (name, identity) in CICS_CERTIFICATE_OUTPUT_NAMES {
        if !identity.pointer() {
            continue;
        }
        let Some(target) = outputs.get(*name) else {
            continue;
        };
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let pointer = resolved_slot(machine, slot)?;
        if !matches!(pointer.length, 4 | 8) {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let value = response
            .outputs
            .get(*name)
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let offset = match value.schema() {
            "mainframe-env.cics.pointer-null@1" if value.bytes().is_empty() => None,
            "mainframe-env.cics.payload@1" if !value.bytes().is_empty() => {
                let offset = combined.len();
                combined.extend_from_slice(value.bytes());
                Some(offset)
            }
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let capacity = retrieve::allocation_capacity(machine, target)?;
        pointer_targets.push((pointer, offset, capacity));
    }
    if pointer_targets.is_empty() {
        return Ok(());
    }
    if combined.is_empty() {
        for (pointer, _, _) in pointer_targets {
            machine.write_reference(&pointer, &vec![0; pointer.length])?;
        }
        return Ok(());
    }
    let available = pointer_targets
        .iter()
        .filter_map(|(_, offset, capacity)| offset.map(|_| *capacity))
        .min()
        .unwrap_or(0);
    if combined.len() > available
        || machine.bases.len().saturating_add(3) > machine.invocation.limits.max_frames as usize
    {
        return Err(MachineProblem::ResourceExhausted);
    }
    let base = machine
        .bases
        .len()
        .checked_add(2)
        .ok_or(MachineProblem::ResourceExhausted)?;
    let addresses = pointer_targets
        .iter()
        .map(|(pointer, offset, _)| match offset {
            Some(offset) => machine.address_bytes_for(base, *offset, pointer.length),
            None => Ok(vec![0; pointer.length]),
        })
        .collect::<Result<Vec<_>, _>>()?;
    machine.bases.push(MARKER_A.to_vec());
    machine.bases.push(MARKER_B.to_vec());
    machine.freed_allocations.extend([base - 2, base - 1]);
    machine.bases.push(combined);
    for ((pointer, _, _), address) in pointer_targets.into_iter().zip(addresses) {
        machine.write_reference(&pointer, &address)?;
    }
    Ok(())
}
