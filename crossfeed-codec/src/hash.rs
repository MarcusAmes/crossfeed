use sha2::Digest;

pub fn md5_hex(input: &[u8]) -> String {
    format!("{:x}", md5::compute(input))
}

pub fn sha1_hex(input: &[u8]) -> String {
    let mut hasher = sha1::Sha1::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

pub fn sha224_hex(input: &[u8]) -> String {
    let mut hasher = sha2::Sha224::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

pub fn sha384_hex(input: &[u8]) -> String {
    let mut hasher = sha2::Sha384::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

pub fn sha512_hex(input: &[u8]) -> String {
    let mut hasher = sha2::Sha512::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}
