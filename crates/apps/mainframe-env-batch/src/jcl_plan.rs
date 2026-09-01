use crate::jcl_graph::{JclDependencyGraph, JclGraphProblem};
use crate::jcl_statement::parse_effective_parameters;
use crate::{
    ExecParameterId, JCL_GENERATED_CATALOG_SHA256, JCL_PLAN_CONTRACT, JCL_PLAN_SCHEMA_SHA256,
    JclBundle, JclCapabilityRequirement, JclCapabilityState, JclCatalogSupport,
    JclDiagnosticProjection, JclExpandedStatement, JclExpansionLimits, JclParameterNode,
    JclPlanDocument, JclPlanNode, JclProcedureDefinition, JclRelatedDiagnostic, JclSourceOrigin,
    JclSourceOriginKind, JclSourceSpan, JclStatementId, JclStatementNode, JclSymbolDefinition,
    JclSyntaxLimits, JclValueShape, expand_jcl,
};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity, SourceSpan,
};
use mainframe_env_source::{FileId, SourceBundle, SourceRange};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JclConversionLimits {
    pub syntax: JclSyntaxLimits,
    pub expansion: JclExpansionLimits,
    pub max_plan_nodes: usize,
    pub max_plan_bytes: usize,
    pub max_parameters_per_statement: usize,
    pub max_capabilities: usize,
    pub max_condition_depth: usize,
}

impl Default for JclConversionLimits {
    fn default() -> Self {
        Self {
            syntax: JclSyntaxLimits::default(),
            expansion: JclExpansionLimits::default(),
            max_plan_nodes: 4_194_304,
            max_plan_bytes: 64 * 1024 * 1024,
            max_parameters_per_statement: 256,
            max_capabilities: 4_194_304,
            max_condition_depth: 256,
        }
    }
}

#[derive(Clone, Debug)]
pub struct JclConversion {
    plan: Option<JclPlanDocument>,
    legacy_plan: Option<crate::JobPlan>,
    diagnostics: Vec<Diagnostic>,
    diagnostic_projection: Vec<JclDiagnosticProjection>,
}

impl JclConversion {
    #[must_use]
    pub fn plan(&self) -> Option<&JclPlanDocument> {
        self.plan.as_ref()
    }

