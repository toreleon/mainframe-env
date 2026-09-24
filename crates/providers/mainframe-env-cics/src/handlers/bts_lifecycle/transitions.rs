//! Pure BTS process/activity lifecycle transitions on one validated snapshot.

use super::*;

/// Local execution request saved before a coordinator child attach.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtsRunTicket {
    pub activity_id: String,
    pub activation_epoch: u64,
    pub program: String,
    pub transid: String,
    pub userid: String,
    pub input_event: String,
    pub synchronous: bool,
}

impl BtsProcess {
    /// Start one initial or dormant activity, with a new fenced activation epoch.
    /// The caller checks the named dormant input event in the event authority
    /// before persisting this transition with its replay record.
    pub fn start(
        &mut self,
        activity_id: &str,
        input_event: Option<&str>,
        synchronous: bool,
    ) -> Result<BtsRunTicket, HostProblem> {
        let root = activity_id == self.root_id;
        let activity = self
            .activities
            .get_mut(activity_id)
            .ok_or_else(|| missing(root))?;
        let event = match (activity.mode, input_event) {
            (BtsMode::Initial, None) => "DFHINITIAL".to_string(),
            (BtsMode::Dormant, Some(event)) => {
                validate_name(event, 16, false)?;
                event.to_string()
            }
            _ => return Err(ineligible(root)),
        };
        if activity.suspended && synchronous {
            return Err(condition("INVREQ", 16, 20));
        }
        activity.activation_epoch = activity
            .activation_epoch
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        activity.mode = BtsMode::Active;
        activity.checkpoint = None;
        Ok(BtsRunTicket {
            activity_id: activity.id.clone(),
            activation_epoch: activity.activation_epoch,
            program: activity.program.clone(),
            transid: activity.transid.clone(),
            userid: activity.userid.clone(),
            input_event: event,
            synchronous,
        })
    }

