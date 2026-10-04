use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

macro_rules! host_name {
    ($name:ident, $validate:expr, $deserialize_max:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        /// Owned host name normalized to ASCII uppercase and checked by its family grammar.
        /// Construction validates spelling and length; it grants no resource access.
        /// Dataset components are nonempty, at most eight bytes and admit alphanumerics/@#$*%.
        /// Simple names admit alphanumerics/@#$-_ without a component rule; resource names
        /// additionally admit dots, wildcards and slashes. Deserialization caps members at
        /// eight bytes and other names at 246; new() can impose a tighter byte bound.
        pub struct $name(String);
        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value, $deserialize_max).map_err(serde::de::Error::custom)
            }
        }
        impl $name {
            /// Normalize ASCII case and reject empty, over-byte-limit or invalid family spelling.
            /// Deserialization uses the family's fixed ceiling; callers may choose a tighter one.
            pub fn new(
                value: impl Into<String>,
                max_bytes: usize,
            ) -> Result<Self, HostNameProblem> {
                let value = value.into().to_ascii_uppercase();
                if value.is_empty() || value.len() > max_bytes || !($validate)(&value) {
                    return Err(HostNameProblem::Invalid);
                }
                Ok(Self(value))
            }
            #[must_use]
            /// Borrow the exact normalized name without resolving or authorizing its resource.
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

fn qualified(value: &str) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part.len() <= 8
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$' | b'*' | b'%')
            })
    })
}
fn simple(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
    })
}
fn resource(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'@' | b'#' | b'$' | b'.' | b'*' | b'%' | b'-' | b'_' | b'/'
            )
    })
}

host_name!(DatasetName, qualified, 246);
host_name!(MemberName, simple, 8);
host_name!(ProgramName, simple, 246);
host_name!(ClassName, simple, 246);
host_name!(MethodName, simple, 246);
host_name!(RuntimeServiceName, simple, 246);
host_name!(JobName, simple, 246);
host_name!(SessionId, simple, 246);
host_name!(ResourceName, resource, 246);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Host-name admission failure before any provider lookup or resource operation.
pub enum HostNameProblem {
    /// Empty, over-limit or invalid family spelling after ASCII case normalization.
    Invalid,
}
impl fmt::Display for HostNameProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host resource name is invalid")
    }
}
impl std::error::Error for HostNameProblem {}
