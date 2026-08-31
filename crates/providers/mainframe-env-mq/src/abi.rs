use mainframe_env_source::{
    HOST_ABI_SOURCE_LIBRARY_CONTRACT, HOST_ABI_SOURCE_LICENSE, HOST_ABI_SOURCE_ORIGIN,
    HostAbiLibraryDefinition, HostAbiMember, HostAbiSubsystem,
};

const MEMBERS: [HostAbiMember; 6] = [
    HostAbiMember {
        name: "CMQGMOV",
        behavior: "Reached MQGMO options and wait interval in a bounded version-4 layout.",
        source: include_str!("../abi/CMQGMOV.cpy"),
    },
    HostAbiMember {
        name: "CMQMDV",
        behavior: "364-byte MQMD version-2 layout with reached message and correlation fields.",
        source: include_str!("../abi/CMQMDV.cpy"),
    },
    HostAbiMember {
        name: "CMQODV",
        behavior: "400-byte MQOD version-4 layout with reached object names and type.",
        source: include_str!("../abi/CMQODV.cpy"),
    },
    HostAbiMember {
        name: "CMQPMOV",
        behavior: "Reached MQPMO options in a bounded version-3 layout.",
        source: include_str!("../abi/CMQPMOV.cpy"),
    },
    HostAbiMember {
        name: "CMQTML",
        behavior: "Reached 684-byte MQ trigger-message layout.",
        source: include_str!("../abi/CMQTML.cpy"),
    },
    HostAbiMember {
        name: "CMQV",
        behavior: "Reached MQ completion, reason, option, format, and identifier constants.",
        source: include_str!("../abi/CMQV.cpy"),
    },
];

const MQ_ABI: HostAbiLibraryDefinition = HostAbiLibraryDefinition {
    contract: HOST_ABI_SOURCE_LIBRARY_CONTRACT,
    id: "mainframe-env.mq-cobol-abi@1",
    library_name: "mq-cobol-abi-v1",
    subsystem: HostAbiSubsystem::Mq,
    version: "0.2.0-reached",
    license: HOST_ABI_SOURCE_LICENSE,
    origin: HOST_ABI_SOURCE_ORIGIN,
    members: &MEMBERS,
};

#[must_use]
pub const fn mq_abi_library() -> HostAbiLibraryDefinition {
    MQ_ABI
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_source::{SourceLimits, materialize_host_abi_libraries};

    #[test]
    fn mq_owns_exact_reached_abi_members() {
        let library = mq_abi_library();
        assert_eq!(library.members.len(), 6);
        assert!(
            library
                .members
                .iter()
                .find(|member| member.name == "CMQMDV")
                .unwrap()
                .source
                .contains("MQMD-CORRELID PIC X(24)")
        );
        assert!(
            library
                .members
                .iter()
                .find(|member| member.name == "CMQV")
                .unwrap()
                .source
                .contains("MQRC-NO-MSG-AVAILABLE PIC S9(9) BINARY VALUE 2033")
        );
        assert_eq!(
            materialize_host_abi_libraries(&[library], SourceLimits::default())
                .unwrap()
                .files
                .len(),
            6
        );
    }
}
