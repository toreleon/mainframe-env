use super::*;

pub(super) fn keep_best_failure(best: &mut Option<CandidateFailure>, candidate: CandidateFailure) {
    if best
        .as_ref()
        .is_none_or(|current| candidate.score > current.score)
    {
        *best = Some(candidate);
    }
}

pub(super) fn validate_candidate(
    descriptor: &CicsApplicationRegistryDescriptor,
    clauses: &Clauses,
    options: &[String],
    present: &BTreeSet<&str>,
    semantic: &SemanticModel,
) -> Result<(), String> {
    if descriptor.recognition_status == CicsApplicationConstraintStatus::Pending
        || descriptor.constraint_status == CicsApplicationConstraintStatus::Pending
    {
        return Err(format!(
            "CICS {} has an unfrozen source contract",
            operation::command_label(descriptor)
        ));
    }

    let mut canonical_spellings = BTreeMap::<&str, &str>::new();
    for name in present {
        let canonical = compatibility_alias_target(descriptor, name).unwrap_or(name);
        if let Some(existing) = canonical_spellings.insert(canonical, name) {
            return Err(format!(
                "CICS {} options {existing} and {name} are aliases and mutually exclusive",
                operation::command_label(descriptor)
            ));
        }
    }

    let mut condition_clause_count = 0usize;
    let mut aid_clause_count = 0usize;
    for name in present {
        let Some(shape) = command_recognition::option_value_shape(descriptor, name) else {
            if let Some(condition_clauses) = descriptor.condition_clauses
                && is_condition_name(name)
            {
                condition_clause_count += 1;
                let value = clauses.get(*name);
                match (condition_clauses.label_operand, value) {
                    (CicsApplicationConditionLabelOperand::Optional, Some(tokens))
                        if !is_single_condition_label(tokens) =>
                    {
                        return Err(format!(
                            "CICS {} condition {name} requires one label operand",
                            operation::command_label(descriptor)
                        ));
                    }
                    (CicsApplicationConditionLabelOperand::Optional, _) => {}
                    (CicsApplicationConditionLabelOperand::Forbidden, Some(_)) => {
                        return Err(format!(
                            "CICS {} condition {name} forbids a label operand",
                            operation::command_label(descriptor)
                        ));
                    }
                    (CicsApplicationConditionLabelOperand::Forbidden, None) => {}
                }
                continue;
            }
            if descriptor.label_tokens == ["HANDLE", "AID"] && is_aid_name(name) {
                aid_clause_count += 1;
                if clauses
                    .get(*name)
                    .is_some_and(|tokens| !is_single_condition_label(tokens))
                {
                    return Err(format!(
                        "CICS HANDLE AID option {name} requires one label operand"
                    ));
                }
                continue;
            }
            return Err(format!(
                "CICS {} has unknown or unreviewed top-level option {name}",
                operation::command_label(descriptor)
            ));
        };
        let has_value = clauses.contains_key(*name);
        if descriptor.label_tokens == ["DUMP", "TRANSACTION"] && *name == "DUMPID" && has_value {
            continue;
        }
        match (shape, has_value) {
            (CicsApplicationOptionValueShape::Flag, true) => {
                return Err(format!(
                    "CICS {} option {name} is a flag and rejects a parenthesized operand",
                    operation::command_label(descriptor)
                ));
            }
            (CicsApplicationOptionValueShape::Value, false) => {
                if interval_control::bare_brexit_discriminator(descriptor.official_row, name) {
                    continue;
                }
                return Err(format!(
                    "CICS {} option {name} requires a parenthesized operand",
                    operation::command_label(descriptor)
                ));
            }
            (CicsApplicationOptionValueShape::BoundedAmbiguity, true)
                if descriptor.runtime_operation == Some("WriteOperator")
                    && matches!(*name, "ACTION" | "REPLY" | "REPLYLENGTH") => {}
            (CicsApplicationOptionValueShape::BoundedAmbiguity, true)
                if descriptor.runtime_operation == Some("ExtractCertificate")
                    && certificate_control::bounded_output_name(name) => {}
            (CicsApplicationOptionValueShape::BoundedAmbiguity, true)
                if descriptor.runtime_operation == Some("ExtractTcpip")
                    && matches!(*name, "CLNTIPFAMILY" | "SRVRIPFAMILY" | "SSLTYPE") => {}
            (CicsApplicationOptionValueShape::BoundedAmbiguity, true)
                if descriptor.runtime_operation == Some("StartBrexit") && *name == "BRDATA" => {}
            (CicsApplicationOptionValueShape::BoundedAmbiguity, _) => {
                if matches!(
                    descriptor.label_tokens,
                    ["FETCH", "ANY"] | ["FETCH", "CHILD"]
                ) && *name == "COMPSTATUS"
                    && has_value
                {
                    continue;
                }
                if web_control::reviewed_ambiguous_shape(descriptor, name, has_value)
                    || bts_lifecycle::reviewed_ambiguous_shape(descriptor, name, has_value)
                    || channel_container::reviewed_ambiguous_shape(descriptor, name, has_value)
                {
                    continue;
                }
                return Err(format!(
                    "CICS {} option {name} has a source-bounded operand shape",
                    operation::command_label(descriptor)
                ));
            }
            _ => {}
        }
    }
    if let Some(condition_clauses) = descriptor.condition_clauses
        && !(condition_clauses.minimum_occurrences..=condition_clauses.maximum_occurrences)
            .contains(&condition_clause_count)
    {
        return Err(format!(
            "CICS {} requires {}..={} EIBRESP condition clauses, found {condition_clause_count}",
            operation::command_label(descriptor),
            condition_clauses.minimum_occurrences,
            condition_clauses.maximum_occurrences,
        ));
    }
    if descriptor.label_tokens == ["HANDLE", "AID"] && aid_clause_count > 16 {
        return Err(format!(
            "CICS HANDLE AID permits at most 16 AID clauses, found {aid_clause_count}"
        ));
    }

    if !descriptor
        .required_discriminator_options
        .iter()
        .all(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} is missing a required command discriminator",
            operation::command_label(descriptor)
        ));
    }
    if let Some(name) = descriptor
        .forbidden_discriminator_options
        .iter()
        .find(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} forbids discriminator {name}",
            operation::command_label(descriptor)
        ));
    }
    if descriptor.required_discriminator_options.is_empty()
        && descriptor.forbidden_discriminator_options.is_empty()
        && !descriptor.discriminator_options.is_empty()
        && !descriptor
            .discriminator_options
            .iter()
            .any(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} is missing a source-reviewed command discriminator",
            operation::command_label(descriptor)
        ));
    }

    match descriptor.cobol_applicability {
        CicsApplicationCobolApplicability::Allowed => {}
        CicsApplicationCobolApplicability::NotApplicable => {
            return Err(format!(
                "CICS {} is not applicable to COBOL",
                operation::command_label(descriptor)
            ));
        }
        CicsApplicationCobolApplicability::Conditional
        | CicsApplicationCobolApplicability::BoundedAmbiguity => {
            return Err(format!(
                "CICS {} has no unconditional source-reviewed COBOL form",
                operation::command_label(descriptor)
            ));
        }
    }

    if descriptor.runtime_operation == Some("Assign") {
        assign_validation::validate(clauses, present, semantic)?;
    }
    journal_control::validate_candidate(descriptor, clauses)?;

    if let Some(name) = descriptor
        .required_options
        .iter()
        .find(|name| !option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} requires option {name}",
            operation::command_label(descriptor)
        ));
    }
    for alternative in descriptor.alternative_groups {
        let count = alternative
            .members
            .iter()
            .filter(|name| option_is_present(descriptor, present, name))
            .count();
        if alternative.required && count == 0 {
            return Err(format!(
                "CICS {} requires one of {}",
                operation::command_label(descriptor),
                alternative.members.join(", ")
            ));
        }
    }
    for dependency in descriptor.dependencies {
        if option_is_present(descriptor, present, dependency.option)
            && let Some(required) = dependency
                .requires
                .iter()
                .find(|required| !option_is_present(descriptor, present, required))
        {
            return Err(format!(
                "CICS {} option {} requires {required}",
                operation::command_label(descriptor),
                dependency.option
            ));
        }
    }
    for group in descriptor.mutual_exclusion_groups {
        let selected = group
            .iter()
            .filter(|name| option_is_present(descriptor, present, name))
            .copied()
            .collect::<Vec<_>>();
        if selected.len() > 1 {
            return Err(format!(
                "CICS {} options {} are mutually exclusive",
                operation::command_label(descriptor),
                selected.join(", ")
            ));
        }
    }

    for (name, value) in clauses {
        let Some(limit) = descriptor
            .options
            .iter()
            .find(|option| option.name == name)
            .and_then(|option| option.source_max_value_bytes)
        else {
            continue;
        };
        let source_characters = descriptor.family == "bts-control"
            && matches!(name.as_str(), "PROCESS" | "ACTIVITY" | "CHANNEL");
        let known = if source_characters {
            command_recognition::statically_known_value_characters(value, semantic)
        } else {
            command_recognition::statically_known_value_bytes(value, semantic)
        };
        if known.is_some_and(|length| length > limit) {
            return Err(format!(
                "CICS {} option {name} exceeds its source maximum of {limit} {}",
                operation::command_label(descriptor),
                if source_characters {
                    "characters"
                } else {
                    "bytes"
                }
            ));
        }
    }

    // Parsing keeps valued and flag options separate; use both here so future
    // callers cannot accidentally validate only one representation.
    debug_assert_eq!(present.len(), clauses.len() + options.len());
    Ok(())
}
