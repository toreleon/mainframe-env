//! Unchanged CICS seed alias READ/browse comparison under the existing owner.
use super::*;

pub(super) fn alias_routes(
    owner: &Owner,
    server: &ProductServer,
    oracle: &Oracle,
    phase: &str,
) -> Result<serde_json::Value, CorpusProblem> {
    let service = server.cics_service();
    let mut invocation =
        authorization_invocation(&format!("seed-{phase}"), true, ServiceClass::Interactive)?;
    let mut grants = invocation.principal.grants().clone();
    grants.insert(
        CapabilityId::new("host.dataset.read", InvocationLimits::default())
            .map_err(package_problem)?,
    );
    invocation.principal = Principal::new(
        invocation.principal.id().clone(),
        grants,
        InvocationLimits::default(),
    )
    .map_err(package_problem)?;
    let session = SessionId::new(format!("seed-{phase}"), 64).map_err(package_problem)?;
    owner.verify()?;
    service
        .create_session(&session, 24, 80)
        .map_err(terminal_problem)?;
    owner.verify()?;
    service
        .register_run(invocation.clone(), &session, "SEED", "CARDDEMO", "LOCAL")
        .map_err(terminal_problem)?;
    let mut sequence = 0;
    let mut retained = Vec::new();
    for alias in &oracle.aliases {
        refuse(
            !complete_host_expectations(alias),
            "frozen complete metadata/full alias rows/keys differ",
        )?;
        let file = || {
            Ok::<_, CorpusProblem>(BTreeMap::from([(
                "FILE".into(),
                operand(alias.name.as_bytes().to_vec())?,
            )]))
        };
        let mut args = file()?;
        args.insert("RIDFLD".into(), operand(alias.host_keys[0].clone())?);
        let read = command(
            owner,
            &service,
            &invocation,
            &mut sequence,
            CicsOperation::Read,
            args,
        )?;
        refuse(
            !row_reply(&read, &alias.host_rows[0], None),
            "actual source FILE READ selected wrong full row",
        )?;
        let mut args = file()?;
        args.insert("RIDFLD".into(), operand(alias.host_keys[0].clone())?);
        args.insert(
            "OPTION.GTEQ".into(),
            BoundedPayload::new(
                "mainframe-env.cics.option@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .map_err(package_problem)?,
        );
        let start = command(
            owner,
            &service,
            &invocation,
            &mut sequence,
            CicsOperation::StartBrowse,
            args,
        )?;
        refuse(
            !normal(&start) || !start.payload.bytes().is_empty(),
            "actual source FILE STARTBR failed",
        )?;
        let mut observed = Vec::new();
        for (row, key) in alias.host_rows.iter().zip(&alias.host_keys) {
            let next = command(
                owner,
                &service,
                &invocation,
                &mut sequence,
                CicsOperation::ReadNext,
                file()?,
            )?;
            refuse(
                !row_reply(&next, row, Some(key)),
                "complete actual FILE browse row/key/order differs",
            )?;
            observed.push(serde_json::json!({"row_base64":base64::engine::general_purpose::STANDARD.encode(next.payload.bytes()), "key_base64":base64::engine::general_purpose::STANDARD.encode(next.outputs["RIDFLD"].bytes()), "response":next.response,"response2":next.response2}));
        }
        let eof = command(
            owner,
            &service,
            &invocation,
            &mut sequence,
            CicsOperation::ReadNext,
            file()?,
        )?;
        refuse(!eof_reply(&eof), "actual full FILE browse EOF differs")?;
        let end = command(
            owner,
            &service,
            &invocation,
            &mut sequence,
            CicsOperation::EndBrowse,
            file()?,
        )?;
        refuse(
            !normal(&end) || !end.payload.bytes().is_empty(),
            "actual source FILE ENDBR failed",
        )?;
        retained.push(serde_json::json!({"alias":alias.name,"dataset":alias.dataset,"aix":alias.aix,"read_base64":base64::engine::general_purpose::STANDARD.encode(read.payload.bytes()),"browse":observed,"eof":[eof.condition,eof.response,eof.response2],"end_response":end.response}));
    }
    let returned = command(
        owner,
        &service,
        &invocation,
        &mut sequence,
        CicsOperation::Return,
        BTreeMap::new(),
    )?;
    refuse(
        returned.condition != "NORMAL"
            || returned.response != 0
            || returned.disposition != CicsDisposition::Returned,
        "actual file-route participant did not return",
    )?;
    Ok(
        serde_json::json!({"phase":phase,"run_unit":invocation.run_unit_id.as_str(),"commands":sequence,"aliases":retained}),
    )
}
