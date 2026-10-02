//! Pure public type metadata: no evaluator, syntax or statement-row credit.

use mainframe_env_db2::{
    Db2ArithmeticConstantError, Db2ArithmeticConstantLimits, Db2ArithmeticContext,
    Db2ArithmeticErrorCode, Db2ArithmeticOperand, Db2ArithmeticRangeCheck, Db2ArithmeticSide,
    Db2ArithmeticZeroCheck, Db2AstLimits, Db2BinaryOperator, Db2BuiltInDataType, Db2BuiltInType,
    Db2CaseElse, Db2DataType, Db2DecimalArithmetic, Db2Nullability, Db2ResolvedType,
    Db2ResultCombinationContext, Db2ResultCombinationErrorCode, Db2ResultCombinationLimits,
    Db2ResultTypeOperand, Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2UnaryOperator,
    classify_db2_arithmetic_constants, combine_db2_result_types, resolve_db2_binary_arithmetic,
    resolve_db2_type, resolve_db2_unary_arithmetic,
};

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

fn ty(kind: Db2BuiltInType, args: &[u32], nullability: Db2Nullability) -> Db2ResolvedType {
    resolve_db2_type(
        &Db2DataType::BuiltIn(
            Db2BuiltInDataType::new(kind, args.to_vec(), false, Db2AstLimits::default()).unwrap(),
        ),
        nullability,
    )
    .unwrap()
}

fn context(mode: Db2DecimalArithmetic, minimum: u32) -> Db2ArithmeticContext {
    Db2ArithmeticContext::new(mode, minimum, span(0, 3)).unwrap()
}

#[test]
fn arithmetic_constant_provenance_is_verified_not_inferred_from_a_type() {
    let source = "00001,1.0";
    let constants =
        classify_db2_arithmetic_constants(source, &[span(0, 5), span(6, 9)], Default::default())
            .unwrap();
    assert_eq!(constants[0].integer_constant_digits(), Some(5));
    let ordinary =
        Db2ArithmeticOperand::resolved(constants[0].resolved_type().clone(), constants[0].span())
            .unwrap();
    assert_eq!(ordinary.integer_constant_digits(), None);
    let add = |left: &Db2ArithmeticOperand| {
        resolve_db2_binary_arithmetic(
            Db2BinaryOperator::Add,
            left,
            &constants[1],
            context(Db2DecimalArithmetic::Dec31, 0),
            span(0, source.len()),
        )
        .unwrap()
    };
    let result = add(&constants[0]);
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 7,
            scale: 1
        }
    );
    assert_eq!(
        add(&ordinary).resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 13,
            scale: 1
        }
    );
    assert_eq!(result.into_operand().integer_constant_digits(), None);
    let limits = Db2ArithmeticConstantLimits {
        max_operands: 1,
        ..Default::default()
    };
    assert!(matches!(
        classify_db2_arithmetic_constants(source, &[span(0, 5), span(6, 9)], limits),
        Err(Db2ArithmeticConstantError::Arithmetic(error)) if error.code == Db2ArithmeticErrorCode::OperandLimit
    ));
    let mut wrong = span(0, 5);
    wrong.end.column += 1;
    assert!(matches!(
        classify_db2_arithmetic_constants(source, &[wrong], Default::default()),
        Err(Db2ArithmeticConstantError::Constant(_))
    ));
}

