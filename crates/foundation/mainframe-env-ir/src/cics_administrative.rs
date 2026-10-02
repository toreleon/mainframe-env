//! Source-bounded CICS SPI and FEPI command identities.
//!
//! Identity rows and reviewed option facts are distinct projections. Partial
//! grammar facts carry Pending completeness and cannot select a runtime route.
//! The existing application-command and CICS runtime authorities are unchanged.

use crate::{
    CicsApplicationConstraintStatus, CicsApplicationCvdaDomain, CicsApplicationCvdaNumericDomain,
    CicsApplicationCvdaNumericValue, CicsApplicationOptionAlternative,
    CicsApplicationOptionDependency, CicsApplicationOptionDescriptor,
    CicsApplicationOptionDirection, CicsApplicationOptionValueShape,
};

/// CICS administrative programming interface that owns an official identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsAdministrativeInterface {
    /// CICS system programming interface.
    Spi,
    /// CICS front end programming interface.
    Fepi,
}

/// One source-backed identity with no executable semantic binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsAdministrativeCommandIdentity {
    /// Stable official catalog row.
    pub official_row: &'static str,
    /// Interface table containing the source row.
    pub interface: CicsAdministrativeInterface,
    /// Exact normalized label retained by the frozen official catalog.
    pub label: &'static str,
    /// Whitespace-separated identity tokens; these are not a grammar.
    pub label_tokens: &'static [&'static str],
    /// Canonical two-byte EIBFN retained by the official catalog.
    pub eibfn: [u8; 2],
    /// Additional EIBFN rows for a deduplicated source label.
    pub additional_eibfn_codes: &'static [[u8; 2]],
    /// Distinct official rows that share the canonical EIBFN.
    pub shared_eibfn_rows: &'static [&'static str],
    /// Exact reviewed source gap that blocks semantic projection.
    pub source_gap: &'static str,
}

include!("generated/cics_spi_fepi_registry.rs");

/// Partial source-reviewed grammar facts for one administrative command.
///
/// This reuses the common CICS operand and constraint types. It is not an
/// executable descriptor: browse forms, value-dependent rules, CVDA aliases
/// and other explicit source gaps must be resolved before compiler admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsAdministrativeGrammarContract {
    /// Stable official row, separate from the application-command registry.
    pub official_row: &'static str,
    /// Normative family input that owns these facts.
    pub family: &'static str,
    /// Exact source-reviewed command label.
    pub label: &'static str,
    /// Exact command-body baseline.
    pub source_baseline: &'static str,
    /// Pinned command-body topic path.
    pub source_topic: &'static str,
    /// SHA-256 of that command body, with the sha256 prefix.
    pub source_sha256: &'static str,
    /// Reviewed top-level operand shapes and IBM storage ceilings.
    pub options: &'static [CicsApplicationOptionDescriptor],
    /// Optional scoped symbolic CVDA facts, without numeric or alias inference.
    pub cvda_domains: &'static [CicsApplicationCvdaDomain],
    /// Optional reference numbers for explicitly sourced domain members only.
    pub cvda_numeric_domains: &'static [CicsApplicationCvdaNumericDomain],
    /// Unconditional required operands only.
    pub required_options: &'static [&'static str],
    /// Required or optional alternatives, using the existing CICS constraint type.
    pub alternative_groups: &'static [CicsApplicationOptionAlternative],
    /// Source-reviewed option-presence dependencies only.
    pub dependencies: &'static [CicsApplicationOptionDependency],
    /// Groups whose members cannot occur together.
    pub mutual_exclusion_groups: &'static [&'static [&'static str]],
    /// Optional source-reviewed forms using the common CICS operand types.
    pub forms: &'static [CicsAdministrativeGrammarForm],
    /// Completeness of all grammar facts, including conditional forms.
    pub constraint_status: CicsApplicationConstraintStatus,
}

/// Partial operand facts for a source-reviewed administrative command form.
///
/// Selector options are required clauses within this closed form. The facts do
/// not perform recognition, select a handler, or grant compiler admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsAdministrativeGrammarForm {
    /// Stable form identity within one normative command contract.
    pub id: &'static str,
    /// Positive source selectors, separate from runtime dispatch.
    pub selector_options: &'static [&'static str],
    /// Form-specific shapes/directions using the existing CICS operand type.
    pub options: &'static [CicsApplicationOptionDescriptor],
    /// Optional scoped symbolic CVDA facts, without numeric or alias inference.
    pub cvda_domains: &'static [CicsApplicationCvdaDomain],
    /// Optional form-local reference numbers, without admission or alias credit.
    pub cvda_numeric_domains: &'static [CicsApplicationCvdaNumericDomain],
    /// Unconditional required clauses within this form.
    pub required_options: &'static [&'static str],
    /// Required or optional choices using the existing CICS constraint type.
    pub alternative_groups: &'static [CicsApplicationOptionAlternative],
    /// Form-local source option dependencies.
    pub dependencies: &'static [CicsApplicationOptionDependency],
    /// Form-local groups whose clauses cannot occur together.
    pub mutual_exclusion_groups: &'static [&'static [&'static str]],
    /// Completeness remains independent from runtime readiness.
    pub constraint_status: CicsApplicationConstraintStatus,
}

