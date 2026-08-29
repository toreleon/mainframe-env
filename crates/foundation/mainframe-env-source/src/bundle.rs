use crate::{FileId, LogicalPath, SourceId, SourceProblem};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

/// Enforced limits for one semantic source closure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLimits {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_path_bytes: usize,
    pub max_options: usize,
    pub max_option_bytes: usize,
    pub max_provenance_edges: usize,
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            max_files: 256,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_bytes: 16 * 1024 * 1024,
            max_path_bytes: 512,
            max_options: 128,
            max_option_bytes: 1024,
            max_provenance_edges: 65_536,
        }
    }
}

/// Source layout convention.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceFormat {
    Fixed,
    Free,
    Variable,
}

/// Declared byte encoding.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceEncoding {
    Utf8,
    Ebcdic(u16),
}

/// Exact source bytes and their semantic metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    id: FileId,
    path: LogicalPath,
    bytes: Vec<u8>,
    format: SourceFormat,
    encoding: SourceEncoding,
}

impl SourceFile {
    /// Constructs an input file before bundle-local IDs are assigned.
    pub fn input(
        path: impl Into<String>,
        bytes: Vec<u8>,
        format: SourceFormat,
        encoding: SourceEncoding,
        limits: SourceLimits,
    ) -> Result<Self, SourceProblem> {
        if bytes.len() > limits.max_file_bytes {
            return Err(SourceProblem::FileTooLarge);
        }
        Ok(Self {
            id: FileId::from_index(0)?,
            path: LogicalPath::new(path, limits.max_path_bytes)?,
            bytes,
            format,
            encoding,
        })
    }

    #[must_use]
    pub const fn id(&self) -> FileId {
        self.id
    }

    #[must_use]
    pub fn path(&self) -> &LogicalPath {
        &self.path
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn format(&self) -> SourceFormat {
        self.format
    }

    #[must_use]
    pub const fn encoding(&self) -> SourceEncoding {
        self.encoding
    }
}

/// Half-open byte range in one exact source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRange {
    pub file: FileId,
    pub bytes: Range<usize>,
}

/// Origin of generated or expanded source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceKind {
    Copy,
    Replace,
    Precompiler,
    Generated,
}

/// Path-based provenance input used before IDs are assigned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceEdgeInput {
    pub generated_path: LogicalPath,
    pub generated_bytes: Range<usize>,
    pub origin_path: LogicalPath,
    pub origin_bytes: Range<usize>,
    pub kind: ProvenanceKind,
}

/// Validated provenance edge with bundle-local file IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceEdge {
    pub generated: SourceRange,
    pub origin: SourceRange,
    pub kind: ProvenanceKind,
}

/// Complete deterministic semantic source closure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBundle {
    id: SourceId,
    primary: FileId,
    files: Vec<SourceFile>,
    options: BTreeMap<String, String>,
    provenance: Vec<ProvenanceEdge>,
    total_bytes: usize,
    libraries: Vec<SourceLibrary>,
}

/// One explicitly ordered logical source library.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLibrary {
    name: String,
    members: Vec<LogicalPath>,
}

impl SourceLibrary {
    pub fn new(
        name: impl Into<String>,
        mut members: Vec<LogicalPath>,
        limits: SourceLimits,
    ) -> Result<Self, LibraryProblem> {
        let name = name.into();
        if name.is_empty()
            || name.len() > limits.max_path_bytes
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(LibraryProblem::InvalidLibraryName(name));
        }
        if members.is_empty() || members.len() > limits.max_files {
            return Err(LibraryProblem::InvalidMemberCount(name));
        }
        members.sort();
        if members.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(LibraryProblem::DuplicateMemberPath(name));
        }
        let mut member_names = BTreeSet::new();
        for member in &members {
            let leaf = member
                .as_str()
                .rsplit('/')
                .next()
                .unwrap_or(member.as_str());
            let stem = leaf.split('.').next().unwrap_or(leaf).to_ascii_uppercase();
            if !member_names.insert(stem.clone()) {
                return Err(LibraryProblem::DuplicateMemberName {
                    member: stem,
                    library: name,
                });
            }
        }
        Ok(Self { name, members })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn members(&self) -> &[LogicalPath] {
        &self.members
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LibraryProblem {
    InvalidLibraryName(String),
    InvalidMemberCount(String),
    DuplicateLibraryName(String),
    DuplicateMemberPath(String),
    DuplicateMemberName { member: String, library: String },
    UnknownMemberPath(LogicalPath),
    PrimaryIsLibraryMember(LogicalPath),
    UnassignedFile(LogicalPath),
    MissingMember(String),
    AmbiguousMember { member: String, library: String },
    Source(SourceProblem),
}

impl fmt::Display for LibraryProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "source library failed: {self:?}")
    }
}

