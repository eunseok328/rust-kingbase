use bytes::BytesMut;
use postgres_protocol::message::backend::Message;

#[test]
fn sharding_challenge_can_be_fragmented_and_preserves_next_message() {
    let mut bytes = BytesMut::from(&b"R\0\0\0\x0c\0\0\0\x15\x01\x02\x03"[..]);
    assert!(Message::parse(&mut bytes).unwrap().is_none());
    bytes.extend_from_slice(b"\x04R\0\0\0\x08\0\0\0\0");
    let Some(Message::AuthenticationMd5ShardingPassword(body)) =
        Message::parse(&mut bytes).unwrap()
    else {
        panic!("expected MD5_SHARDING challenge");
    };
    assert_eq!(body.salt(), [1, 2, 3, 4]);
    assert!(matches!(
        Message::parse(&mut bytes).unwrap(),
        Some(Message::AuthenticationOk)
    ));
    assert!(bytes.is_empty());
}

#[test]
fn sharding_challenge_requires_exactly_four_salt_bytes() {
    for size in [0, 1, 2, 3, 5] {
        let mut bytes = BytesMut::from(&b"R"[..]);
        bytes.extend_from_slice(&(8_u32 + size).to_be_bytes());
        bytes.extend_from_slice(&21_i32.to_be_bytes());
        bytes.extend_from_slice(&vec![0; size as usize]);
        let expected = if size < 4 {
            std::io::ErrorKind::UnexpectedEof
        } else {
            std::io::ErrorKind::InvalidInput
        };
        assert_eq!(Message::parse(&mut bytes).err().unwrap().kind(), expected);
    }
}
