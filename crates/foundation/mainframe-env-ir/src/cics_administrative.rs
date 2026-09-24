//! Source-bounded CICS SPI and FEPI command identities.
//!
//! This module deliberately contains no grammar matcher, runtime operation,
//! handler, route, response map, resource state, or recovery behavior. Those
//! facts remain blocked until command-specific IBM bodies are pinned and
//! reviewed. The existing application-command and CICS runtime authorities are
//! unchanged.

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
}
