use super::*;
use mainframe_env_host_api::CicsResponse;
use mainframe_env_ir::{
    CICS_EXECUTABLE_DESCRIPTORS, CicsCondition, CicsEffectPlan, CicsExecutableDescriptor,
    CicsOperandName, CicsOperandValue, CicsOperationContract, CicsOutputName, CicsPlanLimits,
    CicsPlanOperation, CicsPlanOption, CicsStorageSlot, Effect, Module, OperationCatalog,
    OperationSchema, OperationSemanticContract, cics_executable_descriptor,
    cics_executable_descriptor_for_identity, cobol_layout_definition_identity,
    decode_cics_effect_plan, verify_semantic_contracts,
};

const PLAN_ATTRIBUTE: &str = "cics_plan";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CicsTarget {
    Legacy(String),
    Resolved(CicsStorageSlot),
}

pub(super) fn operation_identities() -> Vec<OperationIdentity> {
    CICS_EXECUTABLE_DESCRIPTORS
        .iter()
        .map(|descriptor| descriptor.identity())
        .collect()
}

pub(super) fn is_typed(operation: &Operation) -> bool {
    expected_operation(&operation.identity).is_some()
}

pub(super) fn write_context(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    response: &CicsResponse,
) -> Result<(), MachineProblem> {
    for (name, value) in [
        ("EIBRESP", i128::from(response.response)),
        ("EIBRESP2", i128::from(response.response2)),
    ] {
        machine.write_decimal(
            name,
            Decimal {
                coefficient: value,
                scale: 0,
            },
        )?;
    }
    if let Some(descriptor) =
        mainframe_env_ir::cics_application_registry_for_runtime_operation(operation.runtime_name())
    {
        machine.write("EIBFN", &descriptor.eibfn)?;
    }
    if operation == CicsOperation::ReceiveMap {
        machine.write("EIBAID", &[response.aid])?;
    }
    machine.write("EIBTRNID", response.transaction.as_bytes())?;
    Ok(())
}

pub(super) fn validate_module_operations(module: &Module) -> Result<(), MachineProblem> {
    let mut catalog = OperationCatalog::default();
    for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
        catalog
            .register(operation_schema(descriptor))
            .expect("unique typed CICS identity");
    }
    verify_semantic_contracts(module, &catalog)
        .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))
}

fn operation_schema(descriptor: CicsExecutableDescriptor) -> OperationSchema {
    let mut schema = OperationSchema::pure(descriptor.identity(), 0, 0);
    schema.required_attributes = [PLAN_ATTRIBUTE.into()].into_iter().collect();
    schema.allowed_effects = descriptor.effects.iter().copied().collect();
    schema.runtime_import = Some(descriptor.runtime_import.into());
    schema.semantic_contract = OperationSemanticContract::CicsEffect(CicsOperationContract {
        plan_attribute: PLAN_ATTRIBUTE.into(),
        expected_operation: Some(descriptor.operation),
        layout_definition_operation: Some(cobol_layout_definition_identity()),
    });
    schema
}

pub(super) fn validate_machine(machine: &ReferenceMachine) -> Result<(), MachineProblem> {
    for operation in machine
        .operations
        .iter()
        .filter(|operation| is_typed(operation))
    {
        let plan = plan(operation)?;
        validate_declared_slots(operation, &plan)?;
        for slot in plan_slots(&plan)? {
            validate_machine_slot(machine, operation, &slot, SlotUse::Input)?;
        }
        for output in &plan.outputs {
            let slot_use = match output.name {
                CicsOutputName::Into => SlotUse::Output,
                CicsOutputName::Resp | CicsOutputName::Resp2 => SlotUse::NumericOutput,
            };
            validate_machine_slot(machine, operation, &output.target, slot_use)?;
        }
    }
    Ok(())
}

