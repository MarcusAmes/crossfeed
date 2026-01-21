use openssl::pkey::PKey;
use openssl::ssl::{
    AlpnError, SslAcceptor, SslAcceptorBuilder, SslMethod, SslOptions, SslVerifyMode,
};
use openssl::x509::X509;

use super::types::{LeafCertificate, TlsError, TlsErrorKind};

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub allow_legacy: bool,
    pub alpn_protocols: Vec<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            allow_legacy: false,
            alpn_protocols: Vec::new(),
        }
    }
}

pub fn build_acceptor(config: &TlsConfig, leaf: &LeafCertificate) -> Result<SslAcceptor, TlsError> {
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())
        .map_err(|err| TlsError::new(TlsErrorKind::OpenSsl, err.to_string()))?;

    apply_legacy(&mut builder, config.allow_legacy)?;
    apply_alpn(&mut builder, &config.alpn_protocols)?;

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

fn apply_alpn(
    builder: &mut SslAcceptorBuilder,
    protocols: &[String],
) -> Result<(), TlsError> {
    if protocols.is_empty() {
        return Ok(());
    }
    let server_protocols: Vec<Vec<u8>> = protocols
        .iter()
        .map(|protocol| protocol.as_bytes().to_vec())
        .collect();
    builder.set_alpn_select_callback(move |_, client| {
        select_alpn_from_client(client, &server_protocols).ok_or(AlpnError::NOACK)
    });
    Ok(())
}

fn select_alpn_from_client<'a>(
    client: &'a [u8],
    server_protocols: &[Vec<u8>],
) -> Option<&'a [u8]> {
    let mut index = 0;
    while index < client.len() {
        let length = *client.get(index)? as usize;
        let start = index + 1;
        let end = start + length;
        let proto = client.get(start..end)?;
        for server in server_protocols {
            if server.as_slice() == proto {
                return Some(proto);
            }
        }
        index = end;
    }
    None
}
