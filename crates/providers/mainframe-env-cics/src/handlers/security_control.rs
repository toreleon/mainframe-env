//! Source-bounded CICS security-control command handling.

use super::super::{CicsService, Run, decimal_payload};
mod authority;
mod change;
mod verify;
pub use authority::{
    CicsCredentialChangeRequest, CicsCredentialDetails, CicsCredentialFailure, CicsCredentialKind,
    CicsCredentialRequest, CicsCredentialVerification, CicsSecurityAccess,
    CicsSecurityAccessReason, CicsSecurityAuthority,
};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

const QUERY_LEVELS: [(&str, u8, i64, i64); 4] = [
    ("READ", 2, 2801, 2802),
    ("UPDATE", 3, 2803, 2804),
    ("CONTROL", 4, 2805, 2806),
    ("ALTER", 5, 2807, 2808),
];

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::ChangePassword => change::password(service, run, request, tick),
        CicsOperation::QuerySecurity => query_security(service, run, request, tick),
        CicsOperation::VerifyPassword => verify::password(service, run, request, tick),
        CicsOperation::VerifyPhrase => verify::phrase(service, run, request, tick),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn query_security(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_query_shape(request)?;
    let actor = run.invocation.principal.id().clone();
    let target = if let Some(user) = text(request, "USERID")? {
        let user = user.trim_end().to_ascii_uppercase();
        if user.is_empty() || user.contains(' ') {
            return Err(condition("INVREQ", 16, 32));
        }
        PrincipalId::new(user, InvocationLimits::default())
            .map_err(|_| condition("USERIDERR", 69, 11))?
    } else {
        actor.clone()
    };
    let resource = text(request, "RESID")?
        .ok_or_else(|| condition("INVREQ", 16, 9))?
        .trim_end()
        .to_ascii_uppercase();
    if resource.is_empty() || resource.contains(' ') {
        return Err(condition("INVREQ", 16, 9));
    }
    let (class, checked_resource, custom_class) =
        match (text(request, "RESCLASS")?, text(request, "RESTYPE")?) {
            (Some(class), None) => {
                let class = class.trim_end().to_ascii_uppercase();
                let length = number(request, "RESIDLENGTH")?;
                if !(1..=246).contains(&length) || length as usize != resource.len() {
                    return Err(condition("LENGERR", 22, 6));
                }
                if class.is_empty()
                    || class.len() > 8
                    || matches!(class.as_str(), "DATASET" | "GROUP" | "USER")
                {
                    return Err(condition("NOTFND", 13, 5));
                }
                (class, resource, true)
            }
            (None, Some(restype)) => {
                let restype = restype.trim_end().to_ascii_uppercase();
                if resource.len() > 12 {
                    return Err(condition("NOTFND", 13, 1));
                }
                let (class, checked_resource) =
                    installed_resource(service, run, &restype, &resource)?
                        .ok_or_else(|| condition("NOTFND", 13, 1))?;
                (class, checked_resource, false)
            }
            _ => return Err(condition("INVREQ", 16, 9)),
        };
    if target != actor {
        service
            .authorize(
                run,
                "SURROGAT",
                &format!("{}.DFHQUERY", target.as_str()),
                AccessIntent::Read,
            )
            .map_err(|problem| match problem {
                HostProblem::Unauthorized => condition("NOTAUTH", 70, 102),
                other => other,
            })?;
    }
    let correlation = format!(
        "CICS:QUERY SECURITY:{}:{}",
        run.invocation.run_unit_id, run.host_sequence
    );
    let access = service.security_authority()?.query_access(
        &actor,
        &target,
        &class,
        &checked_resource,
        tick,
        &correlation,
    )?;
    match access.reason {
        CicsSecurityAccessReason::PrincipalNotFound => {
            return Err(condition("USERIDERR", 69, 11));
        }
        CicsSecurityAccessReason::PrincipalInactive => {
            return Err(condition("USERIDERR", 69, 12));
        }
        CicsSecurityAccessReason::PolicyUnavailable => {
            return Err(condition("INVREQ", 16, 10));
        }
        CicsSecurityAccessReason::ClassInactive => {
            return Err(condition("NOTFND", 13, 5));
        }
        CicsSecurityAccessReason::ProfileNotFound if custom_class => {
            return Err(condition("NOTFND", 13, 8));
        }
        _ => {}
    }
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for (name, rank, allowed, denied) in QUERY_LEVELS {
        if request.arguments.contains_key(name) {
            response.outputs.insert(
                name.into(),
                decimal_payload(if access.granted_rank >= rank {
                    allowed
                } else {
                    denied
                })?,
            );
        }
    }
    Ok(response)
}

