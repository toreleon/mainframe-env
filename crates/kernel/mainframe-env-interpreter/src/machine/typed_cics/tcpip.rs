//! Runtime buffer capacities for checked EXTRACT TCPIP outputs.

use super::*;

pub(super) fn add_output_arguments(
    machine: &ReferenceMachine,
    arguments: &mut BTreeMap<String, BoundedPayload>,
    key: &str,
    identity: CicsTcpipOutput,
    slot: &CicsStorageSlot,
) -> Result<(), MachineProblem> {
    if identity.buffer_length() {
        arguments.insert(
            key.into(),
            payload(
                "mainframe-env.cics.decimal@1",
                read_integer_slot(machine, slot)?.to_string().into_bytes(),
            )?,
        );
    }
    if matches!(
        identity,
        CicsTcpipOutput::ClientName
            | CicsTcpipOutput::ServerName
            | CicsTcpipOutput::ClientAddress
            | CicsTcpipOutput::ServerAddress
    ) {
        arguments.insert(
            format!("{key}.MAXLENGTH"),
            payload(
                "mainframe-env.cics.decimal@1",
                resolved_slot(machine, slot)?
                    .length
                    .to_string()
                    .into_bytes(),
            )?,
        );
    }
    Ok(())
}
