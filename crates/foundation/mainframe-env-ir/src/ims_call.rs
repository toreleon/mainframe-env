//! Generated IMS call-family identities from the immutable official catalog.

/// One official row in the pinned IMS call/command comparison table.
///
/// Duplicate spellings are intentional: `INIT`, `ISRT`, `CHKP`, and `XRST`
/// participate in more than one official family, and command-level get names
/// map to both ordinary and hold-call rows. Callers must retain the row identity
/// instead of assuming a spelling uniquely selects semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsCallFamilyDescriptor {
    pub ordinal: u8,
    pub official_row: &'static str,
    pub label: &'static str,
    pub source_locator: &'static str,
    pub call_names: &'static [&'static str],
    pub command_names: &'static [&'static str],
}

include!("generated/ims_call_registry.rs");

/// Find the unique official family for a row identity.
#[must_use]
pub fn ims_call_family_for_row(row_id: &str) -> Option<&'static ImsCallFamilyDescriptor> {
    IMS_CALL_FAMILIES
        .iter()
        .find(|family| family.official_row == row_id)
}

/// Return every official family containing a call-level spelling.
pub fn ims_call_families_for_call(
    call_name: &str,
) -> impl Iterator<Item = &'static ImsCallFamilyDescriptor> + '_ {
    IMS_CALL_FAMILIES.iter().filter(move |family| {
        family
            .call_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(call_name))
    })
}

/// Return every official family containing a command-level spelling.
pub fn ims_call_families_for_command(
    command_name: &str,
) -> impl Iterator<Item = &'static ImsCallFamilyDescriptor> + '_ {
    IMS_CALL_FAMILIES.iter().filter(move |family| {
        family
            .command_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(command_name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_registry_preserves_the_official_denominator_and_memberships() {
        assert_eq!(IMS_CALL_FAMILIES.len(), IMS_CALL_FAMILY_COUNT);
        assert_eq!(IMS_CALL_FAMILY_COUNT, 25);
        assert_eq!(
            IMS_CALL_FAMILIES
                .iter()
                .map(|family| family.call_names.len())
                .sum::<usize>(),
            IMS_CALL_NAME_MEMBERSHIPS
        );
        assert_eq!(IMS_CALL_NAME_MEMBERSHIPS, 30);
        assert_eq!(
            IMS_CALL_FAMILIES
                .iter()
                .map(|family| family.command_names.len())
                .sum::<usize>(),
            IMS_COMMAND_NAME_MEMBERSHIPS
        );
        assert_eq!(IMS_COMMAND_NAME_MEMBERSHIPS, 30);
        assert!(IMS_CALL_FAMILIES.iter().enumerate().all(|(index, family)| {
            usize::from(family.ordinal) == index + 1
                && family.official_row.ends_with(&format!(":{:04}", index + 1))
        }));
    }

    #[test]
    fn lookup_retains_ambiguous_spelling_as_distinct_official_rows() {
        let init = ims_call_families_for_call("init")
            .map(|family| family.ordinal)
            .collect::<Vec<_>>();
        assert_eq!(init, [1, 12, 13, 14]);
        let isrt = ims_call_families_for_call("ISRT")
            .map(|family| family.ordinal)
            .collect::<Vec<_>>();
        assert_eq!(isrt, [8, 9]);
        let command_get = ims_call_families_for_command("gnp")
            .map(|family| family.ordinal)
            .collect::<Vec<_>>();
        assert_eq!(command_get, [5, 6]);
    }

    #[test]
    fn row_lookup_is_exact_and_does_not_infer_from_names() {
        let row = "ibm-ims-15.6-dli-2026-08-31:dli-call-families:0019";
        assert_eq!(
            ims_call_family_for_row(row).map(|family| family.ordinal),
            Some(19)
        );
        assert!(ims_call_family_for_row("SCHD").is_none());
    }
}
