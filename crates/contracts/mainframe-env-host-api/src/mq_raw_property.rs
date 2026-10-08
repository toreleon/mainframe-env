//! Owned reconciled MQIMPO1 and MQCHARV observations with four-byte null slots.
//!
//! This private, non-Serde profile is ASCII/NormalBigEndian only. It reconciles
//! incomplete vendor declarations using pinned field descriptions and complete
//! MQCHARV declarations; it does not certify CMQIMPOV or a universal pointer ABI.
//! Capturing bytes gives neither call admission nor pointer, handle, SAF or UOW
//! authority. The embedding must separately validate compiled groups and freeze
//! its trusted profile. Requested/returned value encodings never select storage
//! encoding. Complete captured groups, including suffixes, remain exact.

use crate::mq_mqi::property::{MqPropertyInquiryObservation, MqPropertyObservation};
use crate::mq_mqi::{MqMqiCall, MqMqiLimits};
use crate::mq_raw_layout::{
    MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding, mq_raw_cobol_long,
};
use crate::mq_status::MqReviewedStatus;
use std::ops::Range;

mod generated {
    use super::*;
    include!("mq_raw_property/generated.rs");
}
#[cfg(test)]
mod tests;

pub use generated::MQ_RAW_PROPERTY_PROJECTION_SHA256;

/// Complete named layouts under the explicitly owned null-slot storage profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawPropertyKind {
    /// Standalone CHARV: four null bytes followed by four signed MQLONGs.
    CharvNullSlot4AsciiNormal,
    /// Reconciled IMPO1: Reserved1 is four chars and TypeString is eight bytes.
    Impo1NullSlot4AsciiNormal,
}

/// Storage classes retain characters separately from pointer replacement bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawPropertyFieldKind {
    /// Four-byte signed binary observation; capture permits the entire i32 range.
    Long,
    /// Fixed raw character bytes; no trimming, padding or transcoding on capture.
    Characters,
    /// Four all-zero bytes, never an address or executable token.
    NullSlot,
}

/// Generator-owned field identity relative to its complete structure prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawPropertyField {
    /// Source field name, including `ReturnedName.` for embedded CHARV members.
    pub name: &'static str,
    /// Exact storage class under this owned profile.
    pub kind: MqRawPropertyFieldKind,
    /// Byte offset from the actual containing structure start.
    pub offset: usize,
    /// Fixed byte width, without host pointer-size inference.
    pub width: usize,
    defined_standard_output: bool,
}

/// Complete generated layout; CHARV has neither StrucId nor Version fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRawPropertyLayout {
    /// Named owned profile; no alternative pointer width is inferred.
    pub kind: MqRawPropertyKind,
    /// Complete required prefix in bytes, excluding addressed name storage.
    pub prefix_bytes: usize,
    /// Every declared member in order, including reserved and output bytes.
    pub fields: &'static [MqRawPropertyField],
    identifier: Option<[u8; 4]>,
    version: Option<i32>,
}

/// Returns reviewed descriptors; it performs no runtime admission or allocation.
pub fn mq_raw_property_layout(kind: MqRawPropertyKind) -> &'static MqRawPropertyLayout {
    generated::LAYOUTS
        .iter()
        .find(|layout| layout.kind == kind)
        .expect("generated complete raw property layout")
}

/// Failures never write the supplied group or reconstruct pointer authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawPropertyProblem {
    /// Trusted structure encoding is not this reviewed ASCII/big-endian profile.
    UnsupportedEncoding,
    /// Truncated prefix, misaligned start or group exceeding the owned byte bound.
    Capacity,
    /// Checked allocation failed before any publication or destination write.
    Allocation,
    /// IMPO structure ID differs from the reviewed four character bytes.
    StructureIdentifier,
    /// IMPO version is negative, newer or otherwise outside the reviewed version.
    Version,
    /// An exact named field is missing from this layout.
    Field,
    /// A requested accessor or write uses a different storage class.
    FieldKind,
    /// Pointer replacement contains nonzero bytes; dereferencing is unsupported.
    Pointer,
    /// Offset is absent/nonpositive or addresses outside the fixed containing group.
    Offset,
    /// Output buffer size is zero, negative, truncated or exceeds the group.
    BufferCapacity,
    /// Output/protected byte ranges intersect or a protected range is malformed.
    Overlap,
    /// A scalar write cannot fit the compiled PIC S9(9) BINARY domain.
    CobolLongRange,
    /// Output is undefined, outside the existing standard observation or wrong call.
    OutputPending,
    /// Destination capacity, encoding or any captured byte changed before commit.
    StaleCapture,
    /// Final batch is empty or exceeds the finite plan bound.
    BatchCapacity,
}

