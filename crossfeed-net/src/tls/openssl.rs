use openssl::pkey::PKey;
use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslMethod, SslOptions, SslVerifyMode};
use openssl::x509::X509;

use super::types::{LeafCertificate, TlsError, TlsErrorKind};

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub allow_legacy: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            allow_legacy: false,
        }
    }
}

pub fn build_acceptor(config: &TlsConfig, leaf: &LeafCertificate) -> Result<SslAcceptor, TlsError> {
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;

    apply_legacy(&mut builder, config.allow_legacy)?;

    let cert = X509::from_pem(&leaf.cert_pem)
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;
    let key = PKey::private_key_from_pem(&leaf.key_pem)
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;

    builder
        .set_certificate(&cert)
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;
    builder
        .set_private_key(&key)
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;

    builder.set_verify(SslVerifyMode::NONE);

    Ok(builder.build())
}

fn apply_legacy(builder: &mut SslAcceptorBuilder, allow_legacy: bool) -> Result<(), TlsError> {
    if allow_legacy {
        builder.set_options(SslOptions::NO_TICKET);
        builder.clear_options(SslOptions::NO_SSLV2 | SslOptions::NO_SSLV3);
        builder
            .set_cipher_list("ALL:@SECLEVEL=0")
            .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;
    } else {
        builder.set_options(SslOptions::NO_SSLV2 | SslOptions::NO_SSLV3);
    }
    Ok(())
}
