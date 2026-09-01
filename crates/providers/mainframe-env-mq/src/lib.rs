//! Bounded durable MQ request/reply, correlation, trigger, and syncpoint authority.

#![forbid(unsafe_code)]

mod abi;
mod service;

pub use abi::mq_abi_library;

pub use service::{MqInstallReceipt, MqLimits, MqQueueDefinition, MqService, mq_providers};
