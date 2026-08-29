use std::fmt;

/// Stable file identity within one source bundle.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileId(u32);

impl FileId {
    /// Validates a stable one-based file identifier from a wire representation.
    pub fn new(value: u32) -> Result<Self, SourceProblem> {
        if value == 0 {
            Err(SourceProblem::UnknownProvenanceFile)
        } else {
            Ok(Self(value))
        }
    }

    pub(crate) fn from_index(index: usize) -> Result<Self, SourceProblem> {
        let one_based = index
            .checked_add(1)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(SourceProblem::TooManyFiles)?;
        Ok(Self(one_based))
    }

    /// Returns the stable one-based numeric representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Validated repository-independent logical path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalPath(String);

impl LogicalPath {
    /// Validates a slash-separated logical path.
    pub fn new(value: impl Into<String>, max_bytes: usize) -> Result<Self, SourceProblem> {
        let value = value.into();
        if value.is_empty() {
            return Err(SourceProblem::EmptyPath);
        }
        if value.len() > max_bytes {
            return Err(SourceProblem::PathTooLong);
        }
        if value.starts_with('/') || value.starts_with('\\') || value.contains('\0') {
            return Err(SourceProblem::InvalidPath);
        }
        if value
            .split(['/', '\\'])
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Err(SourceProblem::InvalidPath);
        }
        Ok(Self(value.replace('\\', "/")))
    }

    /// Returns the normalized logical path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// SHA-256 semantic identity for an entire source bundle.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId([u8; 32]);

impl SourceId {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns lower-case hexadecimal without allocating intermediate objects.
    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

impl fmt::Debug for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SourceId")
            .field(&self.to_hex())
            .finish()
    }
}

/// Stable validation failures for source construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceProblem {
    EmptyPath,
    InvalidPath,
    PathTooLong,
    DuplicatePath,
    PrimaryMissing,
    TooManyFiles,
    FileTooLarge,
    TotalBytesExceeded,
    TooManyOptions,
    OptionTooLarge,
    TooManyProvenanceEdges,
    InvalidRange,
    UnknownProvenanceFile,
}

impl fmt::Display for SourceProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            match self {
                Self::EmptyPath => "source path is empty",
                Self::InvalidPath => "source path is not a portable logical path",
                Self::PathTooLong => "source path exceeds its byte limit",
                Self::DuplicatePath => "source bundle contains a duplicate logical path",
                Self::PrimaryMissing => "source bundle primary path is absent",
                Self::TooManyFiles => "source bundle file limit exceeded",
                Self::FileTooLarge => "source file byte limit exceeded",
                Self::TotalBytesExceeded => "source bundle total byte limit exceeded",
                Self::TooManyOptions => "compiler option count limit exceeded",
                Self::OptionTooLarge => "compiler option key or value limit exceeded",
                Self::TooManyProvenanceEdges => "source provenance edge limit exceeded",
                Self::InvalidRange => "source provenance range is invalid",
                Self::UnknownProvenanceFile => "source provenance names an unknown file",
            }
        )
    }
}

impl std::error::Error for SourceProblem {}
