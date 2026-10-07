//! Versioned logical GSAM application-area contract, not a physical dataset adapter.
use crate::HostProblem;
use serde::{Deserialize, Serialize};

/// Complete application record framing admitted by the logical GSAM adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsGsamRecordFormat {
    /// Fixed-length data and optional leading control byte.
    F,
    /// Variable-length area beginning with a two-byte big-endian LL.
    V,
    /// Undefined-length data with a separate owned four-byte length operand.
    U,
}

/// Declared access-method applicability; this adapter does not perform device I/O.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsGsamAccessMethod {
    /// BSAM permits fixed, variable and undefined records.
    Bsam,
    /// VSAM permits fixed and variable records.
    Vsam,
}

/// Explicit carriage-control selection retained in the complete application area.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsGsamControl {
    /// No control byte precedes application data.
    None,
    /// Retain the ASA control byte at the format-defined offset.
    Asa,
    /// Retain the machine control byte at the format-defined offset.
    Machine,
}

/// Resolved characteristics supplied by signed metadata. No label/JCL inference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsGsamFormat {
    /// Closed format-contract version; only version 1 is admitted.
    pub version: u8,
    /// Declared complete application-area framing.
    pub record_format: ImsGsamRecordFormat,
    /// Declared method restricting format applicability.
    pub access_method: ImsGsamAccessMethod,
    /// Explicit owned bound corresponding to BLKSIZE, not block allocation.
    pub block_size: u32,
    /// Explicit control-byte interpretation without label or JCL inference.
    pub control: ImsGsamControl,
}

impl ImsGsamFormat {
    /// Bounds describe complete application I/O areas, including LL/control bytes.
    pub fn validate(&self, minimum: usize, maximum: usize) -> Result<(), HostProblem> {
        if self.version != 1 {
            return Err(HostProblem::Unsupported);
        }
        let control = usize::from(self.control != ImsGsamControl::None);
        let (floor, ceiling) = match self.record_format {
            ImsGsamRecordFormat::F => (1 + control, 32760),
            ImsGsamRecordFormat::V => (
                2 + control,
                if self.access_method == ImsGsamAccessMethod::Bsam {
                    32754
                } else {
                    32756
                },
            ),
            ImsGsamRecordFormat::U => (12, 32760),
        };
        if minimum < floor
            || minimum > maximum
            || maximum > ceiling
            || self.block_size == 0
            || self.block_size > 32760
            || maximum
                + usize::from(
                    self.record_format == ImsGsamRecordFormat::V
                        && self.access_method == ImsGsamAccessMethod::Bsam,
                ) * 2
                > self.block_size as usize
            || self.record_format == ImsGsamRecordFormat::F && minimum != maximum
            || self.record_format == ImsGsamRecordFormat::U
                && self.access_method != ImsGsamAccessMethod::Bsam
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }

    /// Complete owned area: trailing storage is never silently consumed/discarded.
    pub fn validate_area(
        &self,
        data: &[u8],
        minimum: usize,
        maximum: usize,
    ) -> Result<(), HostProblem> {
        self.validate(minimum, maximum)?;
        if data.len() < minimum || data.len() > maximum {
            return Err(HostProblem::Malformed);
        }
        if self.record_format == ImsGsamRecordFormat::V
            && (data.len() < 2 || usize::from(u16::from_be_bytes([data[0], data[1]])) != data.len())
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(kind: ImsGsamRecordFormat, access: ImsGsamAccessMethod) -> ImsGsamFormat {
        ImsGsamFormat {
            version: 1,
            record_format: kind,
            access_method: access,
            block_size: 32760,
            control: ImsGsamControl::None,
        }
    }

    #[test]
    fn gsam_format_source_limits_and_complete_application_areas() {
        let v = format(ImsGsamRecordFormat::V, ImsGsamAccessMethod::Bsam);
        assert_eq!(v.validate(2, 32754), Ok(()));
        assert_eq!(v.validate(2, 32755), Err(HostProblem::Malformed));
        assert_eq!(
            format(ImsGsamRecordFormat::V, ImsGsamAccessMethod::Vsam).validate(2, 32756),
            Ok(())
        );
        assert_eq!(v.validate_area(&[0, 2], 2, 8), Ok(()));
        assert_eq!(v.validate_area(&[0, 5, 0, 0xff, 0x41], 2, 8), Ok(()));
        for area in [&[0, 0][..], &[0, 1], &[0, 3], &[0, 2, 0], &[0, 6, 0, 0]] {
            assert_eq!(v.validate_area(area, 2, 8), Err(HostProblem::Malformed));
        }
        let u = format(ImsGsamRecordFormat::U, ImsGsamAccessMethod::Bsam);
        assert_eq!(u.validate(12, 32760), Ok(()));
        assert_eq!(u.validate(11, 32760), Err(HostProblem::Malformed));
        assert_eq!(u.validate(12, 32761), Err(HostProblem::Malformed));
        assert_eq!(
            format(ImsGsamRecordFormat::U, ImsGsamAccessMethod::Vsam).validate(12, 16),
            Err(HostProblem::Malformed)
        );
        assert_eq!(u.validate_area(b"0123456789AB", 12, 16), Ok(()));
        let mut limited = v.clone();
        limited.block_size = 9;
        assert_eq!(limited.validate(2, 8), Err(HostProblem::Malformed));
        limited.block_size = 10;
        assert_eq!(limited.validate(2, 8), Ok(()));
        limited.version = 2;
        assert_eq!(limited.validate(2, 8), Err(HostProblem::Unsupported));
    }

    #[test]
    fn gsam_format_dto_is_closed_and_controls_are_explicit() {
        let mut v = format(ImsGsamRecordFormat::V, ImsGsamAccessMethod::Bsam);
        for control in [ImsGsamControl::Asa, ImsGsamControl::Machine] {
            v.control = control;
            assert_eq!(v.validate(2, 8), Err(HostProblem::Malformed));
            assert_eq!(v.validate_area(&[0, 3, 0xff], 3, 8), Ok(()));
        }
        let mut json = serde_json::to_value(&v).unwrap();
        json["raw_pcb"] = serde_json::json!([0, 0, 0, 12]);
        assert!(serde_json::from_value::<ImsGsamFormat>(json).is_err());
        assert_eq!(
            format(ImsGsamRecordFormat::F, ImsGsamAccessMethod::Bsam).validate(2, 8),
            Err(HostProblem::Malformed)
        );
    }
}
