use super::*;

pub(super) fn normalize_parameter(
    identity: crate::JclParameterIdentity,
    raw: &str,
) -> Result<String, String> {
    if matches!(
        identity,
        crate::JclParameterIdentity::Exec(ExecParameterId::Parm)
    ) {
        if raw.chars().any(char::is_control) {
            return Err("PARM contains controls".into());
        }
        let raw = raw.trim();
        let value = if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
            raw[1..raw.len() - 1].replace("''", "'")
        } else {
            raw.to_string()
        };
        if value.chars().count() > 100 || value.chars().any(char::is_control) {
            return Err("PARM exceeds 100 characters or contains controls".into());
        }
        return Ok(value);
    }
    let value = strip_quotes(raw.trim());
    if value.len() > 65_536 || value.chars().any(char::is_control) {
        return Err(format!(
            "{} value is empty, contains controls, or exceeds its bound",
            identity.generated().keyword()
        ));
    }
    let upper = value.to_ascii_uppercase();
    let invalid = |detail: &str| {
        Err(format!(
            "{} value {raw:?} violates generated {} validation: {detail}",
            identity.generated().keyword(),
            validation_name(identity.validation())
        ))
    };
    match identity.validation() {
        JclValueShape::None => {
            if raw.contains('=') {
                invalid("the parameter does not accept a value")
            } else {
                Ok(upper)
            }
        }
        JclValueShape::JclValue => balanced_jcl_value(raw)
            .then_some(raw.trim().to_string())
            .ok_or_else(|| {
                format!(
                    "{} value has unbalanced quotes or parentheses",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Text => (!value.is_empty())
            .then_some(value.to_string())
            .ok_or_else(|| format!("{} text value is empty", identity.generated().keyword())),
        JclValueShape::Name => valid_name(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a bounded JCL name",
                identity.generated().keyword()
            )
        }),
        JclValueShape::NameList => validate_list(value, valid_name)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a bounded JCL name list",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Integer => normalize_integer(identity, value),
        JclValueShape::IntegerOrTuple => {
            let values = list_items(value);
            if values.is_empty()
                || values
                    .iter()
                    .any(|value| normalize_integer(identity, value).is_err())
            {
                invalid("expected one integer or a tuple of bounded integers")
            } else {
                Ok(upper)
            }
        }
        JclValueShape::IntegerOrX => {
            if upper == "X" {
                Ok(upper)
            } else {
                normalize_integer(identity, value)
            }
        }
        JclValueShape::IntegerOrSuffix => {
            let digits = value.trim_end_matches(|character: char| character.is_ascii_alphabetic());
            normalize_integer(identity, digits).map(|_| upper)
        }
        JclValueShape::Boolean => matches!(upper.as_str(), "YES" | "NO")
            .then_some(upper)
            .ok_or_else(|| format!("{} requires YES or NO", identity.generated().keyword())),
        JclValueShape::Enum => identity
            .choices()
            .iter()
            .any(|choice| choice.eq_ignore_ascii_case(value))
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires one of {}",
                    identity.generated().keyword(),
                    identity.choices().join(",")
                )
            }),
        JclValueShape::Size => valid_size(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a nonnegative K/M/G/T size",
                identity.generated().keyword()
            )
        }),
        JclValueShape::SizeOrTuple => validate_list(value, valid_size)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a size or size tuple",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Time => valid_time(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires minutes or a (minutes,seconds) tuple",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Class => (value.len() == 1
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'*'))
        .then_some(upper)
        .ok_or_else(|| {
            format!(
                "{} requires one class character",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Condition => valid_condition(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} condition syntax is malformed",
                identity.generated().keyword()
            )
        }),
        JclValueShape::MessageLevel => {
            valid_message_level(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} requires MSGLEVEL=(0-2,0-1)",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Dataset => valid_dataset(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} requires a valid data set or backward-reference name",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Disposition => valid_disposition(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} disposition tuple is invalid",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Delimiter => (value.len() <= 2 && !value.is_empty())
            .then_some(value.to_string())
            .ok_or_else(|| {
                format!(
                    "{} delimiter must contain one or two characters",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::RecordFormat => {
            valid_record_format(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} record format is invalid",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Path => (!value.is_empty() && value.starts_with('/'))
            .then_some(value.to_string())
            .ok_or_else(|| {
                format!(
                    "{} requires an absolute z/OS UNIX path",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Program | JclValueShape::Procedure => {
            valid_program(value).then_some(upper).ok_or_else(|| {
                format!(
                    "{} requires a 1-8 character program or procedure name",
                    identity.generated().keyword()
                )
            })
        }
        JclValueShape::Restart => valid_restart(value).then_some(upper).ok_or_else(|| {
            format!(
                "{} restart target is invalid",
                identity.generated().keyword()
            )
        }),
        JclValueShape::Sysout => valid_sysout(value)
            .then_some(upper)
            .ok_or_else(|| format!("{} SYSOUT tuple is invalid", identity.generated().keyword())),
        JclValueShape::OutputReference => {
            validate_list(value, |item| valid_name(item.trim_start_matches("*.")))
                .then_some(upper)
                .ok_or_else(|| {
                    format!(
                        "{} OUTPUT reference is invalid",
                        identity.generated().keyword()
                    )
                })
        }
        JclValueShape::BackwardReference => (value.starts_with("*.") && value.len() > 2)
            .then_some(upper)
            .ok_or_else(|| {
                format!(
                    "{} requires a backward DD reference",
                    identity.generated().keyword()
                )
            }),
        JclValueShape::Secret => (1..=8)
            .contains(&value.len())
            .then_some("[redacted]".into())
            .ok_or_else(|| {
                format!(
                    "{} secret length is invalid",
                    identity.generated().keyword()
                )
            }),
    }
}

fn normalize_integer(identity: crate::JclParameterIdentity, value: &str) -> Result<String, String> {
    let parsed = value.parse::<u64>().map_err(|_| {
        format!(
            "{} requires an unsigned integer",
            identity.generated().keyword()
        )
    })?;
    if identity.minimum().is_some_and(|minimum| parsed < minimum)
        || identity.maximum().is_some_and(|maximum| parsed > maximum)
    {
        return Err(format!(
            "{} integer is outside generated range {:?}..={:?}",
            identity.generated().keyword(),
            identity.minimum(),
            identity.maximum()
        ));
    }
    Ok(parsed.to_string())
}
