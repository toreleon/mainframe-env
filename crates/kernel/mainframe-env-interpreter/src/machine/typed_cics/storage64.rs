use super::*;
use crate::storage64::Storage64Problem;

pub(super) fn pending_release_pointer(
    operation: CicsOperation,
    arguments: &BTreeMap<String, BoundedPayload>,
) -> Result<Option<[u8; 8]>, MachineProblem> {
    if operation != CicsOperation::Freemain64 {
        return Ok(None);
    }
    let pointer = arguments
        .get("DATA")
        .or_else(|| arguments.get("DATAPOINTER"))
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    Ok(Some(
        pointer
            .bytes()
            .try_into()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?,
    ))
}

pub(super) fn validate_release_response(
    operation: CicsOperation,
    release64: Option<[u8; 8]>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    if operation != CicsOperation::Freemain64 {
        return if response.outputs.contains_key("FREEMAIN64.POINTER") {
            Err(MachineProblem::UnexpectedHostResult)
        } else {
            Ok(())
        };
    }
    let returned = response.outputs.get("FREEMAIN64.POINTER");
    if response.disposition == CicsDisposition::Complete && response.response == 0 {
        let pointer = returned.ok_or(MachineProblem::UnexpectedHostResult)?;
        if response.outputs.len() != 1
            || pointer.schema() != "mainframe-env.cics.allocated-pointer64@1"
            || Some(pointer.bytes()) != release64.as_ref().map(|bytes| bytes.as_slice())
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    } else if !response.outputs.is_empty() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(())
}

fn classified_pointer(
    machine: &ReferenceMachine,
    bytes: Vec<u8>,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    let address = u64::from_be_bytes(
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?,
    );
    let key = machine
        .storage64_caller_key()
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    let schema =
        match machine
            .storage64
            .can_release(address, machine.invocation.run_unit_id.as_str(), key)
        {
            Ok(()) => "mainframe-env.cics.allocated-pointer64@1",
            Err(Storage64Problem::KeyViolation) => "mainframe-env.cics.key-violation64@1",
            Err(_) => "mainframe-env.cics.invalid-pointer64@1",
        };
    Ok((schema, bytes))
}

pub(super) fn freemain_pointer_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    classified_pointer(machine, read_slot(machine, slot)?)
}

pub(super) fn freemain_data_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    let address = machine
        .storage64_area_bindings
        .get(&slot.qualified_layout_name)
        .copied()
        .unwrap_or(0);
    classified_pointer(machine, address.to_be_bytes().to_vec())
}

pub(super) fn release_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if name != "FREEMAIN64.POINTER" {
        return Ok(false);
    }
    if operation != CicsOperation::Freemain64
        || value.schema() != "mainframe-env.cics.allocated-pointer64@1"
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let address = u64::from_be_bytes(
        value
            .bytes()
            .try_into()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?,
    );
    let key = machine
        .storage64_caller_key()
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    machine
        .storage64
        .release(address, machine.invocation.run_unit_id.as_str(), key)
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    machine
        .storage64_area_bindings
        .retain(|_, bound| *bound != address);
    Ok(true)
}
