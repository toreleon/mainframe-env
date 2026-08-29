use crate::FixedValue;
use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_execution_api::{
    Abend, BoundedPayload, Completion, Condition, IdempotencyKey, Invocation, InvocationLimits,
    Machine, MachineDrive, MachineResume, Quantum, Selector, Suspension, Transfer,
};
use mainframe_env_host_api::{
    CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, DatasetName, DatasetRequest,
    EffectRequest, EffectResult, HostLimits, HostProblem, HostRequest, HostResult, Mutation,
    ProgramName, ProgramRequest, TerminalRequest,
};
use mainframe_env_ir::{
    Attribute, CodecLimits, Module, Operation, OperationIdentity, StorageId, decode_binary,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const NAMESPACE: &str = "mainframe.core.cobol";

#[derive(Clone, Debug, Eq, PartialEq)]
struct StorageView {
    base: usize,
    offset: usize,
    length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingKind {
    Accept { target: String },
    DatasetRead { target: Option<String> },
    Cics,
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Pending {
    sequence: u64,
    kind: PendingKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MachineSnapshot {
    pub schema_version: u32,
    pub program_counter: usize,
    pub effect_sequence: u64,
    pub output: Vec<u8>,
    pub base_storage: Vec<Vec<u8>>,
    pub perform_stack: Vec<usize>,
    pub altered_targets: BTreeMap<String, String>,
}

pub struct ReferenceMachine {
    invocation: Invocation,
    operations: Vec<Operation>,
    bases: Vec<Vec<u8>>,
    views: BTreeMap<String, StorageView>,
    views_by_id: BTreeMap<StorageId, StorageView>,
    labels: BTreeMap<String, usize>,
    altered: BTreeMap<String, String>,
    pc: usize,
    output: Vec<u8>,
    effect_sequence: u64,
    pending: Option<Pending>,
    perform_stack: Vec<usize>,
    deferred_drive: Option<MachineDrive<EffectRequest>>,
}

impl ReferenceMachine {
    pub fn from_binary(
        binary: &[u8],
        invocation: Invocation,
        codec_limits: CodecLimits,
    ) -> Result<Self, MachineProblem> {
        let module = decode_binary(binary, codec_limits)
            .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))?;
        validate_module(&module)?;
        let (bases, views, views_by_id) = storage(&module, invocation.limits.max_storage_bytes)?;
        let operations: Vec<_> = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .cloned()
            .collect();
        let labels = operations
            .iter()
            .enumerate()
            .filter_map(|(index, operation)| {
                (operation.identity.name() == "label")
                    .then(|| argument(operation, 0).map(|name| (normalize(name), index)))
                    .flatten()
            })
            .collect();
        Ok(Self {
            invocation,
            operations,
            bases,
            views,
            views_by_id,
            labels,
            altered: BTreeMap::new(),
            pc: 0,
            output: Vec::new(),
            effect_sequence: 0,
            pending: None,
            perform_stack: Vec::new(),
            deferred_drive: None,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> MachineSnapshot {
        MachineSnapshot {
            schema_version: 1,
            program_counter: self.pc,
            effect_sequence: self.effect_sequence,
            output: self.output.clone(),
            base_storage: self.bases.clone(),
            perform_stack: self.perform_stack.clone(),
            altered_targets: self.altered.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: MachineSnapshot) -> Result<(), MachineProblem> {
        if snapshot.schema_version != 1
            || snapshot.program_counter > self.operations.len()
            || snapshot.base_storage.iter().map(Vec::len).sum::<usize>()
                > self.invocation.limits.max_storage_bytes as usize
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        self.pc = snapshot.program_counter;
        self.effect_sequence = snapshot.effect_sequence;
        self.output = snapshot.output;
        self.bases = snapshot.base_storage;
        self.perform_stack = snapshot.perform_stack;
        self.altered = snapshot.altered_targets;
        self.pending = None;
        self.deferred_drive = None;
        Ok(())
    }

    pub fn restore_checkpoint(&mut self, payload: &BoundedPayload) -> Result<(), MachineProblem> {
        if payload.schema() != "mainframe-env.reference-machine-checkpoint@1" {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let snapshot = decode_snapshot(
            payload.bytes(),
            self.invocation.limits.max_storage_bytes as usize,
            self.invocation.limits.max_output_bytes as usize,
            self.invocation.limits.max_frames as usize,
        )?;
        self.restore(snapshot)
    }

    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<FixedValue> {
        self.read(name).ok().map(FixedValue::new)
    }

    fn resume_host(&mut self, result: EffectResult) -> Result<(), MachineProblem> {
        let pending = self
            .pending
            .take()
            .ok_or(MachineProblem::UnexpectedResume)?;
        result
            .validate(pending.sequence, HostLimits::default())
            .map_err(MachineProblem::Host)?;
        let outcome = result.outcome.map_err(MachineProblem::Host)?;
        match (pending.kind, outcome) {
            (PendingKind::Accept { target }, HostResult::Terminal(payload)) => {
                self.write(&target, payload.bytes())?
            }
            (
                PendingKind::DatasetRead {
                    target: Some(target),
                },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Records {
                    records, ..
                }),
            ) => {
                if let Some(record) = records.first() {
                    self.write(&target, record)?;
                }
            }
            (PendingKind::Cics, HostResult::Cics(response)) => {
                self.deferred_drive = match response.disposition {
                    CicsDisposition::Complete => {
                        (response.response != 0).then_some(MachineDrive::Condition(Condition {
                            name: response.condition,
                            response: response.response,
                            response2: response.response2,
                            handled: false,
                        }))
                    }
                    CicsDisposition::Suspended => Some(MachineDrive::Suspended(Suspension {
                        kind: "cics-terminal".into(),
                        resume_token: format!(
                            "{}:{}",
                            self.invocation.run_unit_id, self.effect_sequence
                        ),
                        state_bytes: response.payload.bytes().len() as u64,
                    })),
                    CicsDisposition::Transfer => {
                        let target = response
                            .target
                            .ok_or(MachineProblem::UnexpectedHostResult)?;
                        Some(MachineDrive::Transfer(Transfer {
                            selector: Selector::new(target, InvocationLimits::default())
                                .map_err(|_| MachineProblem::UnexpectedHostResult)?,
                            payload: response.payload,
                            replace_frame: true,
                        }))
                    }
                    CicsDisposition::Returned => Some(MachineDrive::Completed(self.complete()?)),
                    CicsDisposition::Abended => Some(MachineDrive::Abend(Abend {
                        code: response.condition,
                        reason: Some(format!(
                            "EIBRESP={} EIBRESP2={}",
                            response.response, response.response2
                        )),
                    })),
                };
            }
            (PendingKind::DatasetRead { .. } | PendingKind::Ignore, _) => {}
            _ => return Err(MachineProblem::UnexpectedHostResult),
        }
        Ok(())
    }

    fn execute(&mut self, operation: &Operation) -> Result<Step, MachineProblem> {
        let name = operation.identity.name();
        let args = arguments(operation);
        match name {
            "init" => {
                let bytes = bytes_attribute(operation, "initial")?;
                let reference = operation
                    .storage
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?;
                self.write_storage(reference.storage, bytes)?;
            }
            "display" => {
                let mut line = Vec::new();
                for token in &args {
                    if token.eq_ignore_ascii_case("WITH")
                        || token.eq_ignore_ascii_case("NO")
                        || token.eq_ignore_ascii_case("ADVANCING")
                    {
                        continue;
                    }
                    line.extend(self.resolve(token)?);
                }
                let no_advancing = args.windows(3).any(|window| {
                    window
                        .iter()
                        .map(|item| item.as_str())
                        .eq(["WITH", "NO", "ADVANCING"])
                });
                if !no_advancing {
                    line.push(b'\n');
                }
                self.append_output(&line)?;
            }
            "move" => {
                let to = position(&args, "TO").ok_or(MachineProblem::InvalidOperation)?;
                let value = self.resolve(args.first().ok_or(MachineProblem::InvalidOperation)?)?;
                let target = args.get(to + 1).ok_or(MachineProblem::InvalidOperation)?;
                self.write(target, &value)?;
            }
            "add" | "subtract" | "multiply" | "divide" | "compute" => {
                self.arithmetic(name, &args)?
            }
            "initialize" => {
                for target in &args {
                    if self.views.contains_key(&normalize(target)) {
                        let length = self.read(target)?.len();
                        self.write(target, &vec![b' '; length])?;
                    }
                }
            }
            "set" => {
                if args.len() >= 3 {
                    let value = self.resolve(&args[2])?;
                    self.write(&args[0], &value)?;
                } else {
                    return Err(MachineProblem::InvalidOperation);
                }
            }
            "allocate" => {
                if let Some(target) = args.last() {
                    self.write(target, b"1")?;
                }
            }
            "free" => {
                if let Some(target) = args.first() {
                    let length = self.read(target)?.len();
                    self.write(target, &vec![0; length])?;
                }
            }
            "string" => self.string_op(&args)?,
            "unstring" => self.unstring_op(&args)?,
            "inspect" => self.inspect_op(&args)?,
            "json_generate" => self.generate(&args, true)?,
            "xml_generate" => self.generate(&args, false)?,
            "json_parse" => self.parse_generated(&args, true)?,
            "xml_parse" => self.parse_generated(&args, false)?,
            "if" => self.if_op(&args)?,
            "evaluate" => self.evaluate_op(&args)?,
            "search" => self.search_op(&args)?,
            "go_to" => {
                return Ok(Step::Jump(
                    self.label(args.last().ok_or(MachineProblem::InvalidOperation)?)?,
                ));
            }
            "alter" => {
                if args.len() < 5 {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.altered
                    .insert(normalize(&args[0]), normalize(args.last().unwrap()));
            }
            "perform" => {
                let target = args.first().ok_or(MachineProblem::InvalidOperation)?;
                self.perform_stack.push(self.pc + 1);
                return Ok(Step::Jump(self.label(target)?));
            }
            "exit" => {
                if let Some(return_pc) = self.perform_stack.pop() {
                    return Ok(Step::Jump(return_pc));
                }
            }
            "entry" | "label" | "continue" => {}
            "accept" => return self.accept_effect(&args),
            "call" | "cancel" => return self.program_effect(name, &args),
            "open" | "close" | "read" | "write" => return self.dataset_effect(name, &args),
            "exec_cics" => return self.cics_effect(&args),
            "stop_run" | "go_back" | "halt" => return Ok(Step::Complete),
            _ => return Err(MachineProblem::InvalidOperation),
        }
        Ok(Step::Next)
    }

    fn accept_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let target = normalize(args.first().ok_or(MachineProblem::InvalidOperation)?);
        let request = HostRequest::Terminal(TerminalRequest::Read {
            session: mainframe_env_host_api::SessionId::new(
                self.invocation.run_unit_id.as_str(),
                128,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?,
        });
        self.effect(request, PendingKind::Accept { target })
    }
    fn program_effect(&mut self, name: &str, args: &[String]) -> Result<Step, MachineProblem> {
        let program = ProgramName::new(
            args.first()
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']),
            128,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        let request = if name == "cancel" {
            ProgramRequest::Cancel { program }
        } else {
            ProgramRequest::Call {
                program,
                payload: empty_payload()?,
            }
        };
        self.effect(HostRequest::Program(request), PendingKind::Ignore)
    }
    fn dataset_effect(&mut self, name: &str, args: &[String]) -> Result<Step, MachineProblem> {
        let dataset = DatasetName::new(
            args.first()
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']),
            128,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        let request = match name {
            "read" => DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: 1,
            },
            "write" => {
                let record = args
                    .get(1)
                    .map(|value| self.resolve(value))
                    .transpose()?
                    .unwrap_or_default();
                DatasetRequest::Write {
                    dataset,
                    member: None,
                    records: vec![record],
                    expected_version: None,
                    mutation: self.mutation()?,
                }
            }
            _ => DatasetRequest::Attributes { dataset },
        };
        let target = (name == "read").then(|| args.get(2).cloned()).flatten();
        self.effect(
            HostRequest::Dataset(request),
            PendingKind::DatasetRead { target },
        )
    }
    fn cics_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let operation = CicsOperation::from_tokens(args).ok_or(MachineProblem::UnsupportedForm)?;
        let arguments = cics_arguments(args)?;
        let condition_policy = if args.iter().any(|arg| arg.eq_ignore_ascii_case("NOHANDLE")) {
            CicsConditionPolicy::NoHandle
        } else if let Some(response) = arguments.get("RESP") {
            CicsConditionPolicy::Respond {
                response_field: String::from_utf8_lossy(response.bytes()).into_owned(),
                response2_field: arguments
                    .get("RESP2")
                    .map(|value| String::from_utf8_lossy(value.bytes()).into_owned()),
            }
        } else {
            CicsConditionPolicy::Default
        };
        let mutation = operation
            .is_mutating()
            .then(|| self.mutation())
            .transpose()?;
        self.effect(
            HostRequest::Cics(CicsRequest {
                operation,
                arguments,
                condition_policy,
                mutation,
            }),
            PendingKind::Cics,
        )
    }
    fn effect(&mut self, request: HostRequest, kind: PendingKind) -> Result<Step, MachineProblem> {
        self.effect_sequence = self
            .effect_sequence
            .checked_add(1)
            .ok_or(MachineProblem::ResourceExhausted)?;
        if self.effect_sequence > self.invocation.limits.max_effects {
            return Err(MachineProblem::ResourceExhausted);
        }
        let mutating = request.is_mutating();
        let key = mutating.then(|| self.effect_key()).transpose()?;
        let effect = EffectRequest {
            run_unit: self.invocation.run_unit_id.clone(),
            sequence: self.effect_sequence,
            deadline_tick: self.invocation.deadline_tick,
            idempotency_key: key,
            request,
        };
        effect
            .validate(HostLimits::default())
            .map_err(MachineProblem::Host)?;
        self.pending = Some(Pending {
            sequence: self.effect_sequence,
            kind,
        });
        Ok(Step::Effect(effect))
    }
    fn mutation(&self) -> Result<Mutation, MachineProblem> {
        Ok(Mutation {
            sequence: self.effect_sequence + 1,
            idempotency_key: self.next_effect_key()?,
            transaction: None,
        })
    }
    fn effect_key(&self) -> Result<IdempotencyKey, MachineProblem> {
        IdempotencyKey::new(
            format!(
                "{}:{}",
                self.invocation.idempotency_key.as_str(),
                self.effect_sequence
            ),
            InvocationLimits::default(),
        )
        .map_err(|_| MachineProblem::ResourceExhausted)
    }
    fn next_effect_key(&self) -> Result<IdempotencyKey, MachineProblem> {
        IdempotencyKey::new(
            format!(
                "{}:{}",
                self.invocation.idempotency_key.as_str(),
                self.effect_sequence + 1
            ),
            InvocationLimits::default(),
        )
        .map_err(|_| MachineProblem::ResourceExhausted)
    }

    fn arithmetic(&mut self, name: &str, args: &[String]) -> Result<(), MachineProblem> {
        let (target, value) = match name {
            "compute" => {
                let target = args
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (target, self.eval_expression(&args[2..])?)
            }
            "add" => {
                let pos = position(args, "TO").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (
                    target.clone(),
                    self.number(&target)?
                        .checked_add(self.number(&args[0])?)
                        .ok_or(MachineProblem::SizeError)?,
                )
            }
            "subtract" => {
                let pos = position(args, "FROM").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (
                    target.clone(),
                    self.number(&target)?
                        .checked_sub(self.number(&args[0])?)
                        .ok_or(MachineProblem::SizeError)?,
                )
            }
            "multiply" => {
                let pos = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (
                    target.clone(),
                    self.number(&target)?
                        .checked_mul(self.number(&args[0])?)
                        .ok_or(MachineProblem::SizeError)?,
                )
            }
            "divide" => {
                let pos = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                let divisor = self.number(&args[0])?;
                if divisor == 0 {
                    return Err(MachineProblem::SizeError);
                }
                (target.clone(), self.number(&target)? / divisor)
            }
            _ => return Err(MachineProblem::InvalidOperation),
        };
        self.write(&target, value.to_string().as_bytes())
    }
    fn eval_expression(&self, args: &[String]) -> Result<i128, MachineProblem> {
        let mut iter = args.iter();
        let mut value = self.number(iter.next().ok_or(MachineProblem::InvalidOperation)?)?;
        while let Some(op) = iter.next() {
            let rhs = self.number(iter.next().ok_or(MachineProblem::InvalidOperation)?)?;
            value = match op.as_str() {
                "+" => value.checked_add(rhs),
                "-" => value.checked_sub(rhs),
                "*" => value.checked_mul(rhs),
                "/" if rhs != 0 => value.checked_div(rhs),
                _ => None,
            }
            .ok_or(MachineProblem::SizeError)?;
        }
        Ok(value)
    }
    fn if_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.len() < 4 {
            return Err(MachineProblem::UnsupportedForm);
        }
        let condition = compare(self.number(&args[0])?, &args[1], self.number(&args[2])?);
        if condition && let Some(index) = args.iter().position(|arg| arg == "DISPLAY") {
            let value = self.resolve(
                args.get(index + 1)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            self.append_output(&[value.as_slice(), b"\n"].concat())?;
        }
        Ok(())
    }
    fn evaluate_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let subject = args.first().ok_or(MachineProblem::InvalidOperation)?;
        let value = self.resolve(subject)?;
        let mut index = 1;
        while index + 2 < args.len() {
            if args[index] == "WHEN" && self.resolve(&args[index + 1])? == value {
                if args[index + 2] == "DISPLAY" {
                    let shown = self.resolve(
                        args.get(index + 3)
                            .ok_or(MachineProblem::InvalidOperation)?,
                    )?;
                    self.append_output(&[shown.as_slice(), b"\n"].concat())?;
                }
                break;
            }
            index += 1;
        }
        Ok(())
    }
    fn search_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.is_empty() {
            Err(MachineProblem::InvalidOperation)
        } else {
            Ok(())
        }
    }
    fn string_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let mut value = Vec::new();
        for arg in &args[..into] {
            if !matches!(arg.as_str(), "DELIMITED" | "BY" | "SIZE") {
                value.extend(self.resolve(arg)?);
            }
        }
        self.write(
            args.get(into + 1).ok_or(MachineProblem::InvalidOperation)?,
            &value,
        )
    }
    fn unstring_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let source = self.resolve(args.first().ok_or(MachineProblem::InvalidOperation)?)?;
        let fields: Vec<_> = String::from_utf8_lossy(&source)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        for (target, value) in args[into + 1..].iter().zip(fields) {
            self.write(target, value.as_bytes())?;
        }
        Ok(())
    }
    fn inspect_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let source = self.read(&args[0])?;
        if let Some(replacing) = position(args, "REPLACING")
            && args.len() > replacing + 3
        {
            let from = self.resolve(&args[replacing + 2])?;
            let to = self.resolve(&args[replacing + 4])?;
            if from.len() == 1 && to.len() == 1 {
                let replaced: Vec<_> = source
                    .into_iter()
                    .map(|byte| if byte == from[0] { to[0] } else { byte })
                    .collect();
                self.write(&args[0], &replaced)?;
            }
        }
        Ok(())
    }
    fn generate(&mut self, args: &[String], json: bool) -> Result<(), MachineProblem> {
        if args.len() < 3 {
            return Err(MachineProblem::InvalidOperation);
        }
        let target = &args[0];
        let from = position(args, "FROM")
            .and_then(|i| args.get(i + 1))
            .ok_or(MachineProblem::InvalidOperation)?;
        let value = String::from_utf8_lossy(&self.resolve(from)?)
            .trim()
            .to_string();
        let generated = if json {
            format!("{{\"{from}\":\"{value}\"}}")
        } else {
            format!("<{from}>{value}</{from}>")
        };
        self.write(target, generated.as_bytes())
    }

    fn parse_generated(&mut self, args: &[String], json: bool) -> Result<(), MachineProblem> {
        if args.len() < 3 {
            return Err(MachineProblem::InvalidOperation);
        }
        let source = String::from_utf8(self.resolve(&args[0])?)
            .map_err(|_| MachineProblem::DataException)?;
        let into = position(args, "INTO").ok_or(MachineProblem::UnsupportedForm)?;
        let target = args.get(into + 1).ok_or(MachineProblem::InvalidOperation)?;
        let value = if json {
            let colon = source.find(':').ok_or(MachineProblem::DataException)?;
            source[colon + 1..]
                .trim()
                .trim_end_matches('}')
                .trim()
                .trim_matches('"')
                .to_string()
        } else {
            let start = source.find('>').ok_or(MachineProblem::DataException)? + 1;
            let end = source[start..]
                .find('<')
                .ok_or(MachineProblem::DataException)?
                + start;
            source[start..end].to_string()
        };
        self.write(target, value.as_bytes())
    }

    fn resolve(&self, token: &str) -> Result<Vec<u8>, MachineProblem> {
        let clean = token.trim_matches(['\'', '"']);
        if clean != token {
            return Ok(clean.as_bytes().to_vec());
        }
        if self.views.contains_key(&normalize(token)) {
            self.read(token)
        } else {
            Ok(token.as_bytes().to_vec())
        }
    }
    fn number(&self, token: &str) -> Result<i128, MachineProblem> {
        String::from_utf8_lossy(&self.resolve(token)?)
            .trim()
            .parse()
            .map_err(|_| MachineProblem::DataException)
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, MachineProblem> {
        let view = self
            .views
            .get(&normalize(name))
            .ok_or(MachineProblem::UnknownStorage)?;
        Ok(self.bases[view.base][view.offset..view.offset + view.length].to_vec())
    }
    fn write(&mut self, name: &str, value: &[u8]) -> Result<(), MachineProblem> {
        let view = self
            .views
            .get(&normalize(name))
            .ok_or(MachineProblem::UnknownStorage)?
            .clone();
        let current = &self.bases[view.base][view.offset..view.offset + view.length];
        let numeric = current.iter().all(u8::is_ascii_digit)
            && value
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'));
        if numeric {
            let mut fitted = vec![b'0'; view.length];
            let copy = value.len().min(view.length);
            fitted[view.length - copy..].copy_from_slice(&value[value.len() - copy..]);
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&fitted);
        } else {
            let fitted = FixedValue::fit(value, view.length, false);
            self.bases[view.base][view.offset..view.offset + view.length]
                .copy_from_slice(fitted.bytes());
        }
        Ok(())
    }
    fn write_storage(&mut self, id: StorageId, value: &[u8]) -> Result<(), MachineProblem> {
        let view = self
            .views_by_id
            .get(&id)
            .ok_or(MachineProblem::UnknownStorage)?
            .clone();
        let fitted = FixedValue::fit(value, view.length, false);
        self.bases[view.base][view.offset..view.offset + view.length]
            .copy_from_slice(fitted.bytes());
        Ok(())
    }
    fn append_output(&mut self, bytes: &[u8]) -> Result<(), MachineProblem> {
        let next = self
            .output
            .len()
            .checked_add(bytes.len())
            .ok_or(MachineProblem::ResourceExhausted)?;
        if next > self.invocation.limits.max_output_bytes as usize {
            return Err(MachineProblem::ResourceExhausted);
        }
        self.output.extend_from_slice(bytes);
        Ok(())
    }
    fn label(&self, name: &str) -> Result<usize, MachineProblem> {
        let normalized = normalize(name);
        let target = self.altered.get(&normalized).unwrap_or(&normalized);
        self.labels
            .get(target)
            .copied()
            .ok_or(MachineProblem::UnknownLabel)
    }
    fn complete(&self) -> Result<Completion, MachineProblem> {
        let limits = InvocationLimits {
            max_payload_bytes: self.invocation.limits.max_output_bytes as usize,
            ..InvocationLimits::default()
        };
        Ok(Completion {
            return_code: 0,
            output: BoundedPayload::new("mainframe-env.output@1", self.output.clone(), limits)
                .map_err(|_| MachineProblem::ResourceExhausted)?,
        })
    }
}

enum Step {
    Next,
    Jump(usize),
    Effect(EffectRequest),
    Complete,
}

impl Machine for ReferenceMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<Self::Effect> {
        let result = (|| -> Result<MachineDrive<Self::Effect>, MachineProblem> {
            match resume {
                MachineResume::Start if self.pending.is_none() => {}
                MachineResume::HostResult(result) => self.resume_host(result)?,
                MachineResume::Cancelled => {
                    return Ok(failure_drive(
                        FailureCategory::Cancelled,
                        "execution cancelled",
                    ));
                }
                MachineResume::TimedOut => {
                    return Ok(failure_drive(
                        FailureCategory::TimedOut,
                        "execution timed out",
                    ));
                }
                _ => return Err(MachineProblem::UnexpectedResume),
            }
            if let Some(drive) = self.deferred_drive.take() {
                return Ok(drive);
            }
            let mut steps = 0;
            while steps < quantum.max_steps {
                if self.pc >= self.operations.len() {
                    return Ok(MachineDrive::Completed(self.complete()?));
                }
                let operation = self.operations[self.pc].clone();
                match self.execute(&operation)? {
                    Step::Next => self.pc += 1,
                    Step::Jump(target) => self.pc = target,
                    Step::Effect(effect) => {
                        self.pc += 1;
                        return Ok(MachineDrive::HostCall(effect));
                    }
                    Step::Complete => return Ok(MachineDrive::Completed(self.complete()?)),
                }
                steps += 1;
            }
            Ok(MachineDrive::Continue)
        })();
        match result {
            Ok(drive) => drive,
            Err(problem) => MachineDrive::Failed(problem.execution_problem()),
        }
    }

    fn checkpoint(&self) -> Option<BoundedPayload> {
        if self.pending.is_some() {
            return None;
        }
        let bytes = encode_snapshot(&self.snapshot())?;
        BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@1",
            bytes,
            InvocationLimits {
                max_payload_bytes: usize::try_from(
                    self.invocation
                        .limits
                        .max_storage_bytes
                        .saturating_add(self.invocation.limits.max_output_bytes)
                        .saturating_add(1024 * 1024),
                )
                .ok()?,
                ..InvocationLimits::default()
            },
        )
        .ok()
    }

    fn effect_sequence(&self) -> u64 {
        self.effect_sequence
    }
}