/// Exact captured field value; character observations can contain null/high bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRawPropertyValue<'a> {
    /// Signed normal-endian MQLONG; not narrowed to the COBOL input domain.
    Long(i32),
    /// Complete fixed character bytes, including Reserved1 and TypeString.
    Characters(&'a [u8]),
    /// Exact all-zero four-byte replacement, not a pointer.
    NullSlot(&'a [u8]),
}

/// Immutable capture of a complete fixed containing group, bounded to 64 KiB.
/// `start` is the actual IMPO/standalone CHARV start inside that group. Embedded
/// CHARV offsets resolve from IMPO start, never from its nested member or wrapper.
#[derive(Debug, Eq, PartialEq)]
pub struct MqRawPropertyCapture {
    layout: &'static MqRawPropertyLayout,
    start: usize,
    original: Vec<u8>,
    encoding: MqRawStructureEncoding,
}

fn supported(encoding: MqRawStructureEncoding) -> bool {
    encoding.numbers == MqRawNumberEncoding::NormalBigEndian
        && encoding.characters == MqRawCharacterEncoding::AsciiCompatible
}

impl MqRawPropertyCapture {
    /// Captures only after full prefix/ID/version/null-slot and byte-bound checks.
    /// Options and other scalar observations remain raw, without semantic defaults.
    pub fn capture(
        kind: MqRawPropertyKind,
        group: &[u8],
        start: usize,
        encoding: MqRawStructureEncoding,
    ) -> Result<Self, MqRawPropertyProblem> {
        if !supported(encoding) {
            return Err(MqRawPropertyProblem::UnsupportedEncoding);
        }
        let layout = mq_raw_property_layout(kind);
        let end = start
            .checked_add(layout.prefix_bytes)
            .filter(|end| *end <= group.len())
            .ok_or(MqRawPropertyProblem::Capacity)?;
        if group.len() > generated::MAX_GROUP_BYTES || !start.is_multiple_of(4) {
            return Err(MqRawPropertyProblem::Capacity);
        }
        let prefix = &group[start..end];
        if let Some(identifier) = layout.identifier
            && prefix[..identifier.len()] != identifier
        {
            return Err(MqRawPropertyProblem::StructureIdentifier);
        }
        for field in layout.fields {
            let bytes = &prefix[field.offset..field.offset + field.width];
            if field.kind == MqRawPropertyFieldKind::NullSlot && bytes != [0; 4] {
                return Err(MqRawPropertyProblem::Pointer);
            }
            if field.name == "Version"
                && Some(i32::from_be_bytes(
                    bytes.try_into().expect("generated long width"),
                )) != layout.version
            {
                return Err(MqRawPropertyProblem::Version);
            }
        }
        let mut original = Vec::new();
        original
            .try_reserve_exact(group.len())
            .map_err(|_| MqRawPropertyProblem::Allocation)?;
        original.extend_from_slice(group);
        Ok(Self {
            layout,
            start,
            original,
            encoding,
        })
    }

    /// Exact generated field identities and complete prefix width.
    pub fn layout(&self) -> &'static MqRawPropertyLayout {
        self.layout
    }

    /// Explicit trusted structure encoding captured independently of body fields.
    pub fn encoding(&self) -> MqRawStructureEncoding {
        self.encoding
    }

    /// Actual supplied containing-group capacity, including preserved suffix bytes.
    pub fn capacity(&self) -> usize {
        self.original.len()
    }

    /// Entire unchanged containing group, without normalization or re-encoding.
    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// Complete prefix at its actual start; wrapper and suffix are separate bytes.
    pub fn prefix(&self) -> &[u8] {
        &self.original[self.start..self.start + self.layout.prefix_bytes]
    }

    fn field(&self, name: &str) -> Result<&'static MqRawPropertyField, MqRawPropertyProblem> {
        self.layout
            .fields
            .iter()
            .find(|field| field.name == name)
            .ok_or(MqRawPropertyProblem::Field)
    }

    /// Reads every named field including unsupported diagnostic scalar values.
    pub fn value(&self, name: &str) -> Result<MqRawPropertyValue<'_>, MqRawPropertyProblem> {
        let field = self.field(name)?;
        let bytes = &self.prefix()[field.offset..field.offset + field.width];
        Ok(match field.kind {
            MqRawPropertyFieldKind::Long => MqRawPropertyValue::Long(i32::from_be_bytes(
                bytes.try_into().expect("generated long width"),
            )),
            MqRawPropertyFieldKind::Characters => MqRawPropertyValue::Characters(bytes),
            MqRawPropertyFieldKind::NullSlot => MqRawPropertyValue::NullSlot(bytes),
        })
    }

    /// Reads one signed MQLONG without interpreting sentinels, options or CCSID.
    pub fn long(&self, name: &str) -> Result<i32, MqRawPropertyProblem> {
        match self.value(name)? {
            MqRawPropertyValue::Long(value) => Ok(value),
            _ => Err(MqRawPropertyProblem::FieldKind),
        }
    }

    fn charv_name(&self, name: &str) -> &'static str {
        match (self.layout.kind, name) {
            (MqRawPropertyKind::Impo1NullSlot4AsciiNormal, "VSOffset") => "ReturnedName.VSOffset",
            (MqRawPropertyKind::Impo1NullSlot4AsciiNormal, "VSBufSize") => "ReturnedName.VSBufSize",
            (MqRawPropertyKind::CharvNullSlot4AsciiNormal, "VSOffset") => "VSOffset",
            (MqRawPropertyKind::CharvNullSlot4AsciiNormal, "VSBufSize") => "VSBufSize",
            _ => unreachable!("private finite CHARV accessor"),
        }
    }

    /// Resolves only a positive explicit output offset/capacity beyond the prefix.
    /// Negative offsets, length-derived capacity, null-terminated input and actual
    /// pointers need separate profiles. Protected ranges use containing-group
    /// coordinates and must not overlap this addressed output buffer.
    pub fn charv_region(
        &self,
        protected: &[Range<usize>],
    ) -> Result<Range<usize>, MqRawPropertyProblem> {
        let offset = usize::try_from(self.long(self.charv_name("VSOffset"))?)
            .map_err(|_| MqRawPropertyProblem::Offset)?;
        if offset < self.layout.prefix_bytes {
            return Err(MqRawPropertyProblem::Offset);
        }
        let size = usize::try_from(self.long(self.charv_name("VSBufSize"))?)
            .map_err(|_| MqRawPropertyProblem::BufferCapacity)?;
        if size == 0 {
            return Err(MqRawPropertyProblem::BufferCapacity);
        }
        let start = self
            .start
            .checked_add(offset)
            .ok_or(MqRawPropertyProblem::Offset)?;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= self.capacity())
            .ok_or(MqRawPropertyProblem::BufferCapacity)?;
        let range = start..end;
        if protected.len() > generated::MAX_BATCH_PLANS {
            return Err(MqRawPropertyProblem::Overlap);
        }
        for other in protected {
            if other.start > other.end || other.end > self.capacity() || overlaps(&range, other) {
                return Err(MqRawPropertyProblem::Overlap);
            }
        }
        Ok(range)
    }

    /// Returns exact addressed buffer bytes; this neither decodes a name nor grants
    /// permission to execute a property operation or resolve a process CCSID.
    pub fn charv_bytes(&self, protected: &[Range<usize>]) -> Result<&[u8], MqRawPropertyProblem> {
        Ok(&self.original[self.charv_region(protected)?])
    }

    /// Prepares the existing closed, reviewed standard inquiry observations only.
    /// This writes IMPO encoding/string CCSID/name length/CCSID and the supplied
    /// name prefix. PD, Type, Value and DataLength are separate ABI arguments and
    /// are not written here. TypeString remains captured; unknown-type/conversion
    /// warnings cannot enter this plan. `Absent` leaves all outputs untouched.
    /// The caller still must perform request binding and live authority checks.
    pub fn prepare_inquiry(
        self,
        observation: &MqPropertyObservation,
        status: MqReviewedStatus,
        limits: MqMqiLimits,
        protected: &[Range<usize>],
    ) -> Result<MqRawPropertyWriteback, MqRawPropertyProblem> {
        if self.layout.kind != MqRawPropertyKind::Impo1NullSlot4AsciiNormal
            || status.call() != MqMqiCall::InquireProperty
            || observation
                .validate(MqMqiCall::InquireProperty, limits)
                .is_err()
            || !observation.validate_status(status)
        {
            return Err(MqRawPropertyProblem::OutputPending);
        }
        let mut patches = Vec::new();
        patches
            .try_reserve_exact(5)
            .map_err(|_| MqRawPropertyProblem::Allocation)?;
        match observation {
            MqPropertyObservation::Absent => {}
            MqPropertyObservation::Inquired(value) => {
                self.inquiry_patches(value, protected, &mut patches)?;
            }
            _ => return Err(MqRawPropertyProblem::OutputPending),
        }
        Ok(MqRawPropertyWriteback {
            capture: self,
            patches,
        })
    }

    fn inquiry_patches(
        &self,
        value: &MqPropertyInquiryObservation,
        protected: &[Range<usize>],
        patches: &mut Vec<Patch>,
    ) -> Result<(), MqRawPropertyProblem> {
        let buffer = self.charv_region(protected)?;
        let full_length =
            usize::try_from(value.name_length).map_err(|_| MqRawPropertyProblem::CobolLongRange)?;
        if value.returned_name.len() != full_length.min(buffer.len()) {
            return Err(MqRawPropertyProblem::BufferCapacity);
        }
        self.long_patch("ReturnedEncoding", value.returned_encoding, patches)?;
        if let Some(ccsid) = value.returned_ccsid {
            self.long_patch("ReturnedCCSID", ccsid, patches)?;
        }
        self.long_patch("ReturnedName.VSLength", value.name_length, patches)?;
        self.long_patch("ReturnedName.VSCCSID", value.name_ccsid, patches)?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(value.returned_name.len())
            .map_err(|_| MqRawPropertyProblem::Allocation)?;
        copied.extend_from_slice(&value.returned_name);
        patches.push(Patch {
            range: buffer.start..buffer.start + copied.len(),
            bytes: copied,
        });
        for patch in patches {
            if protected.iter().any(|other| overlaps(&patch.range, other)) {
                return Err(MqRawPropertyProblem::Overlap);
            }
        }
        Ok(())
    }

    fn long_patch(
        &self,
        name: &str,
        value: i32,
        patches: &mut Vec<Patch>,
    ) -> Result<(), MqRawPropertyProblem> {
        let field = self.field(name)?;
        if !field.defined_standard_output || field.kind != MqRawPropertyFieldKind::Long {
            return Err(MqRawPropertyProblem::OutputPending);
        }
        mq_raw_cobol_long(i64::from(value)).map_err(|_| MqRawPropertyProblem::CobolLongRange)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(field.width)
            .map_err(|_| MqRawPropertyProblem::Allocation)?;
        bytes.extend_from_slice(&value.to_be_bytes());
        patches.push(Patch {
            range: self.start + field.offset..self.start + field.offset + field.width,
            bytes,
        });
        Ok(())
    }
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    !a.is_empty() && !b.is_empty() && a.start < b.end && b.start < a.end
}