impl std::error::Error for LibraryProblem {}

impl From<SourceProblem> for LibraryProblem {
    fn from(problem: SourceProblem) -> Self {
        Self::Source(problem)
    }
}

impl SourceBundle {
    /// Validates, orders, assigns IDs, and fingerprints a source closure.
    pub fn new(
        primary_path: &LogicalPath,
        mut files: Vec<SourceFile>,
        options: BTreeMap<String, String>,
        provenance: Vec<ProvenanceEdgeInput>,
        limits: SourceLimits,
    ) -> Result<Self, SourceProblem> {
        if files.is_empty() || files.len() > limits.max_files {
            return Err(SourceProblem::TooManyFiles);
        }
        if options.len() > limits.max_options {
            return Err(SourceProblem::TooManyOptions);
        }
        if options.iter().any(|(key, value)| {
            key.len() > limits.max_option_bytes || value.len() > limits.max_option_bytes
        }) {
            return Err(SourceProblem::OptionTooLarge);
        }
        if provenance.len() > limits.max_provenance_edges {
            return Err(SourceProblem::TooManyProvenanceEdges);
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut paths = BTreeSet::new();
        let mut total_bytes = 0usize;
        for (index, file) in files.iter_mut().enumerate() {
            if !paths.insert(file.path.clone()) {
                return Err(SourceProblem::DuplicatePath);
            }
            if file.bytes.len() > limits.max_file_bytes {
                return Err(SourceProblem::FileTooLarge);
            }
            total_bytes = total_bytes
                .checked_add(file.bytes.len())
                .ok_or(SourceProblem::TotalBytesExceeded)?;
            if total_bytes > limits.max_total_bytes {
                return Err(SourceProblem::TotalBytesExceeded);
            }
            file.id = FileId::from_index(index)?;
        }
        let by_path: BTreeMap<_, _> = files.iter().map(|file| (file.path.clone(), file)).collect();
        let primary = by_path
            .get(primary_path)
            .map(|file| file.id)
            .ok_or(SourceProblem::PrimaryMissing)?;
        let mut validated_edges = Vec::with_capacity(provenance.len());
        for edge in provenance {
            let generated = by_path
                .get(&edge.generated_path)
                .ok_or(SourceProblem::UnknownProvenanceFile)?;
            let origin = by_path
                .get(&edge.origin_path)
                .ok_or(SourceProblem::UnknownProvenanceFile)?;
            validate_range(&edge.generated_bytes, generated.bytes.len())?;
            validate_range(&edge.origin_bytes, origin.bytes.len())?;
            validated_edges.push(ProvenanceEdge {
                generated: SourceRange {
                    file: generated.id,
                    bytes: edge.generated_bytes,
                },
                origin: SourceRange {
                    file: origin.id,
                    bytes: edge.origin_bytes,
                },
                kind: edge.kind,
            });
        }
        validated_edges.sort_by_key(|edge| {
            (
                edge.generated.file,
                edge.generated.bytes.start,
                edge.origin.file,
                edge.origin.bytes.start,
            )
        });
        let id = fingerprint(primary, &files, &options, &validated_edges);
        let default_members = files
            .iter()
            .filter(|file| file.id != primary)
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let libraries = if default_members.is_empty() {
            Vec::new()
        } else {
            vec![SourceLibrary {
                name: "default".to_string(),
                members: default_members,
            }]
        };
        Ok(Self {
            id,
            primary,
            files,
            options,
            provenance: validated_edges,
            total_bytes,
            libraries,
        })
    }

    /// Validates an explicit, ordered library closure while retaining the
    /// legacy `new` constructor and its identities unchanged.
    pub fn with_libraries(
        primary_path: &LogicalPath,
        files: Vec<SourceFile>,
        libraries: Vec<SourceLibrary>,
        options: BTreeMap<String, String>,
        provenance: Vec<ProvenanceEdgeInput>,
        limits: SourceLimits,
    ) -> Result<Self, LibraryProblem> {
        if libraries.len() > limits.max_files {
            return Err(LibraryProblem::InvalidMemberCount("closure".into()));
        }
        let mut bundle = Self::new(primary_path, files, options, provenance, limits)?;
        let known = bundle
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<BTreeSet<_>>();
        let mut names = BTreeSet::new();
        let mut assigned = BTreeSet::new();
        for library in &libraries {
            if !names.insert(library.name.clone()) {
                return Err(LibraryProblem::DuplicateLibraryName(library.name.clone()));
            }
            for member in &library.members {
                if member == primary_path {
                    return Err(LibraryProblem::PrimaryIsLibraryMember(member.clone()));
                }
                if !known.contains(member) {
                    return Err(LibraryProblem::UnknownMemberPath(member.clone()));
                }
                if !assigned.insert(member.clone()) {
                    return Err(LibraryProblem::DuplicateMemberPath(library.name.clone()));
                }
            }
        }
        for file in &bundle.files {
            if file.path != *primary_path && !assigned.contains(&file.path) {
                return Err(LibraryProblem::UnassignedFile(file.path.clone()));
            }
        }
        bundle.libraries = libraries;
        bundle.id = fingerprint_with_libraries(
            bundle.primary,
            &bundle.files,
            &bundle.options,
            &bundle.provenance,
            &bundle.libraries,
        );
        Ok(bundle)
    }

    #[must_use]
    pub const fn id(&self) -> SourceId {
        self.id
    }

    #[must_use]
    pub const fn primary(&self) -> FileId {
        self.primary
    }

    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    #[must_use]
    pub fn options(&self) -> &BTreeMap<String, String> {
        &self.options
    }

    #[must_use]
    pub fn provenance(&self) -> &[ProvenanceEdge] {
        &self.provenance
    }

    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    #[must_use]
    pub fn libraries(&self) -> &[SourceLibrary] {
        &self.libraries
    }

    pub fn resolve_library_member(&self, name: &str) -> Result<&SourceFile, LibraryProblem> {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(LibraryProblem::MissingMember(name.to_string()));
        }
        for library in &self.libraries {
            let matches = library
                .members
                .iter()
                .filter_map(|path| self.files.iter().find(|file| file.path == *path))
                .filter(|file| {
                    let leaf = file
                        .path
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or(file.path.as_str());
                    leaf.split('.')
                        .next()
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(name))
                })
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => {}
                [file] => return Ok(*file),
                _ => {
                    return Err(LibraryProblem::AmbiguousMember {
                        member: name.to_string(),
                        library: library.name.clone(),
                    });
                }
            }
        }
        Err(LibraryProblem::MissingMember(name.to_string()))
    }

    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.iter().find(|file| file.id == id)
    }
}

