//! Authentication protocol support.
use crate::hex::LowerHexWrapper;
use md5::{Digest, Md5};

pub mod sasl;

/// Hashes authentication information in a way suitable for use in response
/// to an `AuthenticationMd5Password` message.
///
/// The resulting string should be sent back to the database in a
/// `PasswordMessage` message.
#[inline]
pub fn md5_hash(username: &[u8], password: &[u8], salt: [u8; 4]) -> String {
    let mut md5 = Md5::new();
    md5.update(password);
    md5.update(username);
    let output = LowerHexWrapper(md5.finalize_reset());
    md5.update(format!("{output:x}"));
    md5.update(salt);
    format!("md5{:x}", LowerHexWrapper(md5.finalize()))
}

/// Hashes authentication information for a KingbaseES `MD5_SHARDING` challenge
/// (authentication request 21).
///
/// Unlike ordinary MD5 authentication, the inner hash combines the password and
/// salt, and the outer hash combines its lowercase hexadecimal form and username.
/// The resulting string is sent in a `PasswordMessage`.
#[inline]
pub fn md5_hash_sharding(username: &[u8], password: &[u8], salt: [u8; 4]) -> String {
    let mut md5 = Md5::new();
    md5.update(password);
    md5.update(salt);
    let output = LowerHexWrapper(md5.finalize_reset());
    md5.update(format!("{output:x}"));
    md5.update(username);
    format!("md5{:x}", LowerHexWrapper(md5.finalize()))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn md5() {
        let username = b"md5_user";
        let password = b"password";
        let salt = [0x2a, 0x3d, 0x8f, 0xe0];

        assert_eq!(
            md5_hash(username, password, salt),
            "md562af4dd09bbb41884907a838a3233294"
        );
    }

    #[test]
    fn md5_sharding() {
        // Fixed vectors for KingbaseES sharding MD5 authentication.
        let cases: &[(&[u8], &[u8], [u8; 4], &str)] = &[
            (
                b"md5_user",
                b"password",
                [0x2a, 0x3d, 0x8f, 0xe0],
                "md5ad8c5e1a41fdbe707b7dbfd5d536f99c",
            ),
            (
                b"test",
                b"secret",
                [1, 2, 3, 4],
                "md56e73ef0161ee52e551e9c80594f811f2",
            ),
            (
                "用户_α".as_bytes(),
                "密码_Päss🔑".as_bytes(),
                [0, 255, 0, 128],
                "md5bf36af4153f9f22184520f5f3ca52958",
            ),
            (b"", b"", [0; 4], "md502385956f2ded894a4f0b02c83a8ee3c"),
        ];
        for &(username, password, salt, expected) in cases {
            assert_eq!(md5_hash_sharding(username, password, salt), expected);
        }
    }
}
