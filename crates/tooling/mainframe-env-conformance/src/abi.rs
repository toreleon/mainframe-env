use mainframe_env_cics::cics_abi_library;
use mainframe_env_db2::db2_abi_library;
use mainframe_env_mq::mq_abi_library;
use mainframe_env_source::{
    HOST_ABI_SOURCE_LIBRARY_CONTRACT, HostAbiLibraryDefinition, SourceLimits,
    materialize_host_abi_libraries,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostAbiInventoryReceipt {
    pub schema_version: String,
    pub status: String,
    pub contract: String,
    pub ordered_subsystems: Vec<String>,
    pub libraries: Vec<HostAbiLibraryReceipt>,
    pub total_members: usize,
    pub compiler_owned_members: usize,
    pub generated_coverage_credit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostAbiLibraryReceipt {
    pub id: String,
    pub library_name: String,
    pub subsystem: String,
    pub version: String,
    pub license: String,
    pub origin: String,
    pub identity: String,
    pub members: Vec<HostAbiMemberReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostAbiMemberReceipt {
    pub name: String,
    pub path: String,
    pub sha256: String,
    pub bytes: usize,
}

pub fn verify_host_abi_libraries() -> Result<HostAbiInventoryReceipt, String> {
    let definitions = [cics_abi_library(), db2_abi_library(), mq_abi_library()];
    let materialized = materialize_host_abi_libraries(&definitions, SourceLimits::default())
        .map_err(|error| error.to_string())?;
    if materialized.files.len() != 9
        || materialized.libraries.len() != 3
        || materialized.identities.len() != 3
    {
        return Err("host ABI inventory does not contain three libraries and nine members".into());
    }
    let names = definitions
        .iter()
        .flat_map(|library| library.members.iter().map(|member| member.name))
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        "CMQGMOV", "CMQMDV", "CMQODV", "CMQPMOV", "CMQTML", "CMQV", "DFHAID", "DFHBMSCA", "SQLCA",
    ]);
    if names != expected {
        return Err("host ABI member ownership differs from the reached inventory".into());
    }
    let libraries = definitions.iter().map(library_receipt).collect::<Vec<_>>();
    Ok(HostAbiInventoryReceipt {
        schema_version: "mainframe-env.host-abi-inventory@1".into(),
        status: "pass".into(),
        contract: HOST_ABI_SOURCE_LIBRARY_CONTRACT.into(),
        ordered_subsystems: definitions
            .iter()
            .map(|library| library.subsystem.slug().into())
            .collect(),
        libraries,
        total_members: materialized.files.len(),
        compiler_owned_members: 0,
        generated_coverage_credit: 0,
    })
}

fn library_receipt(definition: &HostAbiLibraryDefinition) -> HostAbiLibraryReceipt {
    HostAbiLibraryReceipt {
        id: definition.id.into(),
        library_name: definition.library_name.into(),
        subsystem: definition.subsystem.slug().into(),
        version: definition.version.into(),
        license: definition.license.into(),
        origin: definition.origin.into(),
        identity: definition.identity(),
        members: definition
            .members
            .iter()
            .map(|member| HostAbiMemberReceipt {
                name: member.name.into(),
                path: format!("compatibility/{}.cpy", member.name),
                sha256: format!("sha256:{:x}", Sha256::digest(member.source.as_bytes())),
                bytes: member.source.len(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsystem_inventory_is_ordered_licensed_and_zero_credit() {
        let receipt = verify_host_abi_libraries().unwrap();
        assert_eq!(receipt.ordered_subsystems, ["cics", "db2", "mq"]);
        assert_eq!(receipt.total_members, 9);
        assert_eq!(receipt.compiler_owned_members, 0);
        assert_eq!(receipt.generated_coverage_credit, 0);
        assert!(receipt.libraries.iter().all(|library| {
            library.license == "Apache-2.0"
                && library.identity.starts_with("sha256:")
                && !library.members.is_empty()
        }));
    }
}
