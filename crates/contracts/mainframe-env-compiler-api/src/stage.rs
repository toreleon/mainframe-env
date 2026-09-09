use crate::CompilerProblem;
use mainframe_env_ir::{
    LegalModule, LegalityProfile, Module, OperationCatalog, VerificationReport, verify,
    verify_legal,
};
use mainframe_env_source::SourceId;

/// A HIR module whose structural and operation-catalog invariants were checked.
///
/// Its fields are private, and the only constructor runs the authoritative IR
/// verifier. Compiler frontends retain their parsed and semantic analysis as
/// private implementation state rather than accepting caller-authored claims.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHir {
    source: SourceId,
    module: Module,
    report: VerificationReport,
}

impl VerifiedHir {
    pub fn verify(
        source: SourceId,
        module: Module,
        catalog: &OperationCatalog,
    ) -> Result<Self, CompilerProblem> {
        let report = verify(&module, catalog)
            .map_err(|problem| CompilerProblem::Verification(problem.to_string()))?;
        Ok(Self {
            source,
            module,
            report,
        })
    }

    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    #[must_use]
    pub fn module(&self) -> &Module {
        &self.module
    }
    #[must_use]
    pub fn report(&self) -> &VerificationReport {
        &self.report
    }

    /// Consumes the verified HIR proof and binds a lowered module to its source.
    #[must_use]
    pub fn lower(self, module: Module) -> LoweredMir {
        LoweredMir {
            source: self.source,
            verified_hir_report: self.report,
            module,
        }
    }
}

/// A lowered MIR that is provenance-bound to a consumed verified HIR stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweredMir {
    source: SourceId,
    verified_hir_report: VerificationReport,
    module: Module,
}

impl LoweredMir {
    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }

    #[must_use]
    pub fn verified_hir_report(&self) -> &VerificationReport {
        &self.verified_hir_report
    }
}

/// Executable MIR whose complete operation set passed the legality profile.
///
/// Construction consumes [`LoweredMir`]; callers cannot substitute an
/// unrelated source identity at legalization time.
///
/// ```compile_fail
/// # use mainframe_env_compiler_api::LegalizedMir;
/// # use mainframe_env_ir::{LegalityProfile, Module, OperationCatalog};
/// # use mainframe_env_source::SourceId;
/// # fn fabricate(source: SourceId, module: Module, catalog: &OperationCatalog,
/// #              profile: &LegalityProfile) {
/// let _ = LegalizedMir::legalize(source, module, catalog, profile);
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegalizedMir {
    source: SourceId,
    legal: LegalModule,
}

impl LegalizedMir {
    pub fn legalize(
        lowered: LoweredMir,
        catalog: &OperationCatalog,
        profile: &LegalityProfile,
    ) -> Result<Self, CompilerProblem> {
        let legal = verify_legal(lowered.module, catalog, profile)
            .map_err(|problem| CompilerProblem::Legality(problem.to_string()))?;
        Ok(Self {
            source: lowered.source,
            legal,
        })
    }

