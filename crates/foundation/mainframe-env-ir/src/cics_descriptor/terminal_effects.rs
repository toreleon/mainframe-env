//! Effects owned by the local outboard terminal-control route.

use super::Effect;

pub(super) const OUTBOARD_READ_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalRead,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const OUTBOARD_WRITE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
pub(super) const OUTBOARD_WAIT_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalWrite,
    Effect::Suspension,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];

pub(super) const ROUTE_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::TerminalWrite,
    Effect::Clock,
    Effect::Suspension,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
