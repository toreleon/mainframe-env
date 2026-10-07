//! Independent public finite-assignment vectors; no SQL or licensed execution.

use mainframe_env_db2::{
    Db2AssignedFiniteDecFloat, Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2DataType,
    Db2DecFloatAssignmentErrorCode as Code, Db2DecFloatRounding,
    Db2DecFloatRoundingContext as Origin, Db2DecFloatRoundingMode as Mode, Db2FiniteDecFloatSign,
    Db2MaterializedNumericConstant, Db2Nullability, Db2NumericAssignmentErrorCode,
    Db2NumericConstantLimits, Db2ResolvedType, Db2SourceLocation, Db2SourceSpan, Db2SyntaxLimits,
    assign_db2_finite_decfloat_constant, assign_db2_numeric_constant,
    materialize_db2_located_numeric_operand, materialize_db2_numeric_constant, resolve_db2_type,
};

fn target(kind: Db2BuiltInType, args: Vec<u32>, nullability: Db2Nullability) -> Db2ResolvedType {
    let syntax = Db2BuiltInDataType::new(kind, args, false, Db2AstLimits::default()).unwrap();
    resolve_db2_type(&Db2DataType::BuiltIn(syntax), nullability).unwrap()
}

fn proof(text: &str) -> Db2MaterializedNumericConstant {
    let span = Db2SourceSpan {
        start_byte: 0,
        end_byte: text.len(),
        start: Db2SourceLocation::START,
        end: Db2SourceLocation {
            line: 1,
            column: text.len() as u32 + 1,
        },
    };
    materialize_db2_numeric_constant(text, span, Db2NumericConstantLimits::default()).unwrap()
}

fn assign(text: &str, precision: u32, mode: Mode, context: Origin) -> Db2AssignedFiniteDecFloat {
    assign_db2_finite_decfloat_constant(
        &proof(text),
        &target(
            Db2BuiltInType::DecFloat,
            vec![precision],
            Db2Nullability::NotNull,
        ),
        Db2DecFloatRounding { mode, context },
    )
    .unwrap()
}

#[test]
fn public_seven_modes_preserve_both_signs_and_tie_parities() {
    for (mode, even, odd, negative_even, negative_odd) in [
        (
            Mode::Ceiling,
            1_234_567_890_123_457,
            1_234_567_890_123_458,
            -1_234_567_890_123_456,
            -1_234_567_890_123_457,
        ),
        (
            Mode::Down,
            1_234_567_890_123_456,
            1_234_567_890_123_457,
            -1_234_567_890_123_456,
            -1_234_567_890_123_457,
        ),
        (
            Mode::Floor,
            1_234_567_890_123_456,
            1_234_567_890_123_457,
            -1_234_567_890_123_457,
            -1_234_567_890_123_458,
        ),
        (
            Mode::HalfDown,
            1_234_567_890_123_456,
            1_234_567_890_123_457,
            -1_234_567_890_123_456,
            -1_234_567_890_123_457,
        ),
        (
            Mode::HalfEven,
            1_234_567_890_123_456,
            1_234_567_890_123_458,
            -1_234_567_890_123_456,
            -1_234_567_890_123_458,
        ),
        (
            Mode::HalfUp,
            1_234_567_890_123_457,
            1_234_567_890_123_458,
            -1_234_567_890_123_457,
            -1_234_567_890_123_458,
        ),
        (
            Mode::Up,
            1_234_567_890_123_457,
            1_234_567_890_123_458,
            -1_234_567_890_123_457,
            -1_234_567_890_123_458,
        ),
    ] {
        for (literal, coefficient) in [
            ("12345678901234565", even),
            ("12345678901234575", odd),
            ("-12345678901234565", negative_even),
            ("-12345678901234575", negative_odd),
        ] {
            let result = assign(literal, 16, mode, Origin::StaticBindOption);
            assert_eq!(
                (result.coefficient(), result.exponent(), result.inexact()),
                (coefficient, 1, true)
            );
            assert_eq!(
                result.rounding(),
                Db2DecFloatRounding {
                    mode,
                    context: Origin::StaticBindOption
                }
            );
        }
    }
}

