//! Checked AMODE(64) container data transfer at the typed CICS boundary.

use super::*;

fn address(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.try_into().ok()?))
}

pub(super) fn prepare_from(
    machine: &ReferenceMachine,
    arguments: &mut BTreeMap<String, BoundedPayload>,
) -> Result<(), MachineProblem> {
    let pointer = arguments
        .get("FROM")
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let invalid_pointer = || payload("mainframe-env.cics.invalid-pointer64@1", Vec::new());
    let Some(address) = address(pointer.bytes()) else {
        arguments.insert("FROM".into(), invalid_pointer()?);
        return Ok(());
    };
    let key = machine
        .storage64_caller_key()
        .map_err(|_| invalid_plan("PUT64 CONTAINER requires the checked caller ABI"))?;
    let available = match machine.storage64.available_length(
        address,
        machine.invocation.run_unit_id.as_str(),
        key,
    ) {
        Ok(length) => length,
        Err(_) => {
            arguments.insert("FROM".into(), invalid_pointer()?);
            return Ok(());
        }
    };
    let length = match arguments.get("FLENGTH") {
        Some(value) if value.schema() == "mainframe-env.cics.decimal@1" => {
            std::str::from_utf8(value.bytes())
                .ok()
                .and_then(|text| text.parse::<i64>().ok())
                .and_then(|value| usize::try_from(value).ok())
        }
        Some(_) => None,
        None => Some(available),
    };
    let Some(length) = length.filter(|length| *length <= available && *length <= i32::MAX as usize)
    else {
        arguments.insert(
            "FROM".into(),
            payload("mainframe-env.cics.length-error64@1", Vec::new())?,
        );
        return Ok(());
    };
    let bytes = machine
        .read_storage64(address, 0, length)
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    arguments.insert(
        "FROM".into(),
        payload("mainframe-env.cics.storage64-value@1", bytes)?,
    );
    Ok(())
}

pub(super) fn prepare_into(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
    arguments: &mut BTreeMap<String, BoundedPayload>,
) -> Result<(), MachineProblem> {
    let raw = read_slot(machine, slot)?;
    let key = machine
        .storage64_caller_key()
        .map_err(|_| invalid_plan("GET64 CONTAINER requires the checked caller ABI"))?;
    let checked = address(&raw).and_then(|address| {
        machine
            .storage64
            .available_length(address, machine.invocation.run_unit_id.as_str(), key)
            .ok()
            .map(|length| (address, length))
    });
    let (schema, bytes, length) = match checked {
        Some((address, length)) => (
            "mainframe-env.cics.pointer64@1",
            address.to_be_bytes().to_vec(),
            length,
        ),
        None => ("mainframe-env.cics.invalid-pointer64@1", Vec::new(), 0),
    };
    arguments.insert("INTO".into(), payload(schema, bytes)?);
    arguments.insert(
        "INTO.MAXLENGTH".into(),
        payload(
            "mainframe-env.cics.decimal@1",
            length.to_string().into_bytes(),
        )?,
    );
    Ok(())
}
