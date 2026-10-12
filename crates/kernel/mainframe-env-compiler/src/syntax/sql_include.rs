//! SQL INCLUDE precompile selection and missing-member diagnostics.

use super::{SyntaxProblem, find_sql_include};
use mainframe_env_source::SourceBundle;
use std::fmt;

pub(super) fn should_precompile(
    bundle: &SourceBundle,
    source: &str,
) -> Result<bool, SyntaxProblem> {
    bundle.options().get("cobol.sql-precompile").map_or_else(
        || find_sql_include(source, 0).map(|include| include.is_some()),
        |value| Ok(value == "true"),
    )
}

pub(super) fn format_problem(
    problem: &SyntaxProblem,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match problem {
        SyntaxProblem::PrecompilerIncludeNotFound(name) => {
            write!(
                formatter,
                "EXEC SQL INCLUDE member {name} was not found in any source library"
            )?;
            if matches!(name.as_str(), "SQLCA" | "SQLDA") {
                write!(
                    formatter,
                    "; automatic precompiler definitions for {name} are unavailable; supply a library member"
                )?;
            }
            Ok(())
        }
        _ => write!(formatter, "COBOL syntax failed: {problem:?}"),
    }
}