fn validate_range(range: &Range<usize>, file_len: usize) -> Result<(), SourceProblem> {
    if range.start > range.end || range.end > file_len {
        Err(SourceProblem::InvalidRange)
    } else {
        Ok(())
    }
}

fn fingerprint(
    primary: FileId,
    files: &[SourceFile],
    options: &BTreeMap<String, String>,
    provenance: &[ProvenanceEdge],
) -> SourceId {
    let mut digest = Sha256::new();
    field(&mut digest, b"mainframe-env.source@1");
    field(&mut digest, &primary.get().to_be_bytes());
    for file in files {
        field(&mut digest, file.path.as_str().as_bytes());
        field(&mut digest, &[format_tag(file.format)]);
        match file.encoding {
            SourceEncoding::Utf8 => field(&mut digest, &[0]),
            SourceEncoding::Ebcdic(ccsid) => {
                field(&mut digest, &[1]);
                field(&mut digest, &ccsid.to_be_bytes());
            }
        }
        field(&mut digest, &file.bytes);
    }
    for (key, value) in options {
        field(&mut digest, key.as_bytes());
        field(&mut digest, value.as_bytes());
    }
    for edge in provenance {
        field(&mut digest, &edge.generated.file.get().to_be_bytes());
        field(&mut digest, &usize_bytes(edge.generated.bytes.start));
        field(&mut digest, &usize_bytes(edge.generated.bytes.end));
        field(&mut digest, &edge.origin.file.get().to_be_bytes());
        field(&mut digest, &usize_bytes(edge.origin.bytes.start));
        field(&mut digest, &usize_bytes(edge.origin.bytes.end));
        field(&mut digest, &[provenance_tag(edge.kind)]);
    }
    SourceId::from_bytes(digest.finalize().into())
}

fn fingerprint_with_libraries(
    primary: FileId,
    files: &[SourceFile],
    options: &BTreeMap<String, String>,
    provenance: &[ProvenanceEdge],
    libraries: &[SourceLibrary],
) -> SourceId {
    let legacy = fingerprint(primary, files, options, provenance);
    let mut digest = Sha256::new();
    field(&mut digest, b"mainframe-env.source-libraries@1");
    field(&mut digest, legacy.as_bytes());
    for library in libraries {
        field(&mut digest, library.name.as_bytes());
        for member in &library.members {
            field(&mut digest, member.as_str().as_bytes());
        }
    }
    SourceId::from_bytes(digest.finalize().into())
}

fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn usize_bytes(value: usize) -> [u8; 8] {
    (value as u64).to_be_bytes()
}

const fn format_tag(format: SourceFormat) -> u8 {
    match format {
        SourceFormat::Fixed => 0,
        SourceFormat::Free => 1,
        SourceFormat::Variable => 2,
    }
}

