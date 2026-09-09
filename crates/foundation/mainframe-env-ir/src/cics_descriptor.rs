//! Executable descriptors for the bounded typed CICS dialect.

use crate::{CicsPlanOperation, Effect, OperationIdentity};

/// Runtime import required by every executable operation in this dialect.
pub const CICS_RUNTIME_IMPORT: &str = "host.cics";

const READ_EFFECTS: &[Effect] = &[
    Effect::DatasetRead,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Condition,
    Effect::Transaction,
];
const REWRITE_EFFECTS: &[Effect] = &[
    Effect::DatasetWrite,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Condition,
    Effect::Transaction,
];
const SYNCPOINT_EFFECTS: &[Effect] = &[Effect::MemoryWrite, Effect::Condition, Effect::Transaction];

/// Static executable facts owned by the typed CICS dialect.
///
/// Option direction and operation-specific plan shape remain owned by the
/// CICS plan codec validator. Host request mapping and provider transitions
/// deliberately do not belong in this descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsExecutableDescriptor {
    /// Plan operation represented by this executable identity.
    pub operation: CicsPlanOperation,
    /// Executable operation namespace.
    pub namespace: &'static str,
    /// Executable operation name.
    pub name: &'static str,
    /// Executable operation semantic major.
    pub major: u16,
    /// Exact declared effect sequence.
    pub effects: &'static [Effect],
    /// Runtime import required during legalization.
    pub runtime_import: &'static str,
}

impl CicsExecutableDescriptor {
    /// Builds the checked generic IR identity for this descriptor.
    #[must_use]
    pub fn identity(self) -> OperationIdentity {
        OperationIdentity::new(self.namespace, self.name, self.major)
            .expect("dialect-owned typed CICS identity")
    }

    /// Reports whether this descriptor owns an already-decoded identity.
    #[must_use]
    pub fn matches_identity(self, identity: &OperationIdentity) -> bool {
        identity.namespace() == self.namespace
            && identity.name() == self.name
            && identity.major() == self.major
    }
}

/// Complete registry for the bounded typed CICS executable pilot.
pub const CICS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 3] = [
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Read,
        namespace: "cics.file",
        name: "read",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Rewrite,
        namespace: "cics.file",
        name: "rewrite",
        major: 1,
        effects: REWRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Syncpoint,
        namespace: "cics.recovery",
        name: "syncpoint",
        major: 1,
        effects: SYNCPOINT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
];

/// Resolves the executable descriptor for a decoded CICS plan operation.
#[must_use]
pub const fn cics_executable_descriptor(
    operation: CicsPlanOperation,
) -> &'static CicsExecutableDescriptor {
    match operation {
        CicsPlanOperation::Read => &CICS_EXECUTABLE_DESCRIPTORS[0],
        CicsPlanOperation::Rewrite => &CICS_EXECUTABLE_DESCRIPTORS[1],
        CicsPlanOperation::Syncpoint => &CICS_EXECUTABLE_DESCRIPTORS[2],
    }
}

/// Resolves an executable descriptor without accepting adjacent legacy CICS
/// operation identities.
#[must_use]
pub fn cics_executable_descriptor_for_identity(
    identity: &OperationIdentity,
) -> Option<&'static CicsExecutableDescriptor> {
    CICS_EXECUTABLE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.matches_identity(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn executable_registry_is_complete_unique_and_round_trips() {
        assert_eq!(
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.operation)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CicsPlanOperation::Read,
                CicsPlanOperation::Rewrite,
                CicsPlanOperation::Syncpoint,
            ])
        );
        assert_eq!(
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.identity())
                .collect::<BTreeSet<_>>()
                .len(),
            CICS_EXECUTABLE_DESCRIPTORS.len()
        );
        for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
            assert_eq!(
                cics_executable_descriptor(descriptor.operation),
                &descriptor
            );
            assert_eq!(
                cics_executable_descriptor_for_identity(&descriptor.identity()),
                Some(&descriptor)
            );
            assert!(!descriptor.effects.is_empty());
            assert_eq!(descriptor.runtime_import, CICS_RUNTIME_IMPORT);
        }
    }
}