#[test]
fn arithmetic_retains_runtime_checks_and_explicit_context() {
    let left = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::Decimal, &[20, 8], Db2Nullability::Nullable),
        span(0, 1),
    )
    .unwrap();
    let right = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::Decimal, &[20, 3], Db2Nullability::NotNull),
        span(2, 3),
    )
    .unwrap();
    let result = resolve_db2_binary_arithmetic(
        Db2BinaryOperator::Multiply,
        &left,
        &right,
        context(Db2DecimalArithmetic::Dec15, 0),
        span(0, 3),
    )
    .unwrap();
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 31,
            scale: 8
        }
    );
    assert_eq!(
        result.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
    let obligations = result.obligations();
    assert_eq!(obligations.range, Db2ArithmeticRangeCheck::DecimalResult);
    let truncation = obligations.truncation.unwrap();
    assert_eq!(truncation.operand, Db2ArithmeticSide::Right);
    assert_eq!(truncation.temporary_precision, 15);
    assert_eq!(truncation.temporary_scale, 0);
    assert!(truncation.sqlwarn7_if_nonzero_digits_removed);
    assert_eq!(
        obligations
            .multiplication
            .unwrap()
            .leading_zeros_must_exceed,
        15
    );
    let left = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::Decimal, &[31, 0], Db2Nullability::NotNull),
        span(0, 1),
    )
    .unwrap();
    let right = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::Decimal, &[15, 15], Db2Nullability::NotNull),
        span(2, 3),
    )
    .unwrap();
    assert_eq!(
        resolve_db2_binary_arithmetic(
            Db2BinaryOperator::Divide,
            &left,
            &right,
            context(Db2DecimalArithmetic::Dec31, 0),
            span(0, 3),
        )
        .unwrap_err()
        .code,
        Db2ArithmeticErrorCode::NegativeDivisionScale
    );
    let result = resolve_db2_binary_arithmetic(
        Db2BinaryOperator::Divide,
        &left,
        &right,
        context(Db2DecimalArithmetic::Dec31, 4),
        span(0, 3),
    )
    .unwrap();
    assert_eq!(result.calculated_division_scale(), Some(-31));
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 31,
            scale: 4
        }
    );
    assert_eq!(
        result.obligations().zero,
        Db2ArithmeticZeroCheck::DivisorMustBeNonzero
    );
    assert_eq!(
        Db2ArithmeticContext::new(Db2DecimalArithmetic::Dec31, 10, span(0, 3))
            .unwrap_err()
            .code,
        Db2ArithmeticErrorCode::InvalidContext
    );
}

#[test]
fn unary_promotion_and_unresolved_decfloat_conversion_remain_explicit() {
    let operand = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::SmallInt, &[], Db2Nullability::NotNull),
        span(1, 2),
    )
    .unwrap();
    let result =
        resolve_db2_unary_arithmetic(Db2UnaryOperator::Negative, &operand, span(0, 2)).unwrap();
    assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::Integer);
    assert_eq!(result.span(), span(0, 2));
    let left = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::DecFloat, &[16], Db2Nullability::NotNull),
        span(0, 1),
    )
    .unwrap();
    let right = Db2ArithmeticOperand::resolved(
        ty(Db2BuiltInType::Decimal, &[8, 2], Db2Nullability::NotNull),
        span(2, 3),
    )
    .unwrap();
    assert_eq!(
        resolve_db2_binary_arithmetic(
            Db2BinaryOperator::Add,
            &left,
            &right,
            context(Db2DecimalArithmetic::Dec31, 0),
            span(0, 3),
        )
        .unwrap_err()
        .code,
        Db2ArithmeticErrorCode::UnresolvedDecFloatConversion
    );
}

#[test]
fn result_context_preserves_untyped_null_case_and_coalesce_rules() {
    let nonnull = ty(Db2BuiltInType::Integer, &[], Db2Nullability::NotNull);
    let nullable = ty(Db2BuiltInType::BigInt, &[], Db2Nullability::Nullable);
    let entries = [
        Db2ResultTypeOperand::Typed {
            resolved_type: &nonnull,
            span: span(0, 1),
        },
        Db2ResultTypeOperand::UntypedNull { span: span(2, 3) },
        Db2ResultTypeOperand::Typed {
            resolved_type: &nullable,
            span: span(4, 5),
        },
    ];
    let combine =
        |ctx| combine_db2_result_types("x,n,y", span(0, 5), &entries, ctx, Default::default());
    assert_eq!(
        combine(None).unwrap_err().code,
        Db2ResultCombinationErrorCode::MissingApplicationContext
    );
    let coalesce = combine(Some(Db2ResultCombinationContext::Coalesce)).unwrap();
    assert_eq!(coalesce.resolved_type().scalar(), &Db2ScalarType::BigInt);
    assert_eq!(
        coalesce.resolved_type().nullability(),
        Db2Nullability::NotNull
    );
    assert_eq!(coalesce.operands()[1].resolved_type, None);
    assert_eq!(coalesce.operands()[1].obligation.conversion, None);
    assert_eq!(coalesce.steps()[0].right_operand_index, 2);
    let case = combine(Some(Db2ResultCombinationContext::Case {
        else_clause: Db2CaseElse::Omitted,
    }))
    .unwrap();
    assert_eq!(case.resolved_type().nullability(), Db2Nullability::Nullable);
    let nulls = [Db2ResultTypeOperand::UntypedNull { span: span(0, 1) }];
    assert_eq!(
        combine_db2_result_types(
            "n",
            span(0, 1),
            &nulls,
            Some(Db2ResultCombinationContext::OperandRules),
            Default::default(),
        )
        .unwrap_err()
        .code,
        Db2ResultCombinationErrorCode::AllUntypedNull
    );
}

