//! Source-bounded CICS SPI and FEPI command identities.
//!
//! Identity rows and reviewed option facts are distinct projections. Partial
//! grammar facts carry Pending completeness and cannot select a runtime route.
//! The existing application-command and CICS runtime authorities are unchanged.

use crate::{
    CicsApplicationConstraintStatus, CicsApplicationOptionDependency,
    CicsApplicationOptionDescriptor, CicsApplicationOptionDirection,
    CicsApplicationOptionValueShape,
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
    /// Unconditional required operands only.
    pub required_options: &'static [&'static str],
    /// Source-reviewed option-presence dependencies only.
    pub dependencies: &'static [CicsApplicationOptionDependency],
    /// Groups whose members cannot occur together.
    pub mutual_exclusion_groups: &'static [&'static [&'static str]],
    /// Completeness of all grammar facts, including conditional forms.
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
}
