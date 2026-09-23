use super::{Invocation, Session};

pub(super) fn originating_task_for(session: &Session, invocation: &Invocation) -> String {
    if session.run_unit.is_empty() {
        invocation.run_unit_id.as_str().to_string()
    } else {
        session.run_unit.clone()
    }
}
