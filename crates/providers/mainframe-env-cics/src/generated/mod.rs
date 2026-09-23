mod command_descriptors;

pub(super) use command_descriptors::{
    CICS_AID_NAMES, CICS_CONDITION_NAMES, CicsCommandDescriptor, CicsCommandFamily,
    command_descriptor,
};

#[cfg(test)]
pub(super) use command_descriptors::CICS_COMMAND_DESCRIPTORS;
