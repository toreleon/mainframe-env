//! Public qualification contracts; these tests grant no statement-row credit.

use mainframe_env_db2::{
    Db2AstLimits, Db2DynamicQualificationContext, Db2Identifier, Db2QualificationContext,
    Db2QualificationErrorCode, Db2QualificationObject, Db2QualificationOrigin,
    Db2QualificationRequest, Db2QualificationStatus, Db2QualificationUse, Db2QualifiedName,
    Db2SourceLocation, Db2SourceSpan, Db2SynonymCheck, qualify_db2_name,
};

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
        end_byte: 24,
        start: Db2SourceLocation { line: 3, column: 5 },
        end: Db2SourceLocation { line: 3, column: 9 },
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

#[test]
fn public_candidates_own_names_and_preserve_original_spans() {
    let candidate = {
        let local = name(&["item"]);
        let context = Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Run {
            current_schema: Some(
                Db2Identifier::new("Mixed", true, Db2AstLimits::default()).unwrap(),
            ),
        });
        qualify_db2_name(request(&local, Some(&context)), Db2AstLimits::default()).unwrap()
    };
    assert_eq!(candidate.name().parts()[0].value(), "Mixed");
    assert!(candidate.name().parts()[0].is_delimited());
    assert_eq!(candidate.name().parts()[1].value(), "ITEM");
    assert_eq!(candidate.origin(), Db2QualificationOrigin::CurrentSchema);
    assert_eq!(
        candidate.status(),
        Db2QualificationStatus::PendingSynonymAndCatalogLookup
    );
    assert_eq!(candidate.span(), span());

    let explicit = name(&["schema", "item"]);
    let candidate = qualify_db2_name(request(&explicit, None), Db2AstLimits::default()).unwrap();
    assert_eq!(candidate.name(), &explicit);
    assert_eq!(candidate.origin(), Db2QualificationOrigin::Explicit);
    assert_eq!(
        candidate.status(),
        Db2QualificationStatus::PendingCatalogLookup
    );
}

#[test]
fn public_static_and_dynamic_contexts_select_the_declared_schema() {
    let cases = [
        (
            Db2QualificationContext::Static {
                qualifier: Some(identifier("qualifier")),
                owner: Some(identifier("owner")),
            },
            "QUALIFIER",
            Db2QualificationOrigin::StaticQualifier,
        ),
        (
            Db2QualificationContext::Static {
                qualifier: None,
                owner: Some(identifier("owner")),
            },
            "OWNER",
            Db2QualificationOrigin::StaticOwner,
        ),
        (
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::DefaultRun {
                current_schema: Some(identifier("current")),
            }),
            "CURRENT",
            Db2QualificationOrigin::CurrentSchema,
        ),
        (
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Bind {
                qualifier: None,
                owner: Some(identifier("owner")),
            }),
            "OWNER",
            Db2QualificationOrigin::BindOwner,
        ),
        (
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Define {
                routine_owner: Some(identifier("routine")),
            }),
            "ROUTINE",
            Db2QualificationOrigin::RoutineOwner,
        ),
        (
            Db2QualificationContext::Dynamic(Db2DynamicQualificationContext::Invoke {
                invoker: Some(identifier("invoker")),
            }),
            "INVOKER",
            Db2QualificationOrigin::Invoker,
        ),
    ];
    let local = name(&["item"]);
    for (context, schema, origin) in cases {
        let candidate =
            qualify_db2_name(request(&local, Some(&context)), Db2AstLimits::default()).unwrap();
        assert_eq!(candidate.name().parts()[0].value(), schema);
        assert_eq!(candidate.origin(), origin);
    }
}

#[test]
fn external_synonym_assertions_never_establish_object_existence() {
    let local = name(&["item"]);
    let context = Db2QualificationContext::Static {
        qualifier: Some(identifier("schema")),
        owner: None,
    };
    let mut input = request(&local, Some(&context));
    input.synonym_check = Db2SynonymCheck::CallerConfirmedAbsent;
    let candidate = qualify_db2_name(input, Db2AstLimits::default()).unwrap();
    assert_eq!(
        candidate.status(),
        Db2QualificationStatus::PendingCatalogLookup
    );
    input.synonym_check = Db2SynonymCheck::Pending;
    input.object = Db2QualificationObject::Index;
    let candidate = qualify_db2_name(input, Db2AstLimits::default()).unwrap();
    assert_eq!(
        candidate.status(),
        Db2QualificationStatus::PendingCatalogLookup
    );
}

#[test]
fn public_errors_keep_missing_context_and_unsupported_authorities_explicit() {
    let local = name(&["item"]);
    let context = Db2QualificationContext::Static {
        qualifier: Some(identifier("schema")),
        owner: None,
    };
    let mut input = request(&local, Some(&context));
    for (usage, expected) in [
        (
            Db2QualificationUse::ExplainOutput,
            Db2QualificationErrorCode::UnsupportedExplainContext,
        ),
        (
            Db2QualificationUse::CatalogResolution,
            Db2QualificationErrorCode::UnsupportedCatalogResolution,
        ),
        (
            Db2QualificationUse::AuthorizationResolution,
            Db2QualificationErrorCode::UnsupportedAuthorizationResolution,
        ),
    ] {
        input.usage = usage;
        let problem = qualify_db2_name(input, Db2AstLimits::default()).unwrap_err();
        assert_eq!(problem.code, expected);
        assert_eq!(problem.span, span());
    }
    let problem = qualify_db2_name(request(&local, None), Db2AstLimits::default()).unwrap_err();
    assert_eq!(problem.code, Db2QualificationErrorCode::MissingContext);
    let limits = Db2AstLimits {
        max_name_parts: 1,
        ..Db2AstLimits::default()
    };
    let problem = qualify_db2_name(request(&local, Some(&context)), limits).unwrap_err();
    assert_eq!(problem.code, Db2QualificationErrorCode::TooManyNameParts);
}