const fn provenance_tag(kind: ProvenanceKind) -> u8 {
    match kind {
        ProvenanceKind::Copy => 0,
        ProvenanceKind::Replace => 1,
        ProvenanceKind::Precompiler => 2,
        ProvenanceKind::Generated => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(path: &str, bytes: &[u8]) -> SourceFile {
        SourceFile::input(
            path,
            bytes.to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            SourceLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn identity_is_independent_of_input_enumeration_order() {
        let limits = SourceLimits::default();
        let primary = LogicalPath::new("src/main.cbl", limits.max_path_bytes).unwrap();
        let first = SourceBundle::new(
            &primary,
            vec![input("copy/A.cpy", b"A"), input("src/main.cbl", b"MAIN")],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        let second = SourceBundle::new(
            &primary,
            vec![input("src/main.cbl", b"MAIN"), input("copy/A.cpy", b"A")],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        assert_eq!(first.id(), second.id());
    }

    #[test]
    fn exact_bytes_change_identity() {
        let limits = SourceLimits::default();
        let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let first = SourceBundle::new(
            &primary,
            vec![input("main.cbl", b"A")],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        let second = SourceBundle::new(
            &primary,
            vec![input("main.cbl", b"B")],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        assert_ne!(first.id(), second.id());
    }

    #[test]
    fn total_byte_limit_fails_before_publication() {
        let limits = SourceLimits {
            max_total_bytes: 1,
            ..SourceLimits::default()
        };
        let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let result = SourceBundle::new(
            &primary,
            vec![input("main.cbl", b"AB")],
            BTreeMap::new(),
            Vec::new(),
            limits,
        );
        assert_eq!(result.unwrap_err(), SourceProblem::TotalBytesExceeded);
    }

    #[test]
    fn ambient_and_parent_paths_are_rejected() {
        let limit = SourceLimits::default().max_path_bytes;
        assert_eq!(
            LogicalPath::new("../secret", limit),
            Err(SourceProblem::InvalidPath)
        );
        assert_eq!(
            LogicalPath::new("/tmp/main.cbl", limit),
            Err(SourceProblem::InvalidPath)
        );
    }

    #[test]
    fn explicit_library_precedence_is_deterministic_and_identified() {
        let limits = SourceLimits::default();
        let primary = LogicalPath::new("src/main.cbl", limits.max_path_bytes).unwrap();
        let first_path = LogicalPath::new("first/REC.cpy", limits.max_path_bytes).unwrap();
        let second_path = LogicalPath::new("second/REC.cpy", limits.max_path_bytes).unwrap();
        let files = vec![
            input("src/main.cbl", b"COPY REC."),
            input("first/REC.cpy", b"FIRST"),
            input("second/REC.cpy", b"SECOND"),
        ];
        let first = SourceBundle::with_libraries(
            &primary,
            files.clone(),
            vec![
                SourceLibrary::new("first", vec![first_path.clone()], limits).unwrap(),
                SourceLibrary::new("second", vec![second_path.clone()], limits).unwrap(),
            ],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        let second = SourceBundle::with_libraries(
            &primary,
            files,
            vec![
                SourceLibrary::new("second", vec![second_path], limits).unwrap(),
                SourceLibrary::new("first", vec![first_path], limits).unwrap(),
            ],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        assert_eq!(
            first.resolve_library_member("REC").unwrap().bytes(),
            b"FIRST"
        );
        assert_eq!(
            second.resolve_library_member("REC").unwrap().bytes(),
            b"SECOND"
        );
        assert_ne!(first.id(), second.id());
    }

    #[test]
    fn missing_ambiguous_and_unassigned_members_fail_before_compilation() {
        let limits = SourceLimits::default();
        let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let left = LogicalPath::new("copy/REC.cpy", limits.max_path_bytes).unwrap();
        let right = LogicalPath::new("copy/REC.CPY", limits.max_path_bytes).unwrap();
        let ambiguous_library = SourceLibrary::new("copy", vec![left, right], limits);
        assert!(matches!(
            ambiguous_library,
            Err(LibraryProblem::DuplicateMemberName { .. })
        ));

        let bundle = SourceBundle::new(
            &primary,
            vec![
                input("main.cbl", b"COPY REC."),
                input("copy/REC.cpy", b"A"),
                input("copy/REC.CPY", b"B"),
            ],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap();
        assert!(matches!(
            bundle.resolve_library_member("REC"),
            Err(LibraryProblem::AmbiguousMember { .. })
        ));
        assert_eq!(
            bundle.resolve_library_member("MISSING"),
            Err(LibraryProblem::MissingMember("MISSING".into()))
        );

        let unassigned = SourceBundle::with_libraries(
            &primary,
            vec![input("main.cbl", b"MAIN"), input("copy/REC.cpy", b"A")],
            Vec::new(),
            BTreeMap::new(),
            Vec::new(),
            limits,
        );
        assert!(matches!(unassigned, Err(LibraryProblem::UnassignedFile(_))));
    }
}