fn encode_snapshot(snapshot: &MachineSnapshot) -> Option<Vec<u8>> {
    let mut bytes = b"MECP0001".to_vec();
    bytes.extend_from_slice(&snapshot.schema_version.to_be_bytes());
    bytes.extend_from_slice(&u64::try_from(snapshot.program_counter).ok()?.to_be_bytes());
    bytes.extend_from_slice(&snapshot.effect_sequence.to_be_bytes());
    push_bytes(&mut bytes, &snapshot.output)?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.base_storage.len())
            .ok()?
            .to_be_bytes(),
    );
    for storage in &snapshot.base_storage {
        push_bytes(&mut bytes, storage)?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.perform_stack.len())
            .ok()?
            .to_be_bytes(),
    );
    for target in &snapshot.perform_stack {
        bytes.extend_from_slice(&u64::try_from(*target).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.altered_targets.len())
            .ok()?
            .to_be_bytes(),
    );
    for (from, to) in &snapshot.altered_targets {
        push_bytes(&mut bytes, from.as_bytes())?;
        push_bytes(&mut bytes, to.as_bytes())?;
    }
    Some(bytes)
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Option<()> {
    output.extend_from_slice(&u64::try_from(value.len()).ok()?.to_be_bytes());
    output.extend_from_slice(value);
    Some(())
}

