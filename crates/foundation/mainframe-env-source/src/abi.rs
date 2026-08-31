use crate::{
    LibraryProblem, LogicalPath, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits, SourceProblem,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const HOST_ABI_SOURCE_LIBRARY_CONTRACT: &str = "mainframe-env.host-abi-source-library@1";
pub const HOST_ABI_SOURCE_LICENSE: &str = "Apache-2.0";
pub const HOST_ABI_SOURCE_ORIGIN: &str =
    "repository-authored behavioral compatibility definition; no vendor source text";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostAbiSubsystem {
    Cics,
    Db2,
    Mq,
}

impl HostAbiSubsystem {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Cics => "cics",
            Self::Db2 => "db2",
            Self::Mq => "mq",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostAbiMember {
    pub name: &'static str,
    pub behavior: &'static str,
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostAbiLibraryDefinition {
    pub contract: &'static str,
    pub id: &'static str,
    pub library_name: &'static str,
    pub subsystem: HostAbiSubsystem,
    pub version: &'static str,
    pub license: &'static str,
    pub origin: &'static str,
    pub members: &'static [HostAbiMember],
}

impl HostAbiLibraryDefinition {
    #[must_use]
    pub fn identity(self) -> String {
        let mut digest = Sha256::new();
        for value in [
            self.contract,
            self.id,
            self.library_name,
            self.subsystem.slug(),
            self.version,
            self.license,
            self.origin,
        ] {
            digest_field(&mut digest, value.as_bytes());
        }
        for member in self.members {
            digest_field(&mut digest, member.name.as_bytes());
            digest_field(&mut digest, member.behavior.as_bytes());
            digest_field(&mut digest, member.source.as_bytes());
        }
        format!("sha256:{:x}", digest.finalize())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedHostAbiLibraries {
    pub files: Vec<SourceFile>,
    pub libraries: Vec<SourceLibrary>,
    pub identities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAbiProblem {
    InvalidDefinition,
    DuplicateLibrary,
    DuplicateMember,
    Source(SourceProblem),
    Library(LibraryProblem),
}

impl fmt::Display for HostAbiProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host ABI source library failed: {self:?}")
    }
}

impl std::error::Error for HostAbiProblem {}

pub fn materialize_host_abi_libraries(
    definitions: &[HostAbiLibraryDefinition],
    limits: SourceLimits,
) -> Result<MaterializedHostAbiLibraries, HostAbiProblem> {
    if definitions.is_empty() || definitions.len() > 64 {
        return Err(HostAbiProblem::InvalidDefinition);
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    let mut libraries = Vec::with_capacity(definitions.len());
    let mut identities = Vec::with_capacity(definitions.len());
    for definition in definitions {
        if definition.contract != HOST_ABI_SOURCE_LIBRARY_CONTRACT
            || definition.license != HOST_ABI_SOURCE_LICENSE
            || definition.origin != HOST_ABI_SOURCE_ORIGIN
            || definition.id.is_empty()
            || definition.id.len() > 128
            || definition.version.is_empty()
            || definition.version.len() > 64
            || definition.members.is_empty()
            || definition.members.len() > limits.max_files
            || !ids.insert(definition.id)
            || !names.insert(definition.library_name)
        {
            return Err(HostAbiProblem::InvalidDefinition);
        }
        let mut member_names = BTreeSet::new();
        let mut members = Vec::with_capacity(definition.members.len());
        for member in definition.members {
            if member.name.is_empty()
                || member.name.len() > 128
                || !member.name.bytes().all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_')
                })
                || member.behavior.is_empty()
                || member.behavior.len() > 1_024
                || member.source.is_empty()
                || !member.source.contains("PIC")
                || member.source.to_ascii_lowercase().contains("placeholder")
                || !member_names.insert(member.name)
            {
                return Err(HostAbiProblem::InvalidDefinition);
            }
            let path = format!("compatibility/{}.cpy", member.name);
            if !paths.insert(path.clone()) {
                return Err(HostAbiProblem::DuplicateMember);
            }
            let logical =
                LogicalPath::new(&path, limits.max_path_bytes).map_err(HostAbiProblem::Source)?;
            let file = SourceFile::input(
                path,
                member.source.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .map_err(HostAbiProblem::Source)?;
            members.push(logical);
            files.push(file);
        }
        libraries.push(
            SourceLibrary::new(definition.library_name, members, limits)
                .map_err(HostAbiProblem::Library)?,
        );
        identities.push(definition.identity());
    }
    if files.len() > limits.max_files {
        return Err(HostAbiProblem::InvalidDefinition);
    }
    Ok(MaterializedHostAbiLibraries {
        files,
        libraries,
        identities,
    })
}

fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMBERS: [HostAbiMember; 1] = [HostAbiMember {
        name: "EXAMPLE",
        behavior: "Example one-byte field.",
        source: "       01 EXAMPLE PIC X.\n",
    }];
    const DEFINITION: HostAbiLibraryDefinition = HostAbiLibraryDefinition {
        contract: HOST_ABI_SOURCE_LIBRARY_CONTRACT,
        id: "mainframe-env.example-abi@1",
        library_name: "example-abi-v1",
        subsystem: HostAbiSubsystem::Cics,
        version: "1",
        license: HOST_ABI_SOURCE_LICENSE,
        origin: HOST_ABI_SOURCE_ORIGIN,
        members: &MEMBERS,
    };

    #[test]
    fn licensed_ordered_libraries_materialize_with_content_identity() {
        let closure =
            materialize_host_abi_libraries(&[DEFINITION], SourceLimits::default()).unwrap();
        assert_eq!(closure.files.len(), 1);
        assert_eq!(closure.libraries[0].name(), "example-abi-v1");
        assert_eq!(closure.identities, [DEFINITION.identity()]);
        assert_eq!(
            closure.files[0].path().as_str(),
            "compatibility/EXAMPLE.cpy"
        );
    }

    #[test]
    fn incompatible_license_and_duplicate_members_fail_closed() {
        let mut incompatible = DEFINITION;
        incompatible.license = "LicenseRef-unknown";
        assert_eq!(
            materialize_host_abi_libraries(&[incompatible], SourceLimits::default()),
            Err(HostAbiProblem::InvalidDefinition)
        );
        assert_eq!(
            materialize_host_abi_libraries(&[DEFINITION, DEFINITION], SourceLimits::default()),
            Err(HostAbiProblem::InvalidDefinition)
        );
    }
}
