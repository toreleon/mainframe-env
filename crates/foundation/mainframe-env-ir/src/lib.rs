//! Bounded generic IR, schemas, verification, and owned codecs.

#![forbid(unsafe_code)]

mod catalog;
mod codec;
mod model;
mod verify;

pub use catalog::{CatalogProblem, LegalityProfile, OperationCatalog, OperationSchema};
pub use codec::{CodecLimits, IrCodecProblem, decode_binary, encode_binary, parse_text, to_text};
pub use model::{
    Attribute, Block, BlockId, Effect, IrLimits, IrProblem, Module, ModuleBuilder, Operation,
    OperationId, OperationIdentity, Region, RegionId, StorageId, StorageReference, StorageRegion,
    TypeIdentity, ValueId,
};
pub use verify::{LegalModule, VerificationProblem, VerificationReport, verify, verify_legal};

pub const IR_OBJECT_CONTRACT: &str = "mainframe-env.ir@1";
pub const IR_TEXT_CONTRACT: &str = "mainframe-env.ir-text@1";
pub const IR_BINARY_CONTRACT: &str = "mainframe-env.ir-binary@1";
pub const IR_ENVELOPE_CONTRACT: &str = "mainframe-env.ir-envelope@1";
