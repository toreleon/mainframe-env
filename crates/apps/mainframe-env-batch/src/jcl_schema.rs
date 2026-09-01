use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable wire identity for immutable converted JCL plans.
pub const JCL_PLAN_CONTRACT: &str = "mainframe-env.jcl-job-plan@1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JclSourceOriginKind {
    Primary,
    Include,
    Procedure,
    Generated,
    OverrideUse,
    Definition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclSourceOrigin {
    file_id: u32,
    logical_path: String,
    byte_start: usize,
    byte_end: usize,
    kind: JclSourceOriginKind,
}

impl JclSourceOrigin {
    #[must_use]
    pub fn new(
        file_id: u32,
        logical_path: String,
        byte_start: usize,
        byte_end: usize,
        kind: JclSourceOriginKind,
    ) -> Self {
        Self {
            file_id,
            logical_path,
            byte_start,
            byte_end,
            kind,
        }
    }

    #[must_use]
    pub const fn file_id(&self) -> u32 {
        self.file_id
    }

    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    #[must_use]
    pub const fn byte_start(&self) -> usize {
        self.byte_start
    }

    #[must_use]
    pub const fn byte_end(&self) -> usize {
        self.byte_end
    }

    #[must_use]
    pub const fn kind(&self) -> JclSourceOriginKind {
        self.kind
    }
}

/// A serializable projection of an authoritative exact source-byte range.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclSourceSpan {
    file_id: u32,
    logical_path: String,
    byte_start: usize,
    byte_end: usize,
    line: usize,
    column_start: usize,
    column_end: usize,
    origins: Vec<JclSourceOrigin>,
}

