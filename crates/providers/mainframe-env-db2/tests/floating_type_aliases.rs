//! SQL0050 declaration aliases and existing consumers; no floating value or row credit.

use mainframe_env_db2::{
    Db2ArithmeticContext, Db2ArithmeticOperand, Db2ArithmeticRangeCheck,
    Db2AssignmentCompatibility, Db2AssignmentNullability, Db2AstErrorCode, Db2AstLimits,
    Db2BinaryOperator, Db2BuiltInDataType, Db2BuiltInType, Db2CaseElse, Db2ComparisonCompatibility,
    Db2ConversionKind, Db2DataType, Db2DecimalArithmetic, Db2Nullability, Db2ResolvedType,
    Db2ResultCombinationContext, Db2ResultCombinationErrorCode, Db2ResultCombinationLimits,
    Db2ResultTypeOperand, Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2TypeAttributes, Db2TypeErrorCode, classify_db2_assignment,
    classify_db2_comparison, combine_db2_result_types, parse_db2_create_table_statement,
    resolve_db2_binary_arithmetic, resolve_db2_type, resolve_db2_type_with_attributes,
};

fn syntax(kind: Db2BuiltInType, arguments: &[u32], zone: bool) -> Db2DataType {
    Db2DataType::BuiltIn(
        Db2BuiltInDataType::new(kind, arguments.to_vec(), zone, Default::default()).unwrap(),
    )
}

fn ty(kind: Db2BuiltInType, arguments: &[u32], nullable: Db2Nullability) -> Db2ResolvedType {
    resolve_db2_type(&syntax(kind, arguments, false), nullable).unwrap()
}

fn span(start: usize, end: usize) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: start,
        end_byte: end,
        start: Db2SourceLocation {
            line: 1,
            column: start as u32 + 1,
        },
        end: Db2SourceLocation {
            line: 1,
            column: end as u32 + 1,
        },
    }
}

#[test]
fn every_float_precision_has_the_source_defined_canonical_shape() {
    // SQL0050, db2z_sql_createtable.html (104cc7fd...), lines 220..227.
    // Fixed ranges are independently authored from that declaration rule.
    for (precisions, expected, canonical) in [
        (1..=21, Db2ScalarType::Real, Db2BuiltInType::Real),
        (22..=53, Db2ScalarType::Double, Db2BuiltInType::Double),
    ] {
        for precision in precisions {
            for nullability in [Db2Nullability::NotNull, Db2Nullability::Nullable] {
                let input = syntax(Db2BuiltInType::Float, &[precision], false);
                let original = input.clone();
                let resolved = resolve_db2_type(&input, nullability).unwrap();
                assert_eq!(resolved.scalar(), &expected, "FLOAT({precision})");
                assert_eq!(resolved.nullability(), nullability);
                assert_eq!(resolved, ty(canonical, &[], nullability));
                assert_eq!(input, original);
                let Db2DataType::BuiltIn(input) = input else {
                    unreachable!()
                };
                assert_eq!(input.kind(), Db2BuiltInType::Float);
                assert_eq!(input.arguments(), &[precision]);
            }
        }
    }
    for nullability in [Db2Nullability::NotNull, Db2Nullability::Nullable] {
        let default = ty(Db2BuiltInType::Float, &[], nullability);
        assert_eq!(default.scalar(), &Db2ScalarType::Double);
        assert_eq!(default.nullability(), nullability);
        assert_eq!(default, ty(Db2BuiltInType::Float, &[53], nullability));
        assert_eq!(default, ty(Db2BuiltInType::Double, &[], nullability));
    }
    // The public variant remains available, without a public resolved-type constructor.
    assert_ne!(Db2ScalarType::Float { precision: 21 }, Db2ScalarType::Real);
}

