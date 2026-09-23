//! Source-reviewed non-CVDA results for EXTRACT TCPIP.

/// One task TCP/IP result with an exact COBOL receiving representation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsTcpipOutput {
    /// DNS name of the client, when supplied by trusted ingress.
    ClientName,
    /// Client-name buffer capacity and returned length.
    ClientNameLength,
    /// DNS name of the server, when supplied by trusted ingress.
    ServerName,
    /// Server-name buffer capacity and returned length.
    ServerNameLength,
    /// Printable client IP address.
    ClientAddress,
    /// Client-address buffer capacity and returned length.
    ClientAddressLength,
    /// Client IPv4 address in four binary bytes.
    ClientAddressNumeric,
    /// Client IPv6 address in sixteen binary bytes.
    ClientAddress6Numeric,
    /// Printable local IP address.
    ServerAddress,
    /// Server-address buffer capacity and returned length.
    ServerAddressLength,
    /// Local IPv4 address in four binary bytes.
    ServerAddressNumeric,
    /// Local IPv6 address in sixteen binary bytes.
    ServerAddress6Numeric,
    /// TCPIPSERVICE resource name.
    TcpipService,
    /// Five-character local port number.
    PortNumber,
    /// Local port number as fullword binary.
    PortNumberNumeric,
    /// Maximum HTTP server input bytes as fullword binary.
    MaxDataLength,
}

/// Exact non-CVDA spellings in their reserved MCEP v2 tag order.
pub const CICS_TCPIP_OUTPUT_NAMES: &[(&str, CicsTcpipOutput)] = &[
    ("CLIENTNAME", CicsTcpipOutput::ClientName),
    ("CNAMELENGTH", CicsTcpipOutput::ClientNameLength),
    ("SERVERNAME", CicsTcpipOutput::ServerName),
    ("SNAMELENGTH", CicsTcpipOutput::ServerNameLength),
    ("CLIENTADDR", CicsTcpipOutput::ClientAddress),
    ("CADDRLENGTH", CicsTcpipOutput::ClientAddressLength),
    ("CLIENTADDRNU", CicsTcpipOutput::ClientAddressNumeric),
    ("CLNTADDR6NU", CicsTcpipOutput::ClientAddress6Numeric),
    ("SERVERADDR", CicsTcpipOutput::ServerAddress),
    ("SADDRLENGTH", CicsTcpipOutput::ServerAddressLength),
    ("SERVERADDRNU", CicsTcpipOutput::ServerAddressNumeric),
    ("SRVRADDR6NU", CicsTcpipOutput::ServerAddress6Numeric),
    ("TCPIPSERVICE", CicsTcpipOutput::TcpipService),
    ("PORTNUMBER", CicsTcpipOutput::PortNumber),
    ("PORTNUMNU", CicsTcpipOutput::PortNumberNumeric),
    ("MAXDATALEN", CicsTcpipOutput::MaxDataLength),
];

impl CicsTcpipOutput {
    /// Resolve one exact command option spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        CICS_TCPIP_OUTPUT_NAMES
            .iter()
            .find_map(|(spelling, identity)| (*spelling == name).then_some(*identity))
    }

    /// Exact command option spelling.
    pub const fn name(self) -> &'static str {
        CICS_TCPIP_OUTPUT_NAMES[self as usize].0
    }

    /// Reserved MCEP v2 output tag.
    pub const fn tag(self) -> u16 {
        717 + self as u16
    }

    /// Decode one reserved MCEP v2 output tag.
    pub fn from_tag(tag: u16) -> Option<Self> {
        CICS_TCPIP_OUTPUT_NAMES
            .get(usize::from(tag.checked_sub(717)?))
            .map(|(_, identity)| *identity)
    }

    /// Whether this field is an input/output buffer length.
    pub const fn buffer_length(self) -> bool {
        matches!(
            self,
            Self::ClientNameLength
                | Self::ServerNameLength
                | Self::ClientAddressLength
                | Self::ServerAddressLength
        )
    }

    /// Whether this field receives fullword numeric data.
    pub const fn fullword(self) -> bool {
        matches!(self, Self::PortNumberNumeric | Self::MaxDataLength)
    }

    /// Whether this field is fixed-width raw bytes.
    pub const fn raw(self) -> bool {
        matches!(
            self,
            Self::ClientAddressNumeric
                | Self::ClientAddress6Numeric
                | Self::ServerAddressNumeric
                | Self::ServerAddress6Numeric
        )
    }
}
