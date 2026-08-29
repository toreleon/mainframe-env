//! Thin, bounded z/OSMF compatibility gateway.

#![forbid(unsafe_code)]

mod gateway;

pub use gateway::{
    Authentication, GatewayProblem, GatewayRequest, GatewayResponse, ZosmfBackend, ZosmfLimits,
    router,
};

pub const ZOSMF_CONTRACT: &str = "mainframe-env.zosmf@1";