    #[must_use]
    pub fn legacy_plan(&self) -> Option<&crate::JobPlan> {
        self.legacy_plan.as_ref()
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn diagnostic_projection(&self) -> &[JclDiagnosticProjection] {
        &self.diagnostic_projection
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.plan.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JclConversionProblem {
    Expansion(String),
    ResourceLimit(&'static str),
    Serialization,
    PlanGraphInvariant,
    Compatibility,
}

impl fmt::Display for JclConversionProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "JCL conversion failed: {self:?}")
    }
}

impl std::error::Error for JclConversionProblem {}

/// Converts JCL to an immutable typed plan. Deferred runtime facilities become
/// explicit capability requirements and warnings; malformed input prevents a
/// plan from being constructed.
pub fn convert_jcl(
    bundle: &JclBundle,
    limits: JclConversionLimits,
) -> Result<JclConversion, JclConversionProblem> {
    let expansion = expand_jcl(bundle, limits.syntax, limits.expansion)
        .map_err(|problem| JclConversionProblem::Expansion(problem.to_string()))?;
    let source = expansion.source();
    let mut diagnostics = expansion.diagnostics().to_vec();
    let mut statement_nodes = Vec::new();
    let mut plan_nodes = Vec::new();
    let mut capabilities = Vec::new();
    let mut statement_ids = BTreeMap::<(FileId, usize, usize), u32>::new();
    let mut job_count = 0usize;
    let mut step_count = 0usize;
    let mut condition_stack = Vec::<bool>::new();

    for expanded in expansion.statements() {
        if statement_nodes.len() >= limits.max_plan_nodes {
            return Err(JclConversionProblem::ResourceLimit("plan statements"));
        }
        let statement_id = u32::try_from(statement_nodes.len() + 1)
            .map_err(|_| JclConversionProblem::ResourceLimit("plan statement identity"))?;
        let source_range = expanded
            .statement()
            .sources()
            .first()
            .ok_or(JclConversionProblem::PlanGraphInvariant)?;
        statement_ids.insert(
            (
                source_range.file,
                source_range.bytes.start,
                source_range.bytes.end,
            ),
            statement_id,
        );
        let span = projected_statement_span(source, expanded)?;
        if let Err(message) = validate_statement_operands(expanded) {
            diagnostics.push(plan_diagnostic(
                "MEJCL0760",
                &message,
                Severity::Error,
                FailureCategory::MalformedInput,
                Completeness::Incomplete,
                source_range,
            ));
        }
        let parsed_parameters = match parse_effective_parameters(
            expanded.statement().identity(),
            expanded.effective_operands(),
        ) {
            Ok(parameters) => parameters,
            Err(message) => {
                diagnostics.push(plan_diagnostic(
                    "MEJCL0743",
                    &message,
                    Severity::Error,
                    FailureCategory::MalformedInput,
                    Completeness::Incomplete,
                    source_range,
                ));
                Vec::new()
            }
        };
        if parsed_parameters.len() > limits.max_parameters_per_statement {
            return Err(JclConversionProblem::ResourceLimit(
                "parameters per statement",
            ));
        }
        let mut projected_operands = expanded.effective_operands().to_string();
        for parameter in &parsed_parameters {
            if parameter.identity().sensitive() {
                projected_operands =
                    redact_parameter(&projected_operands, parameter.source_keyword());
            }
        }
        let mut parameter_nodes = Vec::with_capacity(parsed_parameters.len());
        for parameter in parsed_parameters {
            let identity = parameter.identity();
            let dynamic_procedure_parameter =
                matches!(
                    identity,
                    crate::JclParameterIdentity::Exec(ExecParameterId::ProcAndProcedureName)
                ) && !parameter.source_keyword().eq_ignore_ascii_case("PROC");
            let normalized_result =
                if identity.validation() == JclValueShape::None && !parameter.positional() {
                    Err("parameter is positional and does not accept '=' syntax".into())
                } else if dynamic_procedure_parameter {
                    balanced_jcl_value(parameter.raw_value())
                        .then(|| parameter.raw_value().trim().to_string())
                        .ok_or_else(|| "procedure parameter value has unbalanced JCL syntax".into())
                } else {
                    normalize_parameter(identity, parameter.raw_value())
                };
            match normalized_result {
                Ok(normalized) => {
                    let raw = if identity.sensitive() {
                        "[redacted]".to_string()
                    } else {
                        parameter.raw_value().to_string()
                    };
                    let normalized = if identity.sensitive() {
                        "[redacted]".to_string()
                    } else {
                        normalized
                    };
                    parameter_nodes.push(JclParameterNode::new(
                        identity.generated(),
                        raw,
                        normalized,
                        identity.outcome(),
                        span.clone(),
                    ));
                    let support = if dynamic_procedure_parameter {
                        JclCatalogSupport::Available
                    } else {
                        identity.support()
                    };
                    let capability = if dynamic_procedure_parameter {
                        "jcl.procedure.parameter"
                    } else {
                        identity.capability()
                    };
                    let state = match support {
                        JclCatalogSupport::Available => JclCapabilityState::Available,
                        JclCatalogSupport::Deferred => JclCapabilityState::Deferred,
                    };
                    capabilities.push(JclCapabilityRequirement::new(
                        capability.into(),
                        state,
                        format!(
                            "{} parameter requires {}",
                            identity.generated().keyword(),
                            capability
                        ),
                        statement_id,
                        Some(identity.generated()),
                        span.clone(),
                    ));
                    if state == JclCapabilityState::Deferred {
                        diagnostics.push(plan_diagnostic(
                            "MEJCL0744",
                            &format!(
                                "accepted parameter requires deferred capability {}",
                                capability
                            ),
                            Severity::Warning,
                            FailureCategory::Unsupported,
                            Completeness::Unsupported,
                            source_range,
                        ));
                    }
                }
                Err(message) => diagnostics.push(plan_diagnostic(
                    "MEJCL0745",
                    &message,
                    Severity::Error,
                    FailureCategory::MalformedInput,
                    Completeness::Incomplete,
                    source_range,
                )),
            }
        }
        let statement_node = JclStatementNode::new(
            statement_id,
            expanded.statement().generated_identity(),
            expanded.effective_name().map(str::to_string),
            projected_operands,
            parameter_nodes,
            span.clone(),
        )
        .with_inline_data(expanded.statement().inline_data().to_vec());
        statement_nodes.push(statement_node);

        let operation = expanded.statement().source_operation();
        let node = if expanded.definition_only() {
            JclPlanNode::Annotation {
                operation: format!("definition-{operation}"),
                statement_id,
            }
        } else {
            match expanded.statement().identity() {
                JclStatementId::Job => {
                    job_count += 1;
                    JclPlanNode::Job {
                        name: expanded
                            .effective_name()
                            .unwrap_or_default()
                            .to_ascii_uppercase(),
                        statement_id,
                    }
                }
                JclStatementId::Exec => {
                    if let Some(program) = assignment(expanded.effective_operands(), "PGM") {
                        step_count += 1;
                        JclPlanNode::Step {
                            name: expanded
                                .effective_name()
                                .unwrap_or_default()
                                .to_ascii_uppercase(),
                            program: strip_quotes(&program).to_ascii_uppercase(),
                            statement_id,
                        }
                    } else {
                        JclPlanNode::Annotation {
                            operation: "procedure-invocation".into(),
                            statement_id,
                        }
                    }
                }
                JclStatementId::Dd => {
                    let effective_name = expanded.effective_name().unwrap_or_default();
                    let (step, name) = effective_name
                        .rsplit_once('.')
                        .map_or((None, effective_name), |(step, name)| {
                            (Some(step.to_string()), name)
                        });
                    JclPlanNode::Dd {
                        name: name.to_ascii_uppercase(),
                        step,
                        statement_id,
                    }
                }
                JclStatementId::Output => JclPlanNode::Output {
                    name: expanded
                        .effective_name()
                        .map(str::to_ascii_uppercase)
                        .unwrap_or_else(|| format!("OUTPUT{statement_id}")),
                    statement_id,
                },
                JclStatementId::Conditional => {
                    validate_condition_statement(
                        operation,
                        expanded.effective_operands(),
                        &mut condition_stack,
                        limits,
                        source_range,
                        &mut diagnostics,
                    );
                    JclPlanNode::Annotation {
                        operation: operation.to_ascii_uppercase(),
                        statement_id,
                    }
                }
                identity => {
                    if let Some(capability) = statement_capability(identity) {
                        capabilities.push(JclCapabilityRequirement::new(
                            capability.into(),
                            JclCapabilityState::Deferred,
                            format!(
                                "{} statement requires {capability}",
                                identity.descriptor().label
                            ),
                            statement_id,
                            None,
                            span.clone(),
                        ));
                        diagnostics.push(plan_diagnostic(
                            "MEJCL0746",
                            &format!(
                                "accepted statement requires deferred capability {capability}"
                            ),
                            Severity::Warning,
                            FailureCategory::Unsupported,
                            Completeness::Unsupported,
                            source_range,
                        ));
                    }
                    JclPlanNode::Annotation {
                        operation: operation.to_ascii_uppercase(),
                        statement_id,
                    }
                }
            }
        };
        plan_nodes.push(node);
    }

    if job_count != 1 {
        diagnostics.push(plan_global_diagnostic(
            "MEJCL0747",
            "a valid converted job must contain exactly one JOB statement",
            expansion.statements().first(),
        ));
    }
    if step_count == 0 {
        diagnostics.push(plan_global_diagnostic(
            "MEJCL0748",
            "a valid executable job plan must contain at least one expanded program step",
            expansion.statements().first(),
        ));
    }
    if !condition_stack.is_empty() {
        diagnostics.push(plan_global_diagnostic(
            "MEJCL0749",
            "IF statement is missing a matching ENDIF",
            expansion.statements().last(),
        ));
    }
    validate_statement_context(&statement_nodes, &plan_nodes, &mut diagnostics);
    if capabilities.len() > limits.max_capabilities {
        return Err(JclConversionProblem::ResourceLimit("plan capabilities"));
    }
    validate_plan_graph(
        expansion.statements(),
        &statement_nodes,
        &plan_nodes,
        limits,
    )?;

    let procedures = expansion
        .procedures()
        .iter()
        .map(|procedure| {
            let definition = projected_source_span(
                source,
                procedure.definition(),
                JclSourceOriginKind::Procedure,
                &[],
            )?;
            let invocation_sites = procedure
                .invocation_sites()
                .iter()
                .map(|site| {
                    projected_source_span(source, site, JclSourceOriginKind::Generated, &[])
                })
                .collect::<Result<Vec<_>, _>>()?;
            let statement_ids = statement_nodes
                .iter()
                .filter(|statement| {
                    statement.source().file_id() == definition.file_id()
                        && statement.source().byte_start() >= definition.byte_start()
                })
                .map(JclStatementNode::id)
                .collect();
            Ok(JclProcedureDefinition::new(
                procedure.name().into(),
                procedure.defaults().clone(),
                statement_ids,
                definition,
                invocation_sites,
            ))
        })
        .collect::<Result<Vec<_>, JclConversionProblem>>()?;
    let symbols = expansion
        .symbols()
        .iter()
        .map(|symbol| {
            let definition = match symbol.definition() {
                Some(definition) => {
                    projected_source_span(source, definition, JclSourceOriginKind::Definition, &[])?
                }
                None => system_symbol_span(source)?,
            };
            let uses = symbol
                .uses()
                .iter()
                .map(|site| {
                    projected_source_span(source, site, JclSourceOriginKind::Generated, &[])
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(JclSymbolDefinition::new(
                symbol.name().into(),
                symbol.value().into(),
                symbol.exported(),
                definition,
                uses,
            ))
        })
        .collect::<Result<Vec<_>, JclConversionProblem>>()?;

    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error);
    let source_identity = format!("sha256:{}", source.id().to_hex());
    let plan = if has_errors {
        None
    } else {
        let plan_identity = plan_identity(
            &source_identity,
            &statement_nodes,
            &symbols,
            &procedures,
            &plan_nodes,
            &capabilities,
        )?;
        let plan = JclPlanDocument::new(
            JCL_GENERATED_CATALOG_SHA256.into(),
            JCL_PLAN_SCHEMA_SHA256.into(),
            source_identity,
            plan_identity,
            statement_nodes,
            symbols,
            procedures,
            plan_nodes,
            capabilities,
        );
        let bytes = serde_json::to_vec(&plan).map_err(|_| JclConversionProblem::Serialization)?;
        if bytes.len() > limits.max_plan_bytes {
            return Err(JclConversionProblem::ResourceLimit("serialized plan bytes"));
        }
        Some(plan)
    };
    let legacy_plan = plan
        .as_ref()
        .map(|plan| crate::jcl::legacy_job_plan(plan, bundle.primary.clone(), limits))
        .transpose()
        .map_err(|_| JclConversionProblem::Compatibility)?;
    let diagnostic_projection = diagnostics
        .iter()
        .map(|diagnostic| project_diagnostic(source, diagnostic))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JclConversion {
        plan,
        legacy_plan,
        diagnostics,
        diagnostic_projection,
    })
}

fn validate_statement_operands(statement: &JclExpandedStatement) -> Result<(), String> {
    let descriptor = statement.statement().identity().descriptor();
    let value = if statement.statement().identity() == JclStatementId::JclCommand {
        statement.effective_name().unwrap_or_default()
    } else {
        statement.effective_operands()
    };
    match descriptor.validation {
        JclValueShape::None => Ok(()),
        JclValueShape::JclValue if balanced_jcl_value(value) => Ok(()),
        JclValueShape::Text if !value.trim().is_empty() => Ok(()),
        JclValueShape::Name if valid_name(value) => Ok(()),
        shape => Err(format!(
            "{} statement violates generated {} operand validation",
            descriptor.label,
            validation_name(shape)
        )),
    }
}

fn normalize_parameter(identity: crate::JclParameterIdentity, raw: &str) -> Result<String, String> {
    let value = strip_quotes(raw.trim());
    if value.len() > 65_536 || value.chars().any(char::is_control) {
        return Err(format!(
            "{} value is empty, contains controls, or exceeds its bound",
            identity.generated().keyword()
        ));
    }
    let upper = value.to_ascii_uppercase();
    let invalid = |detail: &str| {
        Err(format!(
            "{} value {raw:?} violates generated {} validation: {detail}",
            identity.generated().keyword(),
            validation_name(identity.validation())
        ))
    };
    match identity.validation() {
        JclValueShape::None => {
            if raw.contains('=') {
                invalid("the parameter does not accept a value")
            } else {
                Ok(upper)
            }
        }
        JclValueShape::JclValue => balanced_jcl_value(raw)
            .then_some(raw.trim().to_string())
            .ok_or_else(|| {
                format!(
                    "{} value has unbalanced quotes or parentheses",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Text => (!value.is_empty())
            .then_some(value.to_string())
            .ok_or_else(|| format!("{} text value is empty", identity.generated().keyword())),
        JclValueShape::Name => valid_name(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a bounded JCL name",
                identity.generated().keyword()
            )
        }),
        JclValueShape::NameList => validate_list(value, valid_name)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a bounded JCL name list",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Integer => normalize_integer(identity, value),
        JclValueShape::IntegerOrTuple => {
            let values = list_items(value);
            if values.is_empty()
                || values
                    .iter()
                    .any(|value| normalize_integer(identity, value).is_err())
            {
                invalid("expected one integer or a tuple of bounded integers")
            } else {
                Ok(upper)
            }
        }
        JclValueShape::IntegerOrX => {
            if upper == "X" {
                Ok(upper)
            } else {
                normalize_integer(identity, value)
            }
        }
        JclValueShape::IntegerOrSuffix => {
            let digits = value.trim_end_matches(|character: char| character.is_ascii_alphabetic());
            normalize_integer(identity, digits).map(|_| upper)
        }
        JclValueShape::Boolean => matches!(upper.as_str(), "YES" | "NO")
            .then_some(upper)
            .ok_or_else(|| format!("{} requires YES or NO", identity.generated().keyword())),
        JclValueShape::Enum => identity
            .choices()
            .iter()
            .any(|choice| choice.eq_ignore_ascii_case(value))
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires one of {}",
                    identity.generated().keyword(),
                    identity.choices().join(",")
                )
            }),
        JclValueShape::Size => valid_size(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a nonnegative K/M/G/T size",
                identity.generated().keyword()
            )
        }),
        JclValueShape::SizeOrTuple => validate_list(value, valid_size)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a size or size tuple",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Time => valid_time(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires minutes or a (minutes,seconds) tuple",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Class => (value.len() == 1
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'*'))
        .then_some(upper)
        .ok_or_else(|| {
            format!(
                "{} requires one class character",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Condition => valid_condition(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} condition syntax is malformed",
                identity.generated().keyword()
            )
        }),
        JclValueShape::MessageLevel => {
            valid_message_level(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} requires MSGLEVEL=(0-2,0-1)",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Dataset => valid_dataset(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a valid data set or backward-reference name",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Disposition => valid_disposition(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} disposition tuple is invalid",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Delimiter => (value.len() <= 2 && !value.is_empty())
            .then_some(value.to_string())
            .ok_or_else(|| {
                format!(
                    "{} delimiter must contain one or two characters",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::RecordFormat => {
            valid_record_format(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} record format is invalid",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Path => (!value.is_empty() && value.starts_with('/'))
            .then_some(value.to_string())
            .ok_or_else(|| {
                format!(
                    "{} requires an absolute z/OS UNIX path",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Program | JclValueShape::Procedure => {
            valid_program(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} requires a 1-8 character program or procedure name",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Restart => valid_restart(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} restart target is invalid",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Sysout => valid_sysout(value)
            .then_some(upper)
            .ok_or_else(|| format!("{} SYSOUT tuple is invalid", identity.generated().keyword())),
        JclValueShape::OutputReference => {
            validate_list(value, |item| valid_name(item.trim_start_matches("*.")))
                .then_some(upper)
                .ok_or_else(|| {
                    format!(
                        "{} OUTPUT reference is invalid",
                        identity.generated().keyword()
                    )
                })
        }
        JclValueShape::BackwardReference => (value.starts_with("*.") && value.len() > 2)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a backward DD reference",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Secret => (1..=8)
            .contains(&value.len())
            .then_some("[redacted]".into())
            .ok_or_else(|| {
                format!(
                    "{} secret length is invalid",
                    identity.generated().keyword()
                )
            }),
    }
}

fn normalize_integer(identity: crate::JclParameterIdentity, value: &str) -> Result<String, String> {
    let parsed = value.parse::<u64>().map_err(|_| {
        format!(
            "{} requires an unsigned integer",
            identity.generated().keyword()
        )
    })?;
    if identity.minimum().is_some_and(|minimum| parsed < minimum)
        || identity.maximum().is_some_and(|maximum| parsed > maximum)
    {
        return Err(format!(
            "{} integer is outside generated range {:?}..={:?}",
            identity.generated().keyword(),
            identity.minimum(),
            identity.maximum()
        ));
    }
    Ok(parsed.to_string())
}

const fn validation_name(shape: JclValueShape) -> &'static str {
    match shape {
        JclValueShape::None => "none",
        JclValueShape::JclValue => "jcl-value",
        JclValueShape::Text => "text",
        JclValueShape::Name => "name",
        JclValueShape::NameList => "name-list",
        JclValueShape::Integer => "integer",
        JclValueShape::IntegerOrTuple => "integer-or-tuple",
        JclValueShape::IntegerOrX => "integer-or-x",
        JclValueShape::IntegerOrSuffix => "integer-or-suffix",
        JclValueShape::Boolean => "boolean",
        JclValueShape::Enum => "enum",
        JclValueShape::Size => "size",
        JclValueShape::SizeOrTuple => "size-or-tuple",
        JclValueShape::Time => "time",
        JclValueShape::Class => "class",
        JclValueShape::Condition => "condition",
        JclValueShape::MessageLevel => "message-level",
        JclValueShape::Dataset => "dataset",
        JclValueShape::Disposition => "disposition",
        JclValueShape::Delimiter => "delimiter",
        JclValueShape::RecordFormat => "record-format",
        JclValueShape::Path => "path",
        JclValueShape::Program => "program",
        JclValueShape::Procedure => "procedure",
        JclValueShape::Restart => "restart",
        JclValueShape::Sysout => "sysout",
        JclValueShape::OutputReference => "output-reference",
        JclValueShape::BackwardReference => "backward-reference",
        JclValueShape::Secret => "secret",
    }
}

fn balanced_jcl_value(value: &str) -> bool {
    let mut quote = None;
    let mut depth = 0usize;
    for byte in value.bytes() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() && depth > 0 => depth -= 1,
            b')' if quote.is_none() => return false,
            _ => {}
        }
    }
    quote.is_none() && depth == 0 && !value.trim().is_empty()
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'$' | b'#' | b'@' | b'_' | b'-' | b'.' | b'*' | b'&')
        })
}

fn validate_list(value: &str, validator: impl Fn(&str) -> bool) -> bool {
    let values = list_items(value);
    !values.is_empty() && values.iter().all(|value| validator(value))
}

fn list_items(value: &str) -> Vec<&str> {
    value
        .trim()
        .trim_matches(['(', ')'])
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect()
}

fn valid_size(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    if matches!(upper.as_str(), "NOLIMIT" | "MAXIMUM") {
        return true;
    }
    let digits = upper.trim_end_matches(['K', 'M', 'G', 'T']);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_time(value: &str) -> bool {
    let values = list_items(value);
    match values.as_slice() {
        [minutes] => minutes
            .parse::<u64>()
            .is_ok_and(|minutes| minutes <= 357912),
        [minutes, seconds] => {
            minutes
                .parse::<u64>()
                .is_ok_and(|minutes| minutes <= 357912)
                && seconds.parse::<u64>().is_ok_and(|seconds| seconds <= 59)
        }
        _ => false,
    }
}

fn valid_condition(value: &str) -> bool {
    let upper = value.trim_matches(['(', ')']).to_ascii_uppercase();
    if matches!(upper.as_str(), "EVEN" | "ONLY") {
        return true;
    }
    let values = upper.split(',').map(str::trim).collect::<Vec<_>>();
    matches!(values.len(), 2 | 3)
        && values[0]
            .parse::<u16>()
            .is_ok_and(|return_code| return_code <= 4095)
        && matches!(
            values[1],
            "EQ" | "NE" | "GT" | "GE" | "LT" | "LE" | "=" | "¬=" | ">" | ">=" | "<" | "<="
        )
        && values.get(2).is_none_or(|step| valid_name(step))
}

fn valid_message_level(value: &str) -> bool {
    let values = list_items(value);
    matches!(values.as_slice(), [statements, messages]
        if matches!(*statements, "0" | "1" | "2") && matches!(*messages, "0" | "1"))
}

fn valid_dataset(value: &str) -> bool {
    if value.starts_with("*.") {
        return value.len() > 2 && !value.bytes().any(|byte| byte.is_ascii_whitespace());
    }
    let base = value.split('(').next().unwrap_or(value);
    !base.is_empty()
        && base.len() <= 44
        && !base.starts_with('.')
        && !base.ends_with('.')
        && !base.bytes().any(|byte| byte.is_ascii_whitespace())
        && base.split('.').all(|qualifier| {
            !qualifier.is_empty()
                && qualifier.len() <= 8
                && qualifier.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'#' | b'@' | b'&')
                })
        })
}

