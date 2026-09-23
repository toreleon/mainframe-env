//! Durable operator-message authority for WRITE OPERATOR.

mod authority;

pub(super) use authority::list as load_operator_messages;
