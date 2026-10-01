use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::machine) enum CicsAddressSet {
    PointerFromOptionalData {
        target: CicsStorageSlot,
        source: Option<CicsStorageSlot>,
    },
    PointerFromData {
        target: CicsStorageSlot,
        source: CicsStorageSlot,
    },
    DataFromPointer {
        target: CicsStorageSlot,
        source: CicsStorageSlot,
    },
}

pub(super) fn action(plan: &CicsEffectPlan) -> Result<Option<CicsAddressSet>, MachineProblem> {
    if !matches!(
        plan.operation,
        CicsPlanOperation::Address | CicsPlanOperation::AddressSet
    ) {
        return Ok(None);
    }
    let slot = |name| {
        plan.operands
            .iter()
            .find(|operand| operand.name == name)
            .and_then(|operand| match &operand.value {
                CicsOperandValue::Storage(slot) => Some(slot.clone()),
                _ => None,
            })
            .ok_or_else(|| invalid_plan("ADDRESS SET storage role is missing"))
    };
    if plan.operation == CicsPlanOperation::Address {
        let source = plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::UsingAddress)
            .and_then(|operand| match &operand.value {
                CicsOperandValue::Storage(slot) => Some(slot.clone()),
                _ => None,
            });
        return Ok(Some(CicsAddressSet::PointerFromOptionalData {
            target: slot(CicsOperandName::CommareaPointer)?,
            source,
        }));
    }
    if plan
        .operands
        .iter()
        .any(|operand| operand.name == CicsOperandName::SetPointer)
    {
        Ok(Some(CicsAddressSet::PointerFromData {
            target: slot(CicsOperandName::SetPointer)?,
            source: slot(CicsOperandName::UsingAddress)?,
        }))
    } else {
        Ok(Some(CicsAddressSet::DataFromPointer {
            target: slot(CicsOperandName::SetAddress)?,
            source: slot(CicsOperandName::UsingPointer)?,
        }))
    }
}

pub(super) fn apply(
    machine: &mut ReferenceMachine,
    action: &CicsAddressSet,
) -> Result<(), MachineProblem> {
    match action {
        CicsAddressSet::PointerFromOptionalData { target, source } => {
            let target = resolved_slot(machine, target)?;
            let address = match source {
                Some(source) => {
                    let source = resolved_slot(machine, source)?;
                    match machine.address_bytes(&source, target.length) {
                        Ok(address) => address,
                        Err(MachineProblem::DataException) if source.layout.linkage => {
                            vec![0xff, 0, 0, 0]
                        }
                        Err(problem) => return Err(problem),
                    }
                }
                None => vec![0xff, 0, 0, 0],
            };
            machine.write_reference(&target, &address)
        }
        CicsAddressSet::PointerFromData { target, source } => {
            let target = resolved_slot(machine, target)?;
            let source = resolved_slot(machine, source)?;
            let address = machine.address_bytes(&source, target.length)?;
            machine.write_reference(&target, &address)
        }
        CicsAddressSet::DataFromPointer { target, source } => {
            let target = resolved_slot(machine, target)?;
            let source = resolved_slot(machine, source)?;
            let pointer = machine.read_reference(&source)?;
            let address = if pointer.as_slice() == [0xff, 0, 0, 0] {
                None
            } else {
                machine.decode_address(&pointer)?
            };
            machine.assign_linkage_address(&target.layout.name, address)
        }
    }
}
