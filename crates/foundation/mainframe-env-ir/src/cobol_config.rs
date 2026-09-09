//! Dialect-owned decoding of immutable COBOL runtime configuration.

use crate::{Attribute, Module};
use std::fmt;

/// COBOL operation namespace carrying module-wide execution configuration.
pub const COBOL_RUNTIME_CONFIG_NAMESPACE: &str = "mainframe.core.cobol";
/// COBOL operation name carrying module-wide execution configuration.
pub const COBOL_RUNTIME_CONFIG_NAME: &str = "config";
/// Current major version of the COBOL runtime-configuration operation.
pub const COBOL_RUNTIME_CONFIG_MAJOR: u16 = 1;
/// Manifest key binding the effective arithmetic context to the payload.
pub const COBOL_EFFECTIVE_ARITH_OPTION: &str = "cobol.effective-arith";
/// Manifest key binding the effective display-sign convention to the payload.
pub const COBOL_EFFECTIVE_DISPSIGN_OPTION: &str = "cobol.effective-dispsign";
/// Manifest key binding the effective addressing mode to the payload.
pub const COBOL_EFFECTIVE_LP_OPTION: &str = "cobol.effective-lp";

/// Arithmetic context selected for COBOL execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolArithmeticMode {
    Compatible,
    Extended,
}

impl CobolArithmeticMode {
    /// Canonical manifest and IR spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Extended => "extended",
        }
    }
}

/// Display-sign convention selected for COBOL execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolDisplaySign {
    Compatible,
    Separate,
}

impl CobolDisplaySign {
    /// Canonical manifest and IR spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Separate => "separate",
        }
    }
}

/// Addressing mode selected for COBOL compilation and execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolAddressMode {
    Bits32,
    Bits64,
}

impl CobolAddressMode {
    /// Canonical manifest and IR spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bits32 => "32",
            Self::Bits64 => "64",
        }
    }
}

/// Immutable execution choices carried by one COBOL module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CobolRuntimeConfig {
    pub arithmetic_mode: CobolArithmeticMode,
    /// Historical payloads may omit this attribute; their reader owns the
    /// compatibility default rather than rewriting their bytes.
    pub display_sign: Option<CobolDisplaySign>,
    /// `artifact@2` predates this payload marker. Current payloads carry it.
    pub address_mode: Option<CobolAddressMode>,
}

/// Malformed or ambiguous COBOL configuration embedded in an IR module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolRuntimeConfigProblem {
    Duplicate,
    MissingArithmeticMode,
    InvalidArithmeticMode,
    InvalidDisplaySign,
    InvalidAddressMode,
}

impl fmt::Display for CobolRuntimeConfigProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid COBOL runtime configuration: {self:?}")
    }
}

impl std::error::Error for CobolRuntimeConfigProblem {}

/// Decode the single COBOL runtime configuration, if the module carries one.
///
/// The dialect owns the operation identity, attribute types, and value set.
/// Artifact infrastructure can therefore bind a versioned manifest to these
/// semantics without depending on either the COBOL compiler or interpreter.
pub fn cobol_runtime_config(
    module: &Module,
) -> Result<Option<CobolRuntimeConfig>, CobolRuntimeConfigProblem> {
    let mut configurations = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .filter(|operation| {
            operation.identity.namespace() == COBOL_RUNTIME_CONFIG_NAMESPACE
                && operation.identity.name() == COBOL_RUNTIME_CONFIG_NAME
                && operation.identity.major() == COBOL_RUNTIME_CONFIG_MAJOR
        });
    let Some(operation) = configurations.next() else {
        return Ok(None);
    };
    if configurations.next().is_some() {
        return Err(CobolRuntimeConfigProblem::Duplicate);
    }
    let arithmetic_mode = match operation.attributes.get("arithmetic_mode") {
        Some(Attribute::Text(value)) if value == "compatible" => CobolArithmeticMode::Compatible,
        Some(Attribute::Text(value)) if value == "extended" => CobolArithmeticMode::Extended,
        None => return Err(CobolRuntimeConfigProblem::MissingArithmeticMode),
        _ => return Err(CobolRuntimeConfigProblem::InvalidArithmeticMode),
    };
    let display_sign = match operation.attributes.get("display_sign") {
        Some(Attribute::Text(value)) if value == "compatible" => Some(CobolDisplaySign::Compatible),
        Some(Attribute::Text(value)) if value == "separate" => Some(CobolDisplaySign::Separate),
        None => None,
        _ => return Err(CobolRuntimeConfigProblem::InvalidDisplaySign),
    };
    let address_mode = match operation.attributes.get("address_mode") {
        Some(Attribute::Text(value)) if value == "32" => Some(CobolAddressMode::Bits32),
        Some(Attribute::Text(value)) if value == "64" => Some(CobolAddressMode::Bits64),
        None => None,
        _ => return Err(CobolRuntimeConfigProblem::InvalidAddressMode),
    };
    Ok(Some(CobolRuntimeConfig {
        arithmetic_mode,
        display_sign,
        address_mode,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IrLimits, ModuleBuilder, OperationIdentity};
    use std::collections::BTreeMap;

    fn module(attributes: BTreeMap<String, Attribute>, duplicates: bool) -> Module {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let identity = OperationIdentity::new(
            COBOL_RUNTIME_CONFIG_NAMESPACE,
            COBOL_RUNTIME_CONFIG_NAME,
            COBOL_RUNTIME_CONFIG_MAJOR,
        )
        .unwrap();
        builder
            .add_operation(
                block,
                identity.clone(),
                Vec::new(),
                0,
                attributes.clone(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        if duplicates {
            builder
                .add_operation(
                    block,
                    identity,
                    Vec::new(),
                    0,
                    attributes,
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        builder.finish().unwrap()
    }

    #[test]
    fn decodes_current_and_historical_config_shapes() {
        let current = module(
            BTreeMap::from([
                ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                ("display_sign".into(), Attribute::Text("separate".into())),
                ("address_mode".into(), Attribute::Text("64".into())),
            ]),
            false,
        );
        assert_eq!(
            cobol_runtime_config(&current).unwrap(),
            Some(CobolRuntimeConfig {
                arithmetic_mode: CobolArithmeticMode::Extended,
                display_sign: Some(CobolDisplaySign::Separate),
                address_mode: Some(CobolAddressMode::Bits64),
            })
        );

        let historical = module(
            BTreeMap::from([("arithmetic_mode".into(), Attribute::Text("extended".into()))]),
            false,
        );
        assert_eq!(
            cobol_runtime_config(&historical).unwrap(),
            Some(CobolRuntimeConfig {
                arithmetic_mode: CobolArithmeticMode::Extended,
                display_sign: None,
                address_mode: None,
            })
        );
    }

    #[test]
    fn rejects_ambiguous_and_noncanonical_config() {
        let attributes =
            BTreeMap::from([("arithmetic_mode".into(), Attribute::Text("extended".into()))]);
        assert_eq!(
            cobol_runtime_config(&module(attributes, true)),
            Err(CobolRuntimeConfigProblem::Duplicate)
        );
        assert_eq!(
            cobol_runtime_config(&module(
                BTreeMap::from([("arithmetic_mode".into(), Attribute::Text("EXTENDED".into()),)]),
                false,
            )),
            Err(CobolRuntimeConfigProblem::InvalidArithmeticMode)
        );
    }
}
