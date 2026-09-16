use super::{Resolution, ResolutionFailure, clauses, matching_close};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CicsLegacyCompatibilityDescriptor {
    pub(super) official_row: &'static str,
    pub(super) label_tokens: &'static [&'static str],
    pub(super) recognition_head: &'static [&'static str],
    pub(super) runtime_operation: &'static str,
    pub(super) runtime_official_row: &'static str,
    pub(super) required_value_options: &'static [&'static str],
    pub(super) optional_value_options: &'static [&'static str],
    pub(super) optional_flag_options: &'static [&'static str],
    pub(super) application_discriminator_options: &'static [&'static str],
    pub(super) resp2_requires_resp: bool,
    pub(super) reject_unknown_options: bool,
}

include!("../generated_cics_spi_compatibility.rs");

pub(super) fn validated(
    body: &[String],
) -> Resolution<Option<&'static CicsLegacyCompatibilityDescriptor>> {
    for descriptor in CICS_LEGACY_COMPATIBILITY {
        if let Some(matched) = validated_against_legacy_descriptor(descriptor, body)? {
            return Ok(Some(matched));
        }
    }
    Ok(None)
}

fn legacy_compatibility_shape_mismatch(
    descriptor: &'static CicsLegacyCompatibilityDescriptor,
    message: String,
) -> Resolution<Option<&'static CicsLegacyCompatibilityDescriptor>> {
    if descriptor.reject_unknown_options {
        Err(ResolutionFailure::Invalid(message))
    } else {
        Ok(None)
    }
}

fn validated_against_legacy_descriptor(
    descriptor: &'static CicsLegacyCompatibilityDescriptor,
    body: &[String],
) -> Resolution<Option<&'static CicsLegacyCompatibilityDescriptor>> {
    if descriptor.recognition_head.len() > body.len()
        || !descriptor
            .recognition_head
            .iter()
            .zip(body)
            .all(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
    {
        return Ok(None);
    }
    let remainder = &body[descriptor.recognition_head.len()..];
    let Some(selector) = descriptor.required_value_options.first() else {
        return Err(ResolutionFailure::Invalid(
            "compiler SPI compatibility descriptor has no selector".into(),
        ));
    };
    if has_application_discriminator(descriptor, remainder)? {
        return Ok(None);
    }
    let (clauses, options) = clauses(remainder, None)?;
    if !clauses.contains_key(*selector)
        && !options.iter().any(|option| option.as_str() == *selector)
    {
        return Ok(None);
    }
    let value_options = descriptor
        .required_value_options
        .iter()
        .chain(descriptor.optional_value_options)
        .copied()
        .collect::<BTreeSet<_>>();
    let flag_options = descriptor
        .optional_flag_options
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for name in clauses.keys() {
        if flag_options.contains(name.as_str()) {
            return legacy_compatibility_shape_mismatch(
                descriptor,
                format!(
                    "CICS {} option {name} is a flag and rejects a parenthesized operand",
                    descriptor.label_tokens.join(" ")
                ),
            );
        }
        if !value_options.contains(name.as_str()) {
            return legacy_compatibility_shape_mismatch(
                descriptor,
                format!(
                    "CICS {} has unknown legacy SPI option {name}",
                    descriptor.label_tokens.join(" ")
                ),
            );
        }
    }
    for name in &options {
        if value_options.contains(name.as_str()) {
            return legacy_compatibility_shape_mismatch(
                descriptor,
                format!(
                    "CICS {} option {name} requires a parenthesized operand",
                    descriptor.label_tokens.join(" ")
                ),
            );
        }
        if !flag_options.contains(name.as_str()) {
            return legacy_compatibility_shape_mismatch(
                descriptor,
                format!(
                    "CICS {} has unknown legacy SPI option {name}",
                    descriptor.label_tokens.join(" ")
                ),
            );
        }
    }
    if let Some(required) = descriptor
        .required_value_options
        .iter()
        .find(|required| !clauses.contains_key(**required))
    {
        return legacy_compatibility_shape_mismatch(
            descriptor,
            format!(
                "CICS {} requires option {required}",
                descriptor.label_tokens.join(" ")
            ),
        );
    }
    if descriptor.resp2_requires_resp
        && clauses.contains_key("RESP2")
        && !clauses.contains_key("RESP")
    {
        return legacy_compatibility_shape_mismatch(
            descriptor,
            format!(
                "CICS {} option RESP2 requires RESP",
                descriptor.label_tokens.join(" ")
            ),
        );
    }
    debug_assert!(!descriptor.runtime_operation.is_empty());
    debug_assert!(!descriptor.runtime_official_row.is_empty());
    debug_assert!(
        descriptor.official_row.contains(":spi-commands-unique:")
            || descriptor.official_row.contains(":api-commands:")
    );
    Ok(Some(descriptor))
}

fn has_application_discriminator(
    descriptor: &CicsLegacyCompatibilityDescriptor,
    tokens: &[String],
) -> Resolution<bool> {
    let mut position = 0usize;
    while position < tokens.len() {
        let name = tokens[position].to_ascii_uppercase();
        if descriptor
            .application_discriminator_options
            .iter()
            .any(|discriminator| *discriminator == name)
        {
            return Ok(true);
        }
        if tokens.get(position + 1).is_some_and(|token| token == "(") {
            position = matching_close(tokens, position + 1)? + 1;
        } else {
            position += 1;
        }
    }
    Ok(false)
}
