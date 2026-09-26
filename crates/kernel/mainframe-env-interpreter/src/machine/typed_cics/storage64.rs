use super::*;
use crate::storage64::{Storage64Key, Storage64Problem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Storage64Intent {
    Getmain(Option<[u8; 8]>),
    Freemain([u8; 8]),
    GetContainer(Option<(u64, usize)>),
}

pub(super) fn pending_intent(
    machine: &ReferenceMachine,
    operation: CicsOperation,
    arguments: &BTreeMap<String, BoundedPayload>,
) -> Result<Option<Storage64Intent>, MachineProblem> {
    match operation {
        CicsOperation::GetContainer64 => {
            let target = arguments.get("INTO").and_then(|value| {
                if value.schema() != "mainframe-env.cics.pointer64@1" {
                    return None;
                }
                let address = u64::from_be_bytes(value.bytes().try_into().ok()?);
                let capacity = arguments.get("INTO.MAXLENGTH")?.bytes();
                let capacity = std::str::from_utf8(capacity).ok()?.parse().ok()?;
                Some((address, capacity))
            });
            Ok(Some(Storage64Intent::GetContainer(target)))
        }
        CicsOperation::Getmain64 => {
            let length = arguments
                .get("FLENGTH")
                .and_then(|value| std::str::from_utf8(value.bytes()).ok())
                .and_then(|value| value.parse::<i64>().ok())
                .filter(|value| (1..=2_146_435_056).contains(value))
                .and_then(|value| u32::try_from(value).ok());
            let location = match arguments.get("LOCATION").map(BoundedPayload::bytes) {
                None => Some(0),
                Some(b"LOC24") => Some(1),
                Some(b"LOC31") => Some(2),
                Some(_) => None,
            };
            let key = match machine
                .storage64_caller_key()
                .map_err(|_| invalid_plan("GETMAIN64 requires the checked caller ABI"))?
            {
                Storage64Key::User => 0,
                Storage64Key::Cics => 1,
            };
            let key = if arguments.contains_key("OPTION.CICSDATAKEY") {
                1
            } else if arguments.contains_key("OPTION.USERDATAKEY") {
                0
            } else {
                key
            };
            let expected = if arguments.contains_key("OPTION.SHARED")
                || arguments.contains_key("OPTION.CICSDATAKEY")
                    && arguments.contains_key("OPTION.USERDATAKEY")
            {
                None
            } else {
                length.zip(location).map(|(length, location)| {
                    let mut specification = [
                        location,
                        key,
                        0,
                        u8::from(arguments.contains_key("OPTION.EXECUTABLE")),
                        0,
                        0,
                        0,
                        0,
                    ];
                    specification[4..].copy_from_slice(&length.to_be_bytes());
                    specification
                })
            };
            Ok(Some(Storage64Intent::Getmain(expected)))
        }
        CicsOperation::Freemain64 => {
            let pointer = arguments
                .get("DATA")
                .or_else(|| arguments.get("DATAPOINTER"))
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            Ok(Some(Storage64Intent::Freemain(
                pointer
                    .bytes()
                    .try_into()
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?,
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn validate_response(
    operation: CicsOperation,
    intent: Option<Storage64Intent>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    match (operation, intent) {
        (CicsOperation::GetContainer64, Some(Storage64Intent::GetContainer(target))) => {
            if let Some(value) = response.outputs.get("INTO") {
                if target.is_none()
                    || value.schema() != "mainframe-env.cics.payload@1"
                    || value.bytes().len() > target.unwrap().1
                    || response.disposition != CicsDisposition::Complete
                    || !matches!(response.condition.as_str(), "NORMAL" | "LENGERR")
                {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
            } else if response.condition == "NORMAL" && response.response == 0 {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            Ok(())
        }
        (CicsOperation::PutContainer64, None) => Ok(()),
        (CicsOperation::GetContainer64, _) => Err(MachineProblem::UnexpectedHostResult),
        (CicsOperation::Getmain64, Some(Storage64Intent::Getmain(expected))) => {
            validate_getmain_response(expected, response)
        }
        (CicsOperation::Freemain64, Some(Storage64Intent::Freemain(pointer))) => {
            validate_freemain_response(pointer, response)
        }
        (CicsOperation::Getmain64 | CicsOperation::Freemain64, _) => {
            Err(MachineProblem::UnexpectedHostResult)
        }
        (_, None)
            if !response.outputs.contains_key("SET64")
                && !response.outputs.contains_key("FREEMAIN64.POINTER") =>
        {
            Ok(())
        }
        _ => Err(MachineProblem::UnexpectedHostResult),
    }
}

fn validate_getmain_response(
    expected: Option<[u8; 8]>,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    let output = response.outputs.get("SET64");
    if response.disposition == CicsDisposition::Complete
        && response.condition == "NORMAL"
        && response.response == 0
        && response.response2 == 0
    {
        let allocation = output.ok_or(MachineProblem::UnexpectedHostResult)?;
        if response.outputs.len() != 1
            || allocation.schema() != "mainframe-env.cics.storage64-allocation@1"
            || Some(allocation.bytes()) != expected.as_ref().map(|bytes| bytes.as_slice())
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    } else if response.condition == "LENGERR" && response.response == 22 && response.response2 == 1
    {
        let null = output.ok_or(MachineProblem::UnexpectedHostResult)?;
        if response.outputs.len() != 1
            || null.schema() != "mainframe-env.cics.pointer64-null@1"
            || !null.bytes().is_empty()
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
    } else if !response.outputs.is_empty() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(())
}

fn validate_freemain_response(
    expected: [u8; 8],
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    let returned = response.outputs.get("FREEMAIN64.POINTER");
    if response.disposition == CicsDisposition::Complete
        && response.condition == "NORMAL"
        && response.response == 0
        && response.response2 == 0
    {
        let pointer = returned.ok_or(MachineProblem::UnexpectedHostResult)?;
        if response.outputs.len() != 1
            || pointer.schema() != "mainframe-env.cics.allocated-pointer64@1"
            || pointer.bytes() != expected.as_slice()
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

pub(super) fn freemain_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
    name: CicsOperandName,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    match name {
        CicsOperandName::DataPointer64 => freemain_pointer_argument(machine, slot),
        CicsOperandName::DataArea64 => freemain_data_argument(machine, slot),
        _ => Err(invalid_plan("FREEMAIN64 requires DATA or DATAPOINTER")),
    }
}

pub(super) fn validate_slot(
    slot_use: SlotUse,
    layout: &LayoutMetadata,
) -> Result<(), MachineProblem> {
    if matches!(slot_use, SlotUse::Pointer64Output | SlotUse::Pointer64Input)
        && (layout.category != LayoutCategory::Pointer || layout.length != 8)
    {
        return Err(invalid_plan(
            "AMODE(64) SET requires an eight-byte pointer slot",
        ));
    }
    if matches!(slot_use, SlotUse::DataArea64Input)
        && (layout.length == 0
            || matches!(
                layout.category,
                LayoutCategory::Pointer | LayoutCategory::Pointer32
            ))
    {
        return Err(invalid_plan(
            "FREEMAIN64 DATA requires a declared nonpointer area",
        ));
    }
    Ok(())
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