    #[must_use]
    pub const fn source(&self) -> SourceId {
        self.source
    }
    #[must_use]
    pub fn legal(&self) -> &LegalModule {
        &self.legal
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mainframe_env_ir::{
        Attribute, CicsCondition, CicsEffectPlan, CicsNamedOperand, CicsOperandName,
        CicsOperandValue, CicsOperationContract, CicsOutputBinding, CicsOutputName, CicsPlanLimits,
        CicsPlanOperation, CicsStorageSlot, DecimalAssignment, DecimalAssignmentPlan,
        DecimalConditionContract, DecimalExecutionPolicy, DecimalExpression,
        DecimalOperationContract, DecimalPlanLimits, DecimalPlanWireVersion, DecimalReceiver,
        DecimalRoundingPolicy, DecimalStorageSlot, Effect, IrLimits, ModuleBuilder,
        OperationIdentity, OperationSchema, OperationSemanticContract, StorageReference,
        encode_cics_effect_plan, encode_decimal_assignment_plan,
    };
    use mainframe_env_source::{
        LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
    };
    use std::collections::{BTreeMap, BTreeSet};

    const CICS_PLAN_ATTRIBUTE: &str = "cics_plan";

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum CicsPlanFixture {
        Valid,
        Empty,
        WrongType,
        NonCanonical,
        OperationMismatch,
        MissingLayout,
        WrongLayoutExtent,
        ReadOnlyOutput,
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum DecimalPlanFixture {
        Valid,
        ValidCondition,
        EmptyBytes,
        WrongType,
        Truncated,
        NonCanonical,
        WrongVersion,
        OperationMajorMismatch,
        EffectMismatch,
        SlotMismatch,
        MissingLayout,
        NonNumericLayout,
        WrongLayoutExtent,
        MissingConditionOwner,
        OrphanConditionBranch,
        InvalidFalseTarget,
        UnexpectedTrueEdge,
        Oversized,
    }

    pub(crate) struct CicsBoundaryFixture {
        pub(crate) module: Module,
        pub(crate) catalog: OperationCatalog,
        pub(crate) profile: LegalityProfile,
    }

    pub(crate) fn source_id() -> SourceId {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("typed-cics.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "typed-cics.cbl",
            b"EXEC CICS READ".to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
            .unwrap()
            .id()
    }

    pub(crate) fn cics_boundary_fixture(kind: CicsPlanFixture) -> CicsBoundaryFixture {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        let key = builder.add_storage("KEY", 2, None).unwrap();
        let record = builder.add_storage("RECORD", 8, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let define = OperationIdentity::new("mainframe.core.cobol", "define", 1).unwrap();
        let read = OperationIdentity::new("cics.file", "read", 1).unwrap();
        let halt = OperationIdentity::new("test", "halt", 1).unwrap();
        let plan = match kind {
            CicsPlanFixture::OperationMismatch => CicsEffectPlan {
                operation: CicsPlanOperation::Syncpoint,
                operands: Vec::new(),
                options: BTreeSet::new(),
                outputs: Vec::new(),
                condition: CicsCondition::Default,
            },
            _ => read_plan(key, record),
        };
        let mut plan_bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        let plan_attribute = match kind {
            CicsPlanFixture::Empty => Attribute::Bytes(Vec::new()),
            CicsPlanFixture::WrongType => Attribute::Text("not-plan-bytes".into()),
            CicsPlanFixture::NonCanonical => {
                plan_bytes.push(0);
                Attribute::Bytes(plan_bytes)
            }
            CicsPlanFixture::Valid
            | CicsPlanFixture::OperationMismatch
            | CicsPlanFixture::MissingLayout
            | CicsPlanFixture::WrongLayoutExtent
            | CicsPlanFixture::ReadOnlyOutput => Attribute::Bytes(plan_bytes),
        };
        add_layout(
            &mut builder,
            block,
            define.clone(),
            "KEY",
            "alphanumeric",
            2,
        );
        if !matches!(kind, CicsPlanFixture::MissingLayout) {
            add_layout(
                &mut builder,
                block,
                define.clone(),
                "RECORD",
                if matches!(kind, CicsPlanFixture::ReadOnlyOutput) {
                    "condition"
                } else {
                    "alphanumeric"
                },
                if matches!(kind, CicsPlanFixture::WrongLayoutExtent) {
                    7
                } else {
                    8
                },
            );
        }
        let read_effects = cics_read_effects();
        builder
            .add_operation(
                block,
                read.clone(),
                Vec::new(),
                0,
                BTreeMap::from([(CICS_PLAN_ATTRIBUTE.into(), plan_attribute)]),
                read_effects.clone(),
                vec![
                    StorageReference {
                        storage: key,
                        offset: 0,
                        length: 2,
                    },
                    StorageReference {
                        storage: record,
                        offset: 0,
                        length: 8,
                    },
                ],
                None,
            )
            .unwrap();
        builder
            .add_operation(
                block,
                halt.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();

        let define_schema = OperationSchema::pure(define.clone(), 0, 0);
        let mut read_schema = OperationSchema::pure(read.clone(), 0, 0);
        read_schema.required_attributes = BTreeSet::from([CICS_PLAN_ATTRIBUTE.into()]);
        read_schema.allowed_effects = read_effects.into_iter().collect();
        read_schema.semantic_contract =
            OperationSemanticContract::CicsEffect(CicsOperationContract {
                plan_attribute: CICS_PLAN_ATTRIBUTE.into(),
                expected_operation: Some(CicsPlanOperation::Read),
                layout_definition_operation: Some(define.clone()),
            });
        let mut halt_schema = OperationSchema::pure(halt.clone(), 0, 0);
        halt_schema.terminator = true;
        let mut catalog = OperationCatalog::default();
        catalog.register(define_schema).unwrap();
        catalog.register(read_schema).unwrap();
        catalog.register(halt_schema).unwrap();
        CicsBoundaryFixture {
            module: builder.finish().unwrap(),
            catalog,
            profile: LegalityProfile {
                allowed_operations: BTreeSet::from([define, read, halt]),
                allowed_runtime_imports: BTreeSet::new(),
            },
        }
    }

    pub(crate) fn decimal_boundary_fixture(kind: DecimalPlanFixture) -> CicsBoundaryFixture {
        const PLAN: &str = "assignment_plan";
        const STATUS: &str = "typed_condition_status";
        const MASK: &str = "typed_condition_branches";
        const POLARITY: &str = "typed_condition_polarity";
        const SIZE_ERROR: &str = "cobol.arithmetic-size-error@1";

        let mut limits = IrLimits::default();
        if matches!(kind, DecimalPlanFixture::Oversized) {
            limits.max_attribute_bytes = DecimalPlanLimits::default().max_encoded_bytes + 1;
        }
        let mut builder = ModuleBuilder::new(limits);
        let result = builder.add_storage("result", 3, None).unwrap();
        let other = builder.add_storage("other", 3, None).unwrap();
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        let define = OperationIdentity::new("mainframe.core.cobol", "define", 1).unwrap();
        let decimal = OperationIdentity::new(
            "mainframe.decimal",
            "assign",
            if matches!(kind, DecimalPlanFixture::OperationMajorMismatch) {
                1
            } else {
                2
            },
        )
        .unwrap();
        let control = OperationIdentity::new("test", "control", 1).unwrap();
        let halt = OperationIdentity::new("test", "halt", 1).unwrap();
        if !matches!(kind, DecimalPlanFixture::MissingLayout) {
            add_layout(
                &mut builder,
                block,
                define.clone(),
                "RESULT",
                if matches!(kind, DecimalPlanFixture::NonNumericLayout) {
                    "alphanumeric"
                } else {
                    "numeric_display"
                },
                if matches!(kind, DecimalPlanFixture::WrongLayoutExtent) {
                    2
                } else {
                    3
                },
            );
        }
        let target = DecimalStorageSlot {
            storage: if matches!(kind, DecimalPlanFixture::SlotMismatch) {
                other
            } else {
                result
            },
            qualified_layout_name: "RESULT".into(),
        };
        let plan = DecimalAssignmentPlan {
            semantic_origin: "cobol.compute@1".into(),
            policy: DecimalExecutionPolicy::decimal34_v1(),
            assignments: vec![DecimalAssignment {
                expression: DecimalExpression::Literal {
                    coefficient: 1,
                    scale: 0,
                },
                receiver: DecimalReceiver {
                    target: target.clone(),
                    rounding: DecimalRoundingPolicy::Truncation,
                },
            }],
        };
        let mut plan_bytes =
            encode_decimal_assignment_plan(&plan, DecimalPlanLimits::default()).unwrap();
        match kind {
            DecimalPlanFixture::EmptyBytes => plan_bytes.clear(),
            DecimalPlanFixture::Truncated => {
                plan_bytes.pop();
            }
            DecimalPlanFixture::NonCanonical => plan_bytes.push(0),
            DecimalPlanFixture::WrongVersion => {
                plan_bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
            }
            DecimalPlanFixture::Oversized => {
                plan_bytes.resize(DecimalPlanLimits::default().max_encoded_bytes + 1, 0);
            }
            _ => {}
        }
        let plan_attribute = if matches!(kind, DecimalPlanFixture::WrongType) {
            Attribute::Text("not-plan-bytes".into())
        } else {
            Attribute::Bytes(plan_bytes)
        };
        let owner = matches!(
            kind,
            DecimalPlanFixture::ValidCondition
                | DecimalPlanFixture::InvalidFalseTarget
                | DecimalPlanFixture::UnexpectedTrueEdge
        );
        let missing_owner = matches!(kind, DecimalPlanFixture::MissingConditionOwner);
        let mut decimal_attributes = BTreeMap::from([
            (PLAN.into(), plan_attribute),
            (STATUS.into(), Attribute::Text(SIZE_ERROR.into())),
            (
                MASK.into(),
                Attribute::Integer(i64::from(owner || missing_owner)),
            ),
        ]);
        if owner {
            decimal_attributes.extend([
                ("control_node".into(), Attribute::Integer(0)),
                ("control_role".into(), Attribute::Text("statement".into())),
                ("control_parent".into(), Attribute::Integer(-1)),
            ]);
        }
        builder
            .add_operation(
                block,
                decimal.clone(),
                Vec::new(),
                0,
                decimal_attributes,
                if matches!(kind, DecimalPlanFixture::EffectMismatch) {
                    vec![Effect::MemoryRead]
                } else {
                    vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]
                },
                vec![StorageReference {
                    storage: target.storage,
                    offset: 0,
                    length: 3,
                }],
                None,
            )
            .unwrap();

        let orphan = matches!(kind, DecimalPlanFixture::OrphanConditionBranch);
        if owner || orphan {
            let parent = if orphan { 99 } else { 0 };
            let false_target = if matches!(kind, DecimalPlanFixture::InvalidFalseTarget) {
                0
            } else {
                2
            };
            let mut branch = BTreeMap::from([
                ("control_node".into(), Attribute::Integer(1)),
                ("control_role".into(), Attribute::Text("branch".into())),
                ("control_parent".into(), Attribute::Integer(parent)),
                ("edge_branch_false".into(), Attribute::Integer(false_target)),
                (STATUS.into(), Attribute::Text(SIZE_ERROR.into())),
                (POLARITY.into(), Attribute::Boolean(true)),
            ]);
            if matches!(kind, DecimalPlanFixture::UnexpectedTrueEdge) {
                branch.insert("edge_branch_true".into(), Attribute::Integer(2));
            }
            builder
                .add_operation(
                    block,
                    control.clone(),
                    Vec::new(),
                    0,
                    branch,
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
            builder
                .add_operation(
                    block,
                    control.clone(),
                    Vec::new(),
                    0,
                    BTreeMap::from([
                        ("control_node".into(), Attribute::Integer(2)),
                        ("control_role".into(), Attribute::Text("block_end".into())),
                        ("control_parent".into(), Attribute::Integer(parent)),
                    ]),
                    Vec::new(),
                    Vec::new(),
                    None,
                )
                .unwrap();
        }
        builder
            .add_operation(
                block,
                halt.clone(),
                Vec::new(),
                0,
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();

        let mut decimal_schema = OperationSchema::pure(decimal.clone(), 0, 0);
        decimal_schema.required_attributes =
            BTreeSet::from([PLAN.into(), STATUS.into(), MASK.into()]);
        decimal_schema.allowed_effects =
            BTreeSet::from([Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]);
        decimal_schema.semantic_contract =
            OperationSemanticContract::DecimalAssignment(DecimalOperationContract {
                plan_attribute: PLAN.into(),
                expected_plan_version: if decimal.major() == 1 {
                    DecimalPlanWireVersion::LegacyV1
                } else {
                    DecimalPlanWireVersion::PolicyV2
                },
                allowed_semantic_origins: if decimal.major() == 1 {
                    BTreeSet::from(["cobol.compute@1".into()])
                } else {
                    BTreeSet::new()
                },
                layout_definition_operation: Some(define.clone()),
                condition: Some(DecimalConditionContract {
                    status: SIZE_ERROR.into(),
                    status_attribute: STATUS.into(),
                    branch_mask_attribute: MASK.into(),
                    branch_polarity_attribute: POLARITY.into(),
                }),
            });
        let define_schema = OperationSchema::pure(define.clone(), 0, 0);
        let control_schema = OperationSchema::pure(control.clone(), 0, 0);
        let mut halt_schema = OperationSchema::pure(halt.clone(), 0, 0);
        halt_schema.terminator = true;
        let mut catalog = OperationCatalog::default();
        catalog.register(define_schema).unwrap();
        catalog.register(decimal_schema).unwrap();
        catalog.register(control_schema).unwrap();
        catalog.register(halt_schema).unwrap();
        CicsBoundaryFixture {
            module: builder.finish().unwrap(),
            catalog,
            profile: LegalityProfile {
                allowed_operations: BTreeSet::from([define, decimal, control, halt]),
                allowed_runtime_imports: BTreeSet::new(),
            },
        }
    }

    fn add_layout(
        builder: &mut ModuleBuilder,
        block: mainframe_env_ir::BlockId,
        identity: OperationIdentity,
        name: &str,
        category: &str,
        length: i64,
    ) {
        builder
            .add_operation(
                block,
                identity,
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text(name.into())),
                    (
                        "simple_name".into(),
                        Attribute::Text(name.rsplit('.').next().unwrap_or(name).into()),
                    ),
                    ("category".into(), Attribute::Text(category.into())),
                    ("picture".into(), Attribute::Text(String::new())),
                    ("digits".into(), Attribute::Integer(length)),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(length)),
                    ("element_length".into(), Attribute::Integer(length)),
                    ("occurs".into(), Attribute::Integer(1)),
                    ("dynamic".into(), Attribute::Integer(0)),
                    ("parent".into(), Attribute::Text(String::new())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }

    fn read_plan(
        key: mainframe_env_ir::StorageId,
        record: mainframe_env_ir::StorageId,
    ) -> CicsEffectPlan {
        CicsEffectPlan {
            operation: CicsPlanOperation::Read,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::File,
                    value: CicsOperandValue::Literal(b"ACCTDAT".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::Ridfld,
                    value: CicsOperandValue::Storage(CicsStorageSlot {
                        storage: key,
                        qualified_layout_name: "KEY".into(),
                    }),
                },
            ],
            options: BTreeSet::new(),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::Into,
                target: CicsStorageSlot {
                    storage: record,
                    qualified_layout_name: "RECORD".into(),
                },
            }],
            condition: CicsCondition::Default,
        }
    }

    fn cics_read_effects() -> Vec<Effect> {
        vec![
            Effect::DatasetRead,
            Effect::MemoryRead,
            Effect::MemoryWrite,
            Effect::Condition,
            Effect::Transaction,
        ]
    }

    #[test]
    fn typed_cics_plan_is_proven_at_hir_and_mir_stage_boundaries() {
        let valid = cics_boundary_fixture(CicsPlanFixture::Valid);
        let verified = VerifiedHir::verify(source_id(), valid.module.clone(), &valid.catalog)
            .expect("canonical typed CICS HIR should verify");
        assert_eq!(verified.report().operation_count, 4);
        let legalized =
            LegalizedMir::legalize(verified.lower(valid.module), &valid.catalog, &valid.profile)
                .expect("canonical typed CICS MIR should legalize");
        assert_eq!(legalized.legal().report().operation_count, 4);

        for kind in [
            CicsPlanFixture::Empty,
            CicsPlanFixture::WrongType,
            CicsPlanFixture::NonCanonical,
            CicsPlanFixture::OperationMismatch,
            CicsPlanFixture::MissingLayout,
            CicsPlanFixture::WrongLayoutExtent,
            CicsPlanFixture::ReadOnlyOutput,
        ] {
            let invalid = cics_boundary_fixture(kind);
            assert!(
                matches!(
                    VerifiedHir::verify(source_id(), invalid.module.clone(), &invalid.catalog),
                    Err(CompilerProblem::Verification(detail))
                        if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected while constructing VerifiedHir"
            );

            let valid = cics_boundary_fixture(CicsPlanFixture::Valid);
            let hir_proof = VerifiedHir::verify(source_id(), valid.module, &valid.catalog).unwrap();
            assert!(
                matches!(
                    LegalizedMir::legalize(
                        hir_proof.lower(invalid.module),
                        &invalid.catalog,
                        &invalid.profile,
                    ),
                    Err(CompilerProblem::Legality(detail)) if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected while constructing LegalizedMir"
            );
        }
    }

    #[test]
    fn typed_decimal_plan_is_proven_at_hir_and_mir_stage_boundaries() {
        for kind in [
            DecimalPlanFixture::Valid,
            DecimalPlanFixture::ValidCondition,
        ] {
            let valid = decimal_boundary_fixture(kind);
            let verified = VerifiedHir::verify(source_id(), valid.module.clone(), &valid.catalog)
                .expect("canonical typed decimal HIR should verify");
            LegalizedMir::legalize(verified.lower(valid.module), &valid.catalog, &valid.profile)
                .expect("canonical typed decimal MIR should legalize");
        }

        for kind in [
            DecimalPlanFixture::EmptyBytes,
            DecimalPlanFixture::WrongType,
            DecimalPlanFixture::Truncated,
            DecimalPlanFixture::NonCanonical,
            DecimalPlanFixture::WrongVersion,
            DecimalPlanFixture::OperationMajorMismatch,
            DecimalPlanFixture::EffectMismatch,
            DecimalPlanFixture::SlotMismatch,
            DecimalPlanFixture::MissingLayout,
            DecimalPlanFixture::NonNumericLayout,
            DecimalPlanFixture::WrongLayoutExtent,
            DecimalPlanFixture::MissingConditionOwner,
            DecimalPlanFixture::OrphanConditionBranch,
            DecimalPlanFixture::InvalidFalseTarget,
            DecimalPlanFixture::UnexpectedTrueEdge,
            DecimalPlanFixture::Oversized,
        ] {
            let invalid = decimal_boundary_fixture(kind);
            assert!(
                matches!(
                    VerifiedHir::verify(source_id(), invalid.module.clone(), &invalid.catalog),
                    Err(CompilerProblem::Verification(detail))
                        if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected while constructing VerifiedHir"
            );

            let valid = decimal_boundary_fixture(DecimalPlanFixture::Valid);
            let proof = VerifiedHir::verify(source_id(), valid.module, &valid.catalog).unwrap();
            assert!(
                matches!(
                    LegalizedMir::legalize(
                        proof.lower(invalid.module),
                        &invalid.catalog,
                        &invalid.profile,
                    ),
                    Err(CompilerProblem::Legality(detail)) if detail.contains("SemanticMismatch")
                ),
                "{kind:?} must be rejected while constructing LegalizedMir"
            );
        }
    }
}
