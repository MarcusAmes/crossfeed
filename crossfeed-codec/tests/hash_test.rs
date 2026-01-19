use crossfeed_codec::*;

#[test]
fn md5_vector() {
    assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
}

#[test]
fn sha1_vector() {
    assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
}

#[test]
fn sha224_vector() {
    assert_eq!(
        sha224_hex(b""),
        "d14a028c2a3a2bc9476102bb288234c415a2b01f828ea62ac5b3e42f"
    );
}

#[test]
fn sha256_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha384_vector() {
    assert_eq!(
        sha384_hex(b""),
        "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da\
274edebfe76f65fbd51ad2f14898b95b"
    );
}

#[test]
fn sha512_vector() {
    assert_eq!(
        sha512_hex(b"abc"),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
}
