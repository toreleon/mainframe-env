//! Thin, bounded z/OSMF compatibility gateway.

#![forbid(unsafe_code)]

mod gateway;

pub use gateway::{
    Authentication, GatewayBody, GatewayProblem, GatewayRequest, GatewayResponse, ZosmfBackend,
    ZosmfLimits, custom_route_ids, official_route_ids, router,
};

pub const ZOSMF_CONTRACT: &str = "mainframe-env.zosmf@1";