impl JclSourceSpan {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        file_id: u32,
        logical_path: String,
        byte_start: usize,
        byte_end: usize,
        line: usize,
        column_start: usize,
        column_end: usize,
        origins: Vec<JclSourceOrigin>,
    ) -> Self {
        Self {
            file_id,
            logical_path,
            byte_start,
            byte_end,
            line,
            column_start,
            column_end,
            origins,
        }
    }

    #[must_use]
    pub const fn file_id(&self) -> u32 {
        self.file_id
    }

    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    #[must_use]
    pub const fn byte_start(&self) -> usize {
        self.byte_start
    }

    #[must_use]
    pub const fn byte_end(&self) -> usize {
        self.byte_end
    }

    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub const fn column_start(&self) -> usize {
        self.column_start
    }

    #[must_use]
    pub const fn column_end(&self) -> usize {
        self.column_end
    }

    #[must_use]
    pub fn origins(&self) -> &[JclSourceOrigin] {
        &self.origins
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclRelatedDiagnostic {
    message: String,
    span: JclSourceSpan,
}

impl JclRelatedDiagnostic {
    #[must_use]
    pub fn new(message: String, span: JclSourceSpan) -> Self {
        Self { message, span }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn span(&self) -> &JclSourceSpan {
        &self.span
    }
}

/// Stable normalized diagnostic projection. The diagnostic authority remains
/// `mainframe-env.diagnostic@1`; this form only makes converter results
/// serializable with source provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclDiagnosticProjection {
    code: String,
    severity: String,
    category: String,
    message: String,
    primary: Option<JclSourceSpan>,
    related: Vec<JclRelatedDiagnostic>,
}

impl JclDiagnosticProjection {
    #[must_use]
    pub fn new(
        code: String,
        severity: String,
        category: String,
        message: String,
        primary: Option<JclSourceSpan>,
        related: Vec<JclRelatedDiagnostic>,
    ) -> Self {
        Self {
            code,
            severity,
            category,
            message,
            primary,
            related,
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn severity(&self) -> &str {
        &self.severity
    }

    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn primary(&self) -> Option<&JclSourceSpan> {
        self.primary.as_ref()
    }

    #[must_use]
    pub fn related(&self) -> &[JclRelatedDiagnostic] {
        &self.related
    }
}

/// Catalog-derived identity. The row ID remains the official denominator key.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct JclGeneratedIdentity {
    row_id: String,
    family: String,
    ordinal: u16,
    keyword: String,
}

impl JclGeneratedIdentity {
    #[must_use]
    pub fn new(row_id: String, family: String, ordinal: u16, keyword: String) -> Self {
        Self {
            row_id,
            family,
            ordinal,
            keyword,
        }
    }

    #[must_use]
    pub fn row_id(&self) -> &str {
        &self.row_id
    }

    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        self.ordinal
    }

    #[must_use]
    pub fn keyword(&self) -> &str {
        &self.keyword
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JclParameterOutcome {
    JobAttribute,
    StepAttribute,
    AllocationAttribute,
    OutputAttribute,
    ProcedureBinding,
    SymbolBinding,
    PlannerAnnotation,
    CapabilityRequirement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclParameterNode {
    identity: JclGeneratedIdentity,
    raw_value: String,
    normalized_value: String,
    outcome: JclParameterOutcome,
    source: JclSourceSpan,
}

impl JclParameterNode {
    #[must_use]
    pub fn new(
        identity: JclGeneratedIdentity,
        raw_value: String,
        normalized_value: String,
        outcome: JclParameterOutcome,
        source: JclSourceSpan,
    ) -> Self {
        Self {
            identity,
            raw_value,
            normalized_value,
            outcome,
            source,
        }
    }

    #[must_use]
    pub fn identity(&self) -> &JclGeneratedIdentity {
        &self.identity
    }

    #[must_use]
    pub fn raw_value(&self) -> &str {
        &self.raw_value
    }

    #[must_use]
    pub fn normalized_value(&self) -> &str {
        &self.normalized_value
    }

    #[must_use]
    pub const fn outcome(&self) -> JclParameterOutcome {
        self.outcome
    }

    #[must_use]
    pub fn source(&self) -> &JclSourceSpan {
        &self.source
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclStatementNode {
    id: u32,
    identity: JclGeneratedIdentity,
    name: Option<String>,
    raw_operands: String,
    parameters: Vec<JclParameterNode>,
    source: JclSourceSpan,
}

impl JclStatementNode {
    #[must_use]
    pub fn new(
        id: u32,
        identity: JclGeneratedIdentity,
        name: Option<String>,
        raw_operands: String,
        parameters: Vec<JclParameterNode>,
        source: JclSourceSpan,
    ) -> Self {
        Self {
            id,
            identity,
            name,
            raw_operands,
            parameters,
            source,
        }
    }

    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn identity(&self) -> &JclGeneratedIdentity {
        &self.identity
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn raw_operands(&self) -> &str {
        &self.raw_operands
    }

    #[must_use]
    pub fn parameters(&self) -> &[JclParameterNode] {
        &self.parameters
    }

    #[must_use]
    pub fn source(&self) -> &JclSourceSpan {
        &self.source
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclSymbolDefinition {
    name: String,
    value: String,
    exported: bool,
    definition: JclSourceSpan,
    uses: Vec<JclSourceSpan>,
}

impl JclSymbolDefinition {
    #[must_use]
    pub fn new(
        name: String,
        value: String,
        exported: bool,
        definition: JclSourceSpan,
        uses: Vec<JclSourceSpan>,
    ) -> Self {
        Self {
            name,
            value,
            exported,
            definition,
            uses,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn exported(&self) -> bool {
        self.exported
    }

    #[must_use]
    pub fn definition(&self) -> &JclSourceSpan {
        &self.definition
    }

    #[must_use]
    pub fn uses(&self) -> &[JclSourceSpan] {
        &self.uses
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclProcedureDefinition {
    name: String,
    defaults: BTreeMap<String, String>,
    statement_ids: Vec<u32>,
    definition: JclSourceSpan,
    invocation_sites: Vec<JclSourceSpan>,
}

impl JclProcedureDefinition {
    #[must_use]
    pub fn new(
        name: String,
        defaults: BTreeMap<String, String>,
        statement_ids: Vec<u32>,
        definition: JclSourceSpan,
        invocation_sites: Vec<JclSourceSpan>,
    ) -> Self {
        Self {
            name,
            defaults,
            statement_ids,
            definition,
            invocation_sites,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn defaults(&self) -> &BTreeMap<String, String> {
        &self.defaults
    }

    #[must_use]
    pub fn statement_ids(&self) -> &[u32] {
        &self.statement_ids
    }

    #[must_use]
    pub fn definition(&self) -> &JclSourceSpan {
        &self.definition
    }

    #[must_use]
    pub fn invocation_sites(&self) -> &[JclSourceSpan] {
        &self.invocation_sites
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JclCapabilityState {
    Available,
    Deferred,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclCapabilityRequirement {
    capability: String,
    state: JclCapabilityState,
    reason: String,
    statement_id: u32,
    parameter: Option<JclGeneratedIdentity>,
    source: JclSourceSpan,
}

impl JclCapabilityRequirement {
    #[must_use]
    pub fn new(
        capability: String,
        state: JclCapabilityState,
        reason: String,
        statement_id: u32,
        parameter: Option<JclGeneratedIdentity>,
        source: JclSourceSpan,
    ) -> Self {
        Self {
            capability,
            state,
            reason,
            statement_id,
            parameter,
            source,
        }
    }

    #[must_use]
    pub fn capability(&self) -> &str {
        &self.capability
    }

    #[must_use]
    pub const fn state(&self) -> JclCapabilityState {
        self.state
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub const fn statement_id(&self) -> u32 {
        self.statement_id
    }

    #[must_use]
    pub fn parameter(&self) -> Option<&JclGeneratedIdentity> {
        self.parameter.as_ref()
    }

    #[must_use]
    pub fn source(&self) -> &JclSourceSpan {
        &self.source
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum JclPlanNode {
    Job {
        name: String,
        statement_id: u32,
    },
    Step {
        name: String,
        program: String,
        statement_id: u32,
    },
    Dd {
        name: String,
        step: Option<String>,
        statement_id: u32,
    },
    Output {
        name: String,
        statement_id: u32,
    },
    Jecl {
        operation: String,
        statement_id: u32,
    },
    Annotation {
        operation: String,
        statement_id: u32,
    },
}

/// Immutable, deterministic converter output. All collections are private and
/// exposed only as shared views; construction remains converter-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JclPlanDocument {
    schema_version: String,
    catalog_digest: String,
    plan_schema_digest: String,
    source_identity: String,
    plan_identity: String,
    statements: Vec<JclStatementNode>,
    symbols: Vec<JclSymbolDefinition>,
    procedures: Vec<JclProcedureDefinition>,
    nodes: Vec<JclPlanNode>,
    capabilities: Vec<JclCapabilityRequirement>,
}

impl JclPlanDocument {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        catalog_digest: String,
        plan_schema_digest: String,
        source_identity: String,
        plan_identity: String,
        statements: Vec<JclStatementNode>,
        symbols: Vec<JclSymbolDefinition>,
        procedures: Vec<JclProcedureDefinition>,
        nodes: Vec<JclPlanNode>,
        capabilities: Vec<JclCapabilityRequirement>,
    ) -> Self {
        Self {
            schema_version: JCL_PLAN_CONTRACT.into(),
            catalog_digest,
            plan_schema_digest,
            source_identity,
            plan_identity,
            statements,
            symbols,
            procedures,
            nodes,
            capabilities,
        }
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub fn catalog_digest(&self) -> &str {
        &self.catalog_digest
    }

    #[must_use]
    pub fn plan_schema_digest(&self) -> &str {
        &self.plan_schema_digest
    }

    #[must_use]
    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }

    #[must_use]
    pub fn plan_identity(&self) -> &str {
        &self.plan_identity
    }

    #[must_use]
    pub fn statements(&self) -> &[JclStatementNode] {
        &self.statements
    }

    #[must_use]
    pub fn symbols(&self) -> &[JclSymbolDefinition] {
        &self.symbols
    }

    #[must_use]
    pub fn procedures(&self) -> &[JclProcedureDefinition] {
        &self.procedures
    }

    #[must_use]
    pub fn nodes(&self) -> &[JclPlanNode] {
        &self.nodes
    }

    #[must_use]
    pub fn capabilities(&self) -> &[JclCapabilityRequirement] {
        &self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> JclSourceSpan {
        JclSourceSpan::new(
            1,
            "jcl/primary.jcl".into(),
            0,
            8,
            1,
            1,
            9,
            vec![JclSourceOrigin::new(
                1,
                "jcl/primary.jcl".into(),
                0,
                8,
                JclSourceOriginKind::Primary,
            )],
        )
    }

    #[test]
    fn plan_contract_roundtrips_without_mutable_collection_access() {
        let document = JclPlanDocument::new(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
            "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![JclPlanNode::Job {
                name: "J".into(),
                statement_id: 1,
            }],
            Vec::new(),
        );
        let bytes = serde_json::to_vec(&document).unwrap();
        let decoded: JclPlanDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, document);
        assert_eq!(decoded.schema_version(), JCL_PLAN_CONTRACT);
        assert_eq!(decoded.nodes().len(), 1);
    }

    #[test]
    fn source_projection_keeps_exact_definition_and_origin_ranges() {
        let source = span();
        assert_eq!(source.byte_start(), 0);
        assert_eq!(source.byte_end(), 8);
        assert_eq!(source.origins()[0].kind(), JclSourceOriginKind::Primary);
    }
}
