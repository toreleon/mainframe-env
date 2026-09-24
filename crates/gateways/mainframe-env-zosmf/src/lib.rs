//! Thin, bounded z/OSMF compatibility gateway.

#![forbid(unsafe_code)]

mod gateway;
#[path = "generated/zosmf_contracts.rs"]
mod zosmf_contracts;

pub use gateway::{
    Authentication, GatewayBody, GatewayCallContext, GatewayProblem, GatewayRequest,
    GatewayResponse, ZosmfBackend, ZosmfLimits, custom_route_ids, official_route_ids, router,
};
pub use zosmf_contracts::{
    ZOSMF_FAMILY_BACKENDS, ZOSMF_LEGACY_ROUTE_OPERATIONS, ZOSMF_NEW_ADVERTISED_ROUTE_COUNT,
    ZOSMF_NORMALIZATION_CONTRACT, ZOSMF_NORMALIZATION_SHA256, ZOSMF_NORMALIZED_FAMILY_COUNT,
    ZOSMF_NORMALIZED_HEADING_COUNT, ZOSMF_NORMALIZED_OPERATION_COUNT,
    ZOSMF_NORMALIZED_ROUTE_VARIANT_COUNT,
};

pub const ZOSMF_CONTRACT: &str = "mainframe-env.zosmf@1";
