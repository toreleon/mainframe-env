use super::{response, state};
use crate::service::{CicsService, Run, field};
use mainframe_env_host_api::{
    AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

const MAGIC: &[u8; 8] = b"MECDMP01";
const FLAGS: &[&str] = &[
    "COMPLETE", "TASK", "STORAGE", "PROGRAM", "TERMINAL", "TABLES", "FCT", "PCT", "PPT", "SIT",
    "TCT", "TRT", "DCT",
];

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    let is_dump = request.operation == CicsOperation::Dump;
    let raw_code = request
        .arguments
        .get("DUMPCODE")
        .map(|value| value.bytes())
        .unwrap_or_default();
    let code_bytes = raw_code.strip_suffix(b" ").unwrap_or(raw_code);
    let code_bytes = code_bytes.strip_suffix(b" ").unwrap_or(code_bytes);
    let code_bytes = code_bytes.strip_suffix(b" ").unwrap_or(code_bytes);
    let valid_code = code_bytes.is_empty() && is_dump
        || !code_bytes.is_empty()
            && code_bytes.len() <= 4
            && code_bytes.iter().all(|byte| (0x21..=0x7e).contains(byte));
    let code = String::from_utf8_lossy(code_bytes).to_ascii_uppercase();
    let length = if request.arguments.contains_key("FLENGTH") {
        Some(decimal(request, "FLENGTH")?)
    } else if request.arguments.contains_key("LENGTH") {
        Some(decimal(request, "LENGTH")?)
    } else {
        None
    };
    let from = request.arguments.get("FROM").map(|value| value.bytes());
    let selected_length = if let Some(length) = length {
        let maximum = if request.arguments.contains_key("FLENGTH") {
            16_777_215
        } else {
            i16::MAX as i64
        };
        if length < 0 || length > maximum {
            return Err(condition("INVREQ", 16, 13));
        }
        usize::try_from(length).map_err(|_| HostProblem::Malformed)?
    } else {
        from.map_or(0, <[u8]>::len)
    };
    if from.is_some_and(|bytes| selected_length > bytes.len()) {
        return Err(condition("IOERR", 17, 10));
    }
    let segments = decode_segments(request, service.limits.max_diagnostic_payload_bytes)?;
    service.authorize(
        run,
        "CICSDIAG",
        &if valid_code && !code.is_empty() {
            format!("CICS.DIAG.DUMP.{code}")
        } else {
            "CICS.DIAG.DUMP".into()
        },
        AccessIntent::Update,
    )?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let runtime = service.lock()?;
    let current = state::load(service)?;
    if let Some(reply) = current.replay(mutation.idempotency_key.as_str(), digest)? {
        return dump_response(service, run, request, reply);
    }
    if let Some(definition) = valid_code
        .then(|| current.dump_definitions.get(&code))
        .flatten()
    {
        if definition.system_dump {
            return Err(HostProblem::Unsupported);
        }
        if definition.suppress
            || current.dump_counts.get(&code).copied().unwrap_or(0) >= definition.maximum
        {
            return Err(condition("SUPPRESSED", 72, 1));
        }
    }
    if current.traces.len() + current.dumps.len() >= service.limits.max_diagnostic_entries {
        return Err(condition("NOSPACE", 18, 4));
    }
    let dump_count =
        if !service.diagnostic_run_opened.load(Ordering::SeqCst) && current.next_dump_count > 1 {
            ((current.next_dump_count - 1) / 9999 + 1)
                .checked_mul(9999)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| condition("NOSPACE", 18, 4))?
        } else {
            current.next_dump_count
        };
    let run_number = (dump_count - 1) / 9999 + 1;
    if run_number > 9999 {
        return Err(condition("NOSPACE", 18, 4));
    }
    let dump_id = format!("{run_number}/{:04}", (dump_count - 1) % 9999 + 1);
    let selected = selected_sections(request);
    let mut sections = Vec::<(String, Vec<u8>)>::new();
    if !raw_code.is_empty() {
        sections.push(("DUMPCODE".into(), raw_code.to_vec()));
    }
    if selected.contains("TASK") {
        let mut data = Vec::new();
        for (name, value) in [
            ("TRANSACTION", run.transaction.as_bytes()),
            ("APPLID", run.applid.as_bytes()),
            ("SYSID", run.sysid.as_bytes()),
            ("RUNUNIT", run.invocation.run_unit_id.as_str().as_bytes()),
            (
                "PRINCIPAL",
                run.invocation.principal.id().as_str().as_bytes(),
            ),
        ] {
            field(&mut data, name.as_bytes())?;
            field(&mut data, value)?;
        }
        sections.push(("TASK".into(), data));
    }
    if let Some(bytes) = from {
        sections.push(("FROM".into(), bytes[..selected_length].to_vec()));
    }
    for (index, bytes) in segments.into_iter().enumerate() {
        sections.push((format!("SEGMENT{:04}", index + 1), bytes));
    }
    if selected.contains("STORAGE") {
        let mut data = Vec::new();
        field(&mut data, b"RETRIEVE")?;
        field(&mut data, &run.retrieve)?;
        for (name, bytes) in &run.current_records {
            field(&mut data, name.as_bytes())?;
            field(&mut data, bytes)?;
        }
        sections.push(("STORAGE".into(), data));
    }
    if selected.contains("PROGRAM") {
        sections.push((
            "PROGRAM".into(),
            run.current_program
                .current
                .as_deref()
                .unwrap_or("")
                .as_bytes()
                .to_vec(),
        ));
    }
    if selected.contains("TERMINAL") || selected.contains("TCT") {
        let session = runtime.sessions.get(&run.session);
        let data = session
            .map(|session| format!("{}:{}:{}", session.rows, session.columns, session.principal))
            .unwrap_or_default()
            .into_bytes();
        if selected.contains("TERMINAL") {
            sections.push(("TERMINAL".into(), data.clone()));
        }
        if selected.contains("TCT") {
            sections.push(("TCT".into(), data));
        }
    }
    if selected.contains("FCT") {
        sections.push((
            "FCT".into(),
            runtime
                .file_aliases
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ));
    }
    if selected.contains("DCT") {
        let mut data = Vec::new();
        for (code, definition) in &current.dump_definitions {
            field(&mut data, code.as_bytes())?;
            field(
                &mut data,
                format!(
                    "suppress={} maximum={} count={} system={}",
                    definition.suppress,
                    definition.maximum,
                    current.dump_counts.get(code).copied().unwrap_or(0),
                    definition.system_dump
                )
                .as_bytes(),
            )?;
        }
        sections.push(("DCT".into(), data));
    }
    if selected.contains("PCT") {
        sections.push((
            "PCT".into(),
            runtime
                .runs
                .values()
                .map(|run| run.transaction.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ));
    }
    if selected.contains("PPT") {
        sections.push((
            "PPT".into(),
            runtime
                .programs
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
                .into_bytes(),
        ));
    }
    if selected.contains("SIT") {
        sections.push((
            "SIT".into(),
            format!("APPLID={}\nSYSID={}", run.applid, run.sysid).into_bytes(),
        ));
    }
    if selected.contains("TRT") {
        let mut data = Vec::new();
        for trace in current.traces.iter().filter(|trace| {
            trace.run_unit == run.invocation.run_unit_id.as_str() && trace.kind.contains("INTERNAL")
        }) {
            field(&mut data, trace.identifier.as_bytes())?;
            field(&mut data, &trace.data)?;
        }
        sections.push(("TRT".into(), data));
    }
    let data = encode_sections(&sections, service.limits.max_diagnostic_payload_bytes)?;
    let mut next = current.clone();
    let sequence = next.allocate_sequence()?;
    next.next_dump_count = dump_count
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    if valid_code && !code.is_empty() {
        let count = next.dump_counts.entry(code.clone()).or_default();
        *count = count.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
    }
    next.dumps.push(state::CicsDiagnosticDumpRecord {
        sequence,
        dump_id: dump_id.clone(),
        scope: if is_dump { "DUMP" } else { "TRANSACTION" }.into(),
        code,
        sections: sections.iter().map(|(name, _)| name.clone()).collect(),
        data,
        run_unit: run.invocation.run_unit_id.as_str().into(),
        principal: run.invocation.principal.id().as_str().into(),
    });
    let mut reply = state::DiagnosticReply::normal();
    if !valid_code {
        reply.condition = "INVREQ".into();
        reply.response = 16;
        reply.response2 = 13;
    }
    if request.arguments.contains_key("DUMPID") {
        reply.outputs.insert("DUMPID".into(), dump_id.into_bytes());
    }
    next.record_replay(
        mutation.idempotency_key.as_str(),
        digest,
        reply.clone(),
        service.limits,
    )?;
    state::persist(service, current.version, &mut next).map_err(|problem| {
        if problem == HostProblem::ResourceExhausted {
            condition("NOSPACE", 18, 4)
        } else {
            problem
        }
    })?;
    service.diagnostic_run_opened.store(true, Ordering::SeqCst);
    dump_response(service, run, request, reply)
}

