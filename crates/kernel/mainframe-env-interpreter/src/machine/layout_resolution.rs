//! Owner-relative runtime layout lookup for already-admitted bindings.

use super::{
    LayoutMetadata, MachineProblem, ReferenceMachine, decode_decimal, is_numeric, matching_close,
    normalize,
};
use mainframe_env_ir::cobol_layout_reference_matches;

impl ReferenceMachine {
    pub(super) fn active_occurs(&self, layout: &LayoutMetadata) -> Result<usize, MachineProblem> {
        let Some(depending_on) = &layout.depending_on else {
            return Ok(layout.occurs);
        };
        let value = if depending_on.eq_ignore_ascii_case("EIBCALEN") {
            self.decimal(depending_on)?
        } else {
            let target = self
                .relative_layout(layout, depending_on)
                .ok_or(MachineProblem::UnknownStorage)?;
            if !is_numeric(target.category) {
                return Err(MachineProblem::DataException);
            }
            decode_decimal(target, &self.read(&target.name)?)
                .map_err(|_| MachineProblem::DataException)?
        };
        if value.scale != 0 {
            return Err(MachineProblem::DataException);
        }
        let occurs =
            usize::try_from(value.coefficient).map_err(|_| MachineProblem::SubscriptError)?;
        if occurs < layout.occurs_min || occurs > layout.occurs {
            return Err(MachineProblem::SubscriptError);
        }
        Ok(occurs)
    }

    pub(super) fn relative_layout(
        &self,
        owner: &LayoutMetadata,
        reference: &str,
    ) -> Option<&LayoutMetadata> {
        let normalized = normalize(reference);
        if normalized.contains('.')
            && let Some(layout) = self.layouts.get(&normalized)
        {
            return Some(layout);
        }
        let simple = if normalized.contains('.') {
            normalized.rsplit('.').next()?
        } else {
            normalized.split_whitespace().next()?
        };
        let candidates = self.simple_layouts.get(simple)?;
        let owner_components = owner.name.split('.').collect::<Vec<_>>();
        let mut ranked = candidates
            .iter()
            .filter_map(|name| self.layouts.get(name))
            .filter(|candidate| {
                cobol_layout_reference_matches(&candidate.name, &candidate.simple_name, &normalized)
            })
            .map(|candidate| {
                let proximity = owner_components
                    .iter()
                    .zip(candidate.name.split('.'))
                    .take_while(|(left, right)| **left == *right)
                    .count();
                (proximity, candidate)
            })
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(proximity, _)| std::cmp::Reverse(*proximity));
        match ranked.as_slice() {
            [(_, candidate)] => Some(*candidate),
            [(best, candidate), rest @ ..] if rest.first().is_some_and(|(next, _)| next < best) => {
                Some(*candidate)
            }
            _ => None,
        }
    }
}

impl ReferenceMachine {
    // Do not commit a bare receiver prefix when its adjacent modifier is invalid.
    // Earlier complete receivers retain the original sequential MOVE write order.
    pub(super) fn invalid_modified_receiver(
        &self,
        tokens: &[String],
        prefix_end: usize,
    ) -> Option<MachineProblem> {
        if tokens.get(prefix_end).is_none_or(|token| token != "(") {
            return None;
        }
        let mut end = prefix_end;
        while tokens.get(end).is_some_and(|token| token == "(") {
            let Some(close) = matching_close(tokens, end) else {
                return Some(MachineProblem::InvalidOperation);
            };
            end = close + 1;
        }
        Some(
            self.reference(&tokens[..end])
                .err()
                .unwrap_or(MachineProblem::InvalidOperation),
        )
    }
}

pub(super) fn split_reference_modification(
    tokens: &[String],
) -> Result<(Vec<String>, Vec<String>), MachineProblem> {
    let mut start = Vec::new();
    let mut requested = Vec::new();
    let mut found = false;
    for token in tokens {
        if !found {
            if let Some((before, after)) = token.split_once(':') {
                if !before.is_empty() {
                    start.push(before.to_string());
                }
                if !after.is_empty() {
                    requested.push(after.to_string());
                }
                found = true;
            } else {
                start.push(token.clone());
            }
        } else {
            requested.push(token.clone());
        }
    }
    if !found || start.is_empty() {
        return Err(MachineProblem::ReferenceModificationError);
    }
    Ok((start, requested))
}