#[derive(Debug)]
struct Patch {
    range: Range<usize>,
    bytes: Vec<u8>,
}

/// Preencoded touched writes bound to the exact original containing group.
/// It serializes no live token/profile and has no dispatch or publication hooks.
#[derive(Debug)]
pub struct MqRawPropertyWriteback {
    capture: MqRawPropertyCapture,
    patches: Vec<Patch>,
}

impl MqRawPropertyWriteback {
    /// Checks encoding, capacity and every original byte before an infallible
    /// touched-range copy. Caller profile/frame checks must precede this final step.
    pub fn commit(
        self,
        group: &mut [u8],
        encoding: MqRawStructureEncoding,
    ) -> Result<(), MqRawPropertyProblem> {
        Self::commit_batch(std::slice::from_ref(&self), group, encoding)
    }

    /// Commits at most eight plans for the SAME fixed containing group. Every
    /// capture and cross-plan output overlap is checked before any byte changes;
    /// there are no callbacks, allocations or fallible lookups after that check.
    pub fn commit_batch(
        plans: &[Self],
        group: &mut [u8],
        encoding: MqRawStructureEncoding,
    ) -> Result<(), MqRawPropertyProblem> {
        if plans.is_empty() || plans.len() > generated::MAX_BATCH_PLANS {
            return Err(MqRawPropertyProblem::BatchCapacity);
        }
        for (index, plan) in plans.iter().enumerate() {
            if encoding != plan.capture.encoding || group != plan.capture.original {
                return Err(MqRawPropertyProblem::StaleCapture);
            }
            for previous in &plans[..index] {
                let prefix =
                    plan.capture.start..plan.capture.start + plan.capture.layout.prefix_bytes;
                let previous_prefix = previous.capture.start
                    ..previous.capture.start + previous.capture.layout.prefix_bytes;
                if overlaps(&prefix, &previous_prefix)
                    || plan.patches.iter().any(|a| {
                        overlaps(&a.range, &previous_prefix)
                            || previous
                                .patches
                                .iter()
                                .any(|b| overlaps(&a.range, &b.range))
                    })
                    || previous.patches.iter().any(|b| overlaps(&b.range, &prefix))
                {
                    return Err(MqRawPropertyProblem::Overlap);
                }
            }
        }
        for plan in plans {
            for patch in &plan.patches {
                group[patch.range.clone()].copy_from_slice(&patch.bytes);
            }
        }
        Ok(())
    }
}
