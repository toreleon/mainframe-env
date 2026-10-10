//! Transient results produced by actual comparisons, never from expected inventories.
use super::CorpusProblem;
use super::journey_closure::{
    AuthorityKind, MAX_REQUIREMENTS, MAX_ROWS, admit_text, preflight_observed,
};

#[derive(Default)]
pub(super) struct RouteObservations {
    journeys: Vec<(String, Vec<String>)>,
    issues: Vec<(String, Vec<String>)>,
}

impl RouteObservations {
    pub(super) fn journeys(&self) -> &[(String, Vec<String>)] {
        &self.journeys
    }

    pub(super) fn issues(&self) -> &[(String, Vec<String>)] {
        &self.issues
    }
    // Keep the original refusal predicate and lazy error path together with
    // production of its observation. A skipped comparison produces no token.
    pub(super) fn compare(
        &mut self,
        kind: AuthorityKind,
        id: &str,
        requirement: &str,
        refused: bool,
        problem: impl FnOnce() -> Result<CorpusProblem, CorpusProblem>,
    ) -> Result<(), CorpusProblem> {
        if refused {
            return Err(problem()?);
        }
        let rows = match kind {
            AuthorityKind::Journey => &mut self.journeys,
            AuthorityKind::Issue => &mut self.issues,
        };
        preflight_observed(rows)?;
        let mut total = rows
            .iter()
            .try_fold(0usize, |mut total, (id, requirements)| {
                admit_text(id, &mut total)?;
                for requirement in requirements {
                    admit_text(requirement, &mut total)?;
                }
                Ok::<_, CorpusProblem>(total)
            })?;
        let existing = rows.iter().position(|(previous, _)| previous == id);
        if let Some(index) = existing {
            let requirements = &mut rows[index].1;
            if requirements.len() == MAX_REQUIREMENTS
                || requirements.iter().any(|item| item == requirement)
            {
                return Err(CorpusProblem::new(
                    "carddemo.full.observation_invalid",
                    "duplicate or oversized observation",
                ));
            }
            admit_text(requirement, &mut total)?;
            requirements.push(requirement.into());
        } else {
            if rows.len() == MAX_ROWS {
                return Err(CorpusProblem::new(
                    "carddemo.full.observation_invalid",
                    "observation row bound exceeded",
                ));
            }
            admit_text(id, &mut total)?;
            admit_text(requirement, &mut total)?;
            rows.push((id.into(), vec![requirement.into()]));
        }
        Ok(())
    }

    pub(super) fn extend(&mut self, other: Self) -> Result<(), CorpusProblem> {
        // Merge exercised profiles only; never enumerate the authority as output.
        for (destination, incoming) in [
            (&self.journeys, &other.journeys),
            (&self.issues, &other.issues),
        ] {
            preflight_observed(destination)?;
            preflight_observed(incoming)?;
            if destination
                .len()
                .checked_add(incoming.len())
                .is_none_or(|total| total > MAX_ROWS)
            {
                return Err(CorpusProblem::new(
                    "carddemo.full.observation_invalid",
                    "merged observation bound exceeded",
                ));
            }
            // Preflight the complete merge before moving any tokens into place.
            let mut total = 0;
            for (id, requirements) in destination.iter().chain(incoming) {
                admit_text(id, &mut total)?;
                for requirement in requirements {
                    admit_text(requirement, &mut total)?;
                }
            }
            for (id, requirements) in incoming {
                if let Some((_, previous)) = destination
                    .iter()
                    .find(|(previous_id, _)| previous_id == id)
                    && (previous.len() + requirements.len() > MAX_REQUIREMENTS
                        || requirements
                            .iter()
                            .any(|requirement| previous.contains(requirement)))
                {
                    return Err(CorpusProblem::new(
                        "carddemo.full.observation_invalid",
                        "duplicate merged observation",
                    ));
                }
            }
        }
        for (destination, incoming) in [
            (&mut self.journeys, other.journeys),
            (&mut self.issues, other.issues),
        ] {
            for (id, requirements) in incoming {
                if let Some((_, previous)) = destination
                    .iter_mut()
                    .find(|(previous_id, _)| previous_id == &id)
                {
                    previous.extend(requirements);
                } else {
                    destination.push((id, requirements));
                }
            }
        }
        Ok(())
    }
}
