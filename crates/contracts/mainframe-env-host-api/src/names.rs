use std::fmt;

macro_rules! host_name {
    ($name:ident, $validate:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);
        impl $name {
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

host_name!(DatasetName, qualified);
host_name!(MemberName, simple);
host_name!(ProgramName, simple);
host_name!(ClassName, simple);
host_name!(MethodName, simple);
host_name!(RuntimeServiceName, simple);
host_name!(JobName, simple);
host_name!(SessionId, simple);
host_name!(ResourceName, resource);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostNameProblem {
    Invalid,
}
impl fmt::Display for HostNameProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host resource name is invalid")
    }
}
impl std::error::Error for HostNameProblem {}
