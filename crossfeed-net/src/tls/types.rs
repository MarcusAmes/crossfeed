use std::path::PathBuf;

#[derive(Debug)]
pub struct CaMaterial {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

#[derive(Debug)]
pub struct CaMaterialPaths {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

pub struct CaCertificate {
    pub material: CaMaterial,
    pub cert: rcgen::Certificate,
}

#[derive(Debug, Clone)]
pub struct LeafCertificate {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

#[derive(Debug)]
pub struct TlsError {
    pub kind: TlsErrorKind,
    pub message: String,
}

#[derive(Debug)]
pub enum TlsErrorKind {
    Rcgen,
    Io,
    OpenSsl,
}

impl TlsError {
    pub fn new(kind: TlsErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
