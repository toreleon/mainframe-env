//! Identity-only catalog for the pinned CICS application-command denominator.

/// One normalized identity in the pinned CICS application-command catalog.
///
/// Catalog presence records an official row and its EIB function code. It does
/// not register an executable operation, select a handler, or grant coverage
/// credit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationCommandIdentityDescriptor {
    /// Stable semantic identity of the official `api-commands` row.
    pub official_row: &'static str,
    /// Official command label recorded by the pinned catalog.
    pub label: &'static str,
    /// Two EIB function-code bytes, in the order printed by the publication.
    ///
    /// Function codes are not unique command identities: several official
    /// application-command rows intentionally share the same bytes.
    pub eibfn: [u8; 2],
}

mod generated {
    use super::CicsApplicationCommandIdentityDescriptor;

    include!("generated/cics_application_commands.rs");
}

/// Number of mandatory application-command identities in the pinned CICS catalog.
pub const CICS_APPLICATION_COMMAND_COUNT: usize = 263;

/// SHA-256 identity of the canonical CICS application-command identity set.
///
/// This digest covers catalog identities only; it is not a handler-registry,
/// executable-support, semantic-completion, or licensed-differential receipt.
pub const CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256: &str =
    generated::CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256;

/// Returns every pinned CICS application-command identity in official-row order.
///
/// Entries describe the complete catalog denominator without advertising that
/// the corresponding commands are executable.
#[must_use]
pub const fn cics_application_command_identities()
-> &'static [CicsApplicationCommandIdentityDescriptor] {
    generated::CICS_APPLICATION_COMMAND_IDENTITIES
}

/// Resolves one pinned CICS application-command identity by official row ID.
///
/// SPI, FEPI, custom, and unknown identities are not part of this API-only
/// catalog and therefore return `None`.
#[must_use]
pub fn cics_application_command_identity(
    official_row: &str,
) -> Option<&'static CicsApplicationCommandIdentityDescriptor> {
    let identities = cics_application_command_identities();
    identities
        .binary_search_by_key(&official_row, |descriptor| descriptor.official_row)
        .ok()
        .map(|index| &identities[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::official_semantic_identity;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    const BASELINE: &str = "ibm-cics-ts-6x-2026-08-31";
    const API_UNIT: &str = "api-commands";

    fn digest_field(hasher: &mut Sha256, value: &[u8]) {
        hasher.update(u64::try_from(value.len()).unwrap().to_be_bytes());
        hasher.update(value);
    }

    #[test]
    fn application_identity_catalog_is_exact_sorted_and_round_trips() {
        let identities = cics_application_command_identities();
        assert_eq!(identities.len(), CICS_APPLICATION_COMMAND_COUNT);
        assert_eq!(CICS_APPLICATION_COMMAND_COUNT, 263);
        assert!(
            identities
                .windows(2)
                .all(|pair| pair[0].official_row < pair[1].official_row)
        );

        for descriptor in identities {
            let official = official_semantic_identity(descriptor.official_row)
                .expect("generated CICS application row must be an official identity");
            assert_eq!(official.baseline, BASELINE);
            assert_eq!(official.subsystem, "cics");
            assert_eq!(official.unit, API_UNIT);
            assert_eq!(official.label, descriptor.label);
            assert_eq!(
                cics_application_command_identity(descriptor.official_row),
                Some(descriptor)
            );
        }
    }

    #[test]
    fn unknown_and_non_application_identities_do_not_resolve() {
        assert_eq!(
            cics_application_command_identity("custom:cics.command@1"),
            None
        );
        assert_eq!(
            cics_application_command_identity("ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001"),
            None
        );
        assert_eq!(
            cics_application_command_identity("ibm-cics-ts-6x-2026-08-31:fepi-commands:0001"),
            None
        );
        assert_eq!(
            cics_application_command_identity("ibm-cics-ts-6x-2026-08-31:api-commands:0264"),
            None
        );
    }

    #[test]
    fn identity_set_digest_is_domain_separated_and_covers_every_logical_field() {
        let identities = cics_application_command_identities();
        let mut hasher = Sha256::new();
        hasher.update(b"mainframe-env.cics-application-command-identities@2\0");
        hasher.update(u64::try_from(identities.len()).unwrap().to_be_bytes());
        for descriptor in identities {
            digest_field(&mut hasher, descriptor.official_row.as_bytes());
            digest_field(&mut hasher, descriptor.label.as_bytes());
            digest_field(&mut hasher, &descriptor.eibfn);
        }
        assert_eq!(
            CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256,
            format!("sha256:{:x}", hasher.finalize())
        );
    }

    #[test]
    fn eib_function_codes_are_bytes_and_not_unique_command_identities() {
        let mut rows_by_code = BTreeMap::<[u8; 2], Vec<&str>>::new();
        for descriptor in cics_application_command_identities() {
            rows_by_code
                .entry(descriptor.eibfn)
                .or_default()
                .push(descriptor.official_row);
        }
        assert_eq!(rows_by_code.len(), 258);
        assert_eq!(
            rows_by_code.values().filter(|rows| rows.len() > 1).count(),
            4
        );
        assert_eq!(
            rows_by_code.get(&[0x10, 0x08]).map(Vec::as_slice),
            Some(
                [
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0205",
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0206",
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0207",
                ]
                .as_slice()
            )
        );
        assert_eq!(
            rows_by_code.get(&[0x36, 0x02]).map(Vec::as_slice),
            Some(
                [
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0033",
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0036",
                ]
                .as_slice()
            )
        );
        assert_eq!(
            rows_by_code.get(&[0x38, 0x10]).map(Vec::as_slice),
            Some(
                [
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0076",
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0243",
                ]
                .as_slice()
            )
        );
        assert_eq!(
            rows_by_code.get(&[0x56, 0x02]).map(Vec::as_slice),
            Some(
                [
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0201",
                    "ibm-cics-ts-6x-2026-08-31:api-commands:0202",
                ]
                .as_slice()
            )
        );
    }
}
