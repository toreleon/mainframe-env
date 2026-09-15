use mainframe_env_source::{
    HOST_ABI_SOURCE_LIBRARY_CONTRACT, HOST_ABI_SOURCE_LICENSE, HOST_ABI_SOURCE_ORIGIN,
    HostAbiLibraryDefinition, HostAbiMember, HostAbiSubsystem,
};

const MEMBERS: [HostAbiMember; 2] = [
    HostAbiMember {
        name: "DFHAID",
        behavior: "Reached 3270 attention identifiers represented as exact one-byte values.",
        source: include_str!("../abi/DFHAID.cpy"),
    },
    HostAbiMember {
        name: "DFHBMSCA",
        behavior: "Reached BMS field attributes and extended colors represented as exact bytes.",
        source: include_str!("../abi/DFHBMSCA.cpy"),
    },
];

const CICS_ABI: HostAbiLibraryDefinition = HostAbiLibraryDefinition {
    contract: HOST_ABI_SOURCE_LIBRARY_CONTRACT,
    id: "mainframe-env.cics-cobol-abi@1",
    library_name: "cics-cobol-abi-v1",
    subsystem: HostAbiSubsystem::Cics,
    version: "0.2.0-reached",
    license: HOST_ABI_SOURCE_LICENSE,
    origin: HOST_ABI_SOURCE_ORIGIN,
    members: &MEMBERS,
};

#[must_use]
pub const fn cics_abi_library() -> HostAbiLibraryDefinition {
    CICS_ABI
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_source::{SourceLimits, materialize_host_abi_libraries};

    #[test]
    fn cics_owns_exact_reached_abi_members() {
        let library = cics_abi_library();
        assert_eq!(
            library
                .members
                .iter()
                .map(|member| member.name)
                .collect::<Vec<_>>(),
            ["DFHAID", "DFHBMSCA"]
        );
        assert!(
            library.members[0]
                .source
                .contains("DFHENTER PIC X VALUE X'7D'")
        );
        for definition in [
            "DFHCLRP PIC X VALUE X'6A'",
            "DFHPA3 PIC X VALUE X'6B'",
            "DFHPEN PIC X VALUE X'7E'",
            "DFHTRIG PIC X VALUE X'7F'",
            "DFHOPID PIC X VALUE X'E6'",
            "DFHMSRE PIC X VALUE X'E7'",
        ] {
            assert!(library.members[0].source.contains(definition));
        }
        assert!(
            library.members[1]
                .source
                .contains("DFHRED PIC X VALUE X'F2'")
        );
        assert_eq!(
            materialize_host_abi_libraries(&[library], SourceLimits::default())
                .unwrap()
                .files
                .len(),
            2
        );
    }
}