#[test]
fn invalid_float_arguments_zones_and_attributes_still_fail_closed() {
    for nullability in [Db2Nullability::NotNull, Db2Nullability::Nullable] {
        for arguments in [&[0][..], &[54], &[u32::MAX]] {
            assert_eq!(
                resolve_db2_type(
                    &syntax(Db2BuiltInType::Float, arguments, false),
                    nullability
                )
                .unwrap_err()
                .code,
                Db2TypeErrorCode::InvalidPrecision,
            );
        }
        assert_eq!(
            resolve_db2_type(
                &syntax(Db2BuiltInType::Float, &[21, 22], false),
                nullability
            )
            .unwrap_err()
            .code,
            Db2TypeErrorCode::InvalidArguments,
        );
        for arguments in [&[][..], &[1], &[21], &[22], &[53]] {
            assert_eq!(
                resolve_db2_type(&syntax(Db2BuiltInType::Float, arguments, true), nullability)
                    .unwrap_err()
                    .code,
                Db2TypeErrorCode::InvalidTimeZone,
            );
            for (attributes, expected) in [
                (
                    Db2TypeAttributes::new(true, false),
                    Db2TypeErrorCode::UnsupportedCcsid,
                ),
                (
                    Db2TypeAttributes::new(false, true),
                    Db2TypeErrorCode::UnsupportedCollation,
                ),
                (
                    Db2TypeAttributes::new(true, true),
                    Db2TypeErrorCode::UnsupportedCcsid,
                ),
            ] {
                assert_eq!(
                    resolve_db2_type_with_attributes(
                        &syntax(Db2BuiltInType::Float, arguments, false),
                        nullability,
                        attributes,
                    )
                    .unwrap_err()
                    .code,
                    expected,
                );
            }
        }
    }
    assert_eq!(
        Db2BuiltInDataType::new(
            Db2BuiltInType::Float,
            vec![1, 2, 3],
            false,
            Default::default()
        )
        .unwrap_err()
        .code,
        Db2AstErrorCode::InvalidDataType,
    );
}

