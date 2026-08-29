use std::cmp::Ordering;
use std::fmt;

/// Built-in code page. The V1 fixture contract requires CP037.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodePage {
    Cp037,
}

impl CodePage {
    pub fn from_ccsid(ccsid: u16) -> Result<Self, EncodingProblem> {
        match ccsid {
            37 => Ok(Self::Cp037),
            _ => Err(EncodingProblem::UnsupportedCodePage(ccsid)),
        }
    }

    #[must_use]
    pub const fn ccsid(self) -> u16 {
        match self {
            Self::Cp037 => 37,
        }
    }

    /// Decodes each CP037 byte through the owned Latin-1 mapping.
    pub fn decode(self, input: &[u8], max_output_bytes: usize) -> Result<String, EncodingProblem> {
        let mut output = String::with_capacity(input.len());
        for byte in input {
            let latin1 = CP037_TO_LATIN1[usize::from(*byte)];
            let character = char::from(latin1);
            if output.len().saturating_add(character.len_utf8()) > max_output_bytes {
                return Err(EncodingProblem::OutputLimitExceeded);
            }
            output.push(character);
        }
        Ok(output)
    }

    /// Encodes Unicode values representable by the owned CP037 Latin-1 mapping.
    pub fn encode(self, input: &str, max_output_bytes: usize) -> Result<Vec<u8>, EncodingProblem> {
        if input.chars().count() > max_output_bytes {
            return Err(EncodingProblem::OutputLimitExceeded);
        }
        let mut output = Vec::with_capacity(input.len());
        for character in input.chars() {
            let value = u32::from(character);
            let latin1 = u8::try_from(value).map_err(|_| EncodingProblem::UnmappableCharacter)?;
            let byte = CP037_TO_LATIN1
                .iter()
                .position(|candidate| *candidate == latin1)
                .and_then(|position| u8::try_from(position).ok())
                .ok_or(EncodingProblem::UnmappableCharacter)?;
            output.push(byte);
        }
        Ok(output)
    }
}

/// EBCDIC native byte collation.
#[must_use]
pub fn compare_ebcdic(left: &[u8], right: &[u8]) -> Ordering {
    left.cmp(right)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingProblem {
    UnsupportedCodePage(u16),
    UnmappableCharacter,
    OutputLimitExceeded,
    InvalidDecimal,
    DecimalOverflow,
    InvalidWidth,
}

impl fmt::Display for EncodingProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCodePage(ccsid) => write!(formatter, "unsupported CCSID {ccsid}"),
            Self::UnmappableCharacter => write!(formatter, "character is not representable"),
            Self::OutputLimitExceeded => write!(formatter, "encoding output limit exceeded"),
            Self::InvalidDecimal => write!(formatter, "decimal bytes are invalid"),
            Self::DecimalOverflow => write!(formatter, "decimal value exceeds its precision"),
            Self::InvalidWidth => write!(formatter, "binary or decimal width is invalid"),
        }
    }
}

impl std::error::Error for EncodingProblem {}

// IBM-037 to ISO-8859-1. Control bytes retain their Latin-1 control values.
const CP037_TO_LATIN1: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x85, 0x09, 0x86, 0x7f, 0x87, 0x8d, 0x8e, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x8f, 0x0a, 0x08, 0x97, 0x18, 0x19, 0x9c, 0x9d, 0x1c, 0x1d, 0x1e, 0x1f,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x92, 0x17, 0x1b, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x05, 0x06, 0x07,
    0x90, 0x91, 0x16, 0x93, 0x94, 0x95, 0x96, 0x04, 0x98, 0x99, 0x9a, 0x9b, 0x14, 0x15, 0x9e, 0x1a,
    0x20, 0xa0, 0xe2, 0xe4, 0xe0, 0xe1, 0xe3, 0xe5, 0xe7, 0xf1, 0xa2, 0x2e, 0x3c, 0x28, 0x2b, 0x7c,
    0x26, 0xe9, 0xea, 0xeb, 0xe8, 0xed, 0xee, 0xef, 0xec, 0xdf, 0x21, 0x24, 0x2a, 0x29, 0x3b, 0x5e,
    0x2d, 0x2f, 0xc2, 0xc4, 0xc0, 0xc1, 0xc3, 0xc5, 0xc7, 0xd1, 0xa6, 0x2c, 0x25, 0x5f, 0x3e, 0x3f,
    0xf8, 0xc9, 0xca, 0xcb, 0xc8, 0xcd, 0xce, 0xcf, 0xcc, 0x60, 0x3a, 0x23, 0x40, 0x27, 0x3d, 0x22,
    0xd8, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0xab, 0xbb, 0xf0, 0xfd, 0xfe, 0xb1,
    0xb0, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0xaa, 0xba, 0xe6, 0xb8, 0xc6, 0xa4,
    0xb5, 0x7e, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0xa1, 0xbf, 0xd0, 0x5b, 0xde, 0xae,
    0xac, 0xa3, 0xa5, 0xb7, 0xa9, 0xa7, 0xb6, 0xbc, 0xbd, 0xbe, 0xdd, 0xa8, 0xaf, 0x5d, 0xb4, 0xd7,
    0x7b, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0xad, 0xf4, 0xf6, 0xf2, 0xf3, 0xf5,
    0x7d, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0xb9, 0xfb, 0xfc, 0xf9, 0xfa, 0xff,
    0x5c, 0xf7, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0xb2, 0xd4, 0xd6, 0xd2, 0xd3, 0xd5,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xb3, 0xdb, 0xdc, 0xd9, 0xda, 0x9f,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp037_roundtrip_for_cobol_fixture_characters() {
        let page = CodePage::from_ccsid(37).unwrap();
        let input = "HELLO 1234, WORLD!";
        let encoded = page.encode(input, 128).unwrap();
        assert_eq!(page.decode(&encoded, 128).unwrap(), input);
    }

    #[test]
    fn unsupported_page_is_explicit() {
        assert_eq!(
            CodePage::from_ccsid(930),
            Err(EncodingProblem::UnsupportedCodePage(930))
        );
    }

    #[test]
    fn output_is_bounded() {
        assert_eq!(
            CodePage::Cp037.encode("TOO LONG", 2),
            Err(EncodingProblem::OutputLimitExceeded)
        );
    }
}