pub(super) fn execute(
    machine: &mut ReferenceMachine,
    operation: &Operation,
) -> Result<Step, MachineProblem> {
    let plan = plan(operation)?;
    validate_declared_slots(operation, &plan)?;
    validate_runtime_plan(machine, operation, &plan)?;

    let host_operation = host_operation(plan.operation);
    let mut arguments = BTreeMap::new();
    for operand in &plan.operands {
        let (schema, bytes) = match &operand.value {
            CicsOperandValue::Literal(bytes) => ("mainframe-env.cics.literal@1", bytes.clone()),
            CicsOperandValue::Storage(slot)
                if matches!(
                    operand.name,
                    CicsOperandName::Length | CicsOperandName::MaxLifetime
                ) =>
            {
                (
                    "mainframe-env.cics.decimal@1",
                    read_integer_slot(machine, slot)?.to_string().into_bytes(),
                )
            }
            CicsOperandValue::Storage(slot)
                if operand.name == CicsOperandName::Resource
                    && !plan
                        .operands
                        .iter()
                        .any(|value| value.name == CicsOperandName::Length) =>
            {
                (
                    "mainframe-env.cics.storage-identity@1",
                    format!(
                        "{}:{}:{}",
                        machine.invocation.artifact.as_str(),
                        slot.storage.get(),
                        slot.qualified_layout_name
                    )
                    .into_bytes(),
                )
            }
            CicsOperandValue::Storage(slot) => (
                "mainframe-env.cics.storage-value@1",
                read_slot(machine, slot)?,
            ),
            CicsOperandValue::Integer(value) => (
                "mainframe-env.cics.decimal@1",
                value.to_string().into_bytes(),
            ),
        };
        arguments.insert(operand_name(operand.name).into(), payload(schema, bytes)?);
    }

    let mut into = None;
    let outputs = BTreeMap::new();
    let mut response = None;
    let mut response2 = None;
    for output in &plan.outputs {
        let key = output_name(output.name);
        arguments.insert(
            key.into(),
            payload(
                "mainframe-env.cics.argument@1",
                output.target.qualified_layout_name.as_bytes().to_vec(),
            )?,
        );
        let target = CicsTarget::Resolved(output.target.clone());
        match output.name {
            CicsOutputName::Into => into = Some(target),
            CicsOutputName::Resp => response = Some(target),
            CicsOutputName::Resp2 => response2 = Some(target),
        }
    }
    for option in &plan.options {
        arguments.insert(
            format!("OPTION.{}", option_name(*option)),
            payload("mainframe-env.cics.option@1", Vec::new())?,
        );
    }

    let condition_policy = match &plan.condition {
        CicsCondition::Default => CicsConditionPolicy::Default,
        CicsCondition::NoHandle => CicsConditionPolicy::NoHandle,
        CicsCondition::Respond {
            response,
            response2,
        } => CicsConditionPolicy::Respond {
            response_field: response.qualified_layout_name.clone(),
            response2_field: response2
                .as_ref()
                .map(|slot| slot.qualified_layout_name.clone()),
        },
    };
    let no_handle = matches!(plan.condition, CicsCondition::NoHandle);

    let mut mutation = host_operation
        .is_mutating()
        .then(|| machine.mutation())
        .transpose()?;
    if let Some(mutation) = &mut mutation {
        mutation.transaction = Some(
            machine
                .invocation
                .bindings
                .get("cics.transaction")
                .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
                .unwrap_or_else(|| "DEFAULT".into()),
        );
    }
    let argument_summary = argument_summary(&arguments);
    machine.effect(
        HostRequest::Cics(CicsRequest {
            operation: host_operation,
            arguments,
            condition_policy,
            mutation,
        }),
        PendingKind::Cics {
            operation: host_operation,
            argument_summary,
            into,
            outputs,
            response,
            response2,
            no_handle,
        },
    )
}

