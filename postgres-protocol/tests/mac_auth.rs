use bytes::BytesMut;
use postgres_protocol::message::backend::Message;

#[test]
fn mac_request_can_be_fragmented() {
    let mut bytes = BytesMut::from(&b"R\0\0\0\x08\0\0\0"[..]);
    assert!(Message::parse(&mut bytes).unwrap().is_none());
    bytes.extend_from_slice(&[19]);
    assert!(matches!(
        Message::parse(&mut bytes).unwrap(),
        Some(Message::AuthenticationMac)
    ));
    assert!(bytes.is_empty());
}

#[test]
fn mac_request_rejects_unexpected_payload() {
    let mut bytes = BytesMut::from(&b"R\0\0\0\x09\0\0\0\x13\0"[..]);
    assert_eq!(
        Message::parse(&mut bytes).err().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );
}
