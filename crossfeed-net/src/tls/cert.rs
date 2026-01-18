use std::net::IpAddr;

use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, IsCa, SanType};

use super::types::{CaCertificate, LeafCertificate, TlsError, TlsErrorKind};

pub fn generate_leaf_cert(host: &str, ca: &CaCertificate) -> Result<LeafCertificate, TlsError> {
    let mut params = CertificateParams::new(Vec::new());
    params.is_ca = IsCa::NoCa;

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, host);
    params.distinguished_name = dn;

    if let Ok(ip) = host.parse::<IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    } else {
        params.subject_alt_names.push(SanType::DnsName(host.to_string()));
    }

    let cert = Certificate::from_params(params)
        .map_err(|err| TlsError::new(TlsErrorKind::Rcgen, err.to_string()))?;

    let cert_pem = cert
        .serialize_pem_with_signer(&ca.cert)
        .map_err(|err| TlsError::new(TlsErrorKind::Rcgen, err.to_string()))?
        .into_bytes();
    let key_pem = cert.serialize_private_key_pem().into_bytes();

    Ok(LeafCertificate { cert_pem, key_pem })
}