fn selected_sections(request: &CicsRequest) -> BTreeSet<&'static str> {
    let mut selected = FLAGS
        .iter()
        .copied()
        .filter(|flag| request.arguments.contains_key(&format!("OPTION.{flag}")))
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        selected.insert("TASK");
    }
    if selected.remove("COMPLETE") {
        selected.extend(["TASK", "STORAGE", "PROGRAM", "TERMINAL", "TABLES"]);
        if request.operation == CicsOperation::DumpTransaction {
            selected.insert("TRT");
        }
    }
    if selected.remove("TABLES") {
        selected.extend(["FCT", "PCT", "PPT", "SIT", "TCT"]);
        if request.operation == CicsOperation::Dump {
            selected.insert("DCT");
        }
    }
    selected
}

fn decode_segments(request: &CicsRequest, maximum: usize) -> Result<Vec<Vec<u8>>, HostProblem> {
    if !request.arguments.contains_key("NUMSEGMENTS") {
        return Ok(Vec::new());
    }
    let count = decimal(request, "NUMSEGMENTS")?;
    if !(0..=256).contains(&count) {
        return Err(condition("INVREQ", 16, 13));
    }
    let bytes = request
        .arguments
        .get("SEGMENTS")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let count_usize = usize::try_from(count).map_err(|_| HostProblem::Malformed)?;
    let list_bytes = count_usize.checked_mul(4).ok_or(HostProblem::Malformed)?;
    let pointers = request
        .arguments
        .get("SEGMENTLIST")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let lengths = request
        .arguments
        .get("LENGTHLIST")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    if pointers.len() < list_bytes || lengths.len() < list_bytes {
        return Err(HostProblem::Malformed);
    }
    let mut cursor = 0usize;
    let mut result = Vec::new();
    for index in 0..count_usize {
        let end = cursor.checked_add(4).ok_or(HostProblem::Malformed)?;
        let length = u32::from_be_bytes(
            bytes
                .get(cursor..end)
                .ok_or(HostProblem::Malformed)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ) as usize;
        let start = index * 4;
        let declared = i32::from_be_bytes(
            lengths[start..start + 4]
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        );
        if declared < 0 || usize::try_from(declared).ok() != Some(length) {
            return Err(HostProblem::Malformed);
        }
        cursor = end;
        let end = cursor.checked_add(length).ok_or(HostProblem::Malformed)?;
        result.push(
            bytes
                .get(cursor..end)
                .ok_or(HostProblem::Malformed)?
                .to_vec(),
        );
        cursor = end;
        if cursor > maximum {
            return Err(condition("NOSPACE", 18, 4));
        }
    }
    if cursor != bytes.len() {
        return Err(HostProblem::Malformed);
    }
    Ok(result)
}