fn validate_query_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("RESID")
        || !request.arguments.contains_key("RESCLASS") && !request.arguments.contains_key("RESTYPE")
        || !QUERY_LEVELS
            .iter()
            .any(|(name, _, _, _)| request.arguments.contains_key(*name))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "RESCLASS" | "RESID" | "RESTYPE" | "USERID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
                ),
                "RESIDLENGTH" | "LOGMESSAGE" => value.schema() != "mainframe-env.cics.decimal@1",
                "READ" | "UPDATE" | "CONTROL" | "ALTER" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    if let Some(log_message) = text(request, "LOGMESSAGE")?
        && !matches!(log_message.as_str(), "2890" | "2891")
    {
        return Err(condition("INVREQ", 16, 7));
    }
    Ok(())
}

fn text(request: &CicsRequest, name: &str) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed))
        .transpose()
}

fn number(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    text(request, name)?
        .ok_or(HostProblem::Malformed)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn installed_resource(
    service: &CicsService,
    run: &Run,
    restype: &str,
    resource: &str,
) -> Result<Option<(String, String)>, HostProblem> {
    if matches!(restype, "TSQUEUE" | "TSQNAME") {
        return Ok(service
            .store
            .get_provider_state("cics-tsq", resource)
            .map_err(super::store_error)?
            .map(|_| ("QUEUE".into(), format!("CICS.TS.{resource}"))));
    }
    let state = service.lock()?;
    let selected = match restype {
        "FILE" => state
            .file_aliases
            .get(resource)
            .map(|file| ("DATASET".into(), file.dataset.as_str().to_string())),
        "PROGRAM" => (state.programs.contains(resource)
            || state.program_definitions.contains_key(resource))
        .then(|| ("FACILITY".into(), format!("CICS.PROGRAM.{resource}"))),
        "TRANSACTION" | "TRANSATTACH" => {
            (run.transaction == resource).then(|| ("TCICSTRN".into(), format!("CICS.{resource}")))
        }
        "DOCTEMPLATE" => state.document_templates.get(resource).map(|template| {
            (
                "DOCTEMPLATE".into(),
                format!("CICS.DOCTEMPLATE.{}", template.resource),
            )
        }),
        "JOURNALNAME" | "JOURNALNUM" => state
            .journals
            .contains_key(resource)
            .then(|| ("JOURNAL".into(), format!("CICS.JOURNAL.{resource}"))),
        "TDQUEUE" => state
            .transient
            .definitions
            .contains_key(resource)
            .then(|| ("QUEUE".into(), format!("CICS.TD.{resource}"))),
        "XMLTRANSFORM" => state
            .transform_resources
            .contains_key(&(crate::CicsTransformFormat::Xml, resource.to_string()))
            .then(|| ("TRANSFORM".into(), format!("CICS.XML.{resource}"))),
        "PSB" => Some(("PSB".into(), resource.into())),
        "ATOMSERVICE" | "BUNDLE" | "DB2ENTRY" | "EPADAPTER" | "EPADAPTERSET" | "EVENTBINDING"
        | "JVMSERVER" | "SPCOMMAND" => None,
        _ => return Err(condition("NOTFND", 13, 2)),
    };
    Ok(selected)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