fn decode_snapshot(
    bytes: &[u8],
    max_storage: usize,
    max_output: usize,
    max_frames: usize,
) -> Result<MachineSnapshot, MachineProblem> {
    let mut input = SnapshotInput::new(bytes);
    if input.take(8)? != b"MECP0001" {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let schema_version = input.u32()?;
    let program_counter =
        usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    let effect_sequence = input.u64()?;
    let output = input.bytes(max_output)?;
    let base_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if base_count > max_frames.saturating_mul(1024) {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut remaining_storage = max_storage;
    let mut base_storage = Vec::with_capacity(base_count);
    for _ in 0..base_count {
        let storage = input.bytes(remaining_storage)?;
        remaining_storage = remaining_storage
            .checked_sub(storage.len())
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        base_storage.push(storage);
    }
    let stack_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if stack_count > max_frames {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut perform_stack = Vec::with_capacity(stack_count);
    for _ in 0..stack_count {
        perform_stack
            .push(usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?);
    }
    let altered_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if altered_count > max_frames.saturating_mul(4) {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut altered_targets = BTreeMap::new();
    for _ in 0..altered_count {
        let from = String::from_utf8(input.bytes(4096)?)
            .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        let to = String::from_utf8(input.bytes(4096)?)
            .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if altered_targets.insert(from, to).is_some() {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
    }
    if !input.finished() {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    Ok(MachineSnapshot {
        schema_version,
        program_counter,
        effect_sequence,
        output,
        base_storage,
        perform_stack,
        altered_targets,
    })
}

struct SnapshotInput<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SnapshotInput<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], MachineProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, MachineProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, MachineProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
        ))
    }

    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, MachineProblem> {
        let length =
            usize::try_from(self.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if length > max {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        Ok(self.take(length)?.to_vec())
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn cics_arguments(tokens: &[String]) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
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
        index += 1;
    }
    while index < tokens.len() {
        let key = tokens[index].to_ascii_uppercase();
        if key == "END-EXEC" {
            break;
        }
        if tokens.get(index + 1).is_some_and(|token| token == "(") {
            let end = tokens[index + 2..]
                .iter()
                .position(|token| token == ")")
                .map(|offset| index + 2 + offset)
                .ok_or(MachineProblem::InvalidOperation)?;
            let value = tokens[index + 2..end].join(" ");
            arguments.insert(
                key,
                BoundedPayload::new(
                    "mainframe-env.cics.argument@1",
                    value.into_bytes(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
            index = end + 1;
        } else {
            arguments.insert(
                format!("OPTION.{key}"),
                BoundedPayload::new(
                    "mainframe-env.cics.option@1",
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
            index += 1;
        }
    }
    Ok(arguments)
}

type StorageState = (
    Vec<Vec<u8>>,
    BTreeMap<String, StorageView>,
    BTreeMap<StorageId, StorageView>,
);

fn storage(module: &Module, max: u64) -> Result<StorageState, MachineProblem> {
    let total = module
        .storage()
        .iter()
        .filter(|item| item.alias_of.is_none())
        .try_fold(0u64, |sum, item| sum.checked_add(item.size))
        .ok_or(MachineProblem::ResourceExhausted)?;
    if total > max {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut bases: Vec<Vec<u8>> = Vec::new();
    let mut by_id: BTreeMap<StorageId, StorageView> = BTreeMap::new();
    let mut names: BTreeMap<String, StorageView> = BTreeMap::new();
    for item in module.storage() {
        let view = if let Some(alias) = &item.alias_of {
            let target = by_id
                .get(&alias.storage)
                .ok_or(MachineProblem::InvalidArtifact("forward alias".into()))?;
            StorageView {
                base: target.base,
                offset: target.offset + alias.offset as usize,
                length: alias.length as usize,
            }
        } else {
            let base = bases.len();
            bases.push(vec![0; item.size as usize]);
            StorageView {
                base,
                offset: 0,
                length: item.size as usize,
            }
        };
        by_id.insert(item.id, view.clone());
        names.insert(item.name.to_ascii_uppercase(), view);
    }
    Ok((bases, names, by_id))
}
fn validate_module(module: &Module) -> Result<(), MachineProblem> {
    let supported = supported_operations();
    let operations: Vec<_> = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .collect();
    if operations.is_empty()
        || operations
            .last()
            .is_none_or(|op| op.identity.name() != "halt")
        || operations
            .iter()
            .any(|op| !supported.contains(&op.identity))
    {
        return Err(MachineProblem::InvalidArtifact(
            "illegal operation or terminator".into(),
        ));
    }
    Ok(())
}
pub fn supported_operations() -> &'static BTreeSet<OperationIdentity> {
    static SET: OnceLock<BTreeSet<OperationIdentity>> = OnceLock::new();
    SET.get_or_init(|| {
        let names = [
            "init",
            "accept",
            "add",
            "allocate",
            "alter",
            "call",
            "cancel",
            "close",
            "compute",
            "continue",
            "display",
            "divide",
            "entry",
            "evaluate",
            "exec_cics",
            "exit",
            "free",
            "go_back",
            "go_to",
            "if",
            "initialize",
            "inspect",
            "json_generate",
            "json_parse",
            "move",
            "multiply",
            "open",
            "perform",
            "read",
            "search",
            "set",
            "stop_run",
            "string",
            "subtract",
            "unstring",
            "write",
            "xml_generate",
            "xml_parse",
            "label",
            "halt",
        ];
        names
            .into_iter()
            .map(|name| OperationIdentity::new(NAMESPACE, name, 1).expect("static operation"))
            .collect()
    })
}
fn arguments(operation: &Operation) -> Vec<String> {
    operation
        .attributes
        .iter()
        .filter_map(|(name, value)| name.starts_with("arg_").then_some(value))
        .filter_map(|value| match value {
            Attribute::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}
fn argument(operation: &Operation, index: usize) -> Option<&str> {
    operation
        .attributes
        .get(&format!("arg_{index:03}"))
        .and_then(|value| match value {
            Attribute::Text(text) => Some(text.as_str()),
            _ => None,
        })
}
fn bytes_attribute<'a>(operation: &'a Operation, name: &str) -> Result<&'a [u8], MachineProblem> {
    operation
        .attributes
        .get(name)
        .and_then(|value| match value {
            Attribute::Bytes(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .ok_or(MachineProblem::InvalidOperation)
}
fn position(args: &[String], needle: &str) -> Option<usize> {
    args.iter().position(|arg| arg.eq_ignore_ascii_case(needle))
}
fn normalize(value: &str) -> String {
    value
        .trim_matches(['\'', '"', '.', ','])
        .to_ascii_uppercase()
}
fn compare(left: i128, operator: &str, right: i128) -> bool {
    match operator {
        "=" => left == right,
        ">" => left > right,
        "<" => left < right,
        ">=" => left >= right,
        "<=" => left <= right,
        "<>" => left != right,
        _ => false,
    }
}
fn empty_payload() -> Result<BoundedPayload, MachineProblem> {
    BoundedPayload::new(
        "mainframe-env.program-call@1",
        Vec::new(),
        InvocationLimits::default(),
    )
    .map_err(|_| MachineProblem::ResourceExhausted)
}
fn failure_drive(category: FailureCategory, message: &str) -> MachineDrive<EffectRequest> {
    MachineDrive::Failed(
        ExecutionProblem::new(
            DiagnosticCode::new("MEEXEC0001").expect("static code"),
            category,
            Phase::Execute,
            message,
            false,
            false,
            DiagnosticLimits::default(),
        )
        .expect("static problem"),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineProblem {
    InvalidArtifact(String),
    InvalidOperation,
    UnsupportedForm,
    UnknownStorage,
    UnknownLabel,
    DataException,
    SizeError,
    ResourceExhausted,
    UnexpectedResume,
    UnexpectedHostResult,
    IncompatibleSnapshot,
    Host(HostProblem),
}
impl MachineProblem {
    fn execution_problem(&self) -> ExecutionProblem {
        let (category, message) = match self {
            Self::ResourceExhausted => (
                FailureCategory::ResourceExhausted,
                "machine resource exhausted",
            ),
            Self::Host(HostProblem::Unauthorized) => {
                (FailureCategory::Unauthorized, "host authorization denied")
            }
            Self::Host(HostProblem::Cancelled) => (FailureCategory::Cancelled, "host cancelled"),
            Self::Host(HostProblem::TimedOut) => (FailureCategory::TimedOut, "host timed out"),
            Self::Host(HostProblem::UnknownOutcome) => {
                (FailureCategory::UnknownOutcome, "host outcome unknown")
            }
            Self::Host(_) => (FailureCategory::ProviderFailure, "host provider failed"),
            Self::UnsupportedForm => (FailureCategory::Unsupported, "unsupported COBOL form"),
            _ => (
                FailureCategory::MalformedInput,
                "invalid executable artifact or data",
            ),
        };
        ExecutionProblem::new(
            DiagnosticCode::new("MEEXEC0002").expect("static code"),
            category,
            Phase::Execute,
            message,
            false,
            category == FailureCategory::UnknownOutcome,
            DiagnosticLimits::default(),
        )
        .expect("static problem")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_ir::{Effect, IrLimits, ModuleBuilder};
    fn invocation() -> Invocation {
        let l = InvocationLimits::default();
        Invocation::new(
            RequestId::new("req", l).unwrap(),
            ExecutionId::new("exec", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("program:HELLO", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", l).unwrap(),
                BTreeSet::<CapabilityId>::new(),
                l,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("idem", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap()
    }
    fn binary() -> Vec<u8> {
        let mut b = ModuleBuilder::new(IrLimits::default());
        let s = b.add_storage("msg", 5, None).unwrap();
        let r = b.add_region().unwrap();
        let block = b.add_block(r).unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "init", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([("initial".into(), Attribute::Bytes(b"HELLO".to_vec()))]),
            vec![Effect::MemoryWrite],
            vec![mainframe_env_ir::StorageReference {
                storage: s,
                offset: 0,
                length: 5,
            }],
            None,
        )
        .unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "display", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([("arg_000".into(), Attribute::Text("MSG".into()))]),
            vec![Effect::MemoryRead, Effect::TerminalWrite],
            Vec::new(),
            None,
        )
        .unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "halt", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
        mainframe_env_ir::encode_binary(&b.finish().unwrap(), CodecLimits::default()).unwrap()
    }
    #[test]
    fn hello_executes_in_bounded_quanta() {
        let mut m =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        let drive = m.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        match drive {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"HELLO\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn output_limit_fails_typed() {
        let mut i = invocation();
        i.limits.max_output_bytes = 1;
        let mut m = ReferenceMachine::from_binary(&binary(), i, CodecLimits::default()).unwrap();
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
            MachineDrive::Failed(_)
        ));
    }
    #[test]
    fn checkpoint_roundtrip_restores_exact_machine_state_and_rejects_corruption() {
        let invocation = invocation();
        let mut first =
            ReferenceMachine::from_binary(&binary(), invocation.clone(), CodecLimits::default())
                .unwrap();
        assert_eq!(
            first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Continue
        );
        let checkpoint = first.checkpoint().unwrap();
        let mut restored =
            ReferenceMachine::from_binary(&binary(), invocation, CodecLimits::default()).unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        let first_done = first.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        let restored_done = restored.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        assert_eq!(first_done, restored_done);

        let mut damaged = checkpoint.bytes().to_vec();
        damaged.push(0);
        let damaged =
            BoundedPayload::new(checkpoint.schema(), damaged, InvocationLimits::default()).unwrap();
        assert_eq!(
            restored.restore_checkpoint(&damaged),
            Err(MachineProblem::IncompatibleSnapshot)
        );
    }
    #[test]
    fn cics_tokens_lower_to_named_typed_arguments() {
        let tokens = vec![
            "EXEC", "CICS", "SEND", "MAP", "MAPSET", "(", "MENUMS", ")", "MAP", "(", "MENU", ")",
            "ERASE", "END-EXEC",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            CicsOperation::from_tokens(&tokens),
            Some(CicsOperation::SendMap)
        );
        let arguments = cics_arguments(&tokens).unwrap();
        assert_eq!(arguments["MAPSET"].bytes(), b"MENUMS");
        assert_eq!(arguments["MAP"].bytes(), b"MENU");
        assert!(arguments.contains_key("OPTION.ERASE"));
        assert!(arguments.keys().all(|name| !name.starts_with("arg_")));
    }
}
