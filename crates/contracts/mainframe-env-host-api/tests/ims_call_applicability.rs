use mainframe_env_host_api::{
    IMS_CALL_APPLICABILITY, ImsApplicabilityProblem, ImsCallSite, ImsCallSyntax,
    ImsExecutionContext, ImsPcbKind, ImsProcessingOptionClass, ImsSsaForm, validate_ims_call_site,
};

fn site(row: u8, name: &'static str) -> ImsCallSite<'static> {
    ImsCallSite {
        official_row: format!("ibm-ims-15.6-dli-2026-08-31:dli-call-families:{row:04}"),
        name,
        syntax: ImsCallSyntax::Call,
        context: ImsExecutionContext::DbDc,
        pcb_kind: Some(ImsPcbKind::Database),
        organization: Some("HDAM"),
        processing_option: Some(ImsProcessingOptionClass::All),
        ssa_form: ImsSsaForm::Absent,
    }
}

#[test]
fn each_official_row_has_a_generated_reviewed_profile() {
    assert_eq!(IMS_CALL_APPLICABILITY.len(), 25);
    for (index, family) in IMS_CALL_APPLICABILITY.iter().enumerate() {
        assert_eq!(family.ordinal as usize, index + 1);
        assert!(!family.variants.is_empty());
        assert!(
            family
                .variants
                .iter()
                .all(|variant| !variant.source_topics.is_empty())
        );
    }
}

#[test]
fn row_and_spelling_are_both_required_to_disambiguate_repeated_calls() {
    assert!(validate_ims_call_site(&site(5, "GN")).is_ok());
    assert_eq!(
        validate_ims_call_site(&site(6, "GN")),
        Err(ImsApplicabilityProblem::WrongSpelling),
    );
    assert_eq!(
        validate_ims_call_site(&site(26, "GN")),
        Err(ImsApplicabilityProblem::UnknownFamily),
    );
}

#[test]
fn pairwise_forbidden_context_pcb_organization_option_and_ssa_are_distinct() {
    let base = site(5, "GN");
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            context: ImsExecutionContext::Dcctl,
            ..base.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenContext),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            pcb_kind: Some(ImsPcbKind::Io),
            ..base.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenPcbKind),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("UNKNOWN"),
            ..base.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenOrganization),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            processing_option: Some(ImsProcessingOptionClass::Insert),
            ..base.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenProcessingOption),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            ssa_form: ImsSsaForm::SubsetPointer,
            ..base
        }),
        Err(ImsApplicabilityProblem::ForbiddenSsaForm),
    );
}

#[test]
fn specialized_organization_and_ssa_pairs_are_not_unioned_across_variants() {
    assert!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("DEDB"),
            ssa_form: ImsSsaForm::SubsetPointer,
            ..site(5, "GN")
        })
        .is_ok()
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("MSDB"),
            ssa_form: ImsSsaForm::Path,
            ..site(5, "GN")
        }),
        Err(ImsApplicabilityProblem::ForbiddenSsaForm),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("HDAM"),
            ssa_form: ImsSsaForm::SubsetPointer,
            ..site(5, "GN")
        }),
        Err(ImsApplicabilityProblem::ForbiddenSsaForm),
    );
    assert!(
        validate_ims_call_site(&ImsCallSite {
            name: "GU",
            pcb_kind: Some(ImsPcbKind::Gsam),
            organization: Some("GSAM"),
            ssa_form: ImsSsaForm::RecordSearchArgument,
            ..site(5, "GN")
        })
        .is_ok()
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            name: "GN",
            pcb_kind: Some(ImsPcbKind::Gsam),
            organization: Some("GSAM"),
            ssa_form: ImsSsaForm::RecordSearchArgument,
            ..site(5, "GN")
        }),
        Err(ImsApplicabilityProblem::ForbiddenSsaForm),
    );
}

#[test]
fn named_diagnostics_are_stable_and_validation_grants_no_execution_credit() {
    assert_eq!(ImsApplicabilityProblem::UnknownFamily.code(), "IMS1401-001");
    assert_eq!(
        ImsApplicabilityProblem::ForbiddenSsaForm.code(),
        "IMS1401-008"
    );
    assert!(IMS_CALL_APPLICABILITY.iter().all(|family| !family.executed));
}