    /// Bind one coordinator checkpoint to the exact active activation and
    /// current worker lease. A stale completion cannot replace this reference.
    pub fn checkpoint(
        &mut self,
        activity_id: &str,
        activation_epoch: u64,
        owner_lease_epoch: u64,
        reference: &str,
    ) -> Result<(), HostProblem> {
        validate_identifier(reference, 256)?;
        if owner_lease_epoch == 0 {
            return Err(HostProblem::Malformed);
        }
        let activity = self
            .activities
            .get_mut(activity_id)
            .ok_or_else(|| missing(activity_id == self.root_id))?;
        if activity.mode != BtsMode::Active || activity.activation_epoch != activation_epoch {
            return Err(HostProblem::IdempotencyConflict);
        }
        if let Some(current) = &activity.checkpoint {
            if current.owner_lease_epoch > owner_lease_epoch {
                return Err(HostProblem::IdempotencyConflict);
            }
            if current.owner_lease_epoch == owner_lease_epoch && current.reference != reference {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        activity.checkpoint = Some(BtsCheckpoint {
            schema_version: 1,
            activation_epoch,
            owner_lease_epoch,
            reference: reference.into(),
        });
        Ok(())
    }

    /// Complete or defer a running activity only under its activation/lease
    /// fence. The caller posts its completion event in the same owned workflow.
    pub fn finish(
        &mut self,
        activity_id: &str,
        activation_epoch: u64,
        owner_lease_epoch: u64,
        completion: BtsCompletion,
        abcode: Option<&str>,
        abprogram: Option<&str>,
    ) -> Result<(), HostProblem> {
        let activity = self
            .activities
            .get_mut(activity_id)
            .ok_or_else(|| missing(activity_id == self.root_id))?;
        if activity.mode != BtsMode::Active || activity.activation_epoch != activation_epoch {
            return Err(HostProblem::IdempotencyConflict);
        }
        if activity
            .checkpoint
            .as_ref()
            .is_some_and(|saved| saved.owner_lease_epoch != owner_lease_epoch)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        match completion {
            BtsCompletion::Incomplete if abcode.is_none() && abprogram.is_none() => {
                activity.mode = BtsMode::Dormant;
            }
            BtsCompletion::Normal | BtsCompletion::Forced
                if abcode.is_none() && abprogram.is_none() =>
            {
                activity.mode = BtsMode::Complete;
            }
            BtsCompletion::Abend
                if abcode.is_some_and(|code| code.len() == 4)
                    && abprogram.is_some_and(|name| name.len() == 8) =>
            {
                activity.mode = BtsMode::Complete;
            }
            _ => return Err(HostProblem::Malformed),
        }
        activity.completion = completion;
        activity.abcode = abcode.map(str::to_string);
        activity.abprogram = abprogram.map(str::to_string);
        activity.checkpoint = None;
        Ok(())
    }

    /// Set or clear suspension while retaining the processing mode and pending
    /// events. A complete or cancelling subject cannot be reattached.
    pub fn set_suspended(&mut self, activity_id: &str, suspended: bool) -> Result<(), HostProblem> {
        let root = activity_id == self.root_id;
        let activity = self
            .activities
            .get_mut(activity_id)
            .ok_or_else(|| missing(root))?;
        if matches!(activity.mode, BtsMode::Complete | BtsMode::Cancelling) {
            return Err(if root && !suspended {
                condition("PROCESSERR", 108, 14)
            } else if suspended {
                condition("INVREQ", 16, 14)
            } else {
                condition("ACTIVITYERR", 109, 14)
            });
        }
        activity.suspended = suspended;
        Ok(())
    }

    /// Force the eligible subject and all descendants to COMPLETE/FORCED.
    /// The returned IDs identify completion events to post after durable CAS.
    pub fn cancel_subtree(&mut self, activity_id: &str) -> Result<Vec<String>, HostProblem> {
        let root = activity_id == self.root_id;
        let subject = self
            .activities
            .get(activity_id)
            .ok_or_else(|| missing(root))?;
        if !matches!(subject.mode, BtsMode::Initial | BtsMode::Dormant)
            && !(root && subject.mode == BtsMode::Complete)
        {
            return Err(ineligible(root));
        }
        let ids = self.subtree_ids(activity_id);
        if ids.iter().any(|id| {
            self.activities[id].mode == BtsMode::Active
                || self.activities[id].mode == BtsMode::Cancelling
        }) {
            return Err(if root {
                condition("PROCESSBUSY", 106, 13)
            } else {
                condition("ACTIVITYBUSY", 107, 19)
            });
        }
        for id in &ids {
            let activity = self.activities.get_mut(id).expect("validated subtree");
            activity.mode = BtsMode::Complete;
            activity.completion = BtsCompletion::Forced;
            activity.checkpoint = None;
            activity.abcode = None;
            activity.abprogram = None;
        }
        Ok(ids)
    }

    /// Restore INITIAL and remove descendants, retaining the subject's data
    /// containers in their separate durable authority.
    pub fn reset_subtree(&mut self, activity_id: &str) -> Result<Vec<String>, HostProblem> {
        let root = activity_id == self.root_id;
        let subject = self
            .activities
            .get(activity_id)
            .ok_or_else(|| missing(root))?;
        if !matches!(subject.mode, BtsMode::Initial | BtsMode::Complete) {
            return Err(ineligible(root));
        }
        let descendants = self
            .subtree_ids(activity_id)
            .into_iter()
            .filter(|id| id != activity_id)
            .collect::<Vec<_>>();
        if descendants
            .iter()
            .any(|id| self.activities[id].acquired_by.is_some())
        {
            return Err(condition("LOCKED", 100, 0));
        }
        for id in &descendants {
            self.activities.remove(id);
        }
        let subject = self
            .activities
            .get_mut(activity_id)
            .expect("validated subject");
        subject.mode = BtsMode::Initial;
        subject.completion = BtsCompletion::Incomplete;
        subject.checkpoint = None;
        subject.abcode = None;
        subject.abprogram = None;
        Ok(descendants)
    }

    /// Remove one direct child and its descendants. The caller deletes the
    /// matching activity indexes in the same atomic store transaction.
    pub fn delete_child(
        &mut self,
        parent_id: &str,
        child_name: &str,
    ) -> Result<Vec<String>, HostProblem> {
        let child = self
            .child(parent_id, child_name)
            .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
        if !matches!(child.mode, BtsMode::Initial | BtsMode::Complete) {
            return Err(condition("ACTIVITYERR", 109, 14));
        }
        let id = child.id.clone();
        let ids = self.subtree_ids(&id);
        if ids
            .iter()
            .any(|id| self.activities[id].acquired_by.is_some())
        {
            return Err(condition("LOCKED", 100, 0));
        }
        for id in &ids {
            self.activities.remove(id);
        }
        Ok(ids)
    }

    fn subtree_ids(&self, root_id: &str) -> Vec<String> {
        let mut ids = vec![root_id.to_string()];
        let mut offset = 0;
        while offset < ids.len() {
            let parent = ids[offset].clone();
            ids.extend(
                self.activities
                    .values()
                    .filter(|activity| activity.parent_id.as_deref() == Some(&parent))
                    .map(|activity| activity.id.clone()),
            );
            offset += 1;
        }
        ids
    }
}

fn missing(root: bool) -> HostProblem {
    if root {
        condition("PROCESSERR", 108, 5)
    } else {
        condition("ACTIVITYERR", 109, 8)
    }
}

fn ineligible(root: bool) -> HostProblem {
    if root {
        condition("PROCESSERR", 108, 14)
    } else {
        condition("ACTIVITYERR", 109, 14)
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process() -> BtsProcess {
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW").unwrap()
    }

    #[test]
    fn complete_root_suspend_and_resume_use_their_distinct_conditions() {
        let mut state = process();
        let root = state.root_id.clone();
        state.cancel_subtree(&root).unwrap();
        assert_eq!(
            state.set_suspended(&root, true),
            Err(condition("INVREQ", 16, 14))
        );
        assert_eq!(
            state.set_suspended(&root, false),
            Err(condition("PROCESSERR", 108, 14))
        );
    }

    #[test]
    fn initial_dormant_complete_and_reset_are_distinct() {
        let mut state = process();
        let root = state.root_id.clone();
        assert!(state.start(&root, Some("READY"), true).is_err());
        let first = state.start(&root, None, true).unwrap();
        assert_eq!(first.input_event, "DFHINITIAL");
        state
            .checkpoint(&root, first.activation_epoch, 4, "checkpoint-1")
            .unwrap();
        assert!(
            state
                .finish(
                    &root,
                    first.activation_epoch,
                    3,
                    BtsCompletion::Incomplete,
                    None,
                    None
                )
                .is_err()
        );
        state
            .finish(
                &root,
                first.activation_epoch,
                4,
                BtsCompletion::Incomplete,
                None,
                None,
            )
            .unwrap();
        assert_eq!(state.activities[&root].mode, BtsMode::Dormant);
        assert!(state.start(&root, None, false).is_err());
        state.set_suspended(&root, true).unwrap();
        assert!(state.start(&root, Some("READY"), true).is_err());
        let second = state.start(&root, Some("READY"), false).unwrap();
        assert_eq!(second.activation_epoch, 2);
        state
            .finish(
                &root,
                second.activation_epoch,
                4,
                BtsCompletion::Normal,
                None,
                None,
            )
            .unwrap();
        assert_eq!(state.activities[&root].mode, BtsMode::Complete);
        assert!(state.set_suspended(&root, false).is_err());
        assert!(state.start(&root, None, true).is_err());
        state.reset_subtree(&root).unwrap();
        assert_eq!(state.activities[&root].mode, BtsMode::Initial);
    }

    #[test]
    fn cancellation_and_delete_apply_to_descendants_without_touching_siblings() {
        let mut state = process();
        let root = state.root_id.clone();
        let left = BtsLifecycleStore::child_id("TYPE", "ORDER", &root, 1).unwrap();
        let right = BtsLifecycleStore::child_id("TYPE", "ORDER", &root, 2).unwrap();
        for (id, name) in [(&left, "LEFT"), (&right, "RIGHT")] {
            state.activities.insert(
                id.clone(),
                BtsActivity {
                    id: id.clone(),
                    name: name.into(),
                    parent_id: Some(root.clone()),
                    completion_event: Some(name.into()),
                    program: "CHILD".into(),
                    transid: "BTS1".into(),
                    userid: "USER".into(),
                    mode: BtsMode::Initial,
                    completion: BtsCompletion::Incomplete,
                    suspended: false,
                    activation_epoch: 0,
                    checkpoint: None,
                    acquired_by: None,
                    pending_uow: None,
                    abcode: None,
                    abprogram: None,
                },
            );
        }
        state.validate().unwrap();
        assert_eq!(state.cancel_subtree(&left).unwrap(), vec![left.clone()]);
        assert_eq!(state.activities[&left].completion, BtsCompletion::Forced);
        assert_eq!(state.activities[&right].mode, BtsMode::Initial);
        assert_eq!(
            state.delete_child(&root, "LEFT").unwrap(),
            vec![left.clone()]
        );
        assert!(!state.activities.contains_key(&left));
        assert!(state.activities.contains_key(&right));
        state.validate().unwrap();
    }
}
