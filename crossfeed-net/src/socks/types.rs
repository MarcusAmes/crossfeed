#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksVersion {
    V4,
    V5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksCommand {
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksAddress {
    IpV4([u8; 4]),
    IpV6([u8; 16]),
    Domain(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksRequest {
    pub version: SocksVersion,
    pub command: SocksCommand,
    pub address: SocksAddress,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksReply {
    Succeeded,
    GeneralFailure,
    ConnectionNotAllowed,
    NetworkUnreachable,
    HostUnreachable,
    ConnectionRefused,
    TtlExpired,
    CommandNotSupported,
    AddressTypeNotSupported,
    Other(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksResponse {
    pub version: SocksVersion,
    pub reply: SocksReply,
    pub address: SocksAddress,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocksError {
    pub kind: SocksErrorKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksErrorKind {
    InvalidVersion,
    InvalidResponse,
    UnsupportedAddressType,
    UnexpectedEof,
}
