//! Bounded durable MQ request/reply, correlation, trigger, and syncpoint authority.

#![forbid(unsafe_code)]

mod service;

pub use service::{MqInstallReceipt, MqLimits, MqQueueDefinition, MqService, mq_providers};
