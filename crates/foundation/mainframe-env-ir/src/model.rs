use mainframe_env_diagnostics::SourceSpan;
use std::collections::BTreeMap;
use std::fmt;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);
        impl $name {
            pub(crate) fn from_index(index: usize) -> Result<Self, IrProblem> {
                u32::try_from(index)
                    .map(Self)
                    .map_err(|_| IrProblem::IdExhausted)
            }
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

typed_id!(RegionId);
typed_id!(BlockId);
typed_id!(OperationId);
typed_id!(ValueId);
typed_id!(StorageId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OperationIdentity {
    namespace: String,
    name: String,
    major: u16,
}

impl OperationIdentity {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        major: u16,
    ) -> Result<Self, IrProblem> {
        let namespace = namespace.into();
        let name = name.into();
        if major == 0 || !valid_name(&namespace) || !valid_name(&name) {
            return Err(IrProblem::InvalidIdentity);
        }
        Ok(Self {
            namespace,
            name,
            major,
        })
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub const fn major(&self) -> u16 {
        self.major
    }
}

impl fmt::Display for OperationIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}.{}", self.namespace, self.major, self.name)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeIdentity {
    namespace: String,
    name: String,
    major: u16,
}

impl TypeIdentity {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        major: u16,
    ) -> Result<Self, IrProblem> {
        let namespace = namespace.into();
        let name = name.into();
        if major == 0 || !valid_name(&namespace) || !valid_name(&name) {
            return Err(IrProblem::InvalidIdentity);
        }
        Ok(Self {
            namespace,
            name,
            major,
        })
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub const fn major(&self) -> u16 {
        self.major
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Attribute {
    Integer(i64),
    Boolean(bool),
    Text(String),
    Bytes(Vec<u8>),
    Type(TypeIdentity),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Effect {
    MemoryRead,
    MemoryWrite,
    DatasetRead,
    DatasetWrite,
    TerminalRead,
    TerminalWrite,
    ProgramControl,
    Security,
    Spool,
    Clock,
    Audit,
    Suspension,
    Condition,
    Transaction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageReference {
    pub storage: StorageId,
    pub offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageRegion {
    pub id: StorageId,
    pub name: String,
    pub size: u64,
    pub alias_of: Option<StorageReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub id: OperationId,
    pub identity: OperationIdentity,
    pub operands: Vec<ValueId>,
    pub results: Vec<ValueId>,
    pub attributes: BTreeMap<String, Attribute>,
    pub effects: Vec<Effect>,
    pub storage: Vec<StorageReference>,
    pub location: Option<SourceSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Region {
    pub id: RegionId,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Module {
    pub(crate) regions: Vec<Region>,
    pub(crate) storage: Vec<StorageRegion>,
    pub(crate) value_count: u32,
}

impl Module {
    #[must_use]
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }
    #[must_use]
    pub fn storage(&self) -> &[StorageRegion] {
        &self.storage
    }
    #[must_use]
    pub const fn value_count(&self) -> u32 {
        self.value_count
    }

    pub(crate) fn from_parts(
        regions: Vec<Region>,
        storage: Vec<StorageRegion>,
        value_count: u32,
    ) -> Self {
        Self {
            regions,
            storage,
            value_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrLimits {
    pub max_regions: usize,
    pub max_blocks: usize,
    pub max_operations: usize,
    pub max_values: usize,
    pub max_storage_regions: usize,
    pub max_storage_bytes: u64,
    pub max_operands_per_operation: usize,
    pub max_results_per_operation: usize,
    pub max_attributes_per_operation: usize,
    pub max_effects_per_operation: usize,
    pub max_references_per_operation: usize,
    pub max_string_bytes: usize,
    pub max_attribute_bytes: usize,
}

impl Default for IrLimits {
    fn default() -> Self {
        Self {
            max_regions: 256,
            max_blocks: 16_384,
            max_operations: 1_000_000,
            max_values: 1_000_000,
            max_storage_regions: 65_536,
            max_storage_bytes: 256 * 1024 * 1024,
            max_operands_per_operation: 256,
            max_results_per_operation: 64,
            max_attributes_per_operation: 128,
            max_effects_per_operation: 32,
            max_references_per_operation: 64,
            max_string_bytes: 4096,
            max_attribute_bytes: 1024 * 1024,
        }
    }
}

pub struct ModuleBuilder {
    limits: IrLimits,
    regions: Vec<Region>,
    storage: Vec<StorageRegion>,
    blocks: usize,
    operations: usize,
    values: usize,
}

impl ModuleBuilder {
    #[must_use]
    pub fn new(limits: IrLimits) -> Self {
        Self {
            limits,
            regions: Vec::new(),
            storage: Vec::new(),
            blocks: 0,
            operations: 0,
            values: 0,
        }
    }

    pub fn add_storage(
        &mut self,
        name: impl Into<String>,
        size: u64,
        alias_of: Option<StorageReference>,
    ) -> Result<StorageId, IrProblem> {
        let name = name.into();
        if name.is_empty() || name.len() > self.limits.max_string_bytes || size == 0 {
            return Err(IrProblem::InvalidStorage);
        }
        if self.storage.len() >= self.limits.max_storage_regions {
            return Err(IrProblem::LimitExceeded);
        }
        let total = self
            .storage
            .iter()
            .try_fold(size, |sum, item| sum.checked_add(item.size))
            .ok_or(IrProblem::LimitExceeded)?;
        if total > self.limits.max_storage_bytes {
            return Err(IrProblem::LimitExceeded);
        }
        if let Some(alias) = &alias_of {
            let target = self
                .storage
                .get(alias.storage.get() as usize)
                .ok_or(IrProblem::InvalidReference)?;
            validate_extent(alias.offset, alias.length, target.size)?;
            if alias.length != size {
                return Err(IrProblem::InvalidStorage);
            }
        }
        let id = StorageId::from_index(self.storage.len())?;
        self.storage.push(StorageRegion {
            id,
            name,
            size,
            alias_of,
        });
        Ok(id)
    }

    pub fn add_region(&mut self) -> Result<RegionId, IrProblem> {
        if self.regions.len() >= self.limits.max_regions {
            return Err(IrProblem::LimitExceeded);
        }
        let id = RegionId::from_index(self.regions.len())?;
        self.regions.push(Region {
            id,
            blocks: Vec::new(),
        });
        Ok(id)
    }

    pub fn add_block(&mut self, region: RegionId) -> Result<BlockId, IrProblem> {
        if self.blocks >= self.limits.max_blocks {
            return Err(IrProblem::LimitExceeded);
        }
        let id = BlockId::from_index(self.blocks)?;
        let target = self
            .regions
            .get_mut(region.get() as usize)
            .ok_or(IrProblem::InvalidReference)?;
        target.blocks.push(Block {
            id,
            operations: Vec::new(),
        });
        self.blocks += 1;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_operation(
        &mut self,
        block: BlockId,
        identity: OperationIdentity,
        operands: Vec<ValueId>,
        result_count: usize,
        attributes: BTreeMap<String, Attribute>,
        effects: Vec<Effect>,
        storage: Vec<StorageReference>,
        location: Option<SourceSpan>,
    ) -> Result<OperationId, IrProblem> {
        if self.operations >= self.limits.max_operations
            || operands.len() > self.limits.max_operands_per_operation
            || result_count > self.limits.max_results_per_operation
            || attributes.len() > self.limits.max_attributes_per_operation
            || effects.len() > self.limits.max_effects_per_operation
            || storage.len() > self.limits.max_references_per_operation
        {
            return Err(IrProblem::LimitExceeded);
        }
        validate_attributes(&attributes, self.limits)?;
        if operands
            .iter()
            .any(|value| value.get() as usize >= self.values)
        {
            return Err(IrProblem::InvalidReference);
        }
        for reference in &storage {
            let region = self
                .storage
                .get(reference.storage.get() as usize)
                .ok_or(IrProblem::InvalidReference)?;
            validate_extent(reference.offset, reference.length, region.size)?;
        }
        let next_values = self
            .values
            .checked_add(result_count)
            .ok_or(IrProblem::LimitExceeded)?;
        if next_values > self.limits.max_values {
            return Err(IrProblem::LimitExceeded);
        }
        let id = OperationId::from_index(self.operations)?;
        let mut results = Vec::with_capacity(result_count);
        for index in self.values..next_values {
            results.push(ValueId::from_index(index)?);
        }
        let target = self
            .regions
            .iter_mut()
            .flat_map(|region| region.blocks.iter_mut())
            .find(|candidate| candidate.id == block)
            .ok_or(IrProblem::InvalidReference)?;
        target.operations.push(Operation {
            id,
            identity,
            operands,
            results,
            attributes,
            effects,
            storage,
            location,
        });
        self.operations += 1;
        self.values = next_values;
        Ok(id)
    }

    pub fn finish(self) -> Result<Module, IrProblem> {
        if self.regions.is_empty() || self.regions.iter().any(|region| region.blocks.is_empty()) {
            return Err(IrProblem::EmptyStructure);
        }
        Ok(Module::from_parts(
            self.regions,
            self.storage,
            u32::try_from(self.values).map_err(|_| IrProblem::IdExhausted)?,
        ))
    }

    pub(crate) fn operation_results(
        &self,
        block: BlockId,
        operation: OperationId,
    ) -> Option<&[ValueId]> {
        self.regions
            .iter()
            .flat_map(|region| region.blocks.iter())
            .find(|candidate| candidate.id == block)?
            .operations
            .iter()
            .find(|candidate| candidate.id == operation)
            .map(|candidate| candidate.results.as_slice())
    }
}

fn validate_attributes(
    attributes: &BTreeMap<String, Attribute>,
    limits: IrLimits,
) -> Result<(), IrProblem> {
    for (name, value) in attributes {
        if !valid_name(name) || name.len() > limits.max_string_bytes {
            return Err(IrProblem::InvalidAttribute);
        }
        let size = match value {
            Attribute::Text(text) => text.len(),
            Attribute::Bytes(bytes) => bytes.len(),
            _ => 8,
        };
        if size > limits.max_attribute_bytes {
            return Err(IrProblem::LimitExceeded);
        }
    }
    Ok(())
}

pub(crate) fn validate_extent(offset: u64, length: u64, size: u64) -> Result<(), IrProblem> {
    if length == 0 || offset.checked_add(length).is_none_or(|end| end > size) {
        Err(IrProblem::InvalidReference)
    } else {
        Ok(())
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrProblem {
    InvalidIdentity,
    InvalidAttribute,
    InvalidReference,
    InvalidStorage,
    EmptyStructure,
    LimitExceeded,
    IdExhausted,
}

impl fmt::Display for IrProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            match self {
                Self::InvalidIdentity => "IR identity is invalid",
                Self::InvalidAttribute => "IR attribute is invalid",
                Self::InvalidReference => "IR reference is invalid",
                Self::InvalidStorage => "IR storage region is invalid",
                Self::EmptyStructure => "IR module has an empty required structure",
                Self::LimitExceeded => "IR resource limit exceeded",
                Self::IdExhausted => "IR typed ID space exhausted",
            }
        )
    }
}

impl std::error::Error for IrProblem {}
