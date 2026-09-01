use crate::jcl_graph::{JclDependencyGraph, JclGraphProblem};
use crate::jcl_syntax::analyze_jcl_source_file;
use crate::{
    JclBundle, JclParsedStatement, JclStatementId, JclSyntaxLimits, analyze_jcl_syntax,
    parse_jcl_statements,
};
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    RelatedSpan, Severity, SourceSpan,
};
use mainframe_env_source::{FileId, SourceBundle, SourceRange};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JclExpansionLimits {
    pub max_expansion_depth: usize,
    pub max_dependency_edges: usize,
    pub max_expanded_statements: usize,
    pub max_expanded_bytes: usize,
    pub max_symbols: usize,
    pub max_symbol_bytes: usize,
    pub max_symbol_substitutions: usize,
    pub max_procedures: usize,
    pub max_invocations_per_procedure: usize,
}

impl Default for JclExpansionLimits {
    fn default() -> Self {
        Self {
            max_expansion_depth: 32,
            max_dependency_edges: 65_536,
            max_expanded_statements: 262_144,
            max_expanded_bytes: 64 * 1024 * 1024,
            max_symbols: 4_096,
            max_symbol_bytes: 65_536,
            max_symbol_substitutions: 1_000_000,
            max_procedures: 4_096,
            max_invocations_per_procedure: 65_536,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclExpandedStatement {
    statement: JclParsedStatement,
    effective_name: Option<String>,
    effective_operands: String,
    procedure_chain: Vec<String>,
    invocation_sites: Vec<SourceRange>,
    override_sites: Vec<SourceRange>,
    definition_only: bool,
    backward_references: Vec<JclBackwardReference>,
}

impl JclExpandedStatement {
    #[must_use]
    pub fn statement(&self) -> &JclParsedStatement {
        &self.statement
    }

    #[must_use]
    pub fn effective_name(&self) -> Option<&str> {
        self.effective_name.as_deref()
    }

    #[must_use]
    pub fn effective_operands(&self) -> &str {
        &self.effective_operands
    }

    #[must_use]
    pub fn procedure_chain(&self) -> &[String] {
        &self.procedure_chain
    }

    #[must_use]
    pub fn invocation_sites(&self) -> &[SourceRange] {
        &self.invocation_sites
    }

    #[must_use]
    pub fn override_sites(&self) -> &[SourceRange] {
        &self.override_sites
    }

    #[must_use]
    pub const fn definition_only(&self) -> bool {
        self.definition_only
    }

    #[must_use]
    pub fn backward_references(&self) -> &[JclBackwardReference] {
        &self.backward_references
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclBackwardReference {
    expression: String,
    target_name: String,
    use_site: SourceRange,
    definition_site: SourceRange,
}

impl JclBackwardReference {
    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }

    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub fn use_site(&self) -> &SourceRange {
        &self.use_site
    }

    #[must_use]
    pub fn definition_site(&self) -> &SourceRange {
        &self.definition_site
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclExpandedSymbol {
    name: String,
    value: String,
    exported: bool,
    definition: Option<SourceRange>,
    uses: Vec<SourceRange>,
}

impl JclExpandedSymbol {
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
    pub fn definition(&self) -> Option<&SourceRange> {
        self.definition.as_ref()
    }

    #[must_use]
    pub fn uses(&self) -> &[SourceRange] {
        &self.uses
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclExpandedProcedure {
    name: String,
    library: Option<String>,
    defaults: BTreeMap<String, String>,
    definition: SourceRange,
    statement_count: usize,
    invocation_sites: Vec<SourceRange>,
}

impl JclExpandedProcedure {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn library(&self) -> Option<&str> {
        self.library.as_deref()
    }

    #[must_use]
    pub fn defaults(&self) -> &BTreeMap<String, String> {
        &self.defaults
    }

    #[must_use]
    pub fn definition(&self) -> &SourceRange {
        &self.definition
    }

    #[must_use]
    pub const fn statement_count(&self) -> usize {
        self.statement_count
    }

    #[must_use]
    pub fn invocation_sites(&self) -> &[SourceRange] {
        &self.invocation_sites
    }
}

#[derive(Clone, Debug)]
pub struct JclExpansion {
    source: Arc<SourceBundle>,
    statements: Vec<JclExpandedStatement>,
    symbols: Vec<JclExpandedSymbol>,
    procedures: Vec<JclExpandedProcedure>,
    procedure_search: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

impl JclExpansion {
    #[must_use]
    pub fn source(&self) -> &SourceBundle {
        &self.source
    }

    #[must_use]
    pub fn statements(&self) -> &[JclExpandedStatement] {
        &self.statements
    }

    #[must_use]
    pub fn symbols(&self) -> &[JclExpandedSymbol] {
        &self.symbols
    }

    #[must_use]
    pub fn procedures(&self) -> &[JclExpandedProcedure] {
        &self.procedures
    }

    #[must_use]
    pub fn procedure_search(&self) -> &[String] {
        &self.procedure_search
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JclExpansionProblem {
    Syntax(String),
    ResourceLimit(&'static str),
    InternalGraphInvariant,
}

impl fmt::Display for JclExpansionProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "JCL expansion failed: {self:?}")
    }
}

impl std::error::Error for JclExpansionProblem {}

#[derive(Clone, Debug)]
struct ProcedureTemplate {
    name: String,
    library: Option<String>,
    defaults: BTreeMap<String, String>,
    body: Vec<JclParsedStatement>,
    definition: SourceRange,
    invocation_sites: Vec<SourceRange>,
}

#[derive(Clone, Debug)]
struct SymbolValue {
    value: String,
    exported: bool,
    definition: Option<SourceRange>,
    uses: Vec<SourceRange>,
}

struct ExpansionContext<'a> {
    bundle: &'a JclBundle,
    source: Arc<SourceBundle>,
    syntax_limits: JclSyntaxLimits,
    limits: JclExpansionLimits,
    graph: JclDependencyGraph,
    dependency_sites: BTreeMap<String, SourceRange>,
    procedures: BTreeMap<String, ProcedureTemplate>,
    search_order: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    expanded_statements: usize,
    expanded_bytes: usize,
    substitutions: usize,
}

/// Expands INCLUDEs, procedures, symbols, overrides, and backward references
/// without performing any JES scheduling or execution.
pub fn expand_jcl(
    bundle: &JclBundle,
    syntax_limits: JclSyntaxLimits,
    limits: JclExpansionLimits,
) -> Result<JclExpansion, JclExpansionProblem> {
    validate_initial_symbols(&bundle.symbols, limits)?;
    let primary = analyze_jcl_syntax(bundle, syntax_limits)
        .map_err(|problem| JclExpansionProblem::Syntax(problem.to_string()))?;
    let parsed = parse_jcl_statements(&primary);
    let source = primary.source_arc();
    let mut context = ExpansionContext {
        bundle,
        source: Arc::clone(&source),
        syntax_limits,
        limits,
        graph: JclDependencyGraph::new(limits.max_expansion_depth, limits.max_dependency_edges),
        dependency_sites: BTreeMap::new(),
        procedures: BTreeMap::new(),
        search_order: Vec::new(),
        diagnostics: parsed.diagnostics().to_vec(),
        expanded_statements: 0,
        expanded_bytes: 0,
        substitutions: 0,
    };
    let mut symbols = bundle
        .symbols
        .iter()
        .map(|(name, value)| {
            (
                name.to_ascii_uppercase(),
                SymbolValue {
                    value: value.clone(),
                    exported: false,
                    definition: None,
                    uses: Vec::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut statements = Vec::new();
    let mut exported = BTreeSet::new();
    expand_sequence(
        &mut context,
        parsed.statements(),
        &mut symbols,
        &mut exported,
        &[],
        &[],
        &BTreeMap::new(),
        false,
        &mut statements,
    )?;
    context
        .graph
        .complete()
        .map_err(|_| JclExpansionProblem::InternalGraphInvariant)?;
    resolve_backward_references(&mut statements, &mut context.diagnostics);
    let procedures = context
        .procedures
        .into_values()
        .map(|procedure| JclExpandedProcedure {
            name: procedure.name,
            library: procedure.library,
            defaults: procedure.defaults,
            definition: procedure.definition,
            statement_count: procedure.body.len(),
            invocation_sites: procedure.invocation_sites,
        })
        .collect();
    let symbols = symbols
        .into_iter()
        .map(|(name, symbol)| JclExpandedSymbol {
            name,
            value: symbol.value,
            exported: symbol.exported,
            definition: symbol.definition,
            uses: symbol.uses,
        })
        .collect();
    Ok(JclExpansion {
        source,
        statements,
        symbols,
        procedures,
        procedure_search: context.search_order,
        diagnostics: context.diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn expand_sequence(
    context: &mut ExpansionContext<'_>,
    input: &[JclParsedStatement],
    symbols: &mut BTreeMap<String, SymbolValue>,
    exported: &mut BTreeSet<String>,
    procedure_chain: &[String],
    invocation_sites: &[SourceRange],
    overrides: &BTreeMap<String, JclParsedStatement>,
    definition_only: bool,
    output: &mut Vec<JclExpandedStatement>,
) -> Result<(), JclExpansionProblem> {
    let mut index = 0usize;
    let mut current_step = None::<String>;
    while index < input.len() {
        let statement = &input[index];
        match statement.identity() {
            JclStatementId::Include if !definition_only => {
                let site = primary_source(statement)?;
                let substituted = substitute_symbols(context, statement.operands(), symbols, &site);
                push_expanded(
                    context,
                    output,
                    statement,
                    statement.name().map(str::to_string),
                    substituted.clone(),
                    procedure_chain,
                    invocation_sites,
                    Vec::new(),
                    false,
                )?;
                let member = assignment(&substituted, "MEMBER")
                    .map(|value| unquote(&value).to_ascii_uppercase());
                let Some(member) = member else {
                    context.diagnostics.push(expansion_diagnostic(
                        "MEJCL0730",
                        "INCLUDE requires exactly one MEMBER operand",
                        &site,
                        &[],
                    ));
                    index += 1;
                    continue;
                };
                expand_include(
                    context,
                    &member,
                    &site,
                    symbols,
                    exported,
                    procedure_chain,
                    invocation_sites,
                    output,
                )?;
            }
            JclStatementId::Proc if !definition_only => {
                let start = index;
                let mut end = start + 1;
                while end < input.len() && input[end].identity() != JclStatementId::Pend {
                    end += 1;
                }
                if end == input.len() {
                    let source = primary_source(statement)?;
                    context.diagnostics.push(expansion_diagnostic(
                        "MEJCL0731",
                        "PROC is missing its matching PEND",
                        &source,
                        &[],
                    ));
                    return Ok(());
                }
                let name = statement.name().unwrap_or_default().to_ascii_uppercase();
                let definition = primary_source(statement)?;
                let defaults = assignment_map(statement.operands());
                let template = ProcedureTemplate {
                    name: name.clone(),
                    library: None,
                    defaults,
                    body: input[start + 1..end].to_vec(),
                    definition: definition.clone(),
                    invocation_sites: Vec::new(),
                };
                register_procedure(context, template, &definition)?;
                for definition_statement in &input[start..=end] {
                    push_expanded(
                        context,
                        output,
                        definition_statement,
                        definition_statement.name().map(str::to_string),
                        definition_statement.operands().to_string(),
                        procedure_chain,
                        invocation_sites,
                        Vec::new(),
                        true,
                    )?;
                }
                index = end;
            }
            JclStatementId::Set => {
                let site = primary_source(statement)?;
                for (name, raw_value) in ordered_assignments(statement.operands()) {
                    let value = substitute_symbols(context, &raw_value, symbols, &site);
                    if value.len() > context.limits.max_symbol_bytes {
                        return Err(JclExpansionProblem::ResourceLimit("symbol bytes"));
                    }
                    if !symbols.contains_key(&name) && symbols.len() >= context.limits.max_symbols {
                        return Err(JclExpansionProblem::ResourceLimit("symbols"));
                    }
                    symbols.insert(
                        name,
                        SymbolValue {
                            value,
                            exported: false,
                            definition: Some(site.clone()),
                            uses: Vec::new(),
                        },
                    );
                }
                push_expanded(
                    context,
                    output,
                    statement,
                    statement.name().map(str::to_string),
                    statement.operands().to_string(),
                    procedure_chain,
                    invocation_sites,
                    Vec::new(),
                    definition_only,
                )?;
            }
            JclStatementId::Export => {
                let site = primary_source(statement)?;
                for name in export_names(statement.operands()) {
                    if let Some(symbol) = symbols.get_mut(&name) {
                        symbol.exported = true;
                        exported.insert(name);
                    } else {
                        context.diagnostics.push(expansion_diagnostic(
                            "MEJCL0732",
                            "EXPORT names a symbol that is not defined at this point",
                            &site,
                            &[],
                        ));
                    }
                }
                push_expanded(
                    context,
                    output,
                    statement,
                    statement.name().map(str::to_string),
                    statement.operands().to_string(),
                    procedure_chain,
                    invocation_sites,
                    Vec::new(),
                    definition_only,
                )?;
            }
            JclStatementId::Jcllib if !definition_only => {
                let site = primary_source(statement)?;
                let substituted = substitute_symbols(context, statement.operands(), symbols, &site);
                if let Some(order) = assignment(&substituted, "ORDER") {
                    context.search_order = list_values(&order)
                        .into_iter()
                        .map(|value| value.to_ascii_uppercase())
                        .collect();
                }
                push_expanded(
                    context,
                    output,
                    statement,
                    statement.name().map(str::to_string),
                    substituted,
                    procedure_chain,
                    invocation_sites,
                    Vec::new(),
                    false,
                )?;
            }
            JclStatementId::Exec if !definition_only => {
                let site = primary_source(statement)?;
                let substituted = substitute_symbols(context, statement.operands(), symbols, &site);
                if let Some(procedure_name) = procedure_name(&substituted) {
                    let invocation_name = statement
                        .name()
                        .unwrap_or(procedure_name.as_str())
                        .to_ascii_uppercase();
                    let mut local_overrides = BTreeMap::new();
                    let mut next = index + 1;
                    while next < input.len() && is_override_statement(&input[next]) {
                        local_overrides.insert(
                            input[next].name().unwrap().to_ascii_uppercase(),
                            input[next].clone(),
                        );
                        next += 1;
                    }
                    push_expanded(
                        context,
                        output,
                        statement,
                        Some(qualify_name(procedure_chain, &invocation_name)),
                        substituted.clone(),
                        procedure_chain,
                        invocation_sites,
                        Vec::new(),
                        false,
                    )?;
                    expand_procedure(
                        context,
                        &procedure_name,
                        &invocation_name,
                        &substituted,
                        &site,
                        symbols,
                        procedure_chain,
                        invocation_sites,
                        &local_overrides,
                        output,
                    )?;
                    index = next.saturating_sub(1);
                } else {
                    let relative_step = statement.name().unwrap_or_default().to_ascii_uppercase();
                    let mut effective_operands = substituted;
                    let mut sites = Vec::new();
                    if let Some(replacement) = overrides.get(&relative_step) {
                        effective_operands = merge_operands(
                            &effective_operands,
                            &substitute_symbols(
                                context,
                                replacement.operands(),
                                symbols,
                                &primary_source(replacement)?,
                            ),
                        );
                        sites.push(primary_source(replacement)?);
                    }
                    current_step = Some(relative_step);
                    let effective_name =
                        qualify_name(procedure_chain, current_step.as_deref().unwrap_or_default());
                    push_expanded(
                        context,
                        output,
                        statement,
                        Some(effective_name),
                        effective_operands,
                        procedure_chain,
                        invocation_sites,
                        sites,
                        false,
                    )?;
                }
            }
            _ => {
                let site = primary_source(statement)?;
                let mut effective_operands =
                    substitute_symbols(context, statement.operands(), symbols, &site);
                let relative_name = match statement.identity() {
                    JclStatementId::Dd => current_step
                        .as_ref()
                        .and_then(|step| statement.name().map(|name| format!("{step}.{name}"))),
                    _ => statement.name().map(str::to_ascii_uppercase),
                };
                let mut sites = Vec::new();
                if let Some(relative) = &relative_name
                    && let Some(replacement) = overrides.get(relative)
                {
                    effective_operands = merge_operands(
                        &effective_operands,
                        &substitute_symbols(
                            context,
                            replacement.operands(),
                            symbols,
                            &primary_source(replacement)?,
                        ),
                    );
                    sites.push(primary_source(replacement)?);
                }
                let effective_name = relative_name
                    .as_deref()
                    .map(|name| qualify_name(procedure_chain, name));
                push_expanded(
                    context,
                    output,
                    statement,
                    effective_name,
                    effective_operands,
                    procedure_chain,
                    invocation_sites,
                    sites,
                    definition_only,
                )?;
            }
        }
        index += 1;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn expand_include(
    context: &mut ExpansionContext<'_>,
    member: &str,
    site: &SourceRange,
    symbols: &mut BTreeMap<String, SymbolValue>,
    exported: &mut BTreeSet<String>,
    procedure_chain: &[String],
    invocation_sites: &[SourceRange],
    output: &mut Vec<JclExpandedStatement>,
) -> Result<(), JclExpansionProblem> {
    let node = format!("include:{member}");
    context.dependency_sites.insert(node.clone(), site.clone());
    if let Err(problem) = context.graph.enter(node.clone()) {
        graph_diagnostic(context, problem, site);
        return Ok(());
    }
    let result = (|| {
        if !context
            .bundle
            .includes
            .keys()
            .any(|name| name.eq_ignore_ascii_case(member))
        {
            context.diagnostics.push(expansion_diagnostic(
                "MEJCL0733",
                "INCLUDE member was not found in the bounded source closure",
                site,
                &[],
            ));
            return Ok(());
        }
        let path = format!("jcl/includes/{member}.jcl");
        let Some(file) = source_file(&context.source, &path) else {
            context.diagnostics.push(expansion_diagnostic(
                "MEJCL0733",
                "INCLUDE member was not found in the bounded source closure",
                site,
                &[],
            ));
            return Ok(());
        };
        let parsed = parse_source_file(context, file)?;
        expand_sequence(
            context,
            &parsed,
            symbols,
            exported,
            procedure_chain,
            invocation_sites,
            &BTreeMap::new(),
            false,
            output,
        )
    })();
    context
        .graph
        .leave(&node)
        .map_err(|_| JclExpansionProblem::InternalGraphInvariant)?;
    result
}

#[allow(clippy::too_many_arguments)]
fn expand_procedure(
    context: &mut ExpansionContext<'_>,
    procedure_name: &str,
    invocation_name: &str,
    invocation_operands: &str,
    site: &SourceRange,
    caller_symbols: &mut BTreeMap<String, SymbolValue>,
    parent_chain: &[String],
    parent_sites: &[SourceRange],
    overrides: &BTreeMap<String, JclParsedStatement>,
    output: &mut Vec<JclExpandedStatement>,
) -> Result<(), JclExpansionProblem> {
    ensure_procedure(context, procedure_name, site)?;
    let key = procedure_key(None, procedure_name);
    let selected = select_procedure_key(context, procedure_name).unwrap_or(key);
    let Some(mut template) = context.procedures.get(&selected).cloned() else {
        context.diagnostics.push(expansion_diagnostic(
            "MEJCL0734",
            "EXEC procedure was not found in JCLLIB order or the default procedure library",
            site,
            &[],
        ));
        return Ok(());
    };
    let node = format!("procedure:{selected}");
    context.dependency_sites.insert(node.clone(), site.clone());
    if let Err(problem) = context.graph.enter(node.clone()) {
        graph_diagnostic(context, problem, site);
        return Ok(());
    }
    if template.invocation_sites.len() >= context.limits.max_invocations_per_procedure {
        return Err(JclExpansionProblem::ResourceLimit("procedure invocations"));
    }
    template.invocation_sites.push(site.clone());
    context
        .procedures
        .insert(selected.clone(), template.clone());
    let result = (|| {
        let mut local_symbols = caller_symbols.clone();
        for (name, raw_value) in &template.defaults {
            let value =
                substitute_symbols(context, raw_value, caller_symbols, &template.definition);
            local_symbols.insert(
                name.clone(),
                SymbolValue {
                    value,
                    exported: false,
                    definition: Some(template.definition.clone()),
                    uses: Vec::new(),
                },
            );
        }
        for (name, raw_value) in ordered_assignments(invocation_operands) {
            if is_exec_parameter(&name) {
                continue;
            }
            let value = substitute_symbols(context, &raw_value, caller_symbols, site);
            local_symbols.insert(
                name,
                SymbolValue {
                    value,
                    exported: false,
                    definition: Some(site.clone()),
                    uses: Vec::new(),
                },
            );
        }
        let mut chain = parent_chain.to_vec();
        chain.push(invocation_name.to_ascii_uppercase());
        let mut sites = parent_sites.to_vec();
        sites.push(site.clone());
        let mut exported_names = BTreeSet::new();
        expand_sequence(
            context,
            &template.body,
            &mut local_symbols,
            &mut exported_names,
            &chain,
            &sites,
            overrides,
            false,
            output,
        )?;
        for name in exported_names {
            if let Some(value) = local_symbols.get(&name).cloned() {
                caller_symbols.insert(name, value);
            }
        }
        Ok(())
    })();
    context
        .graph
        .leave(&node)
        .map_err(|_| JclExpansionProblem::InternalGraphInvariant)?;
    result
}

fn ensure_procedure(
    context: &mut ExpansionContext<'_>,
    name: &str,
    site: &SourceRange,
) -> Result<(), JclExpansionProblem> {
    if select_procedure_key(context, name).is_some() {
        return Ok(());
    }
    let mut candidates = context.search_order.clone();
    candidates.push("DEFAULT".into());
    for library in candidates {
        let path = if library == "DEFAULT" {
            format!("jcl/procedures/default/{name}.jcl")
        } else {
            format!("jcl/procedures/{library}/{name}.jcl")
        };
        let Some(file) = source_file(&context.source, &path) else {
            continue;
        };
        let statements = parse_source_file(context, file)?;
        let (defaults, body, definition) = cataloged_template(name, &statements, site);
        register_procedure(
            context,
            ProcedureTemplate {
                name: name.into(),
                library: Some(library.clone()),
                defaults,
                body,
                definition,
                invocation_sites: Vec::new(),
            },
            site,
        )?;
        return Ok(());
    }
    Ok(())
}

fn cataloged_template(
    name: &str,
    statements: &[JclParsedStatement],
    fallback: &SourceRange,
) -> (
    BTreeMap<String, String>,
    Vec<JclParsedStatement>,
    SourceRange,
) {
    if let Some(start) = statements
        .iter()
        .position(|statement| statement.identity() == JclStatementId::Proc)
    {
        let end = statements
            .iter()
            .skip(start + 1)
            .position(|statement| statement.identity() == JclStatementId::Pend)
            .map_or(statements.len(), |offset| start + 1 + offset);
        let definition = primary_source(&statements[start]).unwrap_or_else(|_| fallback.clone());
        (
            assignment_map(statements[start].operands()),
            statements[start + 1..end].to_vec(),
            definition,
        )
    } else {
        let definition = statements
            .first()
            .and_then(|statement| statement.sources().first())
            .cloned()
            .unwrap_or_else(|| fallback.clone());
        let _ = name;
        (BTreeMap::new(), statements.to_vec(), definition)
    }
}

fn parse_source_file(
    context: &mut ExpansionContext<'_>,
    file: FileId,
) -> Result<Vec<JclParsedStatement>, JclExpansionProblem> {
    let syntax = analyze_jcl_source_file(Arc::clone(&context.source), file, context.syntax_limits)
        .map_err(|problem| JclExpansionProblem::Syntax(problem.to_string()))?;
    let parsed = parse_jcl_statements(&syntax);
    context
        .diagnostics
        .extend(parsed.diagnostics().iter().cloned());
    Ok(parsed.statements().to_vec())
}

fn register_procedure(
    context: &mut ExpansionContext<'_>,
    template: ProcedureTemplate,
    site: &SourceRange,
) -> Result<(), JclExpansionProblem> {
    if context.procedures.len() >= context.limits.max_procedures {
        return Err(JclExpansionProblem::ResourceLimit("procedures"));
    }
    let key = procedure_key(template.library.as_deref(), &template.name);
    if let Some(previous) = context.procedures.get(&key) {
        context.diagnostics.push(expansion_diagnostic(
            "MEJCL0735",
            "procedure definition collides with an earlier definition in the same search scope",
            site,
            std::slice::from_ref(&previous.definition),
        ));
    } else {
        context.procedures.insert(key, template);
    }
    Ok(())
}

fn select_procedure_key(context: &ExpansionContext<'_>, name: &str) -> Option<String> {
    let instream = procedure_key(None, name);
    if context.procedures.contains_key(&instream) {
        return Some(instream);
    }
    context
        .search_order
        .iter()
        .map(|library| procedure_key(Some(library), name))
        .find(|key| context.procedures.contains_key(key))
        .or_else(|| {
            let default = procedure_key(Some("DEFAULT"), name);
            context.procedures.contains_key(&default).then_some(default)
        })
}

fn procedure_key(library: Option<&str>, name: &str) -> String {
    format!(
        "{}:{}",
        library.unwrap_or("INSTREAM").to_ascii_uppercase(),
        name.to_ascii_uppercase()
    )
}

#[allow(clippy::too_many_arguments)]
fn push_expanded(
    context: &mut ExpansionContext<'_>,
    output: &mut Vec<JclExpandedStatement>,
    statement: &JclParsedStatement,
    effective_name: Option<String>,
    effective_operands: String,
    procedure_chain: &[String],
    invocation_sites: &[SourceRange],
    override_sites: Vec<SourceRange>,
    definition_only: bool,
) -> Result<(), JclExpansionProblem> {
    context.expanded_statements += 1;
    context.expanded_bytes = context
        .expanded_bytes
        .checked_add(effective_operands.len() + statement.inline_data().len())
        .ok_or(JclExpansionProblem::ResourceLimit("expanded bytes"))?;
    if context.expanded_statements > context.limits.max_expanded_statements {
        return Err(JclExpansionProblem::ResourceLimit("expanded statements"));
    }
    if context.expanded_bytes > context.limits.max_expanded_bytes {
        return Err(JclExpansionProblem::ResourceLimit("expanded bytes"));
    }
    output.push(JclExpandedStatement {
        statement: statement.clone(),
        effective_name,
        effective_operands,
        procedure_chain: procedure_chain.to_vec(),
        invocation_sites: invocation_sites.to_vec(),
        override_sites,
        definition_only,
        backward_references: Vec::new(),
    });
    Ok(())
}

fn substitute_symbols(
    context: &mut ExpansionContext<'_>,
    value: &str,
    symbols: &mut BTreeMap<String, SymbolValue>,
    site: &SourceRange,
) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'&' {
            output.push(char::from(bytes[index]));
            index += 1;
            continue;
        }
        if bytes.get(index + 1) == Some(&b'&') {
            output.push('&');
            index += 2;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || matches!(bytes[end], b'$' | b'#' | b'@' | b'_'))
        {
            end += 1;
        }
        if end == start {
            output.push('&');
            index += 1;
            continue;
        }
        let name = value[start..end].to_ascii_uppercase();
        context.substitutions += 1;
        if context.substitutions > context.limits.max_symbol_substitutions {
            context.diagnostics.push(expansion_diagnostic(
                "MEJCL0736",
                "symbol substitution count exceeds the configured bound",
                site,
                &[],
            ));
            output.push_str(&value[index..end]);
        } else if let Some(symbol) = symbols.get_mut(&name) {
            output.push_str(&symbol.value);
            symbol.uses.push(site.clone());
        } else {
            context.diagnostics.push(expansion_diagnostic(
                "MEJCL0737",
                "symbol is not defined at this substitution point",
                site,
                &[],
            ));
            output.push_str(&value[index..end]);
        }
        index = end + usize::from(bytes.get(end) == Some(&b'.'));
    }
    output
}

fn resolve_backward_references(
    statements: &mut [JclExpandedStatement],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut definitions = BTreeMap::<String, SourceRange>::new();
    let mut current_step = None::<String>;
    for statement in statements {
        if statement.definition_only {
            continue;
        }
        if statement.statement.identity() == JclStatementId::Exec
            && procedure_name(&statement.effective_operands).is_none()
        {
            current_step = statement.effective_name.clone();
        }
        let Some(use_site) = statement.statement.sources().first().cloned() else {
            continue;
        };
        for expression in backward_reference_tokens(&statement.effective_operands) {
            let suffix = expression.trim_start_matches("*.").to_ascii_uppercase();
            let target = if suffix.contains('.') {
                suffix
            } else if let Some(step) = &current_step {
                format!("{step}.{suffix}")
            } else {
                suffix
            };
            if let Some(definition_site) = definitions.get(&target).cloned() {
                statement.backward_references.push(JclBackwardReference {
                    expression,
                    target_name: target,
                    use_site: use_site.clone(),
                    definition_site,
                });
            } else {
                diagnostics.push(expansion_diagnostic(
                    "MEJCL0738",
                    "backward reference does not name a preceding DD statement",
                    &use_site,
                    &[],
                ));
            }
        }
        if statement.statement.identity() == JclStatementId::Dd
            && let Some(name) = &statement.effective_name
        {
            definitions.insert(name.to_ascii_uppercase(), use_site);
        }
    }
}

fn backward_reference_tokens(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0usize;
    while index + 2 <= bytes.len() {
        if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'.') {
            let start = index;
            index += 2;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'$' | b'#' | b'@' | b'_' | b'.'))
            {
                index += 1;
            }
            output.push(value[start..index].trim_end_matches('.').to_string());
        } else {
            index += 1;
        }
    }
    output
}

fn graph_diagnostic(
    context: &mut ExpansionContext<'_>,
    problem: JclGraphProblem,
    site: &SourceRange,
) {
    let (code, message, related) = match problem {
        JclGraphProblem::Cycle(nodes) => {
            let related = nodes
                .iter()
                .filter_map(|node| context.dependency_sites.get(node).cloned())
                .collect::<Vec<_>>();
            ("MEJCL0739", "JCL dependency cycle detected", related)
        }
        JclGraphProblem::DepthLimitExceeded => (
            "MEJCL0740",
            "JCL dependency nesting exceeds the configured bound",
            Vec::new(),
        ),
        JclGraphProblem::EdgeLimitExceeded => (
            "MEJCL0741",
            "JCL dependency edge count exceeds the configured bound",
            Vec::new(),
        ),
        JclGraphProblem::UnbalancedTraversal => (
            "MEJCL0742",
            "JCL dependency traversal invariant failed",
            Vec::new(),
        ),
    };
    context
        .diagnostics
        .push(expansion_diagnostic(code, message, site, &related));
}

fn expansion_diagnostic(
    code: &str,
    message: &str,
    primary: &SourceRange,
    related: &[SourceRange],
) -> Diagnostic {
    let limits = DiagnosticLimits::default();
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::new(code).expect("static JCL diagnostic code"),
        Severity::Error,
        Phase::Semantic,
        FailureCategory::MalformedInput,
        Completeness::Incomplete,
        message,
        Some(
            SourceSpan::new(primary.file, primary.bytes.clone())
                .expect("validated JCL primary source"),
        ),
        Redaction::Public,
        limits,
    )
    .expect("bounded JCL expansion diagnostic");
    for source in related.iter().take(limits.max_related) {
        diagnostic
            .add_related(
                RelatedSpan::new(
                    SourceSpan::new(source.file, source.bytes.clone())
                        .expect("validated JCL related source"),
                    "related JCL definition or dependency use",
                    Redaction::Public,
                    limits,
                )
                .expect("bounded JCL related diagnostic"),
                limits,
            )
            .expect("bounded number of related JCL spans");
    }
    diagnostic
}

fn source_file(source: &SourceBundle, path: &str) -> Option<FileId> {
    source
        .files()
        .iter()
        .find(|file| file.path().as_str().eq_ignore_ascii_case(path))
        .map(|file| file.id())
}

fn primary_source(statement: &JclParsedStatement) -> Result<SourceRange, JclExpansionProblem> {
    statement
        .sources()
        .first()
        .cloned()
        .ok_or(JclExpansionProblem::Syntax(
            "parsed statement has no source range".into(),
        ))
}

fn qualify_name(chain: &[String], name: &str) -> String {
    if chain.is_empty() {
        name.to_ascii_uppercase()
    } else {
        format!("{}.{}", chain.join("."), name.to_ascii_uppercase())
    }
}

fn is_override_statement(statement: &JclParsedStatement) -> bool {
    match statement.identity() {
        JclStatementId::Dd => statement.name().is_some_and(|name| name.contains('.')),
        JclStatementId::Exec => {
            statement.name().is_some()
                && assignment(statement.operands(), "PGM").is_none()
                && assignment(statement.operands(), "PROC").is_none()
                && top_level_values(statement.operands())
                    .iter()
                    .all(|operand| operand.trim().is_empty() || operand.contains('='))
        }
        _ => false,
    }
}

fn merge_operands(base: &str, replacement: &str) -> String {
    let mut merged = top_level_values(base)
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();
    for override_value in top_level_values(replacement)
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some((name, _)) = override_value.split_once('=')
            && let Some(position) = merged.iter().position(|value| {
                value
                    .split_once('=')
                    .is_some_and(|(existing, _)| existing.trim().eq_ignore_ascii_case(name.trim()))
            })
        {
            merged[position] = override_value.to_string();
        } else {
            merged.push(override_value.to_string());
        }
    }
    merged.join(",")
}

fn procedure_name(operands: &str) -> Option<String> {
    if assignment(operands, "PGM").is_some() {
        return None;
    }
    assignment(operands, "PROC")
        .map(|value| unquote(&value).to_ascii_uppercase())
        .or_else(|| {
            top_level_values(operands)
                .into_iter()
                .find(|value| !value.contains('='))
                .map(|value| unquote(value).to_ascii_uppercase())
        })
}

fn assignment(operands: &str, selected: &str) -> Option<String> {
    top_level_values(operands).into_iter().find_map(|value| {
        value.split_once('=').and_then(|(name, value)| {
            name.trim()
                .eq_ignore_ascii_case(selected)
                .then(|| value.trim().to_string())
        })
    })
}

fn assignment_map(operands: &str) -> BTreeMap<String, String> {
    ordered_assignments(operands).into_iter().collect()
}

fn ordered_assignments(operands: &str) -> Vec<(String, String)> {
    top_level_values(operands)
        .into_iter()
        .filter_map(|value| {
            value.split_once('=').map(|(name, value)| {
                (
                    name.trim().trim_start_matches('&').to_ascii_uppercase(),
                    unquote(value.trim()).to_string(),
                )
            })
        })
        .collect()
}

fn export_names(operands: &str) -> Vec<String> {
    assignment(operands, "SYMLIST")
        .map(|value| list_values(&value))
        .unwrap_or_else(|| list_values(operands))
        .into_iter()
        .map(|name| name.trim_start_matches('&').to_ascii_uppercase())
        .collect()
}

fn list_values(value: &str) -> Vec<String> {
    let value = value.trim().trim_matches(['(', ')']);
    top_level_values(value)
        .into_iter()
        .map(|item| unquote(item.trim()).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn top_level_values(value: &str) -> Vec<&str> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() => depth = depth.saturating_sub(1),
            b',' if quote.is_none() && depth == 0 => {
                output.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    output.push(&value[start..]);
    output
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches(['\'', '"'])
}

fn is_exec_parameter(name: &str) -> bool {
    matches!(
        name,
        "ABDISPCC"
            | "ACCT"
            | "ADDRSPC"
            | "CCSID"
            | "COND"
            | "DYNAMNBR"
            | "MEMLIMIT"
            | "PARM"
            | "PARMDD"
            | "PERFORM"
            | "PGM"
            | "PROC"
            | "RD"
            | "REGION"
            | "REGIONX"
            | "RLSTMOUT"
            | "TIME"
            | "TVSMSG"
            | "TVSAMCOM"
    )
}

fn validate_initial_symbols(
    symbols: &BTreeMap<String, String>,
    limits: JclExpansionLimits,
) -> Result<(), JclExpansionProblem> {
    if symbols.len() > limits.max_symbols {
        return Err(JclExpansionProblem::ResourceLimit("symbols"));
    }
    if symbols.iter().any(|(name, value)| {
        name.is_empty()
            || name.len() > 64
            || value.len() > limits.max_symbol_bytes
            || !name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'#' | b'@' | b'_')
            })
    }) {
        return Err(JclExpansionProblem::Syntax(
            "initial JCL symbol is invalid or exceeds its bound".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(bundle: JclBundle) -> JclExpansion {
        expand_jcl(
            &bundle,
            JclSyntaxLimits::default(),
            JclExpansionLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn include_symbol_timing_and_nested_procedure_provenance_are_preserved() {
        let expansion = expand(JclBundle {
            primary: "//J JOB CLASS=A\n// SET ROOT=USER\n//I INCLUDE MEMBER=DEFS\n//RUN EXEC PROC=OUTER,HLQ=&ROOT.\n".into(),
            includes: BTreeMap::from([(
                "DEFS".into(),
                "//INNER PROC HLQ=DEFAULT\n//S EXEC PGM=IEFBR14\n//D DD DSNAME=&HLQ..DATA\n// PEND\n//OUTER PROC HLQ=DEFAULT\n//N EXEC PROC=INNER,HLQ=&HLQ.\n// PEND\n".into(),
            )]),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        let dd = expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("RUN.N.S.D"))
            .unwrap();
        assert_eq!(dd.effective_operands(), "DSNAME=USER.DATA");
        assert_eq!(dd.procedure_chain(), &["RUN", "N"]);
        assert_eq!(dd.invocation_sites().len(), 2);
        assert_ne!(
            dd.statement().sources()[0].file,
            dd.invocation_sites()[0].file
        );
    }

    #[test]
    fn jcllib_search_order_selects_the_first_named_library() {
        let expansion = expand(JclBundle {
            primary: "//J JOB\n//L JCLLIB ORDER=(SECOND,FIRST)\n//R EXEC PROC=P\n".into(),
            procedure_libraries: BTreeMap::from([
                (
                    "FIRST".into(),
                    BTreeMap::from([(
                        "P".into(),
                        "//P PROC\n//A EXEC PGM=FIRST\n// PEND\n".into(),
                    )]),
                ),
                (
                    "SECOND".into(),
                    BTreeMap::from([(
                        "P".into(),
                        "//P PROC\n//B EXEC PGM=SECOND\n// PEND\n".into(),
                    )]),
                ),
            ]),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        assert!(expansion.statements().iter().any(|statement| {
            statement.effective_name() == Some("R.B")
                && statement.effective_operands() == "PGM=SECOND"
        }));
        assert_eq!(expansion.procedures()[0].library(), Some("SECOND"));
    }

    #[test]
    fn procedure_dd_override_keeps_use_and_definition_sources() {
        let expansion = expand(JclBundle {
            primary: "//J JOB\n//R EXEC PROC=P\n//S.IN DD DSNAME=OVERRIDE\n".into(),
            cataloged_procedures: BTreeMap::from([(
                "P".into(),
                "//P PROC\n//S EXEC PGM=IEFBR14\n//IN DD DSNAME=DEFAULT\n// PEND\n".into(),
            )]),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        let dd = expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("R.S.IN"))
            .unwrap();
        assert_eq!(dd.effective_operands(), "DSNAME=OVERRIDE");
        assert_eq!(dd.override_sites().len(), 1);
        assert_eq!(dd.invocation_sites().len(), 1);
        assert_ne!(
            dd.statement().sources()[0].file,
            dd.override_sites()[0].file
        );
    }

    #[test]
    fn procedure_exec_override_merges_only_selected_parameters() {
        let expansion = expand(JclBundle {
            primary: "//J JOB\n//R EXEC PROC=P\n//S EXEC PARM=NEW\n".into(),
            cataloged_procedures: BTreeMap::from([(
                "P".into(),
                "//P PROC\n//S EXEC PGM=IEFBR14,PARM=OLD,REGION=4M\n// PEND\n".into(),
            )]),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        let step = expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("R.S"))
            .unwrap();
        assert_eq!(step.effective_operands(), "PGM=IEFBR14,PARM=NEW,REGION=4M");
        assert_eq!(step.override_sites().len(), 1);
    }

    #[test]
    fn include_and_procedure_cycles_fail_with_related_provenance() {
        let include = expand(JclBundle {
            primary: "//J JOB\n//I INCLUDE MEMBER=A\n".into(),
            includes: BTreeMap::from([
                ("A".into(), "//I INCLUDE MEMBER=B\n".into()),
                ("B".into(), "//I INCLUDE MEMBER=A\n".into()),
            ]),
            ..JclBundle::default()
        });
        assert_eq!(
            include.diagnostics().last().unwrap().code().as_str(),
            "MEJCL0739"
        );
        assert!(!include.diagnostics().last().unwrap().related().is_empty());

        let procedure = expand(JclBundle {
            primary: "//J JOB\n//R EXEC PROC=A\n".into(),
            cataloged_procedures: BTreeMap::from([
                ("A".into(), "//A PROC\n//X EXEC PROC=B\n// PEND\n".into()),
                ("B".into(), "//B PROC\n//X EXEC PROC=A\n// PEND\n".into()),
            ]),
            ..JclBundle::default()
        });
        assert!(
            procedure
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0739")
        );
    }

    #[test]
    fn forward_symbol_use_fails_while_later_use_observes_set_timing() {
        let expansion = expand(JclBundle {
            primary: "//J JOB\n//A EXEC PGM=&P.\n// SET P=IEFBR14\n//B EXEC PGM=&P.\n".into(),
            ..JclBundle::default()
        });
        assert!(
            expansion
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "MEJCL0737")
        );
        let second = expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("B"))
            .unwrap();
        assert_eq!(second.effective_operands(), "PGM=IEFBR14");
    }

    #[test]
    fn procedure_export_updates_the_caller_only_after_invocation() {
        let expansion = expand(JclBundle {
            primary: "//P PROC\n// SET PROGRAM=IEFBR14\n// EXPORT SYMLIST=(PROGRAM)\n// PEND\n//J JOB\n//R EXEC PROC=P\n//S EXEC PGM=&PROGRAM.\n".into(),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        let step = expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("S"))
            .unwrap();
        assert_eq!(step.effective_operands(), "PGM=IEFBR14");
        let symbol = expansion
            .symbols()
            .iter()
            .find(|symbol| symbol.name() == "PROGRAM")
            .unwrap();
        assert!(symbol.exported());
        assert!(symbol.definition().is_some());
        assert!(!symbol.uses().is_empty());
    }

    #[test]
    fn backward_reference_requires_a_preceding_dd_and_records_both_sites() {
        let expansion = expand(JclBundle {
            primary:
                "//J JOB\n//S EXEC PGM=IEFBR14\n//ONE DD DSNAME=U.ONE\n//TWO DD DSNAME=*.ONE\n"
                    .into(),
            ..JclBundle::default()
        });
        assert!(expansion.is_complete(), "{:?}", expansion.diagnostics());
        let reference = &expansion
            .statements()
            .iter()
            .find(|statement| statement.effective_name() == Some("S.TWO"))
            .unwrap()
            .backward_references()[0];
        assert_eq!(reference.expression(), "*.ONE");
        assert_eq!(reference.target_name(), "S.ONE");
        assert_ne!(
            reference.use_site().bytes,
            reference.definition_site().bytes
        );
    }

    #[test]
    fn expansion_limits_fail_closed() {
        let result = expand_jcl(
            &JclBundle {
                primary: "//J JOB\n//S EXEC PGM=IEFBR14\n".into(),
                ..JclBundle::default()
            },
            JclSyntaxLimits::default(),
            JclExpansionLimits {
                max_expanded_statements: 1,
                ..JclExpansionLimits::default()
            },
        );
        assert!(matches!(
            result,
            Err(JclExpansionProblem::ResourceLimit("expanded statements"))
        ));
    }
}
