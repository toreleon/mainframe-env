use crate::InvocationLimits;
use std::fmt;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);
        impl $name {
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
pub enum IdentityProblem {
    Invalid,
}

impl fmt::Display for IdentityProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "execution identity is invalid")
    }
}
impl std::error::Error for IdentityProblem {}
