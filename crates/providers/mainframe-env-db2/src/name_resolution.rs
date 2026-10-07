//! Owned common Db2 schema-qualification rules.
//!
//! Source: ibm-db2-for-zos-13-2026-08-13, db2z_resolutionofobjnames.html,
//! SHA-256 a1e2b49f72cd742e3d5a4866417ba9acf30dccca8091d0b48a1483ec50019f8d.
//! This is a candidate kernel, disconnected from binding and execution. No
//! result establishes object existence, synonym absence, or authorization.

use crate::{Db2AstLimits, Db2Identifier, Db2QualifiedName, Db2SourceSpan};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2QualificationObject {
    Alias,
    Index,
    Table,
    View,
    Type,
    Function,
    Procedure,
    GlobalVariable,
    SpecificName,
    Synonym,
}

/// The caller supplies the applicable context; this kernel does not derive
/// DYNAMICRULES behavior from bind options or the runtime environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2QualificationContext {
    /// `owner` is the applicable plan, package, or native SQL procedure owner.
    Static {
        qualifier: Option<Db2Identifier>,
        owner: Option<Db2Identifier>,
    },
    Dynamic(Db2DynamicQualificationContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2DynamicQualificationContext {
    /// Omitted DYNAMICRULES behavior defaults to RUN, with the same required
    /// CURRENT SCHEMA value as explicit RUN.
    DefaultRun {
        current_schema: Option<Db2Identifier>,
    },
    Run {
        current_schema: Option<Db2Identifier>,
    },
    Bind {
        qualifier: Option<Db2Identifier>,
        owner: Option<Db2Identifier>,
    },
    Define {
        routine_owner: Option<Db2Identifier>,
    },
    Invoke {
        invoker: Option<Db2Identifier>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2QualificationUse {
    Ordinary,
    /// EXPLAIN output has special qualification rules. The caller identifies
    /// that context explicitly, rather than dispatching on a production name.
    ExplainOutput,
    CatalogResolution,
    AuthorizationResolution,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SynonymCheck {
    Pending,
    /// An external catalog authority has checked this name for the current
    /// user. This is a caller assertion, not evidence produced by this kernel.
    CallerConfirmedAbsent,
}

#[derive(Clone, Copy, Debug)]
pub struct Db2QualificationRequest<'a> {
    pub name: &'a Db2QualifiedName,
    pub object: Db2QualificationObject,
    pub context: Option<&'a Db2QualificationContext>,
    pub usage: Db2QualificationUse,
    pub synonym_check: Db2SynonymCheck,
    /// Original name span; generated qualification does not invent source bytes.
    pub span: Db2SourceSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2QualificationOrigin {
    Explicit,
    StaticQualifier,
    StaticOwner,
    CurrentSchema,
    BindQualifier,
    BindOwner,
    RoutineOwner,
    Invoker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2QualificationStatus {
    PendingCatalogLookup,
    /// The candidate is conditional on the current user's synonym lookup
    /// finding no synonym. It must not be used to bypass that lookup.
    PendingSynonymAndCatalogLookup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2QualificationCandidate {
    name: Db2QualifiedName,
    origin: Db2QualificationOrigin,
    status: Db2QualificationStatus,
    span: Db2SourceSpan,
}

impl Db2QualificationCandidate {
    /// A schema-qualified candidate, never a resolved catalog object identity.
    #[must_use]
    pub const fn name(&self) -> &Db2QualifiedName {
        &self.name
    }

    #[must_use]
    pub const fn origin(&self) -> Db2QualificationOrigin {
        self.origin
    }

    #[must_use]
    pub const fn status(&self) -> Db2QualificationStatus {
        self.status
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2QualificationErrorCode {
    InvalidLimits,
    IdentifierTooLong,
    TooManyNameParts,
    MissingContext,
    MissingQualifierOrOwner,
    MissingCurrentSchema,
    MissingRoutineOwner,
    MissingInvoker,
    UnsupportedSqlPathObject,
    UnsupportedSynonymObject,
    UnsupportedExplainContext,
    UnsupportedCatalogResolution,
    UnsupportedAuthorizationResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2QualificationError {
    pub code: Db2QualificationErrorCode,
    pub span: Db2SourceSpan,
    /// Fixed, bounded diagnostic text. No caller identifier is echoed.
    pub message: &'static str,
}

impl fmt::Display for Db2QualificationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2QualificationError {}

/// Qualify alias/index/table/view candidates without performing any lookup.
/// Qualified names retain every part, delimiter flag, and the original span.
/// Context identifiers and names are rechecked under the caller's AST limits,
/// even if their AST constructors used larger limits. Expressions and statement
/// lists are not inputs to this kernel; their aggregate limits remain parser-owned.
pub fn qualify_db2_name(
    request: Db2QualificationRequest<'_>,
    limits: Db2AstLimits,
) -> Result<Db2QualificationCandidate, Db2QualificationError> {
    use Db2QualificationErrorCode as Error;
    let fail = |code, message| Db2QualificationError {
        code,
        span: request.span,
        message,
    };
    limits
        .validate()
        .map_err(|_| fail(Error::InvalidLimits, "invalid Db2 AST qualification limits"))?;
    match request.usage {
        Db2QualificationUse::Ordinary => {}
        Db2QualificationUse::ExplainOutput => {
            return Err(fail(
                Error::UnsupportedExplainContext,
                "EXPLAIN output qualification requires a later typed context",
            ));
        }
        Db2QualificationUse::CatalogResolution => {
            return Err(fail(
                Error::UnsupportedCatalogResolution,
                "catalog object resolution is outside the qualification kernel",
            ));
        }
        Db2QualificationUse::AuthorizationResolution => {
            return Err(fail(
                Error::UnsupportedAuthorizationResolution,
                "authorization is outside the qualification kernel",
            ));
        }
    }
    match request.object {
        Db2QualificationObject::Alias
        | Db2QualificationObject::Index
        | Db2QualificationObject::Table
        | Db2QualificationObject::View => {}
        Db2QualificationObject::Synonym => {
            return Err(fail(
                Error::UnsupportedSynonymObject,
                "synonym resolution requires the current user's catalog authority",
            ));
        }
        _ => {
            return Err(fail(
                Error::UnsupportedSqlPathObject,
                "type, function, procedure, variable, and specific-name rules are unsupported",
            ));
        }
    }
    if request.name.parts().len() > limits.max_name_parts {
        return Err(fail(
            Error::TooManyNameParts,
            "name exceeds the AST part limit",
        ));
    }
    for identifier in request.name.parts() {
        check_identifier(identifier, limits, request.span)?;
    }
    // Check all supplied fields, including an owner shadowed by QUALIFIER.
    if let Some(context) = request.context {
        let fields = match context {
            Db2QualificationContext::Static { qualifier, owner }
            | Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Bind {
                qualifier,
                owner,
            }) => [qualifier.as_ref(), owner.as_ref()],
            Db2QualificationContext::Dynamic(
                Db2DynamicQualificationContext::DefaultRun { current_schema }
                | Db2DynamicQualificationContext::Run { current_schema },
            ) => [current_schema.as_ref(), None],
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Define {
                routine_owner,
            }) => [routine_owner.as_ref(), None],
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Invoke {
                invoker,
            }) => [invoker.as_ref(), None],
        };
        for identifier in fields.into_iter().flatten() {
            check_identifier(identifier, limits, request.span)?;
        }
    }
    if request.name.parts().len() > 1 {
        return Ok(Db2QualificationCandidate {
            name: request.name.clone(),
            origin: Db2QualificationOrigin::Explicit,
            status: Db2QualificationStatus::PendingCatalogLookup,
            span: request.span,
        });
    }
    let context = request.context.ok_or_else(|| {
        fail(
            Error::MissingContext,
            "unqualified name requires a static or dynamic context",
        )
    })?;
    let (schema, origin) =
        default_schema(context).map_err(|(code, message)| fail(code, message))?;
    if limits.max_name_parts < 2 {
        return Err(fail(
            Error::TooManyNameParts,
            "generated schema qualification exceeds the AST part limit",
        ));
    }
    let name = Db2QualifiedName::new(
        vec![schema.clone(), request.name.parts()[0].clone()],
        limits,
    )
    .map_err(|_| {
        fail(
            Error::TooManyNameParts,
            "invalid qualified candidate part count",
        )
    })?;
    let status = if request.object != Db2QualificationObject::Index
        && request.synonym_check == Db2SynonymCheck::Pending
    {
        Db2QualificationStatus::PendingSynonymAndCatalogLookup
    } else {
        Db2QualificationStatus::PendingCatalogLookup
    };
    Ok(Db2QualificationCandidate {
        name,
        origin,
        status,
        span: request.span,
    })
}

fn check_identifier(
    identifier: &Db2Identifier,
    limits: Db2AstLimits,
    span: Db2SourceSpan,
) -> Result<(), Db2QualificationError> {
    if identifier.value().len() > limits.max_identifier_bytes {
        return Err(Db2QualificationError {
            code: Db2QualificationErrorCode::IdentifierTooLong,
            span,
            message: "name or context identifier exceeds the AST UTF-8 byte limit",
        });
    }
    Ok(())
}

fn default_schema(
    context: &Db2QualificationContext,
) -> Result<(&Db2Identifier, Db2QualificationOrigin), (Db2QualificationErrorCode, &'static str)> {
    use Db2QualificationErrorCode as Error;
    use Db2QualificationOrigin as Origin;
    let (identifier, origin, missing, message) = match context {
        Db2QualificationContext::Static { qualifier, owner } => (
            qualifier.as_ref().or(owner.as_ref()),
            if qualifier.is_some() {
                Origin::StaticQualifier
            } else {
                Origin::StaticOwner
            },
            Error::MissingQualifierOrOwner,
            "static qualification requires QUALIFIER or the applicable owner",
        ),
        Db2QualificationContext::Dynamic(dynamic) => match dynamic {
            Db2DynamicQualificationContext::DefaultRun { current_schema }
            | Db2DynamicQualificationContext::Run { current_schema } => (
                current_schema.as_ref(),
                Origin::CurrentSchema,
                Error::MissingCurrentSchema,
                "dynamic RUN qualification requires CURRENT SCHEMA",
            ),
            Db2DynamicQualificationContext::Bind { qualifier, owner } => (
                qualifier.as_ref().or(owner.as_ref()),
                if qualifier.is_some() {
                    Origin::BindQualifier
                } else {
                    Origin::BindOwner
                },
                Error::MissingQualifierOrOwner,
                "dynamic BIND qualification requires QUALIFIER or the applicable owner",
            ),
            Db2DynamicQualificationContext::Define { routine_owner } => (
                routine_owner.as_ref(),
                Origin::RoutineOwner,
                Error::MissingRoutineOwner,
                "dynamic DEFINE qualification requires the routine owner",
            ),
            Db2DynamicQualificationContext::Invoke { invoker } => (
                invoker.as_ref(),
                Origin::Invoker,
                Error::MissingInvoker,
                "dynamic INVOKE qualification requires the invoker",
            ),
        },
    };
    identifier
        .map(|identifier| (identifier, origin))
        .ok_or((missing, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Db2SourceLocation, Db2SyntaxLimits, parse_db2_select_core};

    const OBJECTS: [Db2QualificationObject; 4] = [
        Db2QualificationObject::Alias,
        Db2QualificationObject::Index,
        Db2QualificationObject::Table,
        Db2QualificationObject::View,
    ];

    fn identifier(value: &str) -> Db2Identifier {
        Db2Identifier::new(value, false, Db2AstLimits::default()).unwrap()
    }

    fn name(parts: &[&str]) -> Db2QualifiedName {
        Db2QualifiedName::new(
            parts.iter().map(|part| identifier(part)).collect(),
            Db2AstLimits::default(),
        )
        .unwrap()
    }

    fn span() -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: 20,
            end_byte: 21,
            start: Db2SourceLocation { line: 3, column: 5 },
            end: Db2SourceLocation { line: 3, column: 6 },
        }
    }

    fn request<'a>(
        name: &'a Db2QualifiedName,
        context: Option<&'a Db2QualificationContext>,
    ) -> Db2QualificationRequest<'a> {
        Db2QualificationRequest {
            name,
            object: Db2QualificationObject::Table,
            context,
            usage: Db2QualificationUse::Ordinary,
            synonym_check: Db2SynonymCheck::Pending,
            span: span(),
        }
    }

    fn values(candidate: &Db2QualificationCandidate) -> Vec<&str> {
        candidate
            .name()
            .parts()
            .iter()
            .map(Db2Identifier::value)
            .collect()
    }

    fn contexts() -> Vec<(
        Db2QualificationContext,
        &'static str,
        Db2QualificationOrigin,
    )> {
        use Db2DynamicQualificationContext as Dynamic;
        use Db2QualificationContext as Context;
        use Db2QualificationOrigin as Origin;
        vec![
            (
                Context::Static {
                    qualifier: Some(identifier("q")),
                    owner: Some(identifier("o")),
                },
                "Q",
                Origin::StaticQualifier,
            ),
            (
                Context::Static {
                    qualifier: Some(identifier("q")),
                    owner: None,
                },
                "Q",
                Origin::StaticQualifier,
            ),
            (
                Context::Static {
                    qualifier: None,
                    owner: Some(identifier("o")),
                },
                "O",
                Origin::StaticOwner,
            ),
            (
                Context::Dynamic(Dynamic::DefaultRun {
                    current_schema: Some(identifier("run")),
                }),
                "RUN",
                Origin::CurrentSchema,
            ),
            (
                Context::Dynamic(Dynamic::Run {
                    current_schema: Some(identifier("run")),
                }),
                "RUN",
                Origin::CurrentSchema,
            ),
            (
                Context::Dynamic(Dynamic::Bind {
                    qualifier: Some(identifier("b")),
                    owner: Some(identifier("o")),
                }),
                "B",
                Origin::BindQualifier,
            ),
            (
                Context::Dynamic(Dynamic::Bind {
                    qualifier: Some(identifier("b")),
                    owner: None,
                }),
                "B",
                Origin::BindQualifier,
            ),
            (
                Context::Dynamic(Dynamic::Bind {
                    qualifier: None,
                    owner: Some(identifier("o")),
                }),
                "O",
                Origin::BindOwner,
            ),
            (
                Context::Dynamic(Dynamic::Define {
                    routine_owner: Some(identifier("definer")),
                }),
                "DEFINER",
                Origin::RoutineOwner,
            ),
            (
                Context::Dynamic(Dynamic::Invoke {
                    invoker: Some(identifier("caller")),
                }),
                "CALLER",
                Origin::Invoker,
            ),
        ]
    }

    #[test]
    fn static_dynamic_and_default_matrix_keeps_synonym_and_catalog_dependencies() {
        let name = name(&["t"]);
        for (context, expected, origin) in contexts() {
            for object in OBJECTS {
                for synonym_check in [
                    Db2SynonymCheck::Pending,
                    Db2SynonymCheck::CallerConfirmedAbsent,
                ] {
                    let candidate = qualify_db2_name(
                        Db2QualificationRequest {
                            object,
                            synonym_check,
                            ..request(&name, Some(&context))
                        },
                        Db2AstLimits::default(),
                    )
                    .unwrap();
                    assert_eq!(
                        values(&candidate),
                        [expected, "T"],
                        "{context:?} {object:?}"
                    );
                    assert_eq!(candidate.origin(), origin);
                    assert_eq!(candidate.span(), span());
                    assert_eq!(
                        candidate.status(),
                        if object == Db2QualificationObject::Index
                            || synonym_check == Db2SynonymCheck::CallerConfirmedAbsent
                        {
                            Db2QualificationStatus::PendingCatalogLookup
                        } else {
                            Db2QualificationStatus::PendingSynonymAndCatalogLookup
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn qualified_names_preserve_parts_spelling_flags_and_span_without_default_context() {
        let delimited = Db2Identifier::new("MiXeD ", true, Db2AstLimits::default()).unwrap();
        for parts in [
            vec![delimited.clone(), identifier("t")],
            vec![identifier("loc"), delimited, identifier("t")],
        ] {
            let name = Db2QualifiedName::new(parts, Db2AstLimits::default()).unwrap();
            for object in OBJECTS {
                let candidate = qualify_db2_name(
                    Db2QualificationRequest {
                        object,
                        ..request(&name, None)
                    },
                    Db2AstLimits::default(),
                )
                .unwrap();
                assert_eq!(candidate.name(), &name);
                assert_eq!(candidate.origin(), Db2QualificationOrigin::Explicit);
                assert_eq!(
                    candidate.status(),
                    Db2QualificationStatus::PendingCatalogLookup
                );
                assert_eq!(candidate.span(), span());
            }
            for (context, _, _) in contexts() {
                assert_eq!(
                    qualify_db2_name(request(&name, Some(&context)), Db2AstLimits::default())
                        .unwrap()
                        .name(),
                    &name
                );
            }
            let missing_owner = Db2QualificationContext::Static {
                qualifier: None,
                owner: None,
            };
            assert_eq!(
                qualify_db2_name(
                    request(&name, Some(&missing_owner)),
                    Db2AstLimits::default()
                )
                .unwrap()
                .name(),
                &name
            );
        }
    }

    #[test]
    fn every_missing_context_field_fails_for_all_supported_objects() {
        use Db2DynamicQualificationContext as Dynamic;
        use Db2QualificationContext as Context;
        use Db2QualificationErrorCode as Error;
        let cases = [
            (None, Error::MissingContext),
            (
                Some(Context::Static {
                    qualifier: None,
                    owner: None,
                }),
                Error::MissingQualifierOrOwner,
            ),
            (
                Some(Context::Dynamic(Dynamic::DefaultRun {
                    current_schema: None,
                })),
                Error::MissingCurrentSchema,
            ),
            (
                Some(Context::Dynamic(Dynamic::Run {
                    current_schema: None,
                })),
                Error::MissingCurrentSchema,
            ),
            (
                Some(Context::Dynamic(Dynamic::Bind {
                    qualifier: None,
                    owner: None,
                })),
                Error::MissingQualifierOrOwner,
            ),
            (
                Some(Context::Dynamic(Dynamic::Define {
                    routine_owner: None,
                })),
                Error::MissingRoutineOwner,
            ),
            (
                Some(Context::Dynamic(Dynamic::Invoke { invoker: None })),
                Error::MissingInvoker,
            ),
        ];
        let name = name(&["t"]);
        for (context, expected) in cases {
            for object in OBJECTS {
                let error = qualify_db2_name(
                    Db2QualificationRequest {
                        object,
                        ..request(&name, context.as_ref())
                    },
                    Db2AstLimits::default(),
                )
                .unwrap_err();
                assert_eq!(error.code, expected);
                assert_eq!(error.span, span());
                assert!(error.message.len() <= 256);
                assert!(error.to_string().contains("3:5"));
            }
        }
    }

    #[test]
    fn sql_path_and_synonym_objects_fail_even_when_already_qualified() {
        for object in [
            Db2QualificationObject::Type,
            Db2QualificationObject::Function,
            Db2QualificationObject::Procedure,
            Db2QualificationObject::GlobalVariable,
            Db2QualificationObject::SpecificName,
            Db2QualificationObject::Synonym,
        ] {
            for name in [name(&["t"]), name(&["s", "t"])] {
                let error = qualify_db2_name(
                    Db2QualificationRequest {
                        object,
                        ..request(&name, None)
                    },
                    Db2AstLimits::default(),
                )
                .unwrap_err();
                assert_eq!(
                    error.code,
                    if object == Db2QualificationObject::Synonym {
                        Db2QualificationErrorCode::UnsupportedSynonymObject
                    } else {
                        Db2QualificationErrorCode::UnsupportedSqlPathObject
                    }
                );
            }
        }
    }

    #[test]
    fn explain_catalog_and_authorization_requests_fail_explicitly() {
        use Db2QualificationErrorCode as Error;
        for (usage, expected) in [
            (
                Db2QualificationUse::ExplainOutput,
                Error::UnsupportedExplainContext,
            ),
            (
                Db2QualificationUse::CatalogResolution,
                Error::UnsupportedCatalogResolution,
            ),
            (
                Db2QualificationUse::AuthorizationResolution,
                Error::UnsupportedAuthorizationResolution,
            ),
        ] {
            for (context, _, _) in contexts() {
                for object in OBJECTS {
                    for name in [name(&["t"]), name(&["s", "t"])] {
                        assert_eq!(
                            qualify_db2_name(
                                Db2QualificationRequest {
                                    usage,
                                    object,
                                    ..request(&name, Some(&context))
                                },
                                Db2AstLimits::default()
                            )
                            .unwrap_err()
                            .code,
                            expected
                        );
                    }
                }
            }
        }
        // Ordinary production names do not select the EXPLAIN exception.
        let context = Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Bind {
            qualifier: Some(identifier("q")),
            owner: None,
        });
        for value in [
            "PLAN_TABLE",
            "DSN_STATEMNT_TABLE",
            "DSN_FUNCTION_TABLE",
            "OTHER",
        ] {
            let name = name(&[value]);
            assert_eq!(
                values(
                    &qualify_db2_name(request(&name, Some(&context)), Db2AstLimits::default())
                        .unwrap()
                ),
                ["Q", value]
            );
        }
    }

    #[test]
    fn ast_limits_revalidate_name_parts_and_generated_qualification() {
        let context = Db2QualificationContext::Static {
            qualifier: Some(identifier("q")),
            owner: None,
        };
        let one_part = Db2AstLimits {
            max_name_parts: 1,
            ..Db2AstLimits::default()
        };
        for name in [name(&["t"]), name(&["s", "t"])] {
            assert_eq!(
                qualify_db2_name(request(&name, Some(&context)), one_part)
                    .unwrap_err()
                    .code,
                Db2QualificationErrorCode::TooManyNameParts
            );
        }
        let name = name(&["t"]);
        let two_parts = Db2AstLimits {
            max_name_parts: 2,
            ..Db2AstLimits::default()
        };
        assert!(qualify_db2_name(request(&name, Some(&context)), two_parts).is_ok());
        for limits in [
            Db2AstLimits {
                max_identifier_bytes: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_name_parts: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_list_items: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_expression_nodes: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_expression_depth: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_literal_bytes: 0,
                ..two_parts
            },
            Db2AstLimits {
                max_identifier_bytes: 1025,
                ..two_parts
            },
        ] {
            assert_eq!(
                qualify_db2_name(request(&name, Some(&context)), limits)
                    .unwrap_err()
                    .code,
                Db2QualificationErrorCode::InvalidLimits
            );
        }
    }

    #[test]
    fn context_identifier_byte_bounds_cover_all_fields_including_shadowed_owner() {
        use Db2DynamicQualificationContext as Dynamic;
        use Db2QualificationContext as Context;
        let limits = Db2AstLimits {
            max_identifier_bytes: 2,
            ..Db2AstLimits::default()
        };
        let wide = Db2Identifier::new("éé", true, Db2AstLimits::default()).unwrap();
        let contexts = [
            Context::Static {
                qualifier: Some(wide.clone()),
                owner: None,
            },
            Context::Static {
                qualifier: Some(identifier("q")),
                owner: Some(wide.clone()),
            },
            Context::Dynamic(Dynamic::DefaultRun {
                current_schema: Some(wide.clone()),
            }),
            Context::Dynamic(Dynamic::Run {
                current_schema: Some(wide.clone()),
            }),
            Context::Dynamic(Dynamic::Bind {
                qualifier: Some(wide.clone()),
                owner: None,
            }),
            Context::Dynamic(Dynamic::Bind {
                qualifier: Some(identifier("q")),
                owner: Some(wide.clone()),
            }),
            Context::Dynamic(Dynamic::Define {
                routine_owner: Some(wide.clone()),
            }),
            Context::Dynamic(Dynamic::Invoke {
                invoker: Some(wide),
            }),
        ];
        for context in contexts {
            for name in [name(&["t"]), name(&["s", "t"])] {
                assert_eq!(
                    qualify_db2_name(request(&name, Some(&context)), limits)
                        .unwrap_err()
                        .code,
                    Db2QualificationErrorCode::IdentifierTooLong
                );
            }
        }
        let context = Context::Static {
            qualifier: Some(Db2Identifier::new("é", true, Db2AstLimits::default()).unwrap()),
            owner: None,
        };
        let name = name(&["t"]);
        assert_eq!(
            values(&qualify_db2_name(request(&name, Some(&context)), limits).unwrap()),
            ["é", "T"]
        );
        let long_name =
            Db2QualifiedName::new(vec![identifier("long")], Db2AstLimits::default()).unwrap();
        assert_eq!(
            qualify_db2_name(request(&long_name, Some(&context)), limits)
                .unwrap_err()
                .code,
            Db2QualificationErrorCode::IdentifierTooLong
        );
    }

    #[test]
    fn identifier_equivalence_uses_effective_values_and_preserves_delimited_case() {
        let mut candidates = Vec::new();
        for (schema, table, delimited) in [("a", "b", false), ("A  ", "B ", true), ("a", "b", true)]
        {
            let name = Db2QualifiedName::new(
                vec![Db2Identifier::new(table, delimited, Db2AstLimits::default()).unwrap()],
                Db2AstLimits::default(),
            )
            .unwrap();
            let context = Db2QualificationContext::Static {
                qualifier: Some(
                    Db2Identifier::new(schema, delimited, Db2AstLimits::default()).unwrap(),
                ),
                owner: None,
            };
            candidates.push(
                qualify_db2_name(request(&name, Some(&context)), Db2AstLimits::default()).unwrap(),
            );
        }
        assert_eq!(values(&candidates[0]), values(&candidates[1]));
        assert_ne!(values(&candidates[0]), values(&candidates[2]));
        assert!(!candidates[0].name().parts()[0].is_delimited());
        assert!(candidates[1].name().parts()[0].is_delimited());
    }

    #[test]
    fn existing_parser_comments_and_relocation_preserve_candidate_and_error_spans() {
        let context = Db2QualificationContext::Static {
            qualifier: Some(identifier("q")),
            owner: None,
        };
        let mut candidates = Vec::new();
        for source in [
            "SELECT 1 FROM t",
            "-- lead\n SELECT /* nested /* x */ */ 1\nFROM\t t ; -- end",
        ] {
            let parsed =
                parse_db2_select_core(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
                    .unwrap();
            let table = &parsed.sources()[0];
            let request = Db2QualificationRequest {
                span: table.span(),
                ..request(table.name(), Some(&context))
            };
            let candidate = qualify_db2_name(request, Db2AstLimits::default()).unwrap();
            assert_eq!(candidate.span(), table.span());
            assert_eq!(
                &source[candidate.span().start_byte..candidate.span().end_byte],
                "t"
            );
            let error = qualify_db2_name(
                Db2QualificationRequest {
                    context: None,
                    ..request
                },
                Db2AstLimits::default(),
            )
            .unwrap_err();
            assert_eq!(error.span, table.span());
            candidates.push(candidate);
        }
        assert_eq!(values(&candidates[0]), values(&candidates[1]));
        assert_eq!(
            candidates[1].span().start,
            Db2SourceLocation { line: 3, column: 7 }
        );
        assert_ne!(
            candidates[0].span().start_byte,
            candidates[1].span().start_byte
        );
        for source in [
            "SELECT 1 FROM t; SELECT 2 FROM t",
            "SELECT 1 FROM t;;",
            "SELECT 1 FROM t; x",
        ] {
            assert!(
                parse_db2_select_core(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
                    .is_err()
            );
        }
    }
}