fn valid_disposition(value: &str) -> bool {
    let values = list_items(value)
        .into_iter()
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>();
    if values.is_empty() || values.len() > 3 {
        return false;
    }
    matches!(values[0].as_str(), "OLD" | "SHR" | "NEW" | "MOD")
        && values.get(1).is_none_or(|value| {
            matches!(
                value.as_str(),
                "DELETE" | "KEEP" | "PASS" | "CATLG" | "UNCATLG"
            )
        })
        && values
            .get(2)
            .is_none_or(|value| matches!(value.as_str(), "DELETE" | "KEEP" | "CATLG" | "UNCATLG"))
}

fn valid_record_format(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "F" | "FA"
            | "FB"
            | "FBA"
            | "FBM"
            | "FBS"
            | "FM"
            | "U"
            | "V"
            | "VA"
            | "VB"
            | "VBA"
            | "VBM"
            | "VBS"
            | "VM"
    )
}

fn valid_program(value: &str) -> bool {
    (1..=8).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'#' | b'@'))
}

fn valid_restart(value: &str) -> bool {
    let target = value
        .trim_matches(['(', ')'])
        .split(',')
        .next()
        .unwrap_or(value);
    target == "*" || target.split('.').all(valid_program)
}

fn valid_sysout(value: &str) -> bool {
    value == "*" || {
        let values = list_items(value);
        !values.is_empty() && values.len() <= 3 && values[0].len() <= 1
    }
}

