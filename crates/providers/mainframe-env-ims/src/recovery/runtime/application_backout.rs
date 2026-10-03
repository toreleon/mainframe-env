//! Application points use the existing point/replay authority. The live service
//! supplies actual image rows and applies them through witnessed local undo.
use super::*;

pub(crate) struct ApplicationPoint {
    pub(crate) rows: Vec<ProviderStateRecord>,
    pub(crate) user_data: Vec<u8>,
}

impl RecoverySession {
    pub(crate) fn application_point_count(&self, epoch: &ApplicationEpoch) -> usize {
        self.state
            .points
            .iter()
            .filter(|point| point.application_epoch.as_ref() == Some(epoch))
            .count()
    }
    /// Stage an exact source point or cancellation after the live UOW fence.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn application_set_point(
        &self,
        effect_id: &str,
        epoch: &ApplicationEpoch,
        kind: BackoutPointKind,
        token: Option<[u8; 4]>,
        user_data: Vec<u8>,
        rows: Vec<ProviderStateRecord>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        let resources = rows
            .into_iter()
            .map(|row| CapturedResource {
                resource: TrackedResource {
                    namespace: row.namespace,
                    key: row.key,
                    kind: TrackedResourceKind::Database,
                },
                version: Some(row.version),
                payload: Some(row.payload),
            })
            .collect::<Vec<_>>();
        let point = BackoutPoint {
            application_epoch: Some(epoch.clone()),
            token,
            kind,
            user_data,
            resources,
        };
        verify_point(&point, self.limits)?;
        let mut next = self.state.clone();
        if next
            .baseline
            .as_ref()
            .is_some_and(|point| point.application_epoch.is_none())
        {
            return Err(RecoveryProblem::UnknownOutcome);
        }
        if next
            .baseline
            .as_ref()
            .is_none_or(|point| point.application_epoch.as_ref() != Some(epoch))
        {
            next.points.clear();
            next.baseline = Some(BackoutPoint {
                token: None,
                user_data: vec![],
                ..point.clone()
            });
        }
        if let Some(token) = token {
            if let Some(index) = next
                .points
                .iter()
                .position(|point| point.token == Some(token))
            {
                next.points.truncate(index + 1);
                next.points[index] = point;
            } else {
                if next.points.len() >= self.limits.max_backout_points {
                    return Err(RecoveryProblem::LimitExceeded);
                }
                next.points.push(point);
            }
        } else {
            next.points.clear();
        }
        self.record_op(
            next,
            effect_id,
            request_digest("application-set", &(epoch, kind, token)),
            vec![],
            vec![],
        )
    }

    /// A copied point is reference input only, never an unfenced row mutation.
    pub(crate) fn application_point(
        &self,
        token: [u8; 4],
        epoch: &ApplicationEpoch,
    ) -> Result<Option<ApplicationPoint>, RecoveryProblem> {
        let Some(point) = self
            .state
            .points
            .iter()
            .find(|point| point.token == Some(token))
        else {
            return Ok(None);
        };
        match point.application_epoch.as_ref() {
            None => return Err(RecoveryProblem::UnknownOutcome),
            Some(saved) if saved != epoch => return Ok(None),
            Some(_) => {}
        }
        verify_point(point, self.limits)?;
        let rows = point
            .resources
            .iter()
            .map(|captured| {
                if captured.resource.kind != TrackedResourceKind::Database {
                    return Err(RecoveryProblem::CorruptImage);
                }
                Ok(ProviderStateRecord {
                    namespace: captured.resource.namespace.clone(),
                    key: captured.resource.key.clone(),
                    version: captured.version.ok_or(RecoveryProblem::CorruptImage)?,
                    payload: captured
                        .payload
                        .clone()
                        .ok_or(RecoveryProblem::CorruptImage)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ApplicationPoint {
            rows,
            user_data: point.user_data.clone(),
        }))
    }

    /// Record the already witnessed live backout, cancelling subsequent points.
    pub(crate) fn application_backout(
        &self,
        effect_id: &str,
        token: Option<[u8; 4]>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        let mut next = self.state.clone();
        if let Some(token) = token {
            let index = next
                .points
                .iter()
                .position(|point| point.token == Some(token))
                .ok_or(RecoveryProblem::NotFound)?;
            next.points.truncate(index + 1);
        } else {
            next.baseline = None;
            next.points.clear();
        }
        self.record_op(
            next,
            effect_id,
            request_digest("application-backout", &token),
            vec![],
            vec![],
        )
    }

    /// A condition receipt changes no savepoint or live database/UOW state.
    pub(crate) fn application_condition(
        &self,
        effect_id: &str,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        self.record_op(
            self.state.clone(),
            effect_id,
            request_digest("application-condition", &()),
            vec![],
            vec![],
        )
    }
}