#[test]
fn combined_output_owns_types_and_retains_precision_cap_obligations() {
    let result = {
        let left = ty(Db2BuiltInType::Decimal, &[31, 0], Db2Nullability::NotNull);
        let right = ty(Db2BuiltInType::Decimal, &[31, 31], Db2Nullability::NotNull);
        let source = String::from("a,b");
        combine_db2_result_types(
            &source,
            span(0, 3),
            &[
                Db2ResultTypeOperand::Typed {
                    resolved_type: &left,
                    span: span(0, 1),
                },
                Db2ResultTypeOperand::Typed {
                    resolved_type: &right,
                    span: span(2, 3),
                },
            ],
            Some(Db2ResultCombinationContext::OperandRules),
            Default::default(),
        )
        .unwrap()
    };
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 31,
            scale: 31
        }
    );
    assert_eq!(result.steps()[0].uncapped_decimal_precision, Some(62));
    assert_eq!(result.steps()[0].required_whole_digits, Some(31));
    assert!(result.operands()[0].obligation.whole_part_must_be_preserved);
    assert_eq!(result.span(), span(0, 3));
}

#[test]
fn combination_checks_original_utf8_crlf_order_and_aggregate_limits() {
    let resolved = ty(Db2BuiltInType::Integer, &[], Db2Nullability::NotNull);
    let source = "é\r\na,b";
    let located = |start, end, start_column, end_column| Db2SourceSpan {
        start_byte: start,
        end_byte: end,
        start: Db2SourceLocation {
            line: 2,
            column: start_column,
        },
        end: Db2SourceLocation {
            line: 2,
            column: end_column,
        },
    };
    let a = located(4, 5, 1, 2);
    let b = located(6, 7, 3, 4);
    let enclosing = located(4, 7, 1, 4);
    let entry = |span| Db2ResultTypeOperand::Typed {
        resolved_type: &resolved,
        span,
    };
    let ctx = Some(Db2ResultCombinationContext::OperandRules);
    let entries = [entry(a), entry(b)];
    let limits = Db2ResultCombinationLimits {
        max_source_bytes: source.len(),
        max_operands: 2,
    };
    let result = combine_db2_result_types(source, enclosing, &entries, ctx, limits).unwrap();
    assert_eq!(result.operands()[1].span, b);
    assert_eq!(
        combine_db2_result_types(
            source,
            enclosing,
            &entries,
            ctx,
            Db2ResultCombinationLimits {
                max_operands: 1,
                ..limits
            }
        )
        .unwrap_err()
        .code,
        Db2ResultCombinationErrorCode::TooManyOperands
    );
    assert_eq!(
        combine_db2_result_types(
            source,
            enclosing,
            &entries,
            ctx,
            Db2ResultCombinationLimits {
                max_source_bytes: source.len() - 1,
                ..limits
            }
        )
        .unwrap_err()
        .code,
        Db2ResultCombinationErrorCode::SourceTooLarge
    );
    assert_eq!(
        combine_db2_result_types(source, enclosing, &[entry(b), entry(a)], ctx, limits)
            .unwrap_err()
            .code,
        Db2ResultCombinationErrorCode::InvalidOperandOrder
    );
    let wrong = Db2SourceSpan {
        end: Db2SourceLocation { line: 2, column: 3 },
        ..a
    };
    assert_eq!(
        combine_db2_result_types(source, enclosing, &[entry(wrong)], ctx, limits)
            .unwrap_err()
            .code,
        Db2ResultCombinationErrorCode::InvalidSourceSpan
    );
}