pub(super) fn execute_legacy(
    machine: &mut ReferenceMachine,
    args: &[String],
) -> Result<Step, MachineProblem> {
    let operation = CicsOperation::from_tokens(args).ok_or(MachineProblem::UnsupportedForm)?;
    let mut arguments = legacy_arguments(args)?;
    let into = legacy_destination(&arguments, "INTO").map(CicsTarget::Legacy);
    let output_names: &[&str] = match operation {
        CicsOperation::Asktime => &["ABSTIME"],
        CicsOperation::Assign => &["APPLID", "SYSID", "TRANSID", "PRINCIPAL"],
        CicsOperation::FormatTime => &[
            "YYYYMMDD",
            "YYMMDD",
            "MMDDYY",
            "MMDDYYYY",
            "YYDDD",
            "TIME",
            "MILLISECONDS",
        ],
        CicsOperation::Link => &["COMMAREA"],
        CicsOperation::ReadNext | CicsOperation::ReadPrev => &["RIDFLD"],
        _ => &[],
    };
    let outputs = output_names
        .iter()
        .filter_map(|name| {
            legacy_destination(&arguments, name)
                .map(|target| ((*name).into(), CicsTarget::Legacy(target)))
        })
        .collect::<BTreeMap<_, _>>();
    let response_target = legacy_destination(&arguments, "RESP").map(CicsTarget::Legacy);
    let response2_target = legacy_destination(&arguments, "RESP2").map(CicsTarget::Legacy);
    let absolute_time = legacy_destination(&arguments, "ABSTIME");
    for key in [
        "FROM", "COMMAREA", "RIDFLD", "LENGTH", "QUEUE", "MAP", "MAPSET", "TRANSID", "PROGRAM",
        "DATASET", "FILE", "MEMBER", "VERSION",
    ] {
        if outputs.contains_key(key) {
            continue;
        }
        let Some(argument) = arguments.get(key) else {
            continue;
        };
        if argument.schema() == "mainframe-env.cics.literal@1" {
            continue;
        }
        let token = String::from_utf8_lossy(argument.bytes()).into_owned();
        let reference_tokens = token
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if let Ok(reference) = machine.reference(&reference_tokens) {
            let value = machine.read_reference(&reference)?;
            arguments.insert(
                key.into(),
                payload("mainframe-env.cics.storage-value@1", value)?,
            );
        }
    }
    if operation == CicsOperation::FormatTime
        && let Some(target) = absolute_time
    {
        let tokens = target
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let reference = machine.reference(&tokens)?;
        let bytes = machine.read_reference(&reference)?;
        if !is_numeric(reference.layout.category) {
            return Err(MachineProblem::DataException);
        }
        let value = decode_decimal(&reference.layout, &bytes)?;
        if value.scale != 0 {
            return Err(MachineProblem::DataException);
        }
        arguments.insert(
            "ABSTIME".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                value.coefficient.to_string().into_bytes(),
            )?,
        );
    }
    let condition_policy = legacy_condition_policy(args, &arguments)?;
    let mut mutation = operation
        .is_mutating()
        .then(|| machine.mutation())
        .transpose()?;
    if let Some(mutation) = &mut mutation {
        mutation.transaction = Some(
            machine
                .invocation
                .bindings
                .get("cics.transaction")
                .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
                .unwrap_or_else(|| "DEFAULT".into()),
        );
    }
    let argument_summary = argument_summary(&arguments);
    machine.effect(
        HostRequest::Cics(CicsRequest {
            operation,
            arguments,
            condition_policy,
            mutation,
        }),
        PendingKind::Cics {
            operation,
            argument_summary,
            into,
            outputs,
            response: response_target,
            response2: response2_target,
            no_handle: args.iter().any(|argument| argument == "NOHANDLE"),
        },
    )
}

fn legacy_condition_policy(
    args: &[String],
    arguments: &BTreeMap<String, BoundedPayload>,
) -> Result<CicsConditionPolicy, MachineProblem> {
    let response = arguments.get("RESP");
    if response.is_none() && arguments.contains_key("RESP2") {
        return Err(MachineProblem::UnsupportedForm);
    }
    if let Some(response) = response {
        // IBM's common command format defines RESP as implying NOHANDLE while
        // still updating the response area. Preserve that binding when the
        // redundant NOHANDLE keyword is also present.
        Ok(CicsConditionPolicy::Respond {
            response_field: String::from_utf8_lossy(response.bytes()).into_owned(),
            response2_field: arguments
                .get("RESP2")
                .map(|value| String::from_utf8_lossy(value.bytes()).into_owned()),
        })
    } else if args.iter().any(|arg| arg.eq_ignore_ascii_case("NOHANDLE")) {
        Ok(CicsConditionPolicy::NoHandle)
    } else {
        Ok(CicsConditionPolicy::Default)
    }
}

