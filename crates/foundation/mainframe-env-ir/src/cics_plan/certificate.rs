//! Source-reviewed result identities for EXTRACT CERTIFICATE.

/// One selected client-certificate result.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsCertificateOutput {
    /// Full binary certificate pointer.
    Certificate,
    /// Certificate byte length.
    Length,
    /// Serial number pointer.
    SerialNum,
    /// Serial number byte length.
    SerialNumLen,
    /// Certificate-associated user ID.
    UserId,
    /// Selected subject or issuer common-name pointer.
    CommonName,
    /// Common-name byte length.
    CommonNameLen,
    /// Country pointer.
    Country,
    /// Country byte length.
    CountryLen,
    /// State or province pointer.
    State,
    /// State or province byte length.
    StateLen,
    /// Locality pointer.
    Locality,
    /// Locality byte length.
    LocalityLen,
    /// Organization pointer.
    Organization,
    /// Organization byte length.
    OrganizationLen,
    /// Organization unit pointer.
    OrgUnit,
    /// Organization unit byte length.
    OrgUnitLen,
}

/// EXTRACT CERTIFICATE output spellings in their reserved v2 tag order.
pub const CICS_CERTIFICATE_OUTPUT_NAMES: &[(&str, CicsCertificateOutput)] = &[
    ("CERTIFICATE", CicsCertificateOutput::Certificate),
    ("LENGTH", CicsCertificateOutput::Length),
    ("SERIALNUM", CicsCertificateOutput::SerialNum),
    ("SERIALNUMLEN", CicsCertificateOutput::SerialNumLen),
    ("USERID", CicsCertificateOutput::UserId),
    ("COMMONNAME", CicsCertificateOutput::CommonName),
    ("COMMONNAMLEN", CicsCertificateOutput::CommonNameLen),
    ("COUNTRY", CicsCertificateOutput::Country),
    ("COUNTRYLEN", CicsCertificateOutput::CountryLen),
    ("STATE", CicsCertificateOutput::State),
    ("STATELEN", CicsCertificateOutput::StateLen),
    ("LOCALITY", CicsCertificateOutput::Locality),
    ("LOCALITYLEN", CicsCertificateOutput::LocalityLen),
    ("ORGANIZATION", CicsCertificateOutput::Organization),
    ("ORGANIZATLEN", CicsCertificateOutput::OrganizationLen),
    ("ORGUNIT", CicsCertificateOutput::OrgUnit),
    ("ORGUNITLEN", CicsCertificateOutput::OrgUnitLen),
];

impl CicsCertificateOutput {
    /// Resolve an exact source option spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        CICS_CERTIFICATE_OUTPUT_NAMES
            .iter()
            .find_map(|(spelling, output)| (*spelling == name).then_some(*output))
    }

    /// Exact source option spelling.
    pub const fn name(self) -> &'static str {
        CICS_CERTIFICATE_OUTPUT_NAMES[self as usize].0
    }

    /// Reserved v2 output tag.
    pub const fn tag(self) -> u16 {
        700 + self as u16
    }

    /// Decode one reserved v2 output tag.
    pub fn from_tag(tag: u16) -> Option<Self> {
        CICS_CERTIFICATE_OUTPUT_NAMES
            .get(usize::from(tag.checked_sub(700)?))
            .map(|(_, output)| *output)
    }

    /// Whether this output is a next-command-lifetime pointer.
    pub const fn pointer(self) -> bool {
        matches!(
            self,
            Self::Certificate
                | Self::SerialNum
                | Self::CommonName
                | Self::Country
                | Self::State
                | Self::Locality
                | Self::Organization
                | Self::OrgUnit
        )
    }

    /// Whether this output is a fullword binary length.
    pub const fn length(self) -> bool {
        !self.pointer() && !matches!(self, Self::UserId)
    }
}
