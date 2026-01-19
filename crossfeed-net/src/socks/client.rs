use super::types::{
    SocksAddress, SocksError, SocksErrorKind, SocksReply, SocksResponse, SocksVersion,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksAuth {
    NoAuth,
    UserPass { username: String, password: String },
}

pub fn build_handshake_request(version: SocksVersion, auth: &SocksAuth) -> Vec<u8> {
    match version {
        SocksVersion::V4 => Vec::new(),
        SocksVersion::V5 => {
            let methods = match auth {
                SocksAuth::NoAuth => vec![0x00],
                SocksAuth::UserPass { .. } => vec![0x00, 0x02],
            };
            let mut buf = Vec::with_capacity(2 + methods.len());
            buf.push(0x05);
            buf.push(methods.len() as u8);
            buf.extend_from_slice(&methods);
            buf
        }
    }
}

pub fn parse_handshake_response(bytes: &[u8]) -> Result<u8, SocksError> {
    if bytes.len() < 2 {
        return Err(SocksError {
            kind: SocksErrorKind::UnexpectedEof,
            offset: bytes.len(),
        });
    }
    if bytes[0] != 0x05 {
        return Err(SocksError {
            kind: SocksErrorKind::InvalidVersion,
            offset: 0,
        });
    }
    Ok(bytes[1])
}

pub fn build_socks5_connect(address: SocksAddress, port: u16) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x05);
    buf.push(0x01);
    buf.push(0x00);

    encode_address(&mut buf, &address);
    buf.extend_from_slice(&port.to_be_bytes());
    buf
}

pub fn build_socks4_connect(address: SocksAddress, port: u16, user_id: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0x04);
    buf.push(0x01);
    buf.extend_from_slice(&port.to_be_bytes());

    match address {
        SocksAddress::IpV4(ip) => {
            buf.extend_from_slice(&ip);
            buf.extend_from_slice(user_id.as_bytes());
            buf.push(0x00);
        }
        SocksAddress::Domain(domain) => {
            buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            buf.extend_from_slice(user_id.as_bytes());
            buf.push(0x00);
            buf.extend_from_slice(domain.as_bytes());
            buf.push(0x00);
        }
        SocksAddress::IpV6(_) => {
            buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            buf.extend_from_slice(user_id.as_bytes());
            buf.push(0x00);
            buf.extend_from_slice(b"::");
            buf.push(0x00);
        }
    }

    buf
}

pub fn parse_socks_response(bytes: &[u8]) -> Result<SocksResponse, SocksError> {
    if bytes.is_empty() {
        return Err(SocksError {
            kind: SocksErrorKind::UnexpectedEof,
            offset: 0,
        });
    }

    match bytes[0] {
        0x00 | 0x04 => parse_socks4_response(bytes),
        0x05 => parse_socks5_response(bytes),
        _ => Err(SocksError {
            kind: SocksErrorKind::InvalidVersion,
            offset: 0,
        }),
    }
}

fn parse_socks4_response(bytes: &[u8]) -> Result<SocksResponse, SocksError> {
    if bytes.len() < 8 {
        return Err(SocksError {
            kind: SocksErrorKind::UnexpectedEof,
            offset: bytes.len(),
        });
    }
    let reply = match bytes[1] {
        0x5a => SocksReply::Succeeded,
        0x5b => SocksReply::GeneralFailure,
        0x5c => SocksReply::ConnectionNotAllowed,
        0x5d => SocksReply::NetworkUnreachable,
        code => SocksReply::Other(code),
    };
    let port = u16::from_be_bytes([bytes[2], bytes[3]]);
    let address = SocksAddress::IpV4([bytes[4], bytes[5], bytes[6], bytes[7]]);
    Ok(SocksResponse {
        version: SocksVersion::V4,
        reply,
        address,
        port,
    })
}