pub(super) fn write_target(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &CobolValue,
) -> Result<(), MachineProblem> {
    match target {
        CicsTarget::Legacy(target) => {
            let tokens = target
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            machine.write_reference_value(&tokens, value)
        }
        CicsTarget::Resolved(slot) => write_resolved(machine, slot, value),
    }
}

fn write_resolved(
    machine: &mut ReferenceMachine,
    slot: &CicsStorageSlot,
    value: &CobolValue,
) -> Result<(), MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .cloned()
        .ok_or(MachineProblem::UnknownStorage)?;
    let length = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or(MachineProblem::UnknownStorage)?
        .length;
    let reference = ResolvedReference {
        layout,
        offset: 0,
        length,
    };
    if reference.layout.dynamic {
        let bytes = match value {
            CobolValue::Bytes(bytes) => bytes.clone(),
            CobolValue::Decimal(value) => decimal_string(*value).into_bytes(),
        };
        return machine.write_dynamic(&reference.layout, &bytes);
    }
    let bytes = match value {
        CobolValue::Bytes(bytes)
            if is_pointer_like(reference.layout.category)
                && bytes.iter().all(|byte| *byte == 0) =>
        {
            vec![0; reference.length]
        }
        CobolValue::Bytes(bytes) => FixedValue::fit(
            bytes,
            reference.length,
            reference.length == reference.layout.length && reference.layout.justified_right,
        )
        .bytes()
        .to_vec(),
        CobolValue::Decimal(value)
            if reference.length == reference.layout.length
                && is_numeric(reference.layout.category) =>
        {
            encode_decimal(
                &reference.layout,
                if matches!(
                    reference.layout.category,
                    LayoutCategory::FloatShort | LayoutCategory::FloatLong
                ) {
                    *value
                } else {
                    decimal_rescale(*value, reference.layout.scale)?
                },
            )?
        }
        CobolValue::Decimal(value) => {
            FixedValue::fit(decimal_string(*value).as_bytes(), reference.length, false)
                .bytes()
                .to_vec()
        }
    };
    machine.write_reference(&reference, &bytes)
}

fn plan(operation: &Operation) -> Result<CicsEffectPlan, MachineProblem> {
    let expected = expected_operation(&operation.identity)
        .ok_or_else(|| invalid_plan("operation identity is not a supported typed CICS route"))?;
    if !operation.operands.is_empty()
        || !operation.results.is_empty()
        || operation.effects != expected_effects(expected)
    {
        return Err(invalid_plan("operation signature or effects are invalid"));
    }
    if operation.attributes.contains_key("arguments")
        || operation.attributes.contains_key("control_text")
        || operation
            .attributes
            .keys()
            .any(|name| name.starts_with("arg_"))
    {
        return Err(invalid_plan(
            "legacy arguments and control text attributes are forbidden",
        ));
    }
    let bytes = match operation.attributes.get(PLAN_ATTRIBUTE) {
        Some(Attribute::Bytes(bytes)) => bytes,
        _ => return Err(invalid_plan("missing bytes attribute cics_plan")),
    };
    let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default())
        .map_err(|problem| invalid_plan(&problem.to_string()))?;
    if expected != plan.operation {
        return Err(invalid_plan(
            "operation identity does not match plan operation",
        ));
    }
    Ok(plan)
}

fn validate_declared_slots(
    operation: &Operation,
    plan: &CicsEffectPlan,
) -> Result<(), MachineProblem> {
    let expected = plan_slots(plan)?
        .into_iter()
        .map(|slot| slot.storage)
        .collect::<BTreeSet<_>>();
    let mut declared = BTreeSet::new();
    for reference in &operation.storage {
        if reference.offset != 0 || !declared.insert(reference.storage) {
            return Err(invalid_plan(
                "operation storage declarations must be unique offset-zero slots",
            ));
        }
    }
    if declared != expected {
        return Err(invalid_plan(
            "operation storage declarations do not exactly match plan slots",
        ));
    }
    Ok(())
}