fn validate_condition_statement(
    operation: &str,
    operands: &str,
    stack: &mut Vec<bool>,
    limits: JclConversionLimits,
    source: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match operation.to_ascii_uppercase().as_str() {
        "IF" => {
            if stack.len() >= limits.max_condition_depth || !valid_if_expression(operands) {
                diagnostics.push(plan_diagnostic(
                    "MEJCL0750",
                    "IF expression is malformed or exceeds nesting bounds",
                    Severity::Error,
                    FailureCategory::MalformedInput,
                    Completeness::Incomplete,
                    source,
                ));
            } else {
                stack.push(false);
            }
        }
        "ELSE" => match stack.last_mut() {
            Some(seen_else @ false) => *seen_else = true,
            _ => diagnostics.push(plan_diagnostic(
                "MEJCL0751",
                "ELSE has no unmatched IF or repeats an ELSE",
                Severity::Error,
                FailureCategory::MalformedInput,
                Completeness::Incomplete,
                source,
            )),
        },
        "ENDIF" => {
            if stack.pop().is_none() {
                diagnostics.push(plan_diagnostic(
                    "MEJCL0752",
                    "ENDIF has no unmatched IF",
                    Severity::Error,
                    FailureCategory::MalformedInput,
                    Completeness::Incomplete,
                    source,
                ));
            }
        }
        "THEN" => {}
        _ => diagnostics.push(plan_diagnostic(
            "MEJCL0753",
            "conditional statement operation is unknown",
            Severity::Error,
            FailureCategory::MalformedInput,
            Completeness::Incomplete,
            source,
        )),
    }
}

