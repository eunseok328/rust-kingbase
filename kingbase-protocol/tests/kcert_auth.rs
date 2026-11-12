use bytes::BytesMut;
use kingbase_protocol::message::{backend::Message, frontend};

#[test]
fn certificate_challenge_can_be_fragmented() {
    let mut bytes = BytesMut::from(&b"R\0\0\0\x0d\0\0\0\x0eE630"[..]);
    assert!(Message::parse(&mut bytes).unwrap().is_none());
    bytes.extend_from_slice(b"1");
    let Some(Message::AuthenticationKingbaseCertificate(body)) =
        Message::parse(&mut bytes).unwrap()
    else {
        panic!("expected certificate challenge");
    };
    assert_eq!(body.challenge(), b"E630");
    assert_eq!(body.algorithm(), b'1');
    assert!(bytes.is_empty());
}

#[test]
fn certificate_challenge_requires_nonce_and_algorithm() {
    for payload in [&b""[..], &b"1"[..]] {
        let mut bytes = BytesMut::from(&b"R"[..]);
        bytes.extend_from_slice(&(8_u32 + payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&14_i32.to_be_bytes());
        bytes.extend_from_slice(payload);
        assert_eq!(
            Message::parse(&mut bytes).err().unwrap().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn certificate_response_uses_vendor_binary_layout() {
    let mut bytes = BytesMut::new();
    let signature = [0xa5; 256];
    let certificate = b"binary\0certificate";
    frontend::kingbase_certificate_response(b"ABCD", &signature, certificate, &mut bytes).unwrap();
    assert_eq!(bytes[0], b'c');
    assert_eq!(
        u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize,
        bytes.len() - 1
    );
    assert_eq!(&bytes[5..9], b"ABCD");
    assert_eq!(u32::from_le_bytes(bytes[9..13].try_into().unwrap()), 256);
    assert_eq!(
        u32::from_le_bytes(bytes[13..17].try_into().unwrap()) as usize,
        certificate.len()
    );
    assert_eq!(&bytes[17..273], signature);
    assert_eq!(&bytes[273..], certificate);
}