#[test]
fn public_precision_carry_and_zero_quantum_are_owned_exact_observations() {
    for (literal, precision, coefficient, exponent, inexact) in [
        (
            "9999999999999999999999999999999",
            34,
            9_999_999_999_999_999_999_999_999_999_999,
            0,
            false,
        ),
        (
            "9999999999999999999999999999999",
            16,
            1_000_000_000_000_000,
            16,
            true,
        ),
        ("123456789012345600", 16, 1_234_567_890_123_456, 2, false),
        ("0.000", 34, 0, -3, false),
        ("-0.000", 16, 0, -3, false),
        (".0000000000000000000000000000001", 16, 1, -31, false),
    ] {
        let result = assign(
            literal,
            precision,
            Mode::HalfUp,
            Origin::NativeProcedureOption,
        );
        assert_eq!(
            (result.coefficient(), result.exponent(), result.inexact()),
            (coefficient, exponent, inexact)
        );
        assert_eq!(result.precision(), precision);
        assert_eq!(result.sign(), Db2FiniteDecFloatSign::Positive);
    }
}

#[test]
fn public_original_utf8_crlf_proof_survives_source_and_target_scope() {
    let result = {
        let text = String::from("é\r\n - /*n*/ 1.20");
        let located = |start_byte, end_byte, column, end_column| Db2SourceSpan {
            start_byte,
            end_byte,
            start: Db2SourceLocation { line: 2, column },
            end: Db2SourceLocation {
                line: 2,
                column: end_column,
            },
        };
        let source = materialize_db2_located_numeric_operand(
            &text,
            located(5, 17, 2, 14),
            Some(located(5, 6, 2, 3)),
            located(13, 17, 10, 14),
            Db2NumericConstantLimits::default(),
            Db2SyntaxLimits::default(),
        )
        .unwrap();
        let declared = target(Db2BuiltInType::DecFloat, vec![34], Db2Nullability::Nullable);
        assign_db2_finite_decfloat_constant(
            &source,
            &declared,
            Db2DecFloatRounding {
                mode: Mode::Floor,
                context: Origin::StaticCreateViewRegister,
            },
        )
        .unwrap()
    };
    assert_eq!((result.coefficient(), result.exponent()), (-120, -2));
    assert_eq!(result.sign(), Db2FiniteDecFloatSign::Negative);
    assert!(!result.inexact());
    assert_eq!(
        result.span(),
        Db2SourceSpan {
            start_byte: 5,
            end_byte: 17,
            start: Db2SourceLocation { line: 2, column: 2 },
            end: Db2SourceLocation {
                line: 2,
                column: 14
            },
        }
    );
    assert_eq!(result.source().span(), result.span());
    assert_eq!(
        result.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
    assert_eq!(
        result.source().resolved_type().nullability(),
        Db2Nullability::NotNull
    );
}

#[test]
fn public_explicit_origins_do_not_infer_modes_or_retain_prior_status() {
    for origin in [
        Origin::StaticBindOption,
        Origin::NativeProcedureOption,
        Origin::DynamicRegister,
        Origin::StaticCreateViewRegister,
    ] {
        let result = assign(".10000000000000005", 16, Mode::HalfUp, origin);
        assert_eq!(
            (result.coefficient(), result.exponent(), result.inexact()),
            (1_000_000_000_000_001, -16, true)
        );
        assert_eq!(
            result.rounding(),
            Db2DecFloatRounding {
                mode: Mode::HalfUp,
                context: origin
            }
        );
        let exact = assign("1.00", 34, Mode::Down, origin);
        assert_eq!(
            (exact.coefficient(), exact.exponent(), exact.inexact()),
            (100, -2, false)
        );
    }
}

#[test]
fn public_wrong_targets_and_old_context_free_fence_remain_explicit() {
    let source = proof("1");
    let rounding = Db2DecFloatRounding {
        mode: Mode::Up,
        context: Origin::DynamicRegister,
    };
    for (kind, args, expected) in [
        (Db2BuiltInType::Binary, vec![1], Code::IncompatibleTypes),
        (Db2BuiltInType::Date, vec![], Code::IncompatibleTypes),
        (Db2BuiltInType::Integer, vec![], Code::WrongTarget),
        (Db2BuiltInType::Real, vec![], Code::WrongTarget),
        (Db2BuiltInType::VarChar, vec![5], Code::WrongTarget),
    ] {
        let failure = assign_db2_finite_decfloat_constant(
            &source,
            &target(kind, args, Db2Nullability::NotNull),
            rounding,
        )
        .unwrap_err();
        assert_eq!(failure.code, expected);
        assert_eq!(failure.span, source.span());
    }
    assert_eq!(
        assign_db2_numeric_constant(
            &source,
            &target(Db2BuiltInType::DecFloat, vec![16], Db2Nullability::NotNull)
        )
        .unwrap_err()
        .code,
        Db2NumericAssignmentErrorCode::UnsupportedDecFloatTarget
    );
}
