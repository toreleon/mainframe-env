//! Checked COBOL receiving storage for EXTRACT CERTIFICATE.

use super::{
    Clauses, HirCicsOperation, HirCicsOutputBinding, HirCicsOutputName, Resolution,
    ResolutionFailure, complete_data_reference, require_writable,
};
use crate::{CobolUsage, DataCategory, SemanticModel};
use mainframe_env_ir::CICS_CERTIFICATE_OUTPUT_NAMES;

pub(super) const ALLOWED_CLAUSES: &[&str] = &[
    "CERTIFICATE",
    "LENGTH",
    "SERIALNUM",
    "SERIALNUMLEN",
    "USERID",
    "COMMONNAME",
    "COMMONNAMLEN",
    "COUNTRY",
    "COUNTRYLEN",
    "STATE",
    "STATELEN",
    "LOCALITY",
    "LOCALITYLEN",
    "ORGANIZATION",
    "ORGANIZATLEN",
    "ORGUNIT",
    "ORGUNITLEN",
    "RESP",
    "RESP2",
];

pub(super) fn bounded_output_name(name: &str) -> bool {
    matches!(
        name,
        "LENGTH"
            | "SERIALNUMLEN"
            | "USERID"
            | "COMMONNAMLEN"
            | "COUNTRYLEN"
            | "STATELEN"
            | "LOCALITYLEN"
            | "ORGANIZATLEN"
            | "ORGUNITLEN"
    )
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if operation != HirCicsOperation::ExtractCertificate {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (name, identity) in CICS_CERTIFICATE_OUTPUT_NAMES {
        let Some(value) = clauses.get(*name) else {
            continue;
        };
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        if identity.pointer()
            && (!matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32)
                || !matches!(target.length, 4 | 8))
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS EXTRACT CERTIFICATE {name} requires a writable pointer reference"
            )));
        }
        if identity.length()
            && (target.category != DataCategory::Binary || target.length != 4 || target.scale != 0)
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS EXTRACT CERTIFICATE {name} requires fullword binary storage"
            )));
        }
        if *name == "USERID"
            && (target.length != 8
                || !matches!(
                    target.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                ))
        {
            return Err(ResolutionFailure::Invalid(
                "CICS EXTRACT CERTIFICATE USERID requires eight character bytes".into(),
            ));
        }
        result.push(HirCicsOutputBinding {
            name: HirCicsOutputName::Certificate(*identity),
            target,
        });
    }
    Ok(result)
}