include!("generated/cics_administrative_grammar.rs");

/// Looks up partial administrative grammar facts by official row.
///
/// A result supplies source facts only; it grants no handler, recognition,
/// compilation, runtime admission, public route or conformance verdict.
#[must_use]
pub fn cics_administrative_grammar_for_official_row(
    official_row: &str,
) -> Option<&'static CicsAdministrativeGrammarContract> {
    CICS_ADMINISTRATIVE_GRAMMAR_CONTRACTS
        .iter()
        .find(|contract| contract.official_row == official_row)
}

/// Finds an identity by its stable official row.
#[must_use]
pub fn cics_administrative_identity_for_official_row(
    official_row: &str,
) -> Option<&'static CicsAdministrativeCommandIdentity> {
    CICS_SPI_FEPI_IDENTITY_REGISTRY
        .iter()
        .find(|identity| identity.official_row == official_row)
}

/// Returns every identity carrying an EIBFN within one interface.
///
/// EIBFN values are not unique command identities, so this lookup returns an
/// iterator and never selects a runtime route.
pub fn cics_administrative_identities_for_eibfn(
    interface: CicsAdministrativeInterface,
    eibfn: [u8; 2],
) -> impl Iterator<Item = &'static CicsAdministrativeCommandIdentity> {
    CICS_SPI_FEPI_IDENTITY_REGISTRY
        .iter()
        .filter(move |identity| identity.interface == interface && identity.eibfn == eibfn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn registry_has_exact_unique_denominators_and_no_semantic_credit() {
        assert_eq!(CICS_SPI_FEPI_IDENTITY_REGISTRY.len(), 308);
        assert_eq!(
            CICS_SPI_FEPI_IDENTITY_REGISTRY
                .iter()
                .filter(|identity| identity.interface == CicsAdministrativeInterface::Spi)
                .count(),
            269
        );
        assert_eq!(
            CICS_SPI_FEPI_IDENTITY_REGISTRY
                .iter()
                .filter(|identity| identity.interface == CicsAdministrativeInterface::Fepi)
                .count(),
            39
        );
        assert_eq!(
            CICS_SPI_FEPI_IDENTITY_REGISTRY
                .iter()
                .map(|identity| identity.official_row)
                .collect::<BTreeSet<_>>()
                .len(),
            308
        );
        assert_eq!(CICS_SPI_FEPI_RUNTIME_HANDLERS, 0);
        assert_eq!(CICS_SPI_FEPI_COVERAGE_CREDIT, 0);
    }

    #[test]
    fn source_deduplication_and_shared_codes_are_not_collapsed() {
        let netname = cics_administrative_identity_for_official_row(
            "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0144",
        )
        .expect("INQUIRE NETNAME identity");
        assert_eq!(netname.eibfn, [0x52, 0x16]);
        assert_eq!(netname.additional_eibfn_codes, &[[0x52, 0x06]]);

        let allocate = cics_administrative_identities_for_eibfn(
            CicsAdministrativeInterface::Fepi,
            [0x82, 0x10],
        )
        .map(|identity| identity.official_row)
        .collect::<Vec<_>>();
        assert_eq!(
            allocate,
            vec![
                "ibm-cics-ts-6x-2026-08-31:fepi-commands:0002",
                "ibm-cics-ts-6x-2026-08-31:fepi-commands:0003",
            ]
        );
    }

    #[test]
    fn every_identity_remains_bound_to_its_interface_source_gap() {
        for identity in CICS_SPI_FEPI_IDENTITY_REGISTRY {
            let expected = match identity.interface {
                CicsAdministrativeInterface::Spi => "SPI-1001.source-gap.spi-command-bodies",
                CicsAdministrativeInterface::Fepi => "SPI-1001.source-gap.fepi-command-bodies",
            };
            assert_eq!(identity.source_gap, expected);
            assert!(!identity.label.is_empty());
            assert!(!identity.label_tokens.is_empty());
        }
    }

    fn program_contract(suffix: &str) -> &'static CicsAdministrativeGrammarContract {
        cics_administrative_grammar_for_official_row(&format!(
            "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:{suffix}"
        ))
        .expect("reviewed PROGRAM grammar facts")
    }

    #[test]
    fn grammar_facts_are_source_bound_pending_and_separate_from_application_routes() {
        let mut seen = BTreeSet::new();
        for contract in CICS_ADMINISTRATIVE_GRAMMAR_CONTRACTS {
            assert!(seen.insert(contract.official_row));
            let identity = cics_administrative_identity_for_official_row(contract.official_row)
                .expect("official administrative identity");
            assert_eq!(contract.label, identity.label);
            assert_eq!(
                contract.constraint_status,
                CicsApplicationConstraintStatus::Pending
            );
            assert_eq!(contract.source_sha256.len(), 71);
            assert!(contract.source_sha256.starts_with("sha256:"));
            assert!(
                !crate::CICS_APPLICATION_REGISTRY
                    .iter()
                    .any(|application| { application.official_row == contract.official_row })
            );
        }
        assert_eq!(CICS_SPI_FEPI_RUNTIME_HANDLERS, 0);
        assert_eq!(CICS_SPI_FEPI_IDENTITY_REGISTRY.len(), 308);
        assert!(cics_administrative_grammar_for_official_row("unknown").is_none());
    }

    #[test]
    fn program_create_and_discard_retain_distinct_required_and_byte_contracts() {
        let create = program_contract("0026");
        assert_eq!(create.required_options, &["ATTRIBUTES", "PROGRAM"]);
        let attributes = create
            .options
            .iter()
            .find(|option| option.name == "ATTRIBUTES")
            .expect("ATTRIBUTES input");
        assert_eq!(attributes.direction, CicsApplicationOptionDirection::Input);
        assert_eq!(attributes.source_max_value_bytes, Some(32767));
        let attrlen = create
            .options
            .iter()
            .find(|option| option.name == "ATTRLEN")
            .expect("ATTRLEN halfword");
        assert_eq!(attrlen.source_max_value_bytes, Some(2));
        let discard = program_contract("0084");
        assert_eq!(discard.required_options, &["PROGRAM"]);
        assert!(
            !discard
                .options
                .iter()
                .any(|option| option.name == "ATTRIBUTES")
        );
    }

    #[test]
    fn unresolved_inquire_browse_is_not_promoted_to_complete_typing() {
        let inquire = program_contract("0155");
        assert!(inquire.required_options.is_empty());
        assert_eq!(inquire.forms.len(), 1, "browse forms remain unresolved");
        let named = &inquire.forms[0];
        assert_eq!(named.id, "named");
        assert_eq!(named.selector_options, &["PROGRAM"]);
        assert_eq!(named.required_options, &["PROGRAM"]);
        assert_eq!(
            named.constraint_status,
            CicsApplicationConstraintStatus::Pending
        );
        assert!(
            !named
                .options
                .iter()
                .any(|option| matches!(option.name, "START" | "AT" | "NEXT" | "END"))
        );
        for name in [
            "APPLICATION",
            "APPLMAJORVER",
            "APPLMINORVER",
            "APPLMICROVER",
            "PLATFORM",
        ] {
            let receiver = named
                .options
                .iter()
                .find(|option| option.name == name)
                .expect("named application-context receiver");
            assert_eq!(receiver.direction, CicsApplicationOptionDirection::Output);
            let union = inquire
                .options
                .iter()
                .find(|option| option.name == name)
                .expect("union retains browse context inputs");
            assert_eq!(union.direction, CicsApplicationOptionDirection::InputOutput);
        }
        assert!(!named.required_options.contains(&"STATUS"));
        let at = inquire
            .options
            .iter()
            .find(|option| option.name == "AT")
            .expect("source-declared AT with unresolved typing");
        assert_eq!(
            at.value_shape,
            CicsApplicationOptionValueShape::BoundedAmbiguity
        );
        assert_eq!(
            at.direction,
            CicsApplicationOptionDirection::BoundedAmbiguity
        );
        assert_eq!(at.source_max_value_bytes, None);
        assert_eq!(
            inquire.constraint_status,
            CicsApplicationConstraintStatus::Pending
        );
        let operation = inquire
            .options
            .iter()
            .find(|option| option.name == "OPERATION")
            .expect("OPERATION is an output despite source heading");
        assert_eq!(operation.direction, CicsApplicationOptionDirection::Output);
        assert_eq!(operation.source_max_value_bytes, Some(64));
    }

    #[test]
    fn set_version_is_output_without_invented_copy_dependency() {
        let set = program_contract("0241");
        let version = set
            .options
            .iter()
            .find(|option| option.name == "VERSION")
            .expect("conditional VERSION receiver");
        assert_eq!(version.direction, CicsApplicationOptionDirection::Output);
        assert!(
            !set.dependencies
                .iter()
                .any(|dependency| dependency.option == "VERSION")
        );
        assert!(set.dependencies.iter().any(|dependency| {
            dependency.option == "JVM" && dependency.requires == ["JVMCLASS"]
        }));
        assert!(
            set.mutual_exclusion_groups
                .contains(&&["COPY", "NEWCOPY", "PHASEIN"][..])
        );
        assert!(!set.options.iter().any(|option| option.name == "JVMPOOL"));
    }

    #[test]
    fn program_cvda_domains_keep_inquiry_and_update_values_distinct() {
        fn values(
            contract: &CicsAdministrativeGrammarContract,
            option: &str,
        ) -> &'static [&'static str] {
            contract
                .cvda_domains
                .iter()
                .find(|domain| domain.option == option)
                .expect("source-reviewed CVDA domain")
                .values
        }

        let create = program_contract("0026");
        let discard = program_contract("0084");
        let inquire = program_contract("0155");
        let set = program_contract("0241");
        assert_eq!(values(create, "LOGMESSAGE"), &["LOG", "NOLOG"]);
        assert!(discard.cvda_domains.is_empty());
        assert_eq!(values(inquire, "COPY"), &["NOTREQUIRED", "REQUIRED"]);
        assert_eq!(values(set, "COPY"), &["NEWCOPY", "PHASEIN"]);
        assert_eq!(
            values(inquire, "RUNTIME"),
            &["JVM", "LE370", "NONLE370", "NOTAPPLIC", "UNKNOWN", "XPLINK"]
        );
        assert_eq!(values(set, "RUNTIME"), &["JVM", "NOJVM"]);
        assert_eq!(values(set, "VERSION"), &["NEWCOPY", "OLDCOPY"]);
        assert!(
            !inquire
                .cvda_domains
                .iter()
                .any(|domain| domain.option == "VERSION")
        );
        assert_eq!(
            values(inquire, "PROGTYPE"),
            &["MAP", "MAPSET", "PARTITIONSET", "PROGRAM"]
        );
        for contract in [create, discard, inquire, set] {
            assert_eq!(contract.forms[0].cvda_domains, contract.cvda_domains);
            assert_eq!(
                contract.constraint_status,
                CicsApplicationConstraintStatus::Pending
            );
        }
    }

    #[test]
    fn program_numeric_metadata_preserves_scope_collisions_and_unresolved_symbols() {
        fn numbers(
            contract: &CicsAdministrativeGrammarContract,
            option: &str,
        ) -> Vec<(&'static str, i32)> {
            contract
                .cvda_numeric_domains
                .iter()
                .find(|domain| domain.option == option)
                .expect("source-reviewed numeric subset")
                .values
                .iter()
                .map(|value| (value.symbol, value.number))
                .collect()
        }
        let create = program_contract("0026");
        let discard = program_contract("0084");
        let inquire = program_contract("0155");
        let set = program_contract("0241");
        assert_eq!(numbers(create, "LOGMESSAGE"), [("LOG", 54), ("NOLOG", 55)]);
        assert_eq!(
            numbers(inquire, "COPY"),
            [("NOTREQUIRED", 667), ("REQUIRED", 666)]
        );
        assert_eq!(numbers(set, "COPY"), [("NEWCOPY", 167), ("PHASEIN", 168)]);
        assert_eq!(numbers(set, "RUNTIME"), [("JVM", 1080), ("NOJVM", 1081)]);
        assert_eq!(
            numbers(inquire, "PROGTYPE"),
            [
                ("MAP", 155),
                ("MAPSET", 155),
                ("PARTITIONSET", 156),
                ("PROGRAM", 154)
            ]
        );
        for option in ["LANGUAGE", "LANGDEDUCED"] {
            assert!(
                inquire
                    .cvda_domains
                    .iter()
                    .find(|domain| domain.option == option)
                    .unwrap()
                    .values
                    .contains(&"PL1")
            );
            let values = numbers(inquire, option);
            assert!(values.contains(&("PLI", 152)));
            assert!(!values.iter().any(|(symbol, _)| *symbol == "PL1"));
        }
        assert!(discard.cvda_numeric_domains.is_empty());
        for contract in [create, discard, inquire, set] {
            assert_eq!(
                contract.forms[0].cvda_numeric_domains,
                contract.cvda_numeric_domains
            );
            assert_eq!(
                contract.constraint_status,
                CicsApplicationConstraintStatus::Pending
            );
            for domain in contract.cvda_numeric_domains {
                assert_eq!(
                    domain.source_baseline,
                    "ibm-cics-ts-6x-misc-tail-cvda-2026-09-23"
                );
                assert_eq!(
                    domain.source_topic,
                    "SSJL4D_6.x/reference-applications/commands-api/dfha80c.html"
                );
                assert_eq!(
                    domain.source_sha256,
                    "sha256:5b95b620971d42a9f57511b362f9a12dc04e9ad4b9c26f42cc8e7be943221381"
                );
            }
        }
        assert_eq!(CICS_SPI_FEPI_RUNTIME_HANDLERS, 0);
    }
}
