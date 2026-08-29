//! Checked EBCDIC, collation, decimal, and binary byte primitives.

#![forbid(unsafe_code)]

mod codepage;
mod decimal;

pub use codepage::{CodePage, EncodingProblem, compare_ebcdic};
pub use decimal::{
    DecimalValue, Sign, decode_binary, decode_packed, decode_zoned, encode_binary, encode_packed,
    encode_zoned,
};

/// Stable encoding contract identity.
pub const ENCODING_CONTRACT: &str = "mainframe-env.encoding@1";
