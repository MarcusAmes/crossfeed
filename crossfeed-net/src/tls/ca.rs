use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::Datelike;
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, IsCa};

use super::types::{CaCertificate, CaMaterial, CaMaterialPaths, TlsError, TlsErrorKind};

const DEFAULT_CA_VALIDITY_DAYS: u64 = 180;

pub fn generate_ca(common_name: &str) -> Result<CaCertificate, TlsError> {
    let mut params = CertificateParams::new(Vec::new());
    params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "Crossfeed");
    params.distinguished_name = dn;

    let now = SystemTime::now();
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2024, 1, 1);
    if let Some(valid_until) = now.checked_add(Duration::from_secs(DEFAULT_CA_VALIDITY_DAYS * 24 * 3600)) {
        let datetime = chrono::DateTime::<chrono::Utc>::from(valid_until);
        params.not_after = rcgen::date_time_ymd(
            datetime.year(),
            datetime.month() as u8,
            datetime.day() as u8,
        );
    }

    let cert = Certificate::from_params(params)
        .map_err(|err| TlsError::new(TlsErrorKind::Rcgen, err.to_string()))?;

    let cert_pem = cert
        .serialize_pem()
        .map_err(|err| TlsError::new(TlsErrorKind::Rcgen, err.to_string()))?
        .into_bytes();
    let cert_der = cert
        .serialize_der()
        .map_err(|err| TlsError::new(TlsErrorKind::Rcgen, err.to_string()))?;

    let key_pem = cert.serialize_private_key_pem().into_bytes();
    let key_der = cert.serialize_private_key_der();

    Ok(CaCertificate {
        material: CaMaterial {
            cert_pem,
            key_pem,
            cert_der,
            key_der,
        },
        cert,
    })
}

#[allow(dead_code)]
pub fn write_ca_to_dir(dir: impl AsRef<Path>, material: &CaMaterial) -> Result<CaMaterialPaths, TlsError> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir)
        .map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;

    let cert_path = dir.join("crossfeed-ca.pem");
    let key_path = dir.join("crossfeed-ca-key.pem");

    fs::write(&cert_path, &material.cert_pem)
        .map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;
    fs::write(&key_path, &material.key_pem)
        .map_err(|err| TlsError::new(TlsErrorKind::Io, err.to_string()))?;

    Ok(CaMaterialPaths { cert_path, key_path })
}
