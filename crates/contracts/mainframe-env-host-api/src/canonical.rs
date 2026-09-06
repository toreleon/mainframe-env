//! Explicit host encoding. See docs/contracts/EFFECT-CANONICAL-V1.md.
use crate::dataset::*;
use crate::names::*;
use crate::request::*;
use mainframe_env_execution_api::{BoundedPayload, IdempotencyKey, PrincipalId, RunUnitId};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const EFFECT_CANONICAL_SCHEMA: &str = "mainframe-env.effect-canonical@1";
pub const REQUEST_DIGEST_DOMAIN: &[u8] = b"mainframe-env.effect-request@1\0";
pub const RESULT_DIGEST_DOMAIN: &[u8] = b"mainframe-env.effect-result@1\0";
/// A hard ceiling for the canonical journal representation, not the provider's payload budget.
pub const MAX_CANONICAL_EFFECT_BYTES: usize = 64 * 1024 * 1024;

struct Encoder<'a> {
    sink: &'a mut dyn FnMut(&[u8]),
    size: usize,
    limit: usize,
}
impl Encoder<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), HostProblem> {
        let size = self
            .size
            .checked_add(bytes.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if size > self.limit {
            return Err(HostProblem::ResourceExhausted);
        }
        (self.sink)(bytes);
        self.size = size;
        Ok(())
    }
    fn tag(&mut self, value: u8) -> Result<(), HostProblem> {
        self.put(&[value])
    }
    fn length(&mut self, value: usize) -> Result<(), HostProblem> {
        self.put(
            &u64::try_from(value)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_le_bytes(),
        )
    }
    fn text(&mut self, value: &str) -> Result<(), HostProblem> {
        self.tag(1)?;
        self.length(value.len())?;
        self.put(value.as_bytes())
    }
    fn object(&mut self, name: &str, fields: usize) -> Result<(), HostProblem> {
        self.tag(0x40)?;
        self.text(name)?;
        self.length(fields)
    }
    fn variant(&mut self, name: &str, variant: &str, fields: usize) -> Result<(), HostProblem> {
        self.tag(0x41)?;
        self.text(name)?;
        self.text(variant)?;
        self.length(fields)
    }
}
trait Canonical {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem>;
    fn sequence(values: &[Self], out: &mut Encoder<'_>) -> Result<(), HostProblem>
    where
        Self: Sized,
    {
        out.tag(0x30)?;
        out.length(values.len())?;
        for value in values {
            value.encode(out)?;
        }
        Ok(())
    }
}
impl Canonical for str {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.text(self)
    }
}
impl Canonical for String {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.text(self)
    }
}
impl<T: Canonical + ?Sized> Canonical for &T {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        (*self).encode(out)
    }
}
impl Canonical for bool {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(if *self { 4 } else { 3 })
    }
}
impl Canonical for u8 {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(0x10)?;
        out.put(&[*self])
    }
    fn sequence(values: &[Self], out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(2)?;
        out.length(values.len())?;
        out.put(values)
    }
}
macro_rules! integer {
    ($kind:ty, $tag:expr) => {
        impl Canonical for $kind {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                out.tag($tag)?;
                out.put(&self.to_le_bytes())
            }
        }
    };
}
integer!(u16, 0x11);
integer!(u32, 0x12);
integer!(u64, 0x13);
integer!(u128, 0x14);
integer!(i8, 0x18);
integer!(i16, 0x19);
integer!(i32, 0x1a);
integer!(i64, 0x1b);
integer!(i128, 0x1c);
impl Canonical for usize {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(0x15)?;
        out.put(
            &u64::try_from(*self)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_le_bytes(),
        )
    }
}
impl<T: Canonical> Canonical for Option<T> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            None => out.tag(0x20),
            Some(value) => {
                out.tag(0x21)?;
                value.encode(out)
            }
        }
    }
}
impl<T: Canonical, E: Canonical> Canonical for Result<T, E> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Ok(value) => {
                out.tag(0x22)?;
                value.encode(out)
            }
            Err(value) => {
                out.tag(0x23)?;
                value.encode(out)
            }
        }
    }
}
impl<T: Canonical> Canonical for Vec<T> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        T::sequence(self, out)
    }
}
impl<T: Canonical> Canonical for [T] {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        T::sequence(self, out)
    }
}
impl<T: Canonical, const N: usize> Canonical for [T; N] {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        T::sequence(self, out)
    }
}
impl<K: Canonical + Ord, V: Canonical> Canonical for BTreeMap<K, V> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(0x31)?;
        out.length(self.len())?;
        for (key, value) in self {
            key.encode(out)?;
            value.encode(out)?;
        }
        Ok(())
    }
}
impl<A: Canonical, B: Canonical> Canonical for (A, B) {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.tag(0x32)?;
        out.length(2)?;
        self.0.encode(out)?;
        self.1.encode(out)
    }
}
macro_rules! named {
    ($($name:ident),+) => { $(impl Canonical for $name {
        fn encode(&self,out:&mut Encoder<'_>) -> Result<(),HostProblem> {
            out.tag(0x42)?; out.text(stringify!($name))?; out.text(self.as_str())
        }
    })+ };
}
named!(
    ClassName,
    DatasetName,
    JobName,
    MemberName,
    MethodName,
    ProgramName,
    ResourceName,
    RuntimeServiceName,
    SessionId,
    SecretRef,
    PrincipalId,
    RunUnitId,
    IdempotencyKey
);
impl Canonical for BoundedPayload {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("BoundedPayload", 2)?;
        out.text("bytes")?;
        self.bytes().encode(out)?;
        out.text("schema")?;
        self.schema().encode(out)
    }
}
fn encode<T: Canonical + ?Sized>(
    value: &T,
    domain: &[u8],
    limit: usize,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<usize, HostProblem> {
    let mut out = Encoder {
        sink,
        size: 0,
        limit,
    };
    out.put(domain)?;
    value.encode(&mut out)?;
    Ok(out.size)
}
fn digest<T: Canonical + ?Sized>(value: &T, domain: &[u8]) -> Result<[u8; 32], HostProblem> {
    let mut hash = Sha256::new();
    encode(value, domain, MAX_CANONICAL_EFFECT_BYTES, &mut |bytes| {
        hash.update(bytes)
    })?;
    Ok(hash.finalize().into())
}
/// SHA-256 over the versioned request domain and explicit typed bytes.
pub fn canonical_request_digest(value: &HostRequest) -> Result<[u8; 32], HostProblem> {
    digest(value, REQUEST_DIGEST_DOMAIN)
}
/// SHA-256 over the versioned outcome domain, including Ok/Err and error fields.
pub fn canonical_result_digest(
    value: &Result<HostResult, HostProblem>,
) -> Result<[u8; 32], HostProblem> {
    digest(value, RESULT_DIGEST_DOMAIN)
}
/// Count canonical bytes without materializing a diagnostic string or an encoded buffer.
pub fn canonical_request_size(value: &HostRequest, limit: usize) -> Result<usize, HostProblem> {
    encode(value, REQUEST_DIGEST_DOMAIN, limit, &mut |_| {})
}
/// Count a complete result, failing before exceeding the supplied budget.
pub fn canonical_result_size(
    value: &Result<HostResult, HostProblem>,
    limit: usize,
) -> Result<usize, HostProblem> {
    encode(value, RESULT_DIGEST_DOMAIN, limit, &mut |_| {})
}

mod generated;

#[cfg(test)]
mod tests;