fn valid_if_expression(value: &str) -> bool {
    let normalized = value
        .trim()
        .trim_end_matches(|character: char| character.is_ascii_whitespace())
        .strip_suffix("THEN")
        .unwrap_or(value)
        .replace(['(', ')'], " ");
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    words.iter().any(|word| {
        word.eq_ignore_ascii_case("RC")
            || word.eq_ignore_ascii_case("ABEND")
            || word.eq_ignore_ascii_case("ABENDCC")
            || word.eq_ignore_ascii_case("RUN")
    }) && words.iter().all(|word| word.len() <= 64)
}

fn statement_capability(identity: JclStatementId) -> Option<&'static str> {
    let descriptor = identity.descriptor();
    (descriptor.support == JclCatalogSupport::Deferred).then_some(descriptor.capability)
}

fn validate_plan_graph(
    expanded: &[JclExpandedStatement],
    statements: &[JclStatementNode],
    nodes: &[JclPlanNode],
    limits: JclConversionLimits,
) -> Result<(), JclConversionProblem> {
    let known = statements
        .iter()
        .map(JclStatementNode::id)
        .collect::<BTreeSet<_>>();
    if known.len() != statements.len()
        || nodes.len() != statements.len()
        || nodes
            .iter()
            .any(|node| !known.contains(&plan_node_statement(node)))
    {
        return Err(JclConversionProblem::PlanGraphInvariant);
    }
    let mut graph = JclDependencyGraph::new(
        limits.expansion.max_expansion_depth,
        limits.expansion.max_dependency_edges,
    );
    for statement in expanded {
        let chain = statement.procedure_chain();
        for name in chain {
            graph
                .enter(format!("plan-procedure:{name}"))
                .map_err(graph_problem)?;
        }
        for name in chain.iter().rev() {
            graph
                .leave(&format!("plan-procedure:{name}"))
                .map_err(graph_problem)?;
        }
    }
    graph.complete().map_err(graph_problem)
}

fn validate_statement_context(
    statements: &[JclStatementNode],
    nodes: &[JclPlanNode],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let by_id = statements
        .iter()
        .map(|statement| (statement.id(), statement))
        .collect::<BTreeMap<_, _>>();
    let mut seen_job = false;
    let mut seen_step = false;
    let mut terminated = false;
    let mut step_names = BTreeSet::new();
    let mut dd_names = BTreeSet::<(Option<String>, String)>::new();
    let mut output_names = BTreeSet::new();
    for node in nodes {
        let statement_id = plan_node_statement(node);
        let Some(statement) = by_id.get(&statement_id) else {
            continue;
        };
        let source = SourceRange {
            file: FileId::new(statement.source().file_id())
                .expect("plan source file identity is nonzero"),
            bytes: statement.source().byte_start()..statement.source().byte_end(),
        };
        if terminated
            && !matches!(node, JclPlanNode::Annotation { operation, .. } if operation == "NULL")
        {
            diagnostics.push(plan_diagnostic(
                "MEJCL0754",
                "statement appears after the terminating null statement",
                Severity::Error,
                FailureCategory::MalformedInput,
                Completeness::Incomplete,
                &source,
            ));
            continue;
        }
        match node {
            JclPlanNode::Job { name, .. } => {
                if seen_job || seen_step || !valid_program(name) {
                    diagnostics.push(plan_diagnostic(
                        "MEJCL0755",
                        "JOB must be the single named job boundary before executable steps",
                        Severity::Error,
                        FailureCategory::MalformedInput,
                        Completeness::Incomplete,
                        &source,
                    ));
                }
                seen_job = true;
            }
            JclPlanNode::Step { name, .. } => {
                if !seen_job || !valid_qualified_name(name) || !step_names.insert(name.clone()) {
                    diagnostics.push(plan_diagnostic(
                        "MEJCL0756",
                        "EXEC requires a unique qualified step name after JOB",
                        Severity::Error,
                        FailureCategory::MalformedInput,
                        Completeness::Incomplete,
                        &source,
                    ));
                }
                seen_step = true;
            }
            JclPlanNode::Dd { name, step, .. } => {
                let job_dd = step.is_none();
                let job_dd_name = matches!(name.as_str(), "JOBLIB" | "JOBCAT");
                let duplicate = !dd_names.insert((step.clone(), name.clone()));
                if !seen_job
                    || (job_dd && seen_step)
                    || (job_dd && !job_dd_name)
                    || (!job_dd && !seen_step)
                    || (!valid_program(name) && name != "*")
                    || (duplicate && name != "*")
                {
                    diagnostics.push(plan_diagnostic(
                        "MEJCL0757",
                        "DD placement, name, or concatenation context is invalid",
                        Severity::Error,
                        FailureCategory::MalformedInput,
                        Completeness::Incomplete,
                        &source,
                    ));
                }
            }
            JclPlanNode::Output { name, .. } => {
                if !seen_job || !valid_qualified_name(name) || !output_names.insert(name.clone()) {
                    diagnostics.push(plan_diagnostic(
                        "MEJCL0758",
                        "OUTPUT requires a unique name after JOB",
                        Severity::Error,
                        FailureCategory::MalformedInput,
                        Completeness::Incomplete,
                        &source,
                    ));
                }
            }
            JclPlanNode::Annotation { operation, .. } => {
                if operation == "NULL" {
                    terminated = true;
                } else if !operation.starts_with("definition-")
                    && !matches!(operation.as_str(), "COMMENT" | "DELIMITER")
                    && !seen_job
                {
                    diagnostics.push(plan_diagnostic(
                        "MEJCL0759",
                        "job-scoped statement appears before JOB",
                        Severity::Error,
                        FailureCategory::MalformedInput,
                        Completeness::Incomplete,
                        &source,
                    ));
                }
            }
            JclPlanNode::Jecl { .. } => {}
        }
    }
}