fn validate_runtime_plan(
    machine: &ReferenceMachine,
    operation: &Operation,
    plan: &CicsEffectPlan,
) -> Result<(), MachineProblem> {
    for operand in &plan.operands {
        if let CicsOperandValue::Storage(slot) = &operand.value {
            validate_machine_slot(machine, operation, slot, SlotUse::Input)?;
        }
    }
    for output in &plan.outputs {
        validate_machine_slot(
            machine,
            operation,
            &output.target,
            match output.name {
                CicsOutputName::Into => SlotUse::Output,
                CicsOutputName::Resp | CicsOutputName::Resp2 => SlotUse::NumericOutput,
            },
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SlotUse {
    Input,
    Output,
    NumericOutput,
}

fn validate_machine_slot(
    machine: &ReferenceMachine,
    operation: &Operation,
    slot: &CicsStorageSlot,
    slot_use: SlotUse,
) -> Result<(), MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout name is unknown"))?;
    if machine.storage_names_by_id.get(&slot.storage) != Some(&slot.qualified_layout_name) {
        return Err(invalid_plan("plan storage ID does not name its layout"));
    }
    let id_view = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or_else(|| invalid_plan("plan storage slot is unknown"))?;
    let name_view = machine
        .views
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout has no storage view"))?;
    if layout.name != slot.qualified_layout_name || id_view != name_view {
        return Err(invalid_plan(
            "plan storage slot does not match its qualified layout name",
        ));
    }
    if matches!(slot_use, SlotUse::Output | SlotUse::NumericOutput)
        && matches!(
            layout.category,
            LayoutCategory::Condition | LayoutCategory::Rename
        )
    {
        return Err(invalid_plan("plan output is not writable storage"));
    }
    if matches!(slot_use, SlotUse::NumericOutput) && !is_numeric(layout.category) {
        return Err(invalid_plan("RESP and RESP2 outputs must be numeric"));
    }
    let exact_length = u64::try_from(id_view.length)
        .map_err(|_| invalid_plan("plan storage view length is invalid"))?;
    if !operation.storage.iter().any(|reference| {
        reference.storage == slot.storage
            && reference.offset == 0
            && reference.length == exact_length
    }) {
        return Err(invalid_plan(
            "operation does not declare the complete plan storage view",
        ));
    }
    Ok(())
}

fn plan_slots(plan: &CicsEffectPlan) -> Result<Vec<CicsStorageSlot>, MachineProblem> {
    let mut slots = BTreeMap::<StorageId, CicsStorageSlot>::new();
    for operand in &plan.operands {
        if let CicsOperandValue::Storage(slot) = &operand.value {
            insert_slot(&mut slots, slot)?;
        }
    }
    for output in &plan.outputs {
        insert_slot(&mut slots, &output.target)?;
    }
    match &plan.condition {
        CicsCondition::Default | CicsCondition::NoHandle => {}
        CicsCondition::Respond {
            response,
            response2,
        } => {
            insert_slot(&mut slots, response)?;
            if let Some(response2) = response2 {
                insert_slot(&mut slots, response2)?;
            }
        }
    }
    Ok(slots.into_values().collect())
}

fn insert_slot(
    slots: &mut BTreeMap<StorageId, CicsStorageSlot>,
    slot: &CicsStorageSlot,
) -> Result<(), MachineProblem> {
    if slots
        .insert(slot.storage, slot.clone())
        .is_some_and(|existing| existing.qualified_layout_name != slot.qualified_layout_name)
    {
        return Err(invalid_plan(
            "one storage slot names more than one qualified layout",
        ));
    }
    Ok(())
}

fn read_slot(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<Vec<u8>, MachineProblem> {
    let view = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or(MachineProblem::UnknownStorage)?;
    let name_view = machine
        .views
        .get(&slot.qualified_layout_name)
        .ok_or(MachineProblem::UnknownStorage)?;
    if view != name_view {
        return Err(invalid_plan("resolved operand storage view changed"));
    }
    machine.read(&slot.qualified_layout_name)
}

fn read_integer_slot(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<i128, MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .ok_or(MachineProblem::UnknownStorage)?;
    if !is_numeric(layout.category) {
        return Err(MachineProblem::DataException);
    }
    let value = decode_decimal(layout, &read_slot(machine, slot)?)?;
    if value.scale != 0 {
        return Err(MachineProblem::DataException);
    }
    Ok(value.coefficient)
}

fn expected_operation(identity: &OperationIdentity) -> Option<CicsPlanOperation> {
    cics_executable_descriptor_for_identity(identity).map(|descriptor| descriptor.operation)
}

fn expected_effects(operation: CicsPlanOperation) -> &'static [Effect] {
    cics_executable_descriptor(operation).effects
}

const fn host_operation(operation: CicsPlanOperation) -> CicsOperation {
    match operation {
        CicsPlanOperation::Deq => CicsOperation::Deq,
        CicsPlanOperation::Enq => CicsOperation::Enq,
        CicsPlanOperation::Read => CicsOperation::Read,
        CicsPlanOperation::Rewrite => CicsOperation::Rewrite,
        CicsPlanOperation::Syncpoint => CicsOperation::Syncpoint,
    }
}

const fn operand_name(name: CicsOperandName) -> &'static str {
    match name {
        CicsOperandName::File => "FILE",
        CicsOperandName::Dataset => "DATASET",
        CicsOperandName::From => "FROM",
        CicsOperandName::Ridfld => "RIDFLD",
        CicsOperandName::Resource => "RESOURCE",
        CicsOperandName::Length => "LENGTH",
        CicsOperandName::MaxLifetime => "MAXLIFETIME",
    }
}

const fn output_name(name: CicsOutputName) -> &'static str {
    match name {
        CicsOutputName::Into => "INTO",
        CicsOutputName::Resp => "RESP",
        CicsOutputName::Resp2 => "RESP2",
    }
}