fn encode_sections(sections: &[(String, Vec<u8>)], maximum: usize) -> Result<Vec<u8>, HostProblem> {
    let mut data = MAGIC.to_vec();
    data.extend_from_slice(
        &u16::try_from(sections.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, bytes) in sections {
        field(&mut data, name.as_bytes())?;
        field(&mut data, bytes)?;
        if data.len() > maximum {
            return Err(condition("NOSPACE", 18, 4));
        }
    }
    Ok(data)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    let is_dump = request.operation == CicsOperation::Dump;
    if request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "DUMPCODE" | "FROM" => !matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "SEGMENTLIST" | "LENGTHLIST" => {
                is_dump || !matches!(value.schema(), "mainframe-env.cics.storage-value@1")
            }
            "LENGTH" | "FLENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
            "NUMSEGMENTS" => is_dump || value.schema() != "mainframe-env.cics.decimal@1",
            "SEGMENTS" => is_dump || value.schema() != "mainframe-env.cics.dump-segments@1",
            "DUMPID" => is_dump || value.schema() != "mainframe-env.cics.argument@1",
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            name if name.starts_with("OPTION.") => {
                !matches!(
                    name.strip_prefix("OPTION."),
                    Some("NOHANDLE")
                        | Some("COMPLETE")
                        | Some("TASK")
                        | Some("STORAGE")
                        | Some("PROGRAM")
                        | Some("TERMINAL")
                        | Some("TABLES")
                        | Some("FCT")
                        | Some("PCT")
                        | Some("PPT")
                        | Some("SIT")
                        | Some("TCT")
                        | Some("TRT")
                        | Some("DCT")
                ) || value.schema() != "mainframe-env.cics.option@1"
                    || !value.bytes().is_empty()
                    || is_dump && name == "OPTION.TRT"
                    || !is_dump && name == "OPTION.DCT"
            }
            _ => true,
        })
        || request
            .arguments
            .get("DUMPCODE")
            .is_none_or(|value| !(1..=4).contains(&value.bytes().len()))
            && !is_dump
        || request
            .arguments
            .get("DUMPCODE")
            .is_some_and(|value| !(1..=4).contains(&value.bytes().len()))
        || request.arguments.contains_key("LENGTH") && request.arguments.contains_key("FLENGTH")
        || (request.arguments.contains_key("LENGTH") || request.arguments.contains_key("FLENGTH"))
            && !request.arguments.contains_key("FROM")
        || {
            let count = ["SEGMENTLIST", "LENGTHLIST", "NUMSEGMENTS"]
                .iter()
                .filter(|name| request.arguments.contains_key(**name))
                .count();
            count != 0 && count != 3 || (count == 0) != !request.arguments.contains_key("SEGMENTS")
        }
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}

fn dump_response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    reply: state::DiagnosticReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.response == 0 {
        return response(service, run, reply);
    }
    let mut result = super::super::condition(
        service,
        run,
        &request.condition_policy,
        condition(&reply.condition, reply.response, reply.response2),
    )?;
    for (name, bytes) in reply.outputs {
        result.outputs.insert(name, crate::service::bounded(bytes)?);
    }
    Ok(result)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