fn valid_qualified_name(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(valid_program)
}

fn graph_problem(_: JclGraphProblem) -> JclConversionProblem {
    JclConversionProblem::PlanGraphInvariant
}

const fn plan_node_statement(node: &JclPlanNode) -> u32 {
    match node {
        JclPlanNode::Job { statement_id, .. }
        | JclPlanNode::Step { statement_id, .. }
        | JclPlanNode::Dd { statement_id, .. }
        | JclPlanNode::Output { statement_id, .. }
        | JclPlanNode::Jecl { statement_id, .. }
        | JclPlanNode::Annotation { statement_id, .. } => *statement_id,
    }
}

fn projected_statement_span(
    source: &SourceBundle,
    statement: &JclExpandedStatement,
) -> Result<JclSourceSpan, JclConversionProblem> {
    let primary = statement
        .statement()
        .sources()
        .first()
        .ok_or(JclConversionProblem::PlanGraphInvariant)?;
    let mut origins = Vec::new();
    for site in statement.invocation_sites() {
        origins.push(projected_origin(
            source,
            site,
            JclSourceOriginKind::Generated,
        )?);
    }
    for site in statement.override_sites() {
        origins.push(projected_origin(
            source,
            site,
            JclSourceOriginKind::OverrideUse,
        )?);
    }
    let kind = source
        .file(primary.file)
        .map(|file| file.path().as_str())
        .map_or(JclSourceOriginKind::Primary, |path| {
            if path.starts_with("jcl/includes/") {
                JclSourceOriginKind::Include
            } else if path.starts_with("jcl/procedures/") {
                JclSourceOriginKind::Procedure
            } else {
                JclSourceOriginKind::Primary
            }
        });
    projected_source_span(source, primary, kind, &origins)
}

fn projected_source_span(
    source: &SourceBundle,
    range: &SourceRange,
    kind: JclSourceOriginKind,
    extra_origins: &[JclSourceOrigin],
) -> Result<JclSourceSpan, JclConversionProblem> {
    let file = source
        .file(range.file)
        .ok_or(JclConversionProblem::PlanGraphInvariant)?;
    if range.bytes.end > file.bytes().len() || range.bytes.start > range.bytes.end {
        return Err(JclConversionProblem::PlanGraphInvariant);
    }
    let (line, column_start) = line_column(file.bytes(), range.bytes.start);
    let (_, column_end) = line_column(file.bytes(), range.bytes.end);
    let mut origins = vec![projected_origin(source, range, kind)?];
    origins.extend_from_slice(extra_origins);
    Ok(JclSourceSpan::new(
        range.file.get(),
        file.path().as_str().into(),
        range.bytes.start,
        range.bytes.end,
        line,
        column_start.min(81),
        column_end.min(81),
        origins,
    ))
}

fn projected_origin(
    source: &SourceBundle,
    range: &SourceRange,
    kind: JclSourceOriginKind,
) -> Result<JclSourceOrigin, JclConversionProblem> {
    let file = source
        .file(range.file)
        .ok_or(JclConversionProblem::PlanGraphInvariant)?;
    Ok(JclSourceOrigin::new(
        range.file.get(),
        file.path().as_str().into(),
        range.bytes.start,
        range.bytes.end,
        kind,
    ))
}

fn line_column(bytes: &[u8], offset: usize) -> (usize, usize) {
    let offset = offset.min(bytes.len());
    let line = bytes[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let line_start = bytes[..offset]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    (line, offset - line_start + 1)
}

fn system_symbol_span(source: &SourceBundle) -> Result<JclSourceSpan, JclConversionProblem> {
    let file = source
        .files()
        .iter()
        .find(|file| file.path().as_str() == "jcl/system-symbols.json")
        .or_else(|| source.file(source.primary()))
        .ok_or(JclConversionProblem::PlanGraphInvariant)?;
    projected_source_span(
        source,
        &SourceRange {
            file: file.id(),
            bytes: 0..file.bytes().len(),
        },
        JclSourceOriginKind::Definition,
        &[],
    )
}

fn plan_identity(
    source_identity: &str,
    statements: &[JclStatementNode],
    symbols: &[JclSymbolDefinition],
    procedures: &[JclProcedureDefinition],
    nodes: &[JclPlanNode],
    capabilities: &[JclCapabilityRequirement],
) -> Result<String, JclConversionProblem> {
    #[derive(Serialize)]
    struct Material<'a> {
        contract: &'static str,
        catalog: &'static str,
        schema: &'static str,
        source: &'a str,
        statements: &'a [JclStatementNode],
        symbols: &'a [JclSymbolDefinition],
        procedures: &'a [JclProcedureDefinition],
        nodes: &'a [JclPlanNode],
        capabilities: &'a [JclCapabilityRequirement],
    }
    let bytes = serde_json::to_vec(&Material {
        contract: JCL_PLAN_CONTRACT,
        catalog: JCL_GENERATED_CATALOG_SHA256,
        schema: JCL_PLAN_SCHEMA_SHA256,
        source: source_identity,
        statements,
        symbols,
        procedures,
        nodes,
        capabilities,
    })
    .map_err(|_| JclConversionProblem::Serialization)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn project_diagnostic(
    source: &SourceBundle,
    diagnostic: &Diagnostic,
) -> Result<JclDiagnosticProjection, JclConversionProblem> {
    let primary = diagnostic
        .primary()
        .map(|span| {
            projected_source_span(
                source,
                &SourceRange {
                    file: span.file,
                    bytes: span.bytes.clone(),
                },
                JclSourceOriginKind::Primary,
                &[],
            )
        })
        .transpose()?;
    // The frozen shared diagnostic contract intentionally exposes related
    // spans as an immutable slice without serializable field access. Keep the
    // authoritative related spans on `Diagnostic`; do not duplicate or mutate
    // that shared boundary merely for this projection.
    let related = Vec::<JclRelatedDiagnostic>::new();
    Ok(JclDiagnosticProjection::new(
        diagnostic.code().as_str().into(),
        severity_name(diagnostic.severity()).into(),
        category_name(diagnostic.category()).into(),
        diagnostic.public_message().into(),
        primary,
        related,
    ))
}

const fn severity_name(value: Severity) -> &'static str {
    match value {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Information => "information",
    }
}

