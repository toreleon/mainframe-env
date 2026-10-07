//! Stage A DROP/RENAME syntax assurance, with no official bindings or coverage credit.
//! Product parsers receive only authored SQL and limits. This local wire projection
//! observes syntax, not binding, SQLCA, authorization, effects or licensed execution.

use crate::db2_syntax_fixtures::{Fixture, group};
use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation, DriverOutput,
    DriverRef, FixtureRef, ObservationCheck, ObservationRef, SpecProblem,
};
use mainframe_env_db2::{
    Db2DropAliasDesignator, Db2DropObjectKind, Db2Identifier, Db2RenameObjectKind, Db2SourceSpan,
    Db2SyntaxDiagnostic, parse_db2_drop_statement, parse_db2_rename_statement,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_RESULTS: usize = 48;
const MAX_IDENTIFIER_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum Route {
    #[serde(rename = "db2.syntax.drop")]
    Drop,
    #[serde(rename = "db2.syntax.rename")]
    Rename,
}

impl Route {
    const fn driver(self) -> &'static str {
        match self {
            Self::Drop => "db2.syntax.drop",
            Self::Rename => "db2.syntax.rename",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum Kind {
    Table,
    View,
    Index,
    Alias,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl From<Db2SourceSpan> for Span {
    fn from(span: Db2SourceSpan) -> Self {
        Self {
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            start_line: span.start.line,
            start_column: span.start.column,
            end_line: span.end.line,
            end_column: span.end.column,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Component {
    pub value: String,
    pub delimited: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Name {
    pub components: Vec<Component>,
    pub span: Span,
}

// An enum makes omission of the designator field a wire error, including for
// non-alias objects. Delimiter provenance is syntax, not object identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "form", deny_unknown_fields)]
pub(crate) enum Designator {
    NotAlias,
    Omitted,
    ForTable { span: Span },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", deny_unknown_fields)]
pub(crate) enum Outcome {
    Drop {
        kind: Kind,
        name: Name,
        designator: Designator,
        span: Span,
    },
    Rename {
        kind: Kind,
        source: Name,
        destination: Component,
        span: Span,
    },
    Diagnostic {
        code: String,
        line: u32,
        column: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultProjection {
    case_id: String,
    actual: Outcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Projection {
    fixture_id: String,
    route: Route,
    results: Vec<ResultProjection>,
}

/// Dormant Stage A handlers. `bind` adds only handlers referenced by compiled
/// cases; it does not install a CLI route or add official specifications.
pub struct Db2SyntaxRuntime {
    drop_driver: DropDriver,
    rename_driver: RenameDriver,
    observations: [SyntaxObservation; 5],
}

/// Construct dormant syntax handlers for explicit compiled-case composition.
#[must_use]
pub const fn db2_syntax_runtime() -> Db2SyntaxRuntime {
    Db2SyntaxRuntime {
        drop_driver: DropDriver,
        rename_driver: RenameDriver,
        observations: [
            SyntaxObservation(Route::Drop, "db2.syntax.drop.structure"),
            SyntaxObservation(Route::Rename, "db2.syntax.rename.structure"),
            SyntaxObservation(Route::Drop, "db2.syntax.drop.bounds"),
            SyntaxObservation(Route::Rename, "db2.syntax.rename.bounds"),
            SyntaxObservation(Route::Drop, "db2.syntax.drop.limitations"),
        ],
    }
}

impl Db2SyntaxRuntime {
    /// Compose with the existing exact-closure `RuntimeRegistry`. Fixture groups
    /// are `db2.syntax.{drop,rename}.{structure,bounds}` and
    /// `db2.syntax.drop.limitations`. The last records legal deferred syntax,
    /// never an official acceptance expectation. No review is promoted here.
    pub fn bind<'a>(
        &'a self,
        spec: &CompiledSpec,
        drivers: &mut Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        observations: &mut Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        limits: ConformanceLimits,
    ) -> Result<(), SpecProblem> {
        // Validate all relevant references before changing the caller's lists.
        for case in spec.cases() {
            if !case.driver().as_str().starts_with("db2.syntax.") {
                continue;
            }
            let route = [Route::Drop, Route::Rename]
                .into_iter()
                .find(|route| case.driver().as_str() == route.driver())
                .ok_or_else(|| {
                    SpecProblem::RuntimeRegistryIncomplete("unknown Db2 syntax route".into())
                })?;
            fixture_group(case.input().as_str(), route).map_err(SpecProblem::RuntimeFailure)?;
            let observation = format!("{}.exact", case.input());
            if case.expected().len() != 1 || case.expected()[0].as_str() != observation {
                return Err(SpecProblem::RuntimeRegistryIncomplete(
                    "Db2 syntax case needs its exact route observation".into(),
                ));
            }
        }
        let mut selected_drivers = Vec::new();
        let mut selected_observations = Vec::new();
        for route in [Route::Drop, Route::Rename] {
            if !spec
                .cases()
                .any(|case| case.driver().as_str() == route.driver())
            {
                continue;
            }
            let driver = DriverRef::new(route.driver(), limits)?;
            let handler: &dyn ConformanceDriver = match route {
                Route::Drop => &self.drop_driver,
                Route::Rename => &self.rename_driver,
            };
            selected_drivers.push((driver, handler));
        }
        for comparator in &self.observations {
            let reference = ObservationRef::new(format!("{}.exact", comparator.1), limits)?;
            if spec
                .cases()
                .any(|case| case.expected().contains(&reference))
            {
                selected_observations.push((reference, comparator as &dyn ConformanceObservation));
            }
        }
        drivers.extend(selected_drivers);
        observations.extend(selected_observations);
        Ok(())
    }
}

struct DropDriver;
struct RenameDriver;

impl ConformanceDriver for DropDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        execute(fixture, Route::Drop)
    }
}

impl ConformanceDriver for RenameDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        execute(fixture, Route::Rename)
    }
}

fn fixture_group(id: &str, route: Route) -> Result<Vec<Fixture>, String> {
    let (expected_route, fixtures) = group(id).ok_or("unknown Db2 syntax fixture")?;
    if route != expected_route || fixtures.is_empty() || fixtures.len() > MAX_RESULTS {
        return Err("invalid Db2 syntax fixture route or count".into());
    }
    let mut ids = BTreeSet::new();
    for fixture in &fixtures {
        if fixture.source.len() > 1024 || !ids.insert(fixture.id) {
            return Err("invalid authored Db2 syntax fixture bounds or duplicate".into());
        }
    }
    Ok(fixtures)
}

fn execute(fixture: &FixtureRef, route: Route) -> Result<DriverOutput, String> {
    let fixtures = fixture_group(fixture.as_str(), route)?;
    let results = fixtures
        .iter()
        .map(|fixture| {
            Ok(ResultProjection {
                case_id: fixture.id.into(),
                actual: project(fixture, route)?,
            })
        })
        .collect::<Result<_, String>>()?;
    let projection = Projection {
        fixture_id: fixture.as_str().into(),
        route,
        results,
    };
    let bytes = serde_json::to_vec(&projection).map_err(|error| error.to_string())?;
    DriverOutput::new(bytes, output_limits()).map_err(|problem| problem.to_string())
}

fn component(identifier: &Db2Identifier, span: Db2SourceSpan) -> Component {
    Component {
        value: identifier.value().into(),
        delimited: identifier.is_delimited(),
        span: span.into(),
    }
}

fn name(
    parts: &[Db2Identifier],
    spans: &[Db2SourceSpan],
    span: Db2SourceSpan,
) -> Result<Name, String> {
    if parts.len() != spans.len() || parts.is_empty() || parts.len() > 3 {
        return Err("invalid product name projection".into());
    }
    Ok(Name {
        components: parts
            .iter()
            .zip(spans)
            .map(|(part, span)| component(part, *span))
            .collect(),
        span: span.into(),
    })
}

fn diagnostic(error: Db2SyntaxDiagnostic) -> Outcome {
    // The diagnostic API supplies no error byte span. Only its fixed enum code
    // and actual line/column enter the local observation, never message text.
    Outcome::Diagnostic {
        code: format!("{:?}", error.code),
        line: error.location.line,
        column: error.location.column,
    }
}

fn project(fixture: &Fixture, route: Route) -> Result<Outcome, String> {
    match route {
        Route::Drop => {
            match parse_db2_drop_statement(fixture.source, fixture.syntax, fixture.ast) {
                Ok(statement) => Ok(Outcome::Drop {
                    kind: match statement.object_kind() {
                        Db2DropObjectKind::Table => Kind::Table,
                        Db2DropObjectKind::View => Kind::View,
                        Db2DropObjectKind::Index => Kind::Index,
                        Db2DropObjectKind::Alias => Kind::Alias,
                    },
                    name: name(
                        statement.object_name().name().parts(),
                        statement.object_name().part_spans(),
                        statement.object_name().span(),
                    )?,
                    designator: match (
                        statement.alias_designator(),
                        statement.alias_designator_span(),
                    ) {
                        (None, None) => Designator::NotAlias,
                        (Some(Db2DropAliasDesignator::Unspecified), None) => Designator::Omitted,
                        (Some(Db2DropAliasDesignator::ForTable), Some(span)) => {
                            Designator::ForTable { span: span.into() }
                        }
                        _ => return Err("inconsistent product alias designator".into()),
                    },
                    span: statement.span().into(),
                }),
                Err(error) => Ok(diagnostic(error)),
            }
        }
        Route::Rename => {
            match parse_db2_rename_statement(fixture.source, fixture.syntax, fixture.ast) {
                Ok(statement) => Ok(Outcome::Rename {
                    kind: match statement.object_kind() {
                        Db2RenameObjectKind::Table => Kind::Table,
                        Db2RenameObjectKind::Index => Kind::Index,
                    },
                    source: name(
                        statement.source_name().parts(),
                        statement.source_part_spans(),
                        statement.source_span(),
                    )?,
                    destination: component(
                        statement.destination_identifier(),
                        statement.destination_span(),
                    ),
                    span: statement.span().into(),
                }),
                Err(error) => Ok(diagnostic(error)),
            }
        }
    }
}

fn output_limits() -> ConformanceLimits {
    ConformanceLimits {
        max_observation_bytes: MAX_OUTPUT_BYTES,
        ..ConformanceLimits::default()
    }
}

struct SyntaxObservation(Route, &'static str);

impl ConformanceObservation for SyntaxObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        // Apply the local bound even if another producer used looser DriverOutput limits.
        if output.bytes().len() > MAX_OUTPUT_BYTES {
            return Err("oversized Db2 syntax output".into());
        }
        let actual: Projection =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        if actual.fixture_id != self.1 {
            return Err("wrong Db2 syntax fixture identity".into());
        }
        let fixtures = fixture_group(&actual.fixture_id, self.0)?;
        if actual.route != self.0 || actual.results.len() != fixtures.len() {
            return Err("wrong Db2 syntax output route or result count".into());
        }
        let mut ids = BTreeSet::new();
        for result in &actual.results {
            if !ids.insert(&result.case_id) {
                return Err("duplicate Db2 syntax result".into());
            }
            let fixture = fixtures
                .iter()
                .find(|fixture| fixture.id == result.case_id)
                .ok_or("unknown Db2 syntax result")?;
            validate_outcome(&result.actual, fixture.source, self.0)?;
        }
        let expected = Projection {
            fixture_id: actual.fixture_id.clone(),
            route: self.0,
            results: fixtures
                .into_iter()
                .map(|fixture| ResultProjection {
                    case_id: fixture.id.into(),
                    actual: fixture.expected,
                })
                .collect(),
        };
        let matched = actual == expected;
        ObservationCheck::new(
            matched,
            serde_json::to_string(&expected).map_err(|error| error.to_string())?,
            serde_json::to_string(&actual).map_err(|error| error.to_string())?,
            output_limits(),
        )
        .map_err(|problem| problem.to_string())
    }
}

fn validate_span(span: Span, source: &str) -> Result<(), String> {
    if span.start_byte >= span.end_byte
        || span.end_byte > source.len()
        || !source.is_char_boundary(span.start_byte)
        || !source.is_char_boundary(span.end_byte)
        || span.start_line == 0
        || span.start_column == 0
        || span.end_line == 0
        || span.end_column == 0
        || (span.start_line, span.start_column) >= (span.end_line, span.end_column)
        || [
            span.start_line,
            span.start_column,
            span.end_line,
            span.end_column,
        ]
        .into_iter()
        .any(|value| value as usize > source.len() + 1)
    {
        return Err("invalid Db2 syntax span bounds".into());
    }
    Ok(())
}

fn validate_component(component: &Component, source: &str) -> Result<(), String> {
    if component.value.is_empty()
        || component.value.len() > MAX_IDENTIFIER_BYTES
        || component.value.contains('\0')
    {
        return Err("invalid Db2 syntax component bounds".into());
    }
    validate_span(component.span, source)
}

fn validate_name(name: &Name, kind: Kind, source: &str) -> Result<(), String> {
    let maximum = if kind == Kind::Index { 2 } else { 3 };
    if name.components.is_empty() || name.components.len() > maximum {
        return Err("invalid Db2 syntax qualification bounds".into());
    }
    validate_span(name.span, source)?;
    for component in &name.components {
        validate_component(component, source)?;
    }
    Ok(())
}

fn validate_outcome(outcome: &Outcome, source: &str, route: Route) -> Result<(), String> {
    match outcome {
        Outcome::Drop {
            kind,
            name,
            designator,
            span,
        } if route == Route::Drop => {
            validate_name(name, *kind, source)?;
            match (kind, designator) {
                (Kind::Alias, Designator::Omitted) => {}
                (Kind::Alias, Designator::ForTable { span }) => validate_span(*span, source)?,
                (Kind::Table | Kind::View | Kind::Index, Designator::NotAlias) => {}
                _ => return Err("invalid Db2 alias designator".into()),
            }
            validate_span(*span, source)
        }
        Outcome::Rename {
            kind,
            source: name,
            destination,
            span,
        } if route == Route::Rename && matches!(kind, Kind::Table | Kind::Index) => {
            validate_name(name, *kind, source)?;
            validate_component(destination, source)?;
            validate_span(*span, source)
        }
        Outcome::Diagnostic { code, line, column } => {
            const CODES: &[&str] = &[
                "EmptyStatement",
                "InvalidLimits",
                "StatementTooLarge",
                "TooManyTokens",
                "TokenTooLarge",
                "InvalidCharacter",
                "InvalidHostVariable",
                "UnsupportedToken",
                "UnbalancedDelimiter",
                "UnterminatedString",
                "UnterminatedComment",
                "InvalidHex",
                "UnsupportedNumericConstant",
                "UnsupportedStatement",
                "UnexpectedToken",
                "MissingToken",
                "DuplicateClause",
                "InvalidStatementOperand",
            ];
            if !CODES.contains(&code.as_str())
                || *line == 0
                || *column == 0
                || (*line as usize) > source.len() + 1
                || (*column as usize) > source.len() + 1
            {
                return Err("invalid Db2 syntax diagnostic".into());
            }
            Ok(())
        }
        _ => Err("wrong Db2 syntax outcome route or kind".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_coverage::{
        CONFORMANCE_SPEC_DOCUMENT_CONTRACT, CONFORMANCE_SPEC_VERSION_V1, ConformanceRunner,
        CoverageGate, OfficialCatalogRow, RunnerContext, RunnerSelection, RuntimeRegistry, Verdict,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    const GROUPS: &[(&str, Route)] = &[
        ("db2.syntax.drop.structure", Route::Drop),
        ("db2.syntax.rename.structure", Route::Rename),
        ("db2.syntax.drop.bounds", Route::Drop),
        ("db2.syntax.rename.bounds", Route::Rename),
        ("db2.syntax.drop.limitations", Route::Drop),
    ];

    fn output(id: &str, route: Route) -> DriverOutput {
        let fixture = FixtureRef::new(id, output_limits()).unwrap();
        match route {
            Route::Drop => DropDriver.execute(&fixture).unwrap(),
            Route::Rename => RenameDriver.execute(&fixture).unwrap(),
        }
    }

    fn mutant(value: &Value, route: Route) -> bool {
        let output =
            DriverOutput::new(serde_json::to_vec(value).unwrap(), output_limits()).unwrap();
        SyntaxObservation(
            route,
            if route == Route::Drop {
                GROUPS[0].0
            } else {
                GROUPS[1].0
            },
        )
        .evaluate(&output)
        .is_ok_and(|check| check.matched)
    }

    #[test]
    fn db2_syntax_authored_matrix_calls_real_public_parsers() {
        let mut cases = 0;
        for (id, route) in GROUPS {
            let output = output(id, *route);
            let check = SyntaxObservation(*route, id).evaluate(&output).unwrap();
            assert!(
                check.matched,
                "{id}\nexpected={}\nactual={}",
                check.expected, check.actual
            );
            let projection: Projection = serde_json::from_slice(output.bytes()).unwrap();
            cases += projection.results.len();
            // Actual outputs never contain expectations or a product match flag.
            let value: Value = serde_json::from_slice(output.bytes()).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 3);
            assert!(value.get("expected").is_none() && value.get("matched").is_none());
        }
        assert_eq!(cases, 73);
    }

    #[test]
    fn db2_syntax_unknown_fixture_and_cross_route_fail_closed() {
        for id in ["unknown", "DROP TABLE t", "{\"source\":\"DROP TABLE t\"}"] {
            let fixture = FixtureRef::new(id, output_limits());
            if let Ok(fixture) = fixture {
                assert!(DropDriver.execute(&fixture).is_err());
                assert!(RenameDriver.execute(&fixture).is_err());
            }
        }
        let drop = FixtureRef::new(GROUPS[0].0, output_limits()).unwrap();
        let rename = FixtureRef::new(GROUPS[1].0, output_limits()).unwrap();
        assert!(DropDriver.execute(&rename).is_err());
        assert!(RenameDriver.execute(&drop).is_err());
        assert!(
            SyntaxObservation(Route::Rename, GROUPS[1].0)
                .evaluate(&output(GROUPS[0].0, Route::Drop))
                .is_err()
        );
        // A complete, otherwise valid different fixture group must not prove
        // the original group's expectation on the same route.
        assert!(
            SyntaxObservation(Route::Drop, GROUPS[0].0)
                .evaluate(&output("db2.syntax.drop.bounds", Route::Drop))
                .is_err()
        );
        assert!(
            SyntaxObservation(Route::Rename, GROUPS[1].0)
                .evaluate(&output("db2.syntax.rename.bounds", Route::Rename))
                .is_err()
        );
    }

    #[test]
    fn db2_syntax_comparator_rejects_success_kind_name_designator_span_and_route_mutants() {
        let original: Value =
            serde_json::from_slice(output(GROUPS[0].0, Route::Drop).bytes()).unwrap();
        assert!(mutant(&original, Route::Drop));
        for (pointer, replacement) in [
            ("/results/0/actual", json!({"outcome":"success"})),
            ("/results/0/actual/kind", json!("View")),
            ("/results/0/actual/name/components/0/value", json!("WRONG")),
            ("/results/0/actual/name/components/0/delimited", json!(true)),
            ("/results/8/actual/designator", json!({"form":"NotAlias"})),
            ("/results/9/actual/designator", json!({"form":"Omitted"})),
            ("/results/9/actual/designator/span/start_byte", json!(16)),
            ("/results/0/actual/span/end_byte", json!(11)),
            ("/results/0/actual/span/end_column", json!(12)),
            ("/results/0/actual/name/span/start_column", json!(11)),
            (
                "/results/0/actual/name/components/0/span/end_column",
                json!(12),
            ),
            (
                "/results/14/actual/name/components/0/span/start_line",
                json!(1),
            ),
            ("/results/16/actual/code", json!("UnexpectedToken")),
            ("/results/16/actual/column", json!(10)),
            ("/route", json!("db2.syntax.rename")),
            ("/fixture_id", json!("unknown")),
            ("/results/0/case_id", json!("unknown")),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert!(!mutant(&value, Route::Drop), "surviving mutant {pointer}");
        }
        let original: Value =
            serde_json::from_slice(output(GROUPS[1].0, Route::Rename).bytes()).unwrap();
        for (pointer, replacement) in [
            ("/results/0/actual/kind", json!("Index")),
            ("/results/0/actual/source/components/0/value", json!("N")),
            ("/results/0/actual/destination/value", json!("T")),
            ("/results/0/actual/destination/delimited", json!(true)),
            ("/results/0/actual/destination/span/start_byte", json!(17)),
            ("/results/0/actual/destination/span/start_column", json!(18)),
            ("/results/0/actual/source/span/end_column", json!(14)),
            ("/results/0/actual/span/end_byte", json!(18)),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert!(!mutant(&value, Route::Rename), "surviving mutant {pointer}");
        }
    }

    #[test]
    fn db2_syntax_missing_duplicate_extra_results_and_field_mutants() {
        let bytes = output(GROUPS[0].0, Route::Drop);
        let original: Value = serde_json::from_slice(bytes.bytes()).unwrap();
        for pointer in [
            "",
            "/results/0",
            "/results/0/actual",
            "/results/0/actual/name",
            "/results/0/actual/name/components/0",
            "/results/0/actual/span",
            "/results/9/actual/designator",
            "/results/16/actual",
        ] {
            let fields = original.pointer(pointer).unwrap().as_object().unwrap();
            for key in fields.keys() {
                let mut missing = original.clone();
                missing
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(!mutant(&missing, Route::Drop), "missing {pointer}/{key}");
                // Encode a literal duplicate rather than a Value map, which
                // would erase duplicates before the DTO deserializer sees them.
                let field = serde_json::to_string(key).unwrap();
                let encoded = serde_json::to_string(fields).unwrap();
                let duplicate = format!("{{{field}:{},{rest}", fields[key], rest = &encoded[1..]);
                let mut carrier = original.clone();
                *carrier.pointer_mut(pointer).unwrap() = json!("duplicate-sentinel");
                let wire = serde_json::to_string(&carrier)
                    .unwrap()
                    .replace("\"duplicate-sentinel\"", &duplicate);
                let output = DriverOutput::new(
                    wire.into_bytes(),
                    ConformanceLimits {
                        max_observation_bytes: MAX_OUTPUT_BYTES * 2,
                        ..output_limits()
                    },
                )
                .unwrap();
                assert!(
                    SyntaxObservation(Route::Drop, GROUPS[0].0)
                        .evaluate(&output)
                        .is_err(),
                    "duplicate {pointer}/{key}"
                );
            }
            let mut unknown = original.clone();
            unknown
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), json!(0));
            assert!(!mutant(&unknown, Route::Drop), "unknown field {pointer}");
        }
        let mut missing = original.clone();
        missing["results"].as_array_mut().unwrap().remove(0);
        assert!(!mutant(&missing, Route::Drop));
        let mut duplicate = original.clone();
        duplicate["results"][1] = duplicate["results"][0].clone();
        assert!(!mutant(&duplicate, Route::Drop));
        let mut extra = original.clone();
        extra["results"]
            .as_array_mut()
            .unwrap()
            .push(original["results"][0].clone());
        assert!(!mutant(&extra, Route::Drop));
        let mut reordered = original.clone();
        reordered["results"].as_array_mut().unwrap().swap(0, 1);
        assert!(!mutant(&reordered, Route::Drop));
    }

    #[test]
    fn db2_syntax_malformed_encoding_and_nested_bounds_fail_before_admission() {
        for bytes in [
            vec![0xff, 0xfe],
            b"null".to_vec(),
            b"{}".to_vec(),
            b"[]".to_vec(),
            b"{\"route\":\"db2.syntax.drop\"".to_vec(),
        ] {
            let output = DriverOutput::new(bytes, output_limits()).unwrap();
            assert!(
                SyntaxObservation(Route::Drop, GROUPS[0].0)
                    .evaluate(&output)
                    .is_err()
            );
        }
        let mut bytes = output(GROUPS[0].0, Route::Drop).bytes().to_vec();
        bytes.push(b'!');
        assert!(
            SyntaxObservation(Route::Drop, GROUPS[0].0)
                .evaluate(&DriverOutput::new(bytes, output_limits()).unwrap())
                .is_err()
        );
        let mut value: Value =
            serde_json::from_slice(output(GROUPS[0].0, Route::Drop).bytes()).unwrap();
        for (pointer, replacement) in [
            (
                "/results/0/actual/name/components/0/value",
                json!("x".repeat(129)),
            ),
            ("/results/0/actual/name/components/0/value", json!("\0")),
            ("/results/0/actual/name/components", json!([])),
            ("/results/0/actual/span/end_byte", json!(usize::MAX)),
            ("/results/0/actual/span/start_line", json!(0)),
            ("/results/16/actual/code", json!("invented")),
            ("/results/16/actual/line", json!(u32::MAX)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(!mutant(&changed, Route::Drop), "unbounded {pointer}");
        }
        value["results"][0]["actual"]["name"]["components"] = json!([
            value["results"][0]["actual"]["name"]["components"][0].clone(),
            value["results"][0]["actual"]["name"]["components"][0].clone(),
            value["results"][0]["actual"]["name"]["components"][0].clone(),
            value["results"][0]["actual"]["name"]["components"][0].clone(),
        ]);
        assert!(!mutant(&value, Route::Drop));
        let wider = ConformanceLimits {
            max_observation_bytes: MAX_OUTPUT_BYTES + 1,
            ..output_limits()
        };
        let exact = DriverOutput::new(vec![b' '; MAX_OUTPUT_BYTES], wider).unwrap();
        assert!(
            SyntaxObservation(Route::Drop, GROUPS[0].0)
                .evaluate(&exact)
                .is_err()
        );
        let over = DriverOutput::new(vec![b' '; MAX_OUTPUT_BYTES + 1], wider).unwrap();
        assert_eq!(
            SyntaxObservation(Route::Drop, GROUPS[0].0)
                .evaluate(&over)
                .unwrap_err(),
            "oversized Db2 syntax output"
        );
        assert!(DriverOutput::new(vec![0; MAX_OUTPUT_BYTES + 1], output_limits()).is_err());
    }

    #[test]
    fn db2_syntax_duplicate_envelope_fields_are_rejected_within_byte_bound() {
        let id = "db2.syntax.drop.limitations";
        let original: Value = serde_json::from_slice(output(id, Route::Drop).bytes()).unwrap();
        for key in ["fixture_id", "route", "results"] {
            let encoded = serde_json::to_string(&original).unwrap();
            let wire = format!("{{\"{key}\":{},{}", original[key], &encoded[1..]);
            assert!(wire.len() < MAX_OUTPUT_BYTES);
            let output = DriverOutput::new(wire.into_bytes(), output_limits()).unwrap();
            assert!(
                SyntaxObservation(Route::Drop, id)
                    .evaluate(&output)
                    .is_err()
            );
        }
    }

    fn digest(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    // Synthetic ordinary test specification. No official Db2 row, reviewed
    // rule artifact, persisted verdict or ledger is created by these tests.
    fn test_document(routes: &[Route]) -> Value {
        let fixture_digest = digest(include_bytes!("db2_syntax_fixtures.rs"));
        let mut document = json!({
            "schema_version": CONFORMANCE_SPEC_DOCUMENT_CONTRACT,
            "spec_version": CONFORMANCE_SPEC_VERSION_V1,
            "catalog_digest": digest(b"ordinary-synthetic-catalog"), "shard_count": 1,
            "registries": {
                "operations": [], "input_shapes": [], "predicates": [],
                "transitions": [], "observations": [], "conditions": [],
                "recoveries": [], "oracles": [], "drivers": [], "fixtures": [],
                "scenario_steps": [], "failure_points": []
            },
            "rows": [], "obligations": [], "cases": [], "scenarios": []
        });
        for route in routes {
            let fixture_id = format!("{}.structure", route.driver());
            let row_id = format!("ordinary-test:{}", route.driver());
            for registry in ["operations", "input_shapes", "transitions", "drivers"] {
                document["registries"][registry]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(route.driver()));
            }
            document["registries"]["observations"]
                .as_array_mut()
                .unwrap()
                .push(json!(format!("{}.structure.exact", route.driver())));
            document["registries"]["fixtures"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":fixture_id, "digest":fixture_digest}));
            document["rows"].as_array_mut().unwrap().push(json!({
                "row_id":row_id, "operation":route.driver(), "input":route.driver(),
                "preconditions":[], "transition":route.driver(), "postconditions":[format!("{}.structure.exact", route.driver())],
                "conditions":[], "recovery":null, "oracle":null,
                "applicable_gates":["recognized"], "obligations":["adapter"]
            }));
            document["obligations"].as_array_mut().unwrap().push(json!({
                "row_id":row_id, "obligation_id":"adapter", "applicable_gates":["recognized"]
            }));
            document["cases"].as_array_mut().unwrap().push(json!({
                "spec_version":CONFORMANCE_SPEC_VERSION_V1, "row_id":row_id,
                "obligation_id":"adapter", "gate":"recognized", "test_id":route.driver(),
                "driver":route.driver(), "input":fixture_id, "preconditions":[],
                "expected":[format!("{}.structure.exact", route.driver())], "recovery":null, "oracle":null
            }));
        }
        document
    }

    fn compile(document: &Value, routes: &[Route]) -> CompiledSpec {
        let catalog = routes
            .iter()
            .map(|route| {
                OfficialCatalogRow::new(
                    format!("ordinary-test:{}", route.driver()),
                    "ordinary-test",
                    "syntax-adapter",
                    "synthetic-ordinary-test",
                    [CoverageGate::Recognized],
                    output_limits(),
                )
                .unwrap()
            })
            .collect();
        CompiledSpec::compile_json(
            &digest(b"ordinary-synthetic-catalog"),
            catalog,
            &serde_json::to_vec(document).unwrap(),
            output_limits(),
        )
        .unwrap()
    }

    #[test]
    fn db2_syntax_conditional_registry_closure_and_runner_use_same_adapters() {
        let runtime = db2_syntax_runtime();
        for routes in [
            &[][..],
            &[Route::Drop][..],
            &[Route::Rename][..],
            &[Route::Drop, Route::Rename][..],
        ] {
            let spec = compile(&test_document(routes), routes);
            let mut drivers = Vec::new();
            let mut observations = Vec::new();
            runtime
                .bind(&spec, &mut drivers, &mut observations, output_limits())
                .unwrap();
            assert_eq!(drivers.len(), routes.len());
            assert_eq!(observations.len(), routes.len());
            let registry =
                RuntimeRegistry::new(&spec, drivers, vec![], observations, output_limits())
                    .unwrap();
            let runner = ConformanceRunner::new(&spec, registry, output_limits());
            let context = RunnerContext::new(
                digest(b"ordinary-candidate"),
                "ordinary-test",
                output_limits(),
            )
            .unwrap();
            if routes.is_empty() {
                assert!(
                    runner
                        .run(
                            &RunnerSelection::local("db2", None, output_limits()).unwrap(),
                            &context
                        )
                        .is_err()
                );
            } else {
                let report = runner
                    .run(
                        &RunnerSelection::local("ordinary-test", None, output_limits()).unwrap(),
                        &context,
                    )
                    .unwrap();
                assert_eq!(
                    report
                        .batches
                        .iter()
                        .map(|batch| batch.events.len())
                        .sum::<usize>(),
                    routes.len()
                );
                assert!(
                    report
                        .batches
                        .iter()
                        .flat_map(|batch| &batch.events)
                        .all(|event| event.verdict == Verdict::Pass)
                );
            }
        }
        let spec = compile(&test_document(&[Route::Drop]), &[Route::Drop]);
        assert!(RuntimeRegistry::new(&spec, vec![], vec![], vec![], output_limits()).is_err());
        let mut drivers = Vec::new();
        let mut observations = Vec::new();
        runtime
            .bind(&spec, &mut drivers, &mut observations, output_limits())
            .unwrap();
        // Duplicate composition and surplus handlers are rejected by the shared owner.
        runtime
            .bind(&spec, &mut drivers, &mut observations, output_limits())
            .unwrap();
        assert!(
            RuntimeRegistry::new(&spec, drivers, vec![], observations, output_limits()).is_err()
        );
        let empty = compile(&test_document(&[]), &[]);
        assert!(
            RuntimeRegistry::new(
                &empty,
                vec![(
                    DriverRef::new(Route::Drop.driver(), output_limits()).unwrap(),
                    &DropDriver
                )],
                vec![],
                vec![],
                output_limits()
            )
            .is_err()
        );
    }

    #[test]
    fn db2_syntax_bad_compiled_references_fail_without_partial_installation() {
        let runtime = db2_syntax_runtime();
        for replacement in ["unknown", "db2.syntax.rename.structure"] {
            let mut document = test_document(&[Route::Drop]);
            document["registries"]["fixtures"][0]["id"] = json!(replacement);
            document["cases"][0]["input"] = json!(replacement);
            let spec = compile(&document, &[Route::Drop]);
            let mut drivers = Vec::new();
            let mut observations = Vec::new();
            assert!(
                runtime
                    .bind(&spec, &mut drivers, &mut observations, output_limits())
                    .is_err()
            );
            assert!(drivers.is_empty() && observations.is_empty());
        }
        for replacement in ["db2.syntax.unknown", "db2.syntax.drop.exact-wrong"] {
            let mut document = test_document(&[Route::Drop]);
            if replacement == "db2.syntax.unknown" {
                document["registries"]["drivers"][0] = json!(replacement);
                document["cases"][0]["driver"] = json!(replacement);
            } else {
                document["registries"]["observations"][0] = json!(replacement);
                document["rows"][0]["postconditions"][0] = json!(replacement);
                document["cases"][0]["expected"][0] = json!(replacement);
            }
            let spec = compile(&document, &[Route::Drop]);
            let mut drivers = Vec::new();
            let mut observations = Vec::new();
            assert!(
                runtime
                    .bind(&spec, &mut drivers, &mut observations, output_limits())
                    .is_err()
            );
            assert!(drivers.is_empty() && observations.is_empty());
        }
        let mut document = test_document(&[]);
        document["registries"]["drivers"] = json!([Route::Drop.driver()]);
        let spec = compile(&document, &[]);
        let mut drivers = Vec::new();
        let mut observations = Vec::new();
        runtime
            .bind(&spec, &mut drivers, &mut observations, output_limits())
            .unwrap();
        assert!(drivers.is_empty() && observations.is_empty());
        assert!(
            RuntimeRegistry::new(&spec, drivers, vec![], observations, output_limits()).is_err()
        );
    }

    #[test]
    fn db2_syntax_owned_output_and_unchanged_official_claim_boundary() {
        let owned = {
            let runtime = db2_syntax_runtime();
            let fixture = FixtureRef::new(GROUPS[0].0, output_limits()).unwrap();
            runtime.drop_driver.execute(&fixture).unwrap()
        };
        assert!(
            SyntaxObservation(Route::Drop, GROUPS[0].0)
                .evaluate(&owned)
                .unwrap()
                .matched
        );
        let official: Value =
            serde_json::from_str(include_str!("../../../../conformance/spec/v1/spec.json"))
                .unwrap();
        assert!(
            official["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| !row["row_id"].as_str().unwrap().contains("ibm-db2"))
        );
        assert!(
            official["registries"]["drivers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|id| !id.as_str().unwrap().starts_with("db2."))
        );
        let catalog: Value = serde_json::from_str(include_str!(
            "../../../../conformance/subsystems/coverage/catalogs/db2.json"
        ))
        .unwrap();
        assert_eq!(catalog["mandatory_rows"], 174);
        assert_eq!(catalog["units"][0]["denominator"], 158);
        assert_eq!(catalog["units"][1]["denominator"], 16);
        assert_eq!(
            catalog["units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| unit["rows"].as_array().unwrap().len())
                .sum::<usize>(),
            174
        );
    }
}
