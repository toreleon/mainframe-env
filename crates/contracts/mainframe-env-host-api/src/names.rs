use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

macro_rules! host_name {
    ($name:ident, $validate:expr, $deserialize_max:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        /// Owned ASCII-uppercased host name validated for its resource domain.
        /// Construction checks the selected name grammar and byte ceiling; serde decoding uses
        /// the domain's fixed ceiling. Accepted names contain only ASCII domain characters.
        /// Dataset qualifiers have 1–8 characters separated by dots; dataset serde limits the
        /// complete name to 246 bytes. Member serde limits names to 8 bytes; other names to 246.
        /// Simple names admit letters, digits, @, #, $, - and _. Resource names also admit
        /// dots, *, %, and /. Dataset qualifiers admit letters, digits, @, #, $, * and %.
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
            /// Own and ASCII-uppercase a name within the caller's byte ceiling.
            /// Reject empty values or bytes outside this resource domain's accepted grammar.
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
            /// Borrow the validated, ASCII-uppercased name without allocating.
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
/// Name construction failed its domain grammar or byte bound.
pub enum HostNameProblem {
    /// The value is empty, oversized or contains a disallowed name character.
    Invalid,
}
impl fmt::Display for HostNameProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host resource name is invalid")
    }
}
impl std::error::Error for HostNameProblem {}