const fn category_name(value: FailureCategory) -> &'static str {
    match value {
        FailureCategory::MalformedInput => "malformed-input",
        FailureCategory::Unsupported => "unsupported",
        FailureCategory::Condition => "condition",
        FailureCategory::Abend => "abend",
        FailureCategory::Cancelled => "cancelled",
        FailureCategory::TimedOut => "timed-out",
        FailureCategory::Rejected => "rejected",
        FailureCategory::ResourceExhausted => "resource-exhausted",
        FailureCategory::Unauthorized => "unauthorized",
        FailureCategory::ProviderFailure => "provider-failure",
        FailureCategory::InfrastructureFailure => "infrastructure-failure",
        FailureCategory::IncompatibleVersion => "incompatible-version",
        FailureCategory::UnknownOutcome => "unknown-outcome",
    }
}

fn plan_diagnostic(
    code: &str,
    message: &str,
    severity: Severity,
    category: FailureCategory,
    completeness: Completeness,
    source: &SourceRange,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).expect("static JCL plan diagnostic code"),
        severity,
        Phase::Verify,
        category,
        completeness,
        message,
        Some(SourceSpan::new(source.file, source.bytes.clone()).expect("validated plan source")),
        Redaction::Public,
        DiagnosticLimits::default(),
    )
    .expect("bounded JCL plan diagnostic")
}

fn plan_global_diagnostic(
    code: &str,
    message: &str,
    statement: Option<&JclExpandedStatement>,
) -> Diagnostic {
    let source = statement
        .and_then(|statement| statement.statement().sources().first())
        .cloned()
        .unwrap_or(SourceRange {
            file: FileId::new(1).expect("one is a valid source file identity"),
            bytes: 0..0,
        });
    plan_diagnostic(
        code,
        message,
        Severity::Error,
        FailureCategory::MalformedInput,
        Completeness::Incomplete,
        &source,
    )
}

fn assignment(operands: &str, selected: &str) -> Option<String> {
    split_top_level(operands).into_iter().find_map(|value| {
        value.split_once('=').and_then(|(name, value)| {
            name.trim()
                .eq_ignore_ascii_case(selected)
                .then(|| value.trim().to_string())
        })
    })
}