#[test]
fn deq_and_schedule_keep_pcb_options_in_their_own_profiles() {
    assert!(
        validate_ims_call_site(&ImsCallSite {
            pcb_kind: Some(ImsPcbKind::Io),
            organization: None,
            processing_option: None,
            ..site(3, "DEQ")
        })
        .is_ok()
    );
    assert!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("DEDB"),
            ..site(3, "DEQ")
        })
        .is_ok()
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            organization: Some("HDAM"),
            ..site(3, "DEQ")
        }),
        Err(ImsApplicabilityProblem::ForbiddenOrganization),
    );
    assert!(
        validate_ims_call_site(&ImsCallSite {
            name: "SCHD",
            syntax: ImsCallSyntax::Command,
            pcb_kind: None,
            organization: None,
            processing_option: None,
            ..site(19, "PCB")
        })
        .is_ok()
    );
    assert_eq!(
        validate_ims_call_site(&site(19, "PCB")),
        Err(ImsApplicabilityProblem::UnexpectedPcbOrOrganization),
    );
}

#[test]
fn pos_and_gscd_use_source_reviewed_contexts_pcbs_and_ssa_forms() {
    let pos = ImsCallSite {
        organization: Some("DEDB"),
        ssa_form: ImsSsaForm::Qualified,
        ..site(11, "POS")
    };
    assert!(validate_ims_call_site(&pos).is_ok());
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            context: ImsExecutionContext::DbBatch,
            ..pos.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenContext),
    );
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            ssa_form: ImsSsaForm::Path,
            ..pos
        }),
        Err(ImsApplicabilityProblem::ForbiddenSsaForm),
    );

    let gscd = ImsCallSite {
        context: ImsExecutionContext::DbBatch,
        pcb_kind: Some(ImsPcbKind::Io),
        organization: None,
        processing_option: None,
        ..site(7, "GSCD")
    };
    assert!(validate_ims_call_site(&gscd).is_ok());
    assert_eq!(
        validate_ims_call_site(&ImsCallSite {
            context: ImsExecutionContext::DbDc,
            ..gscd.clone()
        }),
        Err(ImsApplicabilityProblem::ForbiddenContext),
    );
    assert!(
        validate_ims_call_site(&ImsCallSite {
            pcb_kind: Some(ImsPcbKind::Database),
            organization: Some("HIDAM"),
            processing_option: Some(ImsProcessingOptionClass::Read),
            ..gscd
        })
        .is_ok()
    );
}

#[test]
fn every_family_and_profile_has_call_and_command_representatives() {
    for family in IMS_CALL_APPLICABILITY {
        for variant in family.variants {
            for syntax in [ImsCallSyntax::Call, ImsCallSyntax::Command] {
                let name = if variant.names.is_empty() {
                    match syntax {
                        ImsCallSyntax::Call => family.call_names[0],
                        ImsCallSyntax::Command => family.command_names[0],
                    }
                } else {
                    variant.names[0]
                };
                let site = ImsCallSite {
                    official_row: family.official_row.to_string(),
                    name,
                    syntax,
                    context: variant.contexts[0],
                    pcb_kind: variant.pcb_kind,
                    organization: variant.organizations.first().copied(),
                    processing_option: variant.processing_options.first().copied(),
                    ssa_form: variant.ssa_forms[0],
                };
                assert!(
                    validate_ims_call_site(&site).is_ok(),
                    "row {} profile {} syntax {:?}",
                    family.ordinal,
                    variant.profile,
                    syntax
                );
            }
        }
    }
}

#[test]
fn bounded_pairwise_classes_remain_valid_without_cartesian_expansion() {
    for family in IMS_CALL_APPLICABILITY {
        for variant in family.variants {
            let base = ImsCallSite {
                official_row: family.official_row.to_string(),
                name: variant
                    .names
                    .first()
                    .copied()
                    .unwrap_or(family.call_names[0]),
                syntax: ImsCallSyntax::Call,
                context: variant.contexts[0],
                pcb_kind: variant.pcb_kind,
                organization: variant.organizations.first().copied(),
                processing_option: variant.processing_options.first().copied(),
                ssa_form: variant.ssa_forms[0],
            };
            for &context in variant.contexts {
                let organizations = if variant.organizations.is_empty() {
                    vec![None]
                } else {
                    variant
                        .organizations
                        .iter()
                        .map(|name| Some(*name))
                        .collect()
                };
                for organization in organizations {
                    assert!(
                        validate_ims_call_site(&ImsCallSite {
                            context,
                            organization,
                            ..base.clone()
                        })
                        .is_ok()
                    );
                }
                for &ssa_form in variant.ssa_forms {
                    assert!(
                        validate_ims_call_site(&ImsCallSite {
                            context,
                            ssa_form,
                            ..base.clone()
                        })
                        .is_ok()
                    );
                }
            }
            for &option in variant.processing_options {
                for &ssa_form in variant.ssa_forms {
                    assert!(
                        validate_ims_call_site(&ImsCallSite {
                            processing_option: Some(option),
                            ssa_form,
                            ..base.clone()
                        })
                        .is_ok()
                    );
                }
            }
        }
    }
}
