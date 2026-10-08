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
    let (libraries, file_count) = preflight_host_abi_libraries(definitions, limits)?;
    let mut files = Vec::with_capacity(file_count);
    let mut identities = Vec::with_capacity(definitions.len());
    for definition in definitions {
        for member in definition.members {
            files.push(
                SourceFile::input(
                    format!("compatibility/{}.cpy", member.name),
                    member.source.as_bytes().to_vec(),
                    SourceFormat::Free,
                    SourceEncoding::Utf8,
                    limits,
                )
                .map_err(HostAbiProblem::Source)?,
            );
        }
        identities.push(definition.identity());
    }
    Ok(MaterializedHostAbiLibraries {
        files,
        libraries,
        identities,
    })
}

fn preflight_host_abi_libraries(
    definitions: &[HostAbiLibraryDefinition],
    limits: SourceLimits,
) -> Result<(Vec<SourceLibrary>, usize), HostAbiProblem> {
    if definitions.is_empty() || definitions.len() > 64 {
        return Err(HostAbiProblem::InvalidDefinition);
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut all_member_names = BTreeSet::new();
    let mut file_count = 0usize;
    let mut total_bytes = 0usize;
    let mut libraries = Vec::with_capacity(definitions.len());
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
        file_count = file_count
            .checked_add(definition.members.len())
            .filter(|count| *count <= limits.max_files)
            .ok_or(HostAbiProblem::InvalidDefinition)?;
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
                || !member_names.insert(member.name)
            {
                return Err(HostAbiProblem::InvalidDefinition);
            }
            if member.source.len() > limits.max_file_bytes {
                return Err(HostAbiProblem::Source(SourceProblem::FileTooLarge));
            }
            total_bytes = total_bytes
                .checked_add(member.source.len())
                .filter(|bytes| *bytes <= limits.max_total_bytes)
                .ok_or(HostAbiProblem::Source(SourceProblem::TotalBytesExceeded))?;
            if !member.source.contains("PIC")
                || member
                    .source
                    .as_bytes()
                    .windows(b"placeholder".len())
                    .any(|window| window.eq_ignore_ascii_case(b"placeholder"))
            {
                return Err(HostAbiProblem::InvalidDefinition);
            }
            if !all_member_names.insert(member.name) {
                return Err(HostAbiProblem::DuplicateMember);
            }
            let path = format!("compatibility/{}.cpy", member.name);
            let logical =
                LogicalPath::new(&path, limits.max_path_bytes).map_err(HostAbiProblem::Source)?;
            members.push(logical);
        }
        libraries.push(
            SourceLibrary::new(definition.library_name, members, limits)
                .map_err(HostAbiProblem::Library)?,
        );
    }
    Ok((libraries, file_count))
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
    const OTHER_MEMBERS: [HostAbiMember; 1] = [HostAbiMember {
        name: "OTHER",
        behavior: "Another one-byte field.",
        source: "       01 OTHER PIC X.\n",
    }];
    const OTHER_DEFINITION: HostAbiLibraryDefinition = HostAbiLibraryDefinition {
        id: "mainframe-env.other-abi@1",
        library_name: "other-abi-v1",
        members: &OTHER_MEMBERS,
        ..DEFINITION
    };
    const TWO_MEMBERS: [HostAbiMember; 2] = [OTHER_MEMBERS[0], MEMBERS[0]];
    const EXAMPLE_IDENTITY: &str =
        "sha256:423ddf970146d5ae114da4e40e3dd0cb7341365e7d6b50e672f10538fc337412";

    #[test]
    fn materialization_rejects_aggregate_bytes_across_members() {
        let definition = HostAbiLibraryDefinition {
            members: &TWO_MEMBERS,
            ..DEFINITION
        };
        let limits = SourceLimits {
            max_total_bytes: MEMBERS[0].source.len() + OTHER_MEMBERS[0].source.len() - 1,
            ..SourceLimits::default()
        };
        assert_eq!(
            materialize_host_abi_libraries(&[definition], limits),
            Err(HostAbiProblem::Source(SourceProblem::TotalBytesExceeded))
        );
    }

    #[test]
    fn materialization_rejects_aggregate_bytes_across_libraries() {
        let limits = SourceLimits {
            max_total_bytes: MEMBERS[0].source.len() + OTHER_MEMBERS[0].source.len() - 1,
            ..SourceLimits::default()
        };
        assert_eq!(
            materialize_host_abi_libraries(&[DEFINITION, OTHER_DEFINITION], limits),
            Err(HostAbiProblem::Source(SourceProblem::TotalBytesExceeded))
        );
    }

    #[test]
    fn materialization_rejects_per_file_bytes() {
        let limits = SourceLimits {
            max_file_bytes: MEMBERS[0].source.len() - 1,
            ..SourceLimits::default()
        };
        assert_eq!(
            materialize_host_abi_libraries(&[OTHER_DEFINITION, DEFINITION], limits),
            Err(HostAbiProblem::Source(SourceProblem::FileTooLarge))
        );
    }

    #[test]
    fn materialization_rejects_total_file_count_across_libraries() {
        let limits = SourceLimits {
            max_files: 1,
            ..SourceLimits::default()
        };
        assert_eq!(
            materialize_host_abi_libraries(&[DEFINITION, OTHER_DEFINITION], limits),
            Err(HostAbiProblem::InvalidDefinition)
        );
    }

    #[test]
    fn materialization_accepts_exact_bounds_preserving_order_bytes_and_identity() {
        let limits = SourceLimits {
            max_files: 2,
            max_file_bytes: MEMBERS[0].source.len(),
            max_total_bytes: MEMBERS[0].source.len() + OTHER_MEMBERS[0].source.len(),
            max_path_bytes: "compatibility/EXAMPLE.cpy".len(),
            ..SourceLimits::default()
        };
        let materialized =
            materialize_host_abi_libraries(&[OTHER_DEFINITION, DEFINITION], limits).unwrap();
        assert_eq!(materialized.files.len(), 2);
        assert_eq!(materialized.files[0].bytes(), b"       01 OTHER PIC X.\n");
        assert_eq!(materialized.files[1].bytes(), b"       01 EXAMPLE PIC X.\n");
        assert_eq!(materialized.libraries[0].name(), "other-abi-v1");
        assert_eq!(materialized.libraries[1].name(), "example-abi-v1");
        assert_eq!(materialized.identities[1], EXAMPLE_IDENTITY);

        let definition = HostAbiLibraryDefinition {
            members: &TWO_MEMBERS,
            ..DEFINITION
        };
        let materialized = materialize_host_abi_libraries(&[definition], limits).unwrap();
        assert_eq!(materialized.files[0].bytes(), b"       01 OTHER PIC X.\n");
        assert_eq!(materialized.files[1].bytes(), b"       01 EXAMPLE PIC X.\n");
        assert_eq!(
            materialized.libraries[0]
                .members()
                .iter()
                .map(LogicalPath::as_str)
                .collect::<Vec<_>>(),
            ["compatibility/EXAMPLE.cpy", "compatibility/OTHER.cpy"]
        );
    }

    #[test]
    fn materialization_rejects_duplicate_library_ids_names_and_members() {
        let same_id = HostAbiLibraryDefinition {
            id: DEFINITION.id,
            ..OTHER_DEFINITION
        };
        let same_library_name = HostAbiLibraryDefinition {
            library_name: DEFINITION.library_name,
            ..OTHER_DEFINITION
        };
        for duplicate in [same_id, same_library_name] {
            assert_eq!(
                materialize_host_abi_libraries(&[DEFINITION, duplicate], SourceLimits::default()),
                Err(HostAbiProblem::InvalidDefinition)
            );
        }
        let same_member = HostAbiLibraryDefinition {
            members: &MEMBERS,
            ..OTHER_DEFINITION
        };
        assert_eq!(
            materialize_host_abi_libraries(&[DEFINITION, same_member], SourceLimits::default()),
            Err(HostAbiProblem::DuplicateMember)
        );
        const DUPLICATE_MEMBERS: [HostAbiMember; 2] = [MEMBERS[0], MEMBERS[0]];
        let duplicate = HostAbiLibraryDefinition {
            members: &DUPLICATE_MEMBERS,
            ..DEFINITION
        };
        assert_eq!(
            materialize_host_abi_libraries(&[duplicate], SourceLimits::default()),
            Err(HostAbiProblem::InvalidDefinition)
        );
    }

    #[test]
    fn materialization_rejects_invalid_definition_metadata() {
        for invalid in [
            HostAbiLibraryDefinition {
                contract: "other@1",
                ..DEFINITION
            },
            HostAbiLibraryDefinition {
                id: "",
                ..DEFINITION
            },
            HostAbiLibraryDefinition {
                version: "",
                ..DEFINITION
            },
            HostAbiLibraryDefinition {
                license: "LicenseRef-unknown",
                ..DEFINITION
            },
            HostAbiLibraryDefinition {
                origin: "vendor source",
                ..DEFINITION
            },
            HostAbiLibraryDefinition {
                members: &[],
                ..DEFINITION
            },
        ] {
            assert_eq!(
                materialize_host_abi_libraries(
                    &[OTHER_DEFINITION, invalid],
                    SourceLimits::default()
                ),
                Err(HostAbiProblem::InvalidDefinition)
            );
        }
        for name in ["", "invalid/name"] {
            let invalid = HostAbiLibraryDefinition {
                library_name: name,
                ..OTHER_DEFINITION
            };
            assert_eq!(
                materialize_host_abi_libraries(&[DEFINITION, invalid], SourceLimits::default()),
                Err(HostAbiProblem::Library(LibraryProblem::InvalidLibraryName(
                    name.into()
                )))
            );
        }
    }

    #[test]
    fn materialization_rejects_invalid_member_metadata_and_source() {
        static INVALID_MEMBERS: [[HostAbiMember; 1]; 6] = [
            [HostAbiMember {
                name: "",
                ..MEMBERS[0]
            }],
            [HostAbiMember {
                name: "invalid/name",
                ..MEMBERS[0]
            }],
            [HostAbiMember {
                behavior: "",
                ..MEMBERS[0]
            }],
            [HostAbiMember {
                source: "",
                ..MEMBERS[0]
            }],
            [HostAbiMember {
                source: "       01 EXAMPLE.\n",
                ..MEMBERS[0]
            }],
            [HostAbiMember {
                source: "       01 EXAMPLE PIC X. *> PlAcEhOlDeR\n",
                ..MEMBERS[0]
            }],
        ];
        for members in &INVALID_MEMBERS {
            let definition = HostAbiLibraryDefinition {
                members,
                ..DEFINITION
            };
            assert_eq!(
                materialize_host_abi_libraries(&[definition], SourceLimits::default()),
                Err(HostAbiProblem::InvalidDefinition)
            );
        }
    }

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
