//! Source-bounded APPC basic principal-facility query.

use super::{ConversationKind, ConversationLedger, GdsAssignFailure, GdsReturnCode};
use crate::service::{CicsService, Run, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

const RETCODE_SCHEMA: &str = "mainframe-env.cics.gds-retcode@1";
const CONVID_SCHEMA: &str = "mainframe-env.cics.convid@1";
const SYSID_SCHEMA: &str = "mainframe-env.cics.sysid@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::GdsAssignConversation
        || !request.arguments.contains_key("RETCODE")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "PRINCONVID" | "PRINSYSID" | "RETCODE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Unsupported);
    }
    super::deadline(service, run)?;
    let ledger = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let principal = ledger.conversations.values().find(|record| {
        record.principal_facility
            && !record.released
            && record.owner.execution == run.invocation.execution_id.as_str()
            && record.owner.run_unit == run.invocation.run_unit_id.as_str()
    });
    if let Some(record) = principal {
        if record.owner.lease_epoch != u64::from(run.invocation.attempt) {
            return Err(HostProblem::IdempotencyConflict);
        }
        service.authorize(
            run,
            "CONNECTION",
            &format!("CICS.CONNECTION.{}", record.system),
            AccessIntent::Execute,
        )?;
    }
    let code = match principal {
        None => GdsAssignFailure::NoPrincipalFacility.retcode(),
        Some(record) if record.kind == ConversationKind::Mro => GdsAssignFailure::NotAppc.retcode(),
        Some(record) if record.kind == ConversationKind::AppcMapped => {
            GdsAssignFailure::NotBasic.retcode()
        }
        Some(_) => GdsReturnCode::NORMAL,
    };
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    if code == GdsReturnCode::NORMAL {
        let record = principal.ok_or(HostProblem::InfrastructureFailure)?;
        if request.arguments.contains_key("PRINCONVID") {
            output(
                &mut result,
                "PRINCONVID",
                CONVID_SCHEMA,
                record.token.to_vec(),
            )?;
        }
        if request.arguments.contains_key("PRINSYSID") {
            let mut sysid = [b' '; 4];
            sysid[..record.system.len()].copy_from_slice(record.system.as_bytes());
            output(&mut result, "PRINSYSID", SYSID_SCHEMA, sysid.to_vec())?;
        }
    }
    output(&mut result, "RETCODE", RETCODE_SCHEMA, code.0.to_vec())?;
    Ok(result)
}

fn output(
    response: &mut CicsResponse,
    name: &str,
    schema: &str,
    bytes: Vec<u8>,
) -> Result<(), HostProblem> {
    response.outputs.insert(
        name.into(),
        BoundedPayload::new(schema, bytes, InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(())
}
