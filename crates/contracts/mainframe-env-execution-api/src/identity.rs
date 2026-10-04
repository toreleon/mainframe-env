//! Exact identity domains used by execution admission and correlation.
//!
//! Request/trace IDs correlate transport; execution/run IDs attribute durable
//! and runtime occurrences. Selector/artifact IDs select code, principal and
//! capability IDs name admitted security subjects, and cancellation/idempotency
//! IDs name control and replay occurrences. None proves authentication,
//! availability, ownership or authorization merely by being well formed.

use crate::InvocationLimits;
use std::fmt;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        /// Case-sensitive owned identity checked against the execution alphabet and byte ceiling.
        /// Equality and ordering compare exact text; different identity types are not interchangeable.
        pub struct $name(String);
        impl $name {
            /// Require nonempty bounded ASCII letters/digits or `. _ - : / @`.
            /// No trimming, case folding, lookup or authority minting occurs.
            pub fn new(
                value: impl Into<String>,
                limits: InvocationLimits,
            ) -> Result<Self, IdentityProblem> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > limits.max_identity_bytes
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
                    })
                {
                    return Err(IdentityProblem::Invalid);
                }
                Ok(Self(value))
            }
            #[must_use]
            /// Borrow the exact validated spelling without allocation or normalization.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

opaque_id!(RequestId);
opaque_id!(ExecutionId);
opaque_id!(RunUnitId);
opaque_id!(PrincipalId);
opaque_id!(TraceId);
opaque_id!(CancellationId);
opaque_id!(IdempotencyKey);
opaque_id!(Selector);
opaque_id!(ArtifactRef);
opaque_id!(CapabilityId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Rejection of an empty, oversized or out-of-alphabet execution identity.
pub enum IdentityProblem {
    /// Input fails the shared identity shape; this is not an authentication decision.
    Invalid,
}

impl fmt::Display for IdentityProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "execution identity is invalid")
    }
}
impl std::error::Error for IdentityProblem {}
