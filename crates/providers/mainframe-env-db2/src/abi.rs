use mainframe_env_source::{
    HOST_ABI_SOURCE_LIBRARY_CONTRACT, HOST_ABI_SOURCE_LICENSE, HOST_ABI_SOURCE_ORIGIN,
    HostAbiLibraryDefinition, HostAbiMember, HostAbiSubsystem,
};

const MEMBERS: [HostAbiMember; 1] = [HostAbiMember {
    name: "SQLCA",
    behavior: "136-byte SQL communication area with reached SQLCODE, SQLERRM, and SQLSTATE fields.",
    source: include_str!("../abi/SQLCA.cpy"),
}];

const DB2_ABI: HostAbiLibraryDefinition = HostAbiLibraryDefinition {
    contract: HOST_ABI_SOURCE_LIBRARY_CONTRACT,
    id: "mainframe-env.db2-cobol-abi@1",
    library_name: "db2-cobol-abi-v1",
    subsystem: HostAbiSubsystem::Db2,
    version: "0.2.0-reached",
    license: HOST_ABI_SOURCE_LICENSE,
    origin: HOST_ABI_SOURCE_ORIGIN,
    members: &MEMBERS,
};

#[must_use]
pub const fn db2_abi_library() -> HostAbiLibraryDefinition {
    DB2_ABI
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_source::{SourceLimits, materialize_host_abi_libraries};

    #[test]
    fn db2_owns_exact_reached_sqlca() {
        let library = db2_abi_library();
        assert_eq!(library.members[0].name, "SQLCA");
        assert!(
            library.members[0]
                .source
                .contains("SQLCABC PIC S9(9) COMP-5 VALUE +136")
        );
        assert_eq!(
            materialize_host_abi_libraries(&[library], SourceLimits::default())
                .unwrap()
                .files
                .len(),
            1
        );
    }
}