fn redact_parameter(operands: &str, selected: &str) -> String {
    split_top_level(operands)
        .into_iter()
        .map(|operand| {
            operand.split_once('=').map_or_else(
                || operand.to_string(),
                |(name, value)| {
                    if name.trim().eq_ignore_ascii_case(selected) {
                        format!("{}=[redacted]", name.trim())
                    } else {
                        format!("{}={}", name.trim(), value.trim())
                    }
                },
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn split_top_level(value: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut depth = 0usize;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() => depth = depth.saturating_sub(1),
            b',' if quote.is_none() && depth == 0 => {
                values.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    values.push(&value[start..]);
    values
}

fn strip_quotes(value: &str) -> &str {
    value.trim().trim_matches(['\'', '"'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DD_PARAMETERS, EXEC_PARAMETERS, JOB_PARAMETERS, JclParameterIdentity, OUTPUT_PARAMETERS,
    };

    fn convert(source: &str) -> JclConversion {
        convert_jcl(
            &JclBundle {
                primary: source.into(),
                ..JclBundle::default()
            },
            JclConversionLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn valid_job_emits_stable_immutable_typed_plan() {
        let source = "//J JOB CLASS=A,PRTY=7,MSGLEVEL=(1,1)\n//S EXEC PGM=IEFBR14,PARM='A,B'\n//IN DD DSNAME=U.INPUT,DISP=SHR,RECFM=FB,LRECL=80\n//OUT DD SYSOUT=*\n//O OUTPUT CLASS=A,DEST=LOCAL\n";
        let first = convert(source);
        let second = convert(source);
        assert!(first.is_valid(), "{:?}", first.diagnostics());
        assert_eq!(first.plan(), second.plan());
        let plan = first.plan().unwrap();
        assert_eq!(plan.schema_version(), JCL_PLAN_CONTRACT);
        assert_eq!(plan.statements().len(), 5);
        assert!(
            plan.nodes().iter().any(
                |node| matches!(node, JclPlanNode::Step { program, .. } if program == "IEFBR14")
            )
        );
        assert!(
            plan.capabilities()
                .iter()
                .any(|requirement| requirement.capability() == "dataset.allocation")
        );
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.7/schemas/jcl-job-plan.schema.json"
        ))
        .unwrap();
        let validator = jsonschema::draft202012::options()
            .offline()
            .build(&schema)
            .unwrap();
        validator
            .validate(&serde_json::to_value(plan).unwrap())
            .unwrap();
    }

    #[test]
    fn every_generated_parameter_has_validation_and_capability_closure() {
        for identity in DD_PARAMETERS
            .iter()
            .map(|entry| JclParameterIdentity::Dd(entry.id))
            .chain(
                EXEC_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Exec(entry.id)),
            )
            .chain(
                JOB_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Job(entry.id)),
            )
            .chain(
                OUTPUT_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Output(entry.id)),
            )
        {
            assert!(!identity.capability().is_empty());
            assert!(!validation_name(identity.validation()).is_empty());
        }
    }

    #[test]
    fn every_generated_parameter_executes_valid_and_invalid_value_obligations() {
        for identity in DD_PARAMETERS
            .iter()
            .map(|entry| JclParameterIdentity::Dd(entry.id))
            .chain(
                EXEC_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Exec(entry.id)),
            )
            .chain(
                JOB_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Job(entry.id)),
            )
            .chain(
                OUTPUT_PARAMETERS
                    .iter()
                    .map(|entry| JclParameterIdentity::Output(entry.id)),
            )
        {
            let valid = valid_sample(identity);
            assert!(
                normalize_parameter(identity, &valid).is_ok(),
                "valid sample failed for {}: {valid}",
                identity.generated().row_id()
            );
            let invalid = invalid_sample(identity.validation());
            assert!(
                normalize_parameter(identity, invalid).is_err(),
                "invalid sample passed for {}: {invalid}",
                identity.generated().row_id()
            );
        }
    }

    #[test]
    fn malformed_ranges_enums_conditions_and_names_prevent_planning() {
        for source in [
            "//J JOB PRTY=16\n//S EXEC PGM=IEFBR14\n",
            "//J JOB CLASS=AB\n//S EXEC PGM=IEFBR14\n",
            "//J JOB\n//S EXEC PGM=TOO-LONG9\n",
            "//J JOB\n//S EXEC PGM=IEFBR14,COND=(X,EQ)\n",
            "//J JOB\n//S EXEC PGM=IEFBR14\n//D DD DISP=(BAD,KEEP)\n",
        ] {
            let conversion = convert(source);
            assert!(!conversion.is_valid(), "unexpected plan for {source}");
            assert!(
                conversion
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0745")
            );
        }
    }

    #[test]
    fn deferred_parameters_are_retained_and_diagnosed_not_dropped() {
        let conversion = convert(
            "//J JOB CLASS=A,REGION=4M\n//S EXEC PGM=IEFBR14\n//D DD DSNAME=U.DATA,AVGREC=K\n",
        );
        assert!(conversion.is_valid());
        let plan = conversion.plan().unwrap();
        assert!(plan.capabilities().iter().any(|requirement| {
            requirement.capability() == "jcl.job.region"
                && requirement.state() == JclCapabilityState::Deferred
        }));
        assert!(conversion.diagnostics().iter().any(|diagnostic| {
            diagnostic.code().as_str() == "MEJCL0744" && diagnostic.severity() == Severity::Warning
        }));
        assert!(
            plan.statements()
                .iter()
                .flat_map(JclStatementNode::parameters)
                .any(|parameter| parameter.identity().keyword() == "AVGREC")
        );
    }

    #[test]
    fn sensitive_job_password_never_enters_plan_or_diagnostic_projection() {
        let conversion =
            convert("//J JOB CLASS=A,USER=USER1,PASSWORD=SECRET\n//S EXEC PGM=IEFBR14\n");
        assert!(conversion.is_valid());
        let bytes = serde_json::to_vec(conversion.plan().unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("SECRET"));
        assert!(
            conversion
                .diagnostic_projection()
                .iter()
                .all(|diagnostic| !diagnostic.message().contains("SECRET"))
        );
    }

    #[test]
    fn condition_nesting_and_statement_capabilities_are_explicit() {
        let valid = convert(
            "//J JOB\n// IF (RC = 0) THEN\n//S EXEC PGM=IEFBR14\n// ELSE\n//T EXEC PGM=IEFBR14\n// ENDIF\n//X XMIT DEST\n",
        );
        assert!(valid.is_valid(), "{:?}", valid.diagnostics());
        assert!(
            valid
                .plan()
                .unwrap()
                .capabilities()
                .iter()
                .any(|requirement| requirement.capability() == "jes.nje.transmit")
        );
        let invalid = convert("//J JOB\n// ELSE\n//S EXEC PGM=IEFBR14\n");
        assert!(!invalid.is_valid());
        assert!(
            invalid
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0751")
        );
    }

    #[test]
    fn inline_bytes_and_expansion_provenance_survive_planning() {
        let conversion = convert_jcl(
            &JclBundle {
                primary: "//J JOB\n//R EXEC PROC=P\n".into(),
                cataloged_procedures: BTreeMap::from([(
                    "P".into(),
                    "//P PROC\n//S EXEC PGM=IEBGENER\n//IN DD *\nONE\n/*\n// PEND\n".into(),
                )]),
                ..JclBundle::default()
            },
            JclConversionLimits::default(),
        )
        .unwrap();
        assert!(conversion.is_valid(), "{:?}", conversion.diagnostics());
        let inline = conversion
            .plan()
            .unwrap()
            .statements()
            .iter()
            .find(|statement| !statement.inline_data().is_empty())
            .unwrap();
        assert_eq!(inline.inline_data(), b"ONE\n");
        assert!(
            inline
                .source()
                .origins()
                .iter()
                .any(|origin| origin.kind() == JclSourceOriginKind::Generated)
        );
    }

    fn valid_sample(identity: JclParameterIdentity) -> String {
        match identity.validation() {
            JclValueShape::None => identity.generated().keyword().into(),
            JclValueShape::JclValue | JclValueShape::Text => "VALUE".into(),
            JclValueShape::Name => "NAME".into(),
            JclValueShape::NameList => "(ONE,TWO)".into(),
            JclValueShape::Integer
            | JclValueShape::IntegerOrTuple
            | JclValueShape::IntegerOrX
            | JclValueShape::IntegerOrSuffix => identity.minimum().unwrap_or(1).to_string(),
            JclValueShape::Boolean => "YES".into(),
            JclValueShape::Enum => identity.choices()[0].into(),
            JclValueShape::Size | JclValueShape::SizeOrTuple => "4M".into(),
            JclValueShape::Time => "1".into(),
            JclValueShape::Class => "A".into(),
            JclValueShape::Condition => "(0,EQ)".into(),
            JclValueShape::MessageLevel => "(1,1)".into(),
            JclValueShape::Dataset => "USER.DATA".into(),
            JclValueShape::Disposition => "(NEW,CATLG,DELETE)".into(),
            JclValueShape::Delimiter => "@@".into(),
            JclValueShape::RecordFormat => "FB".into(),
            JclValueShape::Path => "/tmp/data".into(),
            JclValueShape::Program => "IEFBR14".into(),
            JclValueShape::Procedure => "PROC1".into(),
            JclValueShape::Restart => "STEP1".into(),
            JclValueShape::Sysout => "*".into(),
            JclValueShape::OutputReference => "OUT1".into(),
            JclValueShape::BackwardReference => "*.DD1".into(),
            JclValueShape::Secret => "SECRET".into(),
        }
    }

    const fn invalid_sample(shape: JclValueShape) -> &'static str {
        match shape {
            JclValueShape::None => "VALUE=BAD",
            JclValueShape::JclValue => "(",
            JclValueShape::Text
            | JclValueShape::Name
            | JclValueShape::NameList
            | JclValueShape::Integer
            | JclValueShape::IntegerOrTuple
            | JclValueShape::IntegerOrX
            | JclValueShape::IntegerOrSuffix
            | JclValueShape::Enum
            | JclValueShape::Size
            | JclValueShape::SizeOrTuple
            | JclValueShape::Time
            | JclValueShape::Class
            | JclValueShape::Condition
            | JclValueShape::MessageLevel
            | JclValueShape::Dataset
            | JclValueShape::Disposition
            | JclValueShape::Delimiter
            | JclValueShape::RecordFormat
            | JclValueShape::Path
            | JclValueShape::Program
            | JclValueShape::Procedure
            | JclValueShape::Restart
            | JclValueShape::Sysout
            | JclValueShape::OutputReference
            | JclValueShape::BackwardReference
            | JclValueShape::Secret => "",
            JclValueShape::Boolean => "MAYBE",
        }
    }
}
