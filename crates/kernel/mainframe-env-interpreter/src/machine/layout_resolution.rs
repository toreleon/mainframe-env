//! Owner-relative runtime layout lookup for already-admitted bindings.

use super::{
    LayoutMetadata, MachineProblem, ReferenceMachine, decode_decimal, is_numeric, normalize,
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
