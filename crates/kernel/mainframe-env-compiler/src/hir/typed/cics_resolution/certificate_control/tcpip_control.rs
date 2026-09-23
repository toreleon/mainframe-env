//! Checked receiving storage for source-bounded EXTRACT TCPIP results.

use super::super::{
    Clauses, HirCicsOutputBinding, HirCicsOutputName, Resolution, ResolutionFailure,
    complete_data_reference, require_writable,
};
use crate::{DataCategory, SemanticModel};
use mainframe_env_ir::{CICS_TCPIP_OUTPUT_NAMES, CicsTcpipOutput};

pub(crate) const ALLOWED_CLAUSES: &[&str] = &[
    "CLIENTNAME",
    "CNAMELENGTH",
    "SERVERNAME",
    "SNAMELENGTH",
    "CLIENTADDR",
    "CADDRLENGTH",
    "CLIENTADDRNU",
    "CLNTADDR6NU",
    "SERVERADDR",
    "SADDRLENGTH",
    "SERVERADDRNU",
    "SRVRADDR6NU",
    "TCPIPSERVICE",
    "PORTNUMBER",
    "PORTNUMNU",
    "MAXDATALEN",
    "RESP",
    "RESP2",
];

pub(super) fn outputs(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    for (buffer, length) in [
        ("CLIENTNAME", "CNAMELENGTH"),
        ("SERVERNAME", "SNAMELENGTH"),
        ("CLIENTADDR", "CADDRLENGTH"),
        ("SERVERADDR", "SADDRLENGTH"),
    ] {
        if clauses.contains_key(buffer) != clauses.contains_key(length) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS EXTRACT TCPIP {buffer} requires its {length} buffer length"
            )));
        }
    }
    let mut result = Vec::new();
    for (name, identity) in CICS_TCPIP_OUTPUT_NAMES {
        let Some(value) = clauses.get(*name) else {
            continue;
        };
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        let character = matches!(
            target.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        );
        let binary = target.category == DataCategory::Binary && target.scale == 0;
        let valid = if identity.buffer_length() {
            binary && matches!(target.length, 2 | 4)
        } else if identity.fullword()
            || matches!(
                identity,
                CicsTcpipOutput::ClientAddressNumeric | CicsTcpipOutput::ServerAddressNumeric
            )
        {
            binary && target.length == 4
        } else if matches!(
            identity,
            CicsTcpipOutput::ClientAddress6Numeric | CicsTcpipOutput::ServerAddress6Numeric
        ) {
            character && target.length == 16
        } else if *identity == CicsTcpipOutput::TcpipService {
            character && target.length == 8
        } else if *identity == CicsTcpipOutput::PortNumber {
            target.length == 5 && (character || target.category == DataCategory::NumericDisplay)
        } else {
            character && target.length > 0
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS EXTRACT TCPIP {name} has an invalid receiving storage shape"
            )));
        }
        result.push(HirCicsOutputBinding {
            name: HirCicsOutputName::Tcpip(*identity),
            target,
        });
    }
    Ok(result)
}