const fn option_name(option: CicsPlanOption) -> &'static str {
    match option {
        CicsPlanOption::Update => "UPDATE",
        CicsPlanOption::Rollback => "ROLLBACK",
        CicsPlanOption::NoHandle => "NOHANDLE",
        CicsPlanOption::Task => "TASK",
        CicsPlanOption::Uow => "UOW",
        CicsPlanOption::NoSuspend => "NOSUSPEND",
    }
}

fn payload(schema: &str, bytes: Vec<u8>) -> Result<BoundedPayload, MachineProblem> {
    BoundedPayload::new(schema, bytes, InvocationLimits::default())
        .map_err(|_| MachineProblem::ResourceExhausted)
}

fn argument_summary(arguments: &BTreeMap<String, BoundedPayload>) -> String {
    arguments
        .iter()
        .map(|(name, value)| {
            if matches!(name.as_str(), "MAP" | "MAPSET" | "TRANSID" | "PROGRAM") {
                format!(
                    "{name}={:?}/{}",
                    String::from_utf8_lossy(value.bytes()),
                    value.schema()
                )
            } else {
                format!("{name}=<{} bytes>/{}", value.bytes().len(), value.schema())
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn legacy_arguments(
    tokens: &[String],
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let mut arguments = BTreeMap::new();
    let command = tokens
        .iter()
        .position(|token| {
            !matches!(
                token.to_ascii_uppercase().as_str(),
                "EXEC" | "CICS" | "END-EXEC"
            )
        })
        .ok_or(MachineProblem::InvalidOperation)?;
    let first = tokens[command].to_ascii_uppercase();
    let mut index = command + 1;
    if matches!(first.as_str(), "HANDLE" | "RECEIVE" | "SEND" | "WRITEQ")
        && tokens.get(index).is_some_and(|token| {
            matches!(
                token.to_ascii_uppercase().as_str(),
                "ABEND" | "CONDITION" | "MAP" | "TEXT" | "TD"
            )
        })
    {
        let subcommand = tokens[index].to_ascii_uppercase();
        index += 1;
        if tokens.get(index).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 1..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            arguments.insert(
                subcommand,
                payload(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    raw.trim_matches(['\'', '"']).as_bytes().to_vec(),
                )?,
            );
            index = end + 1;
        }
    }
    while index < tokens.len() {
        let key = tokens[index].to_ascii_uppercase();
        if key == "END-EXEC" {
            break;
        }
        if tokens.get(index + 1).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index + 1).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 2..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            arguments.insert(
                key,
                payload(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    raw.trim_matches(['\'', '"']).as_bytes().to_vec(),
                )?,
            );
            index = end + 1;
        } else {
            arguments.insert(
                format!("OPTION.{key}"),
                payload("mainframe-env.cics.option@1", Vec::new())?,
            );
            index += 1;
        }
    }
    Ok(arguments)
}

fn legacy_destination(arguments: &BTreeMap<String, BoundedPayload>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .map(|value| String::from_utf8_lossy(value.bytes()).into_owned())
}

fn invalid_plan(detail: &str) -> MachineProblem {
    MachineProblem::InvalidArtifact(format!("invalid typed CICS effect plan: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_ir::{
        CicsNamedOperand, CicsOutputBinding, IrLimits, Module, ModuleBuilder, StorageReference,
        encode_cics_effect_plan,
    };

    fn syncpoint_plan() -> CicsEffectPlan {
        CicsEffectPlan {
            operation: CicsPlanOperation::Syncpoint,
            operands: Vec::new(),
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        }
    }

    fn read_plan() -> CicsEffectPlan {
        let mut ids = ModuleBuilder::new(IrLimits::default());
        let into = ids.add_storage("into-x", 1, None).unwrap();
        CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Literal(b"003".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: into,
                    qualified_layout_name: "INTO-X".into(),
                },
            }],
            condition: CicsCondition::Default,
        }
    }

    #[test]
    fn legacy_resp_binding_takes_precedence_over_nohandle() {
        let tokens = [
            "EXEC", "CICS", "ASKTIME", "NOHANDLE", "RESP", "(", "RESP-X", ")", "RESP2", "(",
            "RESP2-X", ")", "END-EXEC",
        ]
        .map(str::to_string);
        let arguments = legacy_arguments(&tokens).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&tokens, &arguments),
            Ok(CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: Some("RESP2-X".into()),
            })
        );

        let nohandle = ["EXEC", "CICS", "ASKTIME", "NOHANDLE", "END-EXEC"].map(str::to_string);
        let arguments = legacy_arguments(&nohandle).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&nohandle, &arguments),
            Ok(CicsConditionPolicy::NoHandle)
        );
    }

    #[test]
    fn legacy_resp2_without_resp_fails_closed() {
        let tokens = [
            "EXEC", "CICS", "ASKTIME", "RESP2", "(", "RESP2-X", ")", "END-EXEC",
        ]
        .map(str::to_string);
        let arguments = legacy_arguments(&tokens).expect("legacy arguments");
        assert_eq!(
            legacy_condition_policy(&tokens, &arguments),
            Err(MachineProblem::UnsupportedForm)
        );
    }

    fn module(
        identity: OperationIdentity,
        plan: &CicsEffectPlan,
        effects: Vec<Effect>,
        legacy_arguments: bool,
        extra_storage: bool,
        transform: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> Module {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let storage = extra_storage.then(|| builder.add_storage("extra", 1, None).unwrap());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let bytes = transform(encode_cics_effect_plan(plan, CicsPlanLimits::default()).unwrap());
        let mut attributes = BTreeMap::from([(PLAN_ATTRIBUTE.into(), Attribute::Bytes(bytes))]);
        if legacy_arguments {
            attributes.insert("arguments".into(), Attribute::Bytes(Vec::new()));
        }
        builder
            .add_operation(
                block,
                identity,
                Vec::new(),
                0,
                attributes,
                effects,
                storage
                    .map(|storage| {
                        vec![StorageReference {
                            storage,
                            offset: 0,
                            length: 1,
                        }]
                    })
                    .unwrap_or_default(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder.finish().unwrap()
    }

    #[test]
    fn typed_cics_plan_identity_signature_and_storage_are_fail_closed() {
        let syncpoint = syncpoint_plan();
        let identity = operation_identities()[4].clone();
        assert!(
            super::super::validate_module(&module(
                identity.clone(),
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                false,
                |bytes| bytes,
            ))
            .is_ok()
        );
        for invalid in [
            module(
                identity.clone(),
                &read_plan(),
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                false,
                |bytes| bytes,
            ),
            module(
                identity.clone(),
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                true,
                false,
                |bytes| bytes,
            ),
            module(
                identity.clone(),
                &syncpoint,
                vec![Effect::Transaction],
                false,
                false,
                |bytes| bytes,
            ),
            module(
                identity,
                &syncpoint,
                expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
                false,
                true,
                |bytes| bytes,
            ),
        ] {
            assert!(matches!(
                super::super::validate_module(&invalid),
                Err(MachineProblem::InvalidArtifact(_))
            ));
        }
    }

    #[test]
    fn malformed_plan_and_unregistered_major_are_rejected() {
        let syncpoint = syncpoint_plan();
        let malformed = module(
            operation_identities()[4].clone(),
            &syncpoint,
            expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
            false,
            false,
            |mut bytes| {
                bytes.push(0);
                bytes
            },
        );
        assert!(matches!(
            super::super::validate_module(&malformed),
            Err(MachineProblem::InvalidArtifact(_))
        ));
        let wrong_major = module(
            OperationIdentity::new("cics.recovery", "syncpoint", 2).unwrap(),
            &syncpoint,
            expected_effects(CicsPlanOperation::Syncpoint).to_vec(),
            false,
            false,
            |bytes| bytes,
        );
        assert!(matches!(
            super::super::validate_module(&wrong_major),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }

    #[test]
    fn dialect_descriptors_drive_interpreter_registry_and_contracts() {
        assert_eq!(
            operation_identities().into_iter().collect::<BTreeSet<_>>(),
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.identity())
                .collect()
        );
        for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
            let identity = descriptor.identity();
            assert_eq!(expected_operation(&identity), Some(descriptor.operation));
            assert_eq!(expected_effects(descriptor.operation), descriptor.effects);
            let schema = operation_schema(descriptor);
            assert_eq!(schema.identity, identity);
            assert_eq!(
                schema.allowed_effects,
                descriptor.effects.iter().copied().collect()
            );
            assert_eq!(
                schema.runtime_import.as_deref(),
                Some(descriptor.runtime_import)
            );
            assert_eq!(
                schema.semantic_contract,
                OperationSemanticContract::CicsEffect(CicsOperationContract {
                    plan_attribute: PLAN_ATTRIBUTE.into(),
                    expected_operation: Some(descriptor.operation),
                    layout_definition_operation: Some(
                        OperationIdentity::new("mainframe.core.cobol", "define", 1).unwrap(),
                    ),
                })
            );
        }
    }

    #[test]
    fn equal_alias_views_cannot_substitute_a_different_storage_identity() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let backing = builder.add_storage("backing", 3, None).unwrap();
        let alias = |storage| StorageReference {
            storage,
            offset: 0,
            length: 3,
        };
        let wrong = builder.add_storage("a", 3, Some(alias(backing))).unwrap();
        let named = builder.add_storage("b", 3, Some(alias(backing))).unwrap();
        assert_ne!(wrong, named);
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text("B".into())),
                    ("simple_name".into(), Attribute::Text("B".into())),
                    ("category".into(), Attribute::Text("alphanumeric".into())),
                    ("picture".into(), Attribute::Text(String::new())),
                    ("digits".into(), Attribute::Integer(0)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(3)),
                    ("element_length".into(), Attribute::Integer(3)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(CicsStorageSlot {
                        storage: wrong,
                        qualified_layout_name: "B".into(),
                    }),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: named,
                    qualified_layout_name: "B".into(),
                },
            }],
            condition: CicsCondition::Default,
        };
        builder
            .add_operation(
                block,
                operation_identities()[2].clone(),
                Vec::new(),
                0,
                BTreeMap::from([(
                    PLAN_ATTRIBUTE.into(),
                    Attribute::Bytes(
                        encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                    ),
                )]),
                expected_effects(CicsPlanOperation::Read).to_vec(),
                vec![alias(wrong), alias(named)],
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(super::super::NAMESPACE, "halt", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        let bytes =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        assert!(matches!(
            ReferenceMachine::from_binary(
                &bytes,
                super::super::tests::invocation(),
                CodecLimits::default(),
            ),
            Err(MachineProblem::InvalidArtifact(_))
        ));
    }
}
