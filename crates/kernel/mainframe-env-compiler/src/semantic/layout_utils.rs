use super::{OccursSpec, SemanticProblem, usage_word};
use crate::generated::cobol_language::DATA_DESCRIPTION_CLAUSES;
use mainframe_env_ir::cobol_source_word_is_undefinable;

pub(super) const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn data_description_clause_boundary(word: &str) -> bool {
    matches!(word, "ASCENDING" | "DEPENDING" | "DESCENDING" | "INDEXED")
        || usage_word(word).is_some()
        || data_description_clause_opener(word)
}

fn data_description_clause_opener(word: &str) -> bool {
    DATA_DESCRIPTION_CLAUSES.iter().any(|descriptor| {
        descriptor.forms.iter().any(|form| {
            for token in form.split_whitespace() {
                let optional = token.starts_with('[');
                let alternatives = token.trim_matches(['[', ']']);
                if word != "IS" && alternatives.split('|').any(|opener| opener == word) {
                    return true;
                }
                if !optional {
                    return false;
                }
            }
            false
        })
    })
}

pub(super) fn index_names(words: &[String]) -> Result<Option<Vec<String>>, SemanticProblem> {
    let positions = words
        .iter()
        .enumerate()
        .filter(|(_, word)| word.as_str() == "INDEXED")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.len() > 1 {
        return Err(SemanticProblem::InvalidOccurs);
    }
    let Some(index) = positions.first().copied() else {
        return Ok(None);
    };
    if words.get(index + 1).is_none_or(|word| word != "BY") {
        return Err(SemanticProblem::InvalidOccurs);
    }
    let mut names = Vec::new();
    for word in &words[index + 2..] {
        if data_description_clause_boundary(word) {
            break;
        }
        if cobol_source_word_is_undefinable(word) {
            return Err(SemanticProblem::InvalidOccurs);
        }
        names.push(word.clone());
    }
    if names.is_empty() {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok(Some(names))
}

pub(super) fn validate_occurs_phrase_order(words: &[String]) -> Result<(), SemanticProblem> {
    let positions = |needle: &str| {
        words
            .iter()
            .enumerate()
            .filter(|(_, word)| word.as_str() == needle)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let occurs = positions("OCCURS");
    let depending = positions("DEPENDING");
    let keys = words
        .iter()
        .enumerate()
        .filter(|(_, word)| matches!(word.as_str(), "ASCENDING" | "DESCENDING"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let indexed = positions("INDEXED");
    if occurs.len() > 1 || depending.len() > 1 || indexed.len() > 1 {
        return Err(SemanticProblem::InvalidOccurs);
    }
    let subphrases = depending
        .iter()
        .chain(keys.iter())
        .chain(indexed.iter())
        .copied()
        .collect::<Vec<_>>();
    if subphrases.is_empty() {
        return Ok(());
    }
    let Some(opener) = occurs.first().copied() else {
        return Err(SemanticProblem::InvalidOccurs);
    };
    let clause_end = words
        .iter()
        .enumerate()
        .skip(opener + 1)
        .find(|(_, word)| {
            !matches!(
                word.as_str(),
                "ASCENDING" | "DEPENDING" | "DESCENDING" | "INDEXED"
            ) && (usage_word(word).is_some() || data_description_clause_opener(word))
        })
        .map_or(words.len(), |(index, _)| index);
    if subphrases.iter().any(|position| *position < opener)
        || subphrases.iter().any(|position| *position >= clause_end)
        || depending.first().is_some_and(|position| {
            keys.iter().any(|key| key < position)
                || indexed.first().is_some_and(|index| index < position)
        })
        || indexed
            .first()
            .is_some_and(|index| keys.iter().any(|key| key > index))
    {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok(())
}

pub(super) fn occurs_range(words: &[String]) -> Result<OccursSpec, SemanticProblem> {
    let Some(index) = words.iter().position(|word| word == "OCCURS") else {
        return Ok(OccursSpec {
            minimum: 1,
            maximum: 1,
            unbounded: false,
        });
    };
    let variable = words.iter().any(|word| word == "DEPENDING");
    let (minimum, maximum_word) = if words.get(index + 1).is_some_and(|word| word == "UNBOUNDED") {
        (1, words.get(index + 1))
    } else {
        let first = words
            .get(index + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(SemanticProblem::InvalidOccurs)?;
        let maximum = if words.get(index + 2).is_some_and(|word| word == "TO") {
            words.get(index + 3)
        } else {
            words.get(index + 1)
        };
        let minimum = if variable && words.get(index + 2).is_none_or(|word| word != "TO") {
            1
        } else {
            first
        };
        (minimum, maximum)
    };
    let unbounded = maximum_word.is_some_and(|value| value == "UNBOUNDED");
    let maximum = if unbounded {
        0
    } else {
        maximum_word
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(SemanticProblem::InvalidOccurs)?
    };
    if (!unbounded && (maximum == 0 || minimum > maximum || maximum > 1_000_000))
        || (!variable && minimum == 0)
        || (variable && !unbounded && minimum >= maximum)
        || (unbounded && !variable)
    {
        return Err(SemanticProblem::InvalidOccurs);
    }
    Ok(OccursSpec {
        minimum,
        maximum,
        unbounded,
    })
}

pub(super) fn values_clause(words: &[String]) -> Vec<String> {
    let start = words
        .iter()
        .position(|word| matches!(word.as_str(), "VALUE" | "VALUES"));
    start.map_or_else(Vec::new, |index| {
        let first = index
            + 1
            + usize::from(
                words
                    .get(index + 1)
                    .is_some_and(|word| matches!(word.as_str(), "IS" | "ARE")),
            );
        words[first..].to_vec()
    })
}
