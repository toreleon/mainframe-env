use super::*;

pub(super) fn valid_condition_list(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let names = text.split('\n').collect::<Vec<_>>();
    let unique = names.iter().copied().collect::<BTreeSet<_>>();
    matches!(names.len(), 1..=16)
        && unique.len() == names.len()
        && names.iter().all(|name| {
            crate::CICS_APPLICATION_CONDITION_NAMES
                .binary_search(name)
                .is_ok()
        })
}

pub(super) fn valid_condition_handlers(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let entries = text
        .split('\n')
        .map(|entry| entry.split_once('\t'))
        .collect::<Option<Vec<_>>>();
    let Some(entries) = entries else {
        return false;
    };
    let unique = entries
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    matches!(entries.len(), 1..=16)
        && unique.len() == entries.len()
        && entries.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && entries.iter().all(|(name, label)| {
            crate::CICS_APPLICATION_CONDITION_NAMES
                .binary_search(name)
                .is_ok()
                && (label.is_empty()
                    || label.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'
                    }))
        })
}

pub(super) fn valid_aid_handlers(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let entries = if text.is_empty() {
        Vec::new()
    } else {
        let entries = text
            .split('\n')
            .map(|entry| entry.split_once('\t'))
            .collect::<Option<Vec<_>>>();
        let Some(entries) = entries else {
            return false;
        };
        entries
    };
    let unique = entries
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    entries.len() <= 16
        && unique.len() == entries.len()
        && entries.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && entries.iter().all(|(name, label)| {
            crate::CICS_APPLICATION_AID_NAMES
                .binary_search(name)
                .is_ok()
                && (label.is_empty()
                    || label.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'
                    }))
        })
}