fn parse_socks5_response(bytes: &[u8]) -> Result<SocksResponse, SocksError> {
    if bytes.len() < 5 {
        return Err(SocksError {
            kind: SocksErrorKind::UnexpectedEof,
            offset: bytes.len(),
        });
    }
    if bytes[1] == 0xFF {
        return Err(SocksError {
            kind: SocksErrorKind::InvalidResponse,
            offset: 1,
        });
    }

    let reply = map_socks5_reply(bytes[1]);
    let address_type = bytes[3];
    let mut cursor = 4;
    let address = match address_type {
        0x01 => {
            if bytes.len() < cursor + 4 {
                return Err(SocksError {
                    kind: SocksErrorKind::UnexpectedEof,
                    offset: bytes.len(),
                });
            }
            let ip = [
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ];
            cursor += 4;
            SocksAddress::IpV4(ip)
        }
        0x03 => {
            if bytes.len() < cursor + 1 {
                return Err(SocksError {
                    kind: SocksErrorKind::UnexpectedEof,
                    offset: bytes.len(),
                });
            }
            let len = bytes[cursor] as usize;
            cursor += 1;
            if bytes.len() < cursor + len {
                return Err(SocksError {
                    kind: SocksErrorKind::UnexpectedEof,
                    offset: bytes.len(),
                });
            }
            let domain = String::from_utf8_lossy(&bytes[cursor..cursor + len]).to_string();
            cursor += len;
            SocksAddress::Domain(domain)
        }
        0x04 => {
            if bytes.len() < cursor + 16 {
                return Err(SocksError {
                    kind: SocksErrorKind::UnexpectedEof,
                    offset: bytes.len(),
                });
            }
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&bytes[cursor..cursor + 16]);
            cursor += 16;
            SocksAddress::IpV6(ip)
        }
        _ => {
            return Err(SocksError {
                kind: SocksErrorKind::UnsupportedAddressType,
                offset: cursor,
            });
        }
    };

    if bytes.len() < cursor + 2 {
        return Err(SocksError {
            kind: SocksErrorKind::UnexpectedEof,
            offset: bytes.len(),
        });
    }
    let port = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]);

    Ok(SocksResponse {
        version: SocksVersion::V5,
        reply,
        address,
        port,
    })
}

fn map_socks5_reply(code: u8) -> SocksReply {
    match code {
        0x00 => SocksReply::Succeeded,
        0x01 => SocksReply::GeneralFailure,
        0x02 => SocksReply::ConnectionNotAllowed,
        0x03 => SocksReply::NetworkUnreachable,
        0x04 => SocksReply::HostUnreachable,
        0x05 => SocksReply::ConnectionRefused,
        0x06 => SocksReply::TtlExpired,
        0x07 => SocksReply::CommandNotSupported,
        0x08 => SocksReply::AddressTypeNotSupported,
        other => SocksReply::Other(other),
    }
}

fn encode_address(buf: &mut Vec<u8>, address: &SocksAddress) {
    match address {
        SocksAddress::IpV4(ip) => {
            buf.push(0x01);
            buf.extend_from_slice(ip);
        }
        SocksAddress::Domain(domain) => {
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend_from_slice(domain.as_bytes());
        }
        SocksAddress::IpV6(ip) => {
            buf.push(0x04);
            buf.extend_from_slice(ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_socks5_handshake_no_auth() {
        let bytes = build_handshake_request(SocksVersion::V5, &SocksAuth::NoAuth);
        assert_eq!(bytes, vec![0x05, 0x01, 0x00]);
    }

    #[test]
    fn builds_socks5_handshake_user_pass() {
        let bytes = build_handshake_request(
            SocksVersion::V5,
            &SocksAuth::UserPass {
                username: "user".to_string(),
                password: "pass".to_string(),
            },
        );
        assert_eq!(bytes, vec![0x05, 0x02, 0x00, 0x02]);
    }

    #[test]
    fn parses_socks5_handshake_response() {
        let method = parse_handshake_response(&[0x05, 0x00]).unwrap();
        assert_eq!(method, 0x00);
    }

    #[test]
    fn builds_socks5_connect_ipv4() {
        let bytes = build_socks5_connect(SocksAddress::IpV4([127, 0, 0, 1]), 8080);
        assert_eq!(
            bytes,
            vec![0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90]
        );
    }

    #[test]
    fn builds_socks4a_connect_domain() {
        let bytes = build_socks4_connect(SocksAddress::Domain("example.com".to_string()), 80, "");
        assert_eq!(
            bytes,
            vec![
                0x04, 0x01, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x00, b'e', b'x', b'a', b'm', b'p',
                b'l', b'e', b'.', b'c', b'o', b'm', 0x00,
            ]
        );
    }

    #[test]
    fn parses_socks5_response_ipv4() {
        let bytes = vec![0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90];
        let response = parse_socks_response(&bytes).unwrap();
        assert_eq!(response.reply, SocksReply::Succeeded);
    }

    #[test]
    fn parses_socks4_response() {
        let bytes = vec![0x00, 0x5a, 0x00, 0x50, 127, 0, 0, 1];
        let response = parse_socks_response(&bytes).unwrap();
        assert_eq!(response.reply, SocksReply::Succeeded);
    }
}
