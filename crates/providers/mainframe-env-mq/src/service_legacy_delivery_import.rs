//! Retained rich-store marker identity; import setup is fixture-only.

pub(crate) const RICH_MARKER_SCHEMA: &str = "mainframe-env.mq-row-store@2";

#[cfg(test)]
#[path = "service_legacy_delivery_import/fixtures.rs"]
mod fixtures;
#[cfg(test)]
pub(crate) use fixtures::*;
