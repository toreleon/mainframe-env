//! Frozen lifecycle outbox payload, mechanically moved from the coordinator.
use crate::LifecycleEventKind;
const DOMAIN: &[u8] = b"mainframe-env.execution-lifecycle@1\0";
/// Exact version-one lifecycle notification bytes. This shared authority owns
/// the existing payload only; it grants no lifecycle transition or publication.
pub fn lifecycle_notification_payload(kind: &LifecycleEventKind) -> Vec<u8> {
    let mut payload = Vec::with_capacity(DOMAIN.len() + 9);
    payload.extend_from_slice(DOMAIN);
    match kind {
        LifecycleEventKind::Admitted => payload.push(1),
        LifecycleEventKind::Queued => payload.push(2),
        LifecycleEventKind::Claimed => payload.push(3),
        LifecycleEventKind::Started => payload.push(4),
        LifecycleEventKind::Completing => payload.push(5),
        LifecycleEventKind::EffectIntent { sequence } => {
            payload.push(6);
            payload.extend_from_slice(&sequence.to_be_bytes());
        }
        LifecycleEventKind::EffectResult { sequence } => {
            payload.push(7);
            payload.extend_from_slice(&sequence.to_be_bytes());
        }
        LifecycleEventKind::Suspended => payload.push(8),
        LifecycleEventKind::HandoffCompleted => payload.push(17),
        LifecycleEventKind::Resumed => payload.push(9),
        LifecycleEventKind::CancellationRequested => payload.push(10),
        LifecycleEventKind::Cancelled => payload.push(11),
        LifecycleEventKind::TimedOut => payload.push(12),
        LifecycleEventKind::Completed { return_code } => {
            payload.push(13);
            payload.extend_from_slice(&return_code.to_be_bytes());
        }
        LifecycleEventKind::Condition => payload.push(14),
        LifecycleEventKind::Abend => payload.push(15),
        LifecycleEventKind::Failed => payload.push(16),
    }
    payload
}