#[test]
fn parser_preserves_float_syntax_ordinary_shapes_and_owned_locations() {
    let (statement, resolved) = {
        let source = String::from(
            "/*é*/\r\nCREATE TABLE t (a FLOAT(21) NOT NULL, b FLOAT(22), c FLOAT, d REAL, e DOUBLE PRECISION, f INTEGER, g DECIMAL(9,2), h CHAR(3), i TIMESTAMP(12))",
        );
        let statement =
            parse_db2_create_table_statement(&source, Default::default(), Default::default())
                .unwrap();
        assert_eq!(
            statement.columns()[0].span(),
            Db2SourceSpan {
                start_byte: 24,
                end_byte: 44,
                start: Db2SourceLocation {
                    line: 2,
                    column: 17
                },
                end: Db2SourceLocation {
                    line: 2,
                    column: 37
                },
            }
        );
        assert_eq!(&source[24..44], "a FLOAT(21) NOT NULL");
        let resolved = statement
            .columns()
            .iter()
            .map(|column| {
                resolve_db2_type(
                    column.data_type(),
                    if column.is_not_null() {
                        Db2Nullability::NotNull
                    } else {
                        Db2Nullability::Nullable
                    },
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        (statement, resolved)
    };
    assert_eq!(statement.table_name().parts()[0].value(), "T");
    for (index, args) in [vec![21], vec![22], vec![]].iter().enumerate() {
        let Db2DataType::BuiltIn(input) = statement.columns()[index].data_type() else {
            unreachable!()
        };
        assert_eq!(input.kind(), Db2BuiltInType::Float);
        assert_eq!(input.arguments(), args);
    }
    let expected = [
        Db2ScalarType::Real,
        Db2ScalarType::Double,
        Db2ScalarType::Double,
        Db2ScalarType::Real,
        Db2ScalarType::Double,
        Db2ScalarType::Integer,
        Db2ScalarType::Decimal {
            precision: 9,
            scale: 2,
        },
        Db2ScalarType::Character { length: 3 },
        Db2ScalarType::Timestamp {
            precision: 12,
            time_zone: mainframe_env_db2::Db2TimeZone::WithoutTimeZone,
        },
    ];
    for (index, expected) in expected.iter().enumerate() {
        assert_eq!(resolved[index].scalar(), expected);
        assert_eq!(
            resolved[index].nullability(),
            if index == 0 {
                Db2Nullability::NotNull
            } else {
                Db2Nullability::Nullable
            }
        );
    }
    assert_eq!(resolved[0].scalar(), &Db2ScalarType::Real);
    assert_eq!(statement.columns()[0].span().start_byte, 24);
}

#[test]
fn parser_negative_forms_and_exact_resource_bounds_remain_enforced() {
    for input in [
        "FLOAT()",
        "FLOAT(-1)",
        "FLOAT(1,)",
        "FLOAT(1,2)",
        "FLOAT(1,2,3)",
        "FLOAT(4294967296)",
        "FLOAT(1.5)",
        "FLOAT(1E1)",
        "FLOAT(21) WITH TIME ZONE",
        "FLOAT(21) WITHOUT TIME ZONE",
        "FLOAT(21) CCSID 1208",
        "FLOAT(21) COLLATE x",
    ] {
        let sql = format!("CREATE TABLE t (a {input})");
        assert!(
            parse_db2_create_table_statement(&sql, Default::default(), Default::default()).is_err(),
            "{input}"
        );
    }
    // The AST can represent these forms; only the type resolver establishes validity.
    for (input, code) in [
        ("FLOAT(0)", Db2TypeErrorCode::InvalidPrecision),
        ("FLOAT(54)", Db2TypeErrorCode::InvalidPrecision),
    ] {
        let sql = format!("CREATE TABLE t (a {input})");
        let parsed =
            parse_db2_create_table_statement(&sql, Default::default(), Default::default()).unwrap();
        assert_eq!(
            resolve_db2_type(parsed.columns()[0].data_type(), Db2Nullability::Nullable)
                .unwrap_err()
                .code,
            code
        );
    }
    let sql = "CREATE TABLE t (a FLOAT(21))";
    let exact = Db2SyntaxLimits {
        max_statement_bytes: sql.len(),
        ..Default::default()
    };
    assert!(parse_db2_create_table_statement(sql, exact, Default::default()).is_ok());
    let short = Db2SyntaxLimits {
        max_statement_bytes: sql.len() - 1,
        ..exact
    };
    assert_eq!(
        parse_db2_create_table_statement(sql, short, Default::default())
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::StatementTooLarge
    );
    let ceiling = Db2SyntaxLimits {
        max_statement_bytes: 8 * 1024 * 1024,
        ..exact
    };
    assert!(parse_db2_create_table_statement(sql, ceiling, Default::default()).is_ok());
    let beyond = Db2SyntaxLimits {
        max_statement_bytes: 8 * 1024 * 1024 + 1,
        ..exact
    };
    assert_eq!(
        parse_db2_create_table_statement(sql, beyond, Default::default())
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::InvalidLimits
    );
    let ast = Db2AstLimits {
        max_list_items: 1,
        ..Default::default()
    };
    assert!(Db2BuiltInDataType::new(Db2BuiltInType::Float, vec![53], false, ast).is_ok());
    assert_eq!(
        Db2BuiltInDataType::new(Db2BuiltInType::Float, vec![53, 1], false, ast)
            .unwrap_err()
            .code,
        Db2AstErrorCode::InvalidDataType
    );
}

#[test]
fn canonical_alias_consumers_keep_identity_and_nullability_obligations() {
    for (precision, canonical) in [
        (1, Db2BuiltInType::Real),
        (21, Db2BuiltInType::Real),
        (22, Db2BuiltInType::Double),
        (53, Db2BuiltInType::Double),
    ] {
        for (source_null, target_null, expected_assignment, expected_comparison) in [
            (
                Db2Nullability::NotNull,
                Db2Nullability::NotNull,
                Db2AssignmentNullability::Safe,
                Db2Nullability::NotNull,
            ),
            (
                Db2Nullability::NotNull,
                Db2Nullability::Nullable,
                Db2AssignmentNullability::Safe,
                Db2Nullability::Nullable,
            ),
            (
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
                Db2AssignmentNullability::RequiresRuntimeNullCheck,
                Db2Nullability::Nullable,
            ),
            (
                Db2Nullability::Nullable,
                Db2Nullability::Nullable,
                Db2AssignmentNullability::Safe,
                Db2Nullability::Nullable,
            ),
        ] {
            let alias = ty(Db2BuiltInType::Float, &[precision], source_null);
            let target = ty(canonical, &[], target_null);
            assert_eq!(
                classify_db2_assignment(&alias, &target),
                Db2AssignmentCompatibility::Compatible {
                    conversion: Db2ConversionKind::Identity,
                    nullability: expected_assignment
                }
            );
            assert_eq!(
                classify_db2_comparison(&alias, &target),
                Db2ComparisonCompatibility::Compatible {
                    conversion: Db2ConversionKind::Identity,
                    result_nullability: expected_comparison
                }
            );
        }
    }
    let real = ty(Db2BuiltInType::Float, &[21], Db2Nullability::NotNull);
    let double = ty(Db2BuiltInType::Float, &[22], Db2Nullability::NotNull);
    assert_eq!(
        classify_db2_comparison(&real, &double),
        Db2ComparisonCompatibility::Compatible {
            conversion: Db2ConversionKind::Numeric,
            result_nullability: Db2Nullability::NotNull
        }
    );
}

#[test]
fn arithmetic_is_double_metadata_while_result_combination_uses_its_own_table() {
    let real = ty(Db2BuiltInType::Float, &[21], Db2Nullability::NotNull);
    let nullable_real = ty(Db2BuiltInType::Float, &[1], Db2Nullability::Nullable);
    let double = ty(Db2BuiltInType::Float, &[22], Db2Nullability::NotNull);
    let integer = ty(Db2BuiltInType::Integer, &[], Db2Nullability::NotNull);
    for (left, right, expected, nullability) in [
        (&real, &real, Db2ScalarType::Real, Db2Nullability::NotNull),
        (
            &real,
            &nullable_real,
            Db2ScalarType::Real,
            Db2Nullability::Nullable,
        ),
        (
            &real,
            &integer,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &integer,
            &real,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &double,
            &real,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &real,
            &double,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &double,
            &double,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &double,
            &integer,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
        (
            &integer,
            &double,
            Db2ScalarType::Double,
            Db2Nullability::NotNull,
        ),
    ] {
        let operands = [
            Db2ResultTypeOperand::Typed {
                resolved_type: left,
                span: span(0, 1),
            },
            Db2ResultTypeOperand::Typed {
                resolved_type: right,
                span: span(2, 3),
            },
        ];
        let result = combine_db2_result_types(
            "a,b",
            span(0, 3),
            &operands,
            Some(Db2ResultCombinationContext::OperandRules),
            Default::default(),
        )
        .unwrap();
        assert_eq!(result.resolved_type().scalar(), &expected);
        assert_eq!(result.resolved_type().nullability(), nullability);
        assert_eq!(result.steps()[0].right_operand_index, 1);
        assert_eq!(result.operands()[0].resolved_type.as_ref(), Some(left));
        for operator in [
            Db2BinaryOperator::Add,
            Db2BinaryOperator::Subtract,
            Db2BinaryOperator::Multiply,
            Db2BinaryOperator::Divide,
        ] {
            let a = Db2ArithmeticOperand::resolved(left.clone(), span(0, 1)).unwrap();
            let b = Db2ArithmeticOperand::resolved(right.clone(), span(2, 3)).unwrap();
            let context =
                Db2ArithmeticContext::new(Db2DecimalArithmetic::Dec31, 0, span(0, 3)).unwrap();
            let result =
                resolve_db2_binary_arithmetic(operator, &a, &b, context, span(0, 3)).unwrap();
            assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::Double);
            assert_eq!(result.resolved_type().nullability(), nullability);
            assert_eq!(
                result.obligations().range,
                Db2ArithmeticRangeCheck::FloatingPointResult
            );
        }
    }
}

#[test]
fn ordered_combination_owns_canonical_inputs_and_keeps_null_context_explicit() {
    let result = {
        let source = String::from("a,b,c,d");
        let real = ty(Db2BuiltInType::Float, &[21], Db2Nullability::NotNull);
        let integer = ty(Db2BuiltInType::Integer, &[], Db2Nullability::NotNull);
        let operands = [
            Db2ResultTypeOperand::Typed {
                resolved_type: &real,
                span: span(0, 1),
            },
            Db2ResultTypeOperand::UntypedNull { span: span(2, 3) },
            Db2ResultTypeOperand::Typed {
                resolved_type: &integer,
                span: span(4, 5),
            },
            Db2ResultTypeOperand::Typed {
                resolved_type: &real,
                span: span(6, 7),
            },
        ];
        assert_eq!(
            combine_db2_result_types(&source, span(0, 7), &operands, None, Default::default())
                .unwrap_err()
                .code,
            Db2ResultCombinationErrorCode::MissingApplicationContext
        );
        let limits = Db2ResultCombinationLimits {
            max_source_bytes: 7,
            max_operands: 4,
        };
        let result = combine_db2_result_types(
            &source,
            span(0, 7),
            &operands,
            Some(Db2ResultCombinationContext::OperandRules),
            limits,
        )
        .unwrap();
        assert_eq!(
            combine_db2_result_types(
                &source,
                span(0, 7),
                &operands,
                Some(Db2ResultCombinationContext::OperandRules),
                Db2ResultCombinationLimits {
                    max_operands: 3,
                    ..limits
                }
            )
            .unwrap_err()
            .code,
            Db2ResultCombinationErrorCode::TooManyOperands
        );
        result
    };
    assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::Double);
    assert_eq!(
        result.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
    assert_eq!(result.span(), span(0, 7));
    assert_eq!(
        result
            .steps()
            .iter()
            .map(|step| step.right_operand_index)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        result.operands()[0]
            .resolved_type
            .as_ref()
            .unwrap()
            .scalar(),
        &Db2ScalarType::Real
    );
    assert_eq!(result.operands()[1].resolved_type, None);
    assert_eq!(result.operands()[1].obligation.conversion, None);
    assert_eq!(
        result.operands()[0].obligation.conversion,
        Some(Db2ConversionKind::Numeric)
    );
    assert!(result.operands()[0].obligation.whole_part_must_be_preserved);
}

fn combine_owned_alias_context(
    precision: u32,
    nullabilities: &[Option<Db2Nullability>],
    context: Db2ResultCombinationContext,
) -> mainframe_env_db2::Db2CombinedResultType {
    // All source, resolved inputs and borrowed operands die on return. The
    // caller's assertions exercise the combination's owned metadata afterward.
    let source = String::from(match nullabilities.len() {
        1 => "a",
        2 => "a,b",
        3 => "a,b,c",
        _ => unreachable!(),
    });
    let types = nullabilities
        .iter()
        .map(|nullable| nullable.map(|n| ty(Db2BuiltInType::Float, &[precision], n)))
        .collect::<Vec<_>>();
    let operands = types
        .iter()
        .enumerate()
        .map(|(index, resolved)| match resolved {
            Some(resolved_type) => Db2ResultTypeOperand::Typed {
                resolved_type,
                span: span(index * 2, index * 2 + 1),
            },
            None => Db2ResultTypeOperand::UntypedNull {
                span: span(index * 2, index * 2 + 1),
            },
        })
        .collect::<Vec<_>>();
    combine_db2_result_types(
        &source,
        span(0, source.len()),
        &operands,
        Some(context),
        Default::default(),
    )
    .unwrap()
}

#[test]
fn canonical_float_case_omitted_and_present_else_have_fixed_nullability() {
    use Db2Nullability::{NotNull, Nullable};
    use Db2ResultCombinationContext::Case;
    // CASE pin fac861b7..., lines 10..18 and 40..49; result-rules pin
    // 7c23258e..., lines 25..36 and Table 1. These are authored expectations,
    // not CASE evaluation or product-derived nullability.
    for (precision, expected) in [(21, Db2ScalarType::Real), (22, Db2ScalarType::Double)] {
        for (inputs, else_clause, expected_null, expected_steps) in [
            (vec![Some(NotNull)], Db2CaseElse::Omitted, Nullable, vec![]),
            (
                vec![Some(NotNull), Some(NotNull)],
                Db2CaseElse::Omitted,
                Nullable,
                vec![1],
            ),
            (
                vec![Some(NotNull), Some(NotNull)],
                Db2CaseElse::Present,
                NotNull,
                vec![1],
            ),
            (
                vec![Some(Nullable), Some(NotNull)],
                Db2CaseElse::Present,
                Nullable,
                vec![1],
            ),
            (
                vec![Some(NotNull), None],
                Db2CaseElse::Present,
                Nullable,
                vec![],
            ),
            (
                vec![None, Some(NotNull), None],
                Db2CaseElse::Present,
                Nullable,
                vec![],
            ),
        ] {
            let context = Case { else_clause };
            let result = combine_owned_alias_context(precision, &inputs, context);
            assert_eq!(result.resolved_type().scalar(), &expected);
            assert_eq!(result.resolved_type().nullability(), expected_null);
            assert_eq!(result.context(), context);
            assert_eq!(result.span(), span(0, inputs.len() * 2 - 1));
            assert_eq!(
                result
                    .steps()
                    .iter()
                    .map(|s| s.right_operand_index)
                    .collect::<Vec<_>>(),
                expected_steps
            );
            for (index, input_null) in inputs.iter().enumerate() {
                let operand = &result.operands()[index];
                assert_eq!(operand.span, span(index * 2, index * 2 + 1));
                match input_null {
                    Some(nullable) => {
                        let input = operand.resolved_type.as_ref().unwrap();
                        assert_eq!(input.scalar(), &expected);
                        assert_eq!(input.nullability(), *nullable);
                        assert_eq!(
                            operand.obligation.conversion,
                            Some(Db2ConversionKind::Identity)
                        );
                    }
                    None => {
                        assert_eq!(operand.resolved_type, None);
                        assert_eq!(operand.obligation.conversion, None);
                    }
                }
                assert!(!operand.obligation.whole_part_must_be_preserved);
            }
        }
    }
}

#[test]
fn canonical_float_coalesce_nullability_and_order_survive_input_scope() {
    use Db2Nullability::{NotNull, Nullable};
    // COALESCE pin 6f6c80fa..., lines 10..23: nullable only if every
    // argument can be null; candidate types follow supplied argument order.
    for (precision, expected) in [(21, Db2ScalarType::Real), (22, Db2ScalarType::Double)] {
        for (inputs, expected_null, expected_steps) in [
            (vec![None, Some(NotNull)], NotNull, vec![]),
            (vec![Some(NotNull), None], NotNull, vec![]),
            (vec![None, Some(Nullable)], Nullable, vec![]),
            (vec![Some(Nullable), None], Nullable, vec![]),
            (vec![None, Some(Nullable), Some(NotNull)], NotNull, vec![2]),
            (vec![Some(NotNull), None, Some(Nullable)], NotNull, vec![2]),
            (
                vec![Some(Nullable), None, Some(Nullable)],
                Nullable,
                vec![2],
            ),
        ] {
            let result = combine_owned_alias_context(
                precision,
                &inputs,
                Db2ResultCombinationContext::Coalesce,
            );
            assert_eq!(result.resolved_type().scalar(), &expected);
            assert_eq!(result.resolved_type().nullability(), expected_null);
            assert_eq!(result.context(), Db2ResultCombinationContext::Coalesce);
            assert_eq!(result.span(), span(0, inputs.len() * 2 - 1));
            assert_eq!(
                result
                    .steps()
                    .iter()
                    .map(|s| s.right_operand_index)
                    .collect::<Vec<_>>(),
                expected_steps
            );
            for (index, nullable) in inputs.iter().enumerate() {
                let operand = &result.operands()[index];
                assert_eq!(operand.span, span(index * 2, index * 2 + 1));
                if let Some(nullable) = nullable {
                    let input = operand.resolved_type.as_ref().unwrap();
                    assert_eq!(input.scalar(), &expected);
                    assert_eq!(input.nullability(), *nullable);
                    assert_eq!(
                        operand.obligation.conversion,
                        Some(Db2ConversionKind::Identity)
                    );
                } else {
                    assert_eq!(operand.resolved_type, None);
                    assert_eq!(operand.obligation.conversion, None);
                }
                assert!(!operand.obligation.whole_part_must_be_preserved);
            }
        }
    }
}

#[test]
fn canonical_alias_case_coalesce_context_and_all_null_fences_remain_mandatory() {
    use Db2ResultCombinationContext::{Case, Coalesce};
    for precision in [21, 22] {
        let alias = ty(Db2BuiltInType::Float, &[precision], Db2Nullability::NotNull);
        let single = [Db2ResultTypeOperand::Typed {
            resolved_type: &alias,
            span: span(0, 1),
        }];
        for context in [
            Coalesce,
            Case {
                else_clause: Db2CaseElse::Present,
            },
        ] {
            let error = combine_db2_result_types(
                "a",
                span(0, 1),
                &single,
                Some(context),
                Default::default(),
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2ResultCombinationErrorCode::InvalidApplicationContext
            );
            assert_eq!(error.span, span(0, 0));
        }
        let ordered = [
            single[0],
            Db2ResultTypeOperand::UntypedNull { span: span(2, 3) },
        ];
        assert_eq!(
            combine_db2_result_types("a,b", span(0, 3), &ordered, None, Default::default())
                .unwrap_err()
                .code,
            Db2ResultCombinationErrorCode::MissingApplicationContext
        );
        for context in [
            Coalesce,
            Case {
                else_clause: Db2CaseElse::Present,
            },
            Case {
                else_clause: Db2CaseElse::Omitted,
            },
        ] {
            let all_null = [
                Db2ResultTypeOperand::UntypedNull { span: span(0, 1) },
                Db2ResultTypeOperand::UntypedNull { span: span(2, 3) },
            ];
            let error = combine_db2_result_types(
                "a,b",
                span(0, 3),
                &all_null,
                Some(context),
                Default::default(),
            )
            .unwrap_err();
            assert_eq!(error.code, Db2ResultCombinationErrorCode::AllUntypedNull);
            assert_eq!(error.span, span(0, 3));
            let reversed = [ordered[1], ordered[0]];
            let error = combine_db2_result_types(
                "a,b",
                span(0, 3),
                &reversed,
                Some(context),
                Default::default(),
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2ResultCombinationErrorCode::InvalidOperandOrder
            );
            assert_eq!(error.span, span(0, 1));
        }
    }
}
