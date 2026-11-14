//! RSA proof for KingbaseES certificate authentication, independent of TLS.

use crate::config::Config;
use kingbase_protocol::message::{backend::AuthenticationKingbaseCertificateBody, frontend};
use openssl::hash::MessageDigest;
use openssl::pkcs12::Pkcs12;
use openssl::pkey::{Id, PKey, Private};
use openssl::rsa::Padding;
use openssl::sign::Signer;
use openssl::x509::X509;
use std::error::Error;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

type AuthError = Box<dyn Error + Send + Sync>;
const MAX_CREDENTIAL_SIZE: usize = 2 * 1024 * 1024;

fn invalid(message: &'static str) -> AuthError {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}

fn read_credential(path: &Path) -> Result<Vec<u8>, AuthError> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_CREDENTIAL_SIZE + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CREDENTIAL_SIZE {
        return Err(invalid("Kingbase credential file exceeds 2 MiB"));
    }
    Ok(bytes)
}

fn load(config: &Config) -> Result<(X509, PKey<Private>), AuthError> {
    let path = config
        .get_kingbase_client_key()
        .ok_or_else(|| invalid("kingbase_client_key is required for kcert authentication"))?;
    let bytes = read_credential(path)?;
    let password = config.get_kingbase_client_key_password();
    // OpenSSL's PEM/PKCS#8/PKCS#12 helpers panic on embedded NULs.
    if password.is_some_and(|password| password.contains(&0)) {
        return Err(invalid(
            "Kingbase private key password must not contain NUL bytes",
        ));
    }
    let pkcs12 = path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("p12") || s.eq_ignore_ascii_case("pfx"));

    let (key, bundled_cert) = if pkcs12 {
        let password = std::str::from_utf8(password.unwrap_or_default())?;
        let bundle = Pkcs12::from_der(&bytes)?.parse2(password)?;
        (
            bundle
                .pkey
                .ok_or_else(|| invalid("PKCS#12 bundle has no private key"))?,
            bundle.cert,
        )
    } else {
        let key = if bytes.starts_with(b"-----BEGIN") {
            match password {
                Some(password) => PKey::private_key_from_pem_passphrase(&bytes, password)?,
                // Use a non-interactive callback: an encrypted key must never prompt on stdin.
                None => PKey::private_key_from_pem_callback(&bytes, |_| Ok(0))?,
            }
        } else {
            match password {
                Some(password) => PKey::private_key_from_pkcs8_passphrase(&bytes, password)?,
                None => PKey::private_key_from_der(&bytes)?,
            }
        };
        (key, None)
    };

    if key.id() != Id::RSA {
        return Err(invalid(
            "Kingbase kcert currently requires an RSA private key",
        ));
    }
    let cert = match config.get_kingbase_client_cert() {
        Some(path) => {
            let bytes = read_credential(path)?;
            if bytes.starts_with(b"-----BEGIN") {
                X509::from_pem(&bytes)?
            } else {
                X509::from_der(&bytes)?
            }
        }
        None => bundled_cert.ok_or_else(|| {
            invalid("kingbase_client_cert is required unless a PKCS#12 bundle supplies it")
        })?,
    };
    if !cert.public_key()?.public_eq(&key) {
        return Err(invalid(
            "Kingbase client certificate and private key do not match",
        ));
    }
    Ok((cert, key))
}

pub(crate) fn response(
    config: &Config,
    challenge: &AuthenticationKingbaseCertificateBody,
) -> Result<bytes::Bytes, AuthError> {
    if challenge.algorithm() != b'1' {
        return Err(invalid(
            "unsupported Kingbase certificate algorithm; only RSA is enabled",
        ));
    }
    if challenge.challenge().len() > 1024 {
        return Err(invalid("Kingbase certificate challenge exceeds 1024 bytes"));
    }
    let (cert, key) = load(config)?;
    // KingbaseES RSA certificate proofs use SHA1WithRSA (PKCS#1 v1.5).
    let mut signer = Signer::new(MessageDigest::sha1(), &key)?;
    signer.set_rsa_padding(Padding::PKCS1)?;
    signer.update(challenge.challenge())?;
    let signature = signer.sign_to_vec()?;
    let certificate = cert.to_der()?;
    let mut nonce = [0; 4];
    openssl::rand::rand_bytes(&mut nonce)?;
    // The wire field is four bytes; use non-NUL ASCII as the vendor client does.
    for byte in &mut nonce {
        *byte = 1 + *byte % 127;
    }
    let mut bytes = bytes::BytesMut::new();
    frontend::kingbase_certificate_response(&nonce, &signature, &certificate, &mut bytes)?;
    Ok(bytes.freeze())
}
