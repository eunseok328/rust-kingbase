#![cfg(all(feature = "runtime", not(target_arch = "wasm32")))]

use std::io::Read;
use tokio_kingbase::{Config, NoTls};

#[allow(dead_code)]
#[path = "support/mac_auth.rs"]
mod support;

fn config(port: u16) -> Config {
    let mut config = Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .ssl_mode(tokio_kingbase::config::SslMode::Disable);
    config
}

fn challenge(stream: &mut std::net::TcpStream, payload: &[u8]) {
    let mut body = 14_i32.to_be_bytes().to_vec();
    body.extend_from_slice(payload);
    support::send(stream, b'R', &body);
}

#[test]
fn certificate_config_parsing_and_password_redaction() {
    let config: Config = "host=localhost kingbase_client_cert='/tmp/client cert.pem' kingbase_client_key=/tmp/client.key kingbase_client_key_password='private pin'".parse().unwrap();
    assert_eq!(
        config.get_kingbase_client_cert().unwrap(),
        std::path::Path::new("/tmp/client cert.pem")
    );
    assert_eq!(
        config.get_kingbase_client_key().unwrap(),
        std::path::Path::new("/tmp/client.key")
    );
    assert_eq!(
        config.get_kingbase_client_key_password(),
        Some(&b"private pin"[..])
    );
    assert!(!format!("{config:?}").contains("private pin"));
    let uri: Config = "kingbase://test@localhost/test?kingbase_client_key=%2Ftmp%2Fa%20b.p12&kingbase_client_key_password=pin%21".parse().unwrap();
    assert_eq!(
        uri.get_kingbase_client_key().unwrap(),
        std::path::Path::new("/tmp/a b.p12")
    );
    assert_eq!(uri.get_kingbase_client_key_password(), Some(&b"pin!"[..]));
}

#[cfg(not(feature = "with-kingbase-cert"))]
#[tokio::test]
async fn certificate_auth_reports_missing_feature() {
    let (port, task) = support::server(|stream| {
        challenge(stream, b"E6301");
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    });
    let error = config(port).connect(NoTls).await.err().unwrap();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .to_string()
            .contains("with-kingbase-cert")
    );
    task.join().unwrap();
}

#[cfg(feature = "with-kingbase-cert")]
mod rsa {
    use super::*;
    use openssl::asn1::Asn1Time;
    use openssl::hash::MessageDigest;
    use openssl::pkcs12::Pkcs12;
    use openssl::pkey::{PKey, Private};
    use openssl::rsa::Rsa;
    use openssl::sign::Verifier;
    use openssl::symm::Cipher;
    use openssl::x509::{X509, X509NameBuilder};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Credentials {
        dir: PathBuf,
        cert: X509,
        key: PKey<Private>,
    }

    impl Credentials {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "rk-kcert-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
            let mut name = X509NameBuilder::new().unwrap();
            name.append_entry_by_text("CN", "test").unwrap();
            let name = name.build();
            let mut cert = X509::builder().unwrap();
            cert.set_version(2).unwrap();
            cert.set_subject_name(&name).unwrap();
            cert.set_issuer_name(&name).unwrap();
            cert.set_pubkey(&key).unwrap();
            cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
                .unwrap();
            cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
                .unwrap();
            cert.sign(&key, MessageDigest::sha256()).unwrap();
            let cert = cert.build();
            let this = Self { dir, cert, key };
            this.write("client.crt", &this.cert.to_pem().unwrap());
            this.write("client.key", &this.key.private_key_to_pem_pkcs8().unwrap());
            this
        }

        fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }

        fn config(&self, port: u16) -> Config {
            let mut config = super::config(port);
            config
                .kingbase_client_cert(self.dir.join("client.crt"))
                .kingbase_client_key(self.dir.join("client.key"));
            config
        }
    }

    impl Drop for Credentials {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).unwrap();
        }
    }

    fn verify(stream: &mut std::net::TcpStream) {
        challenge(stream, b"E6301");
        let body = support::receive(stream, b'c');
        assert!(body[..4].iter().all(|b| *b > 0 && *b < 128));
        let sig_len = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
        let cert_len = u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
        assert_eq!(body.len(), 12 + sig_len + cert_len);
        let cert = X509::from_der(&body[12 + sig_len..]).unwrap();
        let key = cert.public_key().unwrap();
        let mut verifier = Verifier::new(MessageDigest::sha1(), &key).unwrap();
        verifier.update(b"E630").unwrap();
        assert!(verifier.verify(&body[12..12 + sig_len]).unwrap());
    }

    async fn connect_ok(config: &Config) {
        let (client, connection) = config.connect(NoTls).await.unwrap();
        assert!(!client.is_closed());
        drop(client);
        drop(connection);
    }

    #[tokio::test]
    async fn rsa_proof_formats_and_mac() {
        let credentials = Credentials::new();
        for format in ["pem", "encrypted-pem", "der", "encrypted-der", "p12"] {
            let (port, task) = support::server(|stream| {
                support::mac(stream);
                verify(stream);
                support::mac(stream);
                support::finish(stream);
            });
            let mut config = credentials.config(port);
            match format {
                "encrypted-pem" => {
                    config
                        .kingbase_client_key(
                            credentials.write(
                                "encrypted.key",
                                &credentials
                                    .key
                                    .private_key_to_pem_pkcs8_passphrase(
                                        Cipher::aes_256_cbc(),
                                        b"private pin",
                                    )
                                    .unwrap(),
                            ),
                        )
                        .kingbase_client_key_password("private pin");
                }
                "der" => {
                    config
                        .kingbase_client_cert(
                            credentials.write("client.der", &credentials.cert.to_der().unwrap()),
                        )
                        .kingbase_client_key(
                            credentials
                                .write("key.der", &credentials.key.private_key_to_der().unwrap()),
                        );
                }
                "encrypted-der" => {
                    config
                        .kingbase_client_key(
                            credentials.write(
                                "encrypted.der",
                                &credentials
                                    .key
                                    .private_key_to_pkcs8_passphrase(
                                        Cipher::aes_256_cbc(),
                                        b"private pin",
                                    )
                                    .unwrap(),
                            ),
                        )
                        .kingbase_client_key_password("private pin");
                }
                "p12" => {
                    let bundle = Pkcs12::builder()
                        .name("test")
                        .pkey(&credentials.key)
                        .cert(&credentials.cert)
                        .build2("private pin")
                        .unwrap();
                    config = super::config(port);
                    config
                        .kingbase_client_key(
                            credentials.write("bundle.p12", &bundle.to_der().unwrap()),
                        )
                        .kingbase_client_key_password("private pin");
                }
                _ => {}
            }
            connect_ok(&config).await;
            task.join().unwrap();
        }
    }

    #[tokio::test]
    async fn invalid_credentials_and_algorithms_are_rejected_without_sending_proof() {
        let credentials = Credentials::new();
        let other = Credentials::new();
        let encrypted = credentials.write(
            "encrypted.key",
            &credentials
                .key
                .private_key_to_pem_pkcs8_passphrase(Cipher::aes_256_cbc(), b"private pin")
                .unwrap(),
        );
        for case in [
            "missing",
            "mismatch",
            "missing-pin",
            "wrong-pin",
            "nul-pin",
            "sm2",
            "oversized-challenge",
        ] {
            let payload = match case {
                "sm2" => b"E6302".to_vec(),
                "oversized-challenge" => vec![b'1'; 1026],
                _ => b"E6301".to_vec(),
            };
            let (port, task) = support::server(move |stream| {
                challenge(stream, &payload);
                assert_eq!(stream.read(&mut [0]).unwrap(), 0);
            });
            let mut config = credentials.config(port);
            match case {
                "missing" => config = super::config(port),
                "mismatch" => {
                    config.kingbase_client_key(other.dir.join("client.key"));
                }
                "missing-pin" => {
                    config.kingbase_client_key(&encrypted);
                }
                "wrong-pin" => {
                    config
                        .kingbase_client_key(&encrypted)
                        .kingbase_client_key_password("wrong private pin");
                }
                "nul-pin" => {
                    config.kingbase_client_key_password(b"private\0pin");
                }
                _ => {}
            }
            let error = config.connect(NoTls).await.err().unwrap();
            assert!(!format!("{error:?}").contains("private pin"));
            task.join().unwrap();
        }
    }

    #[tokio::test]
    async fn repeated_certificate_request_is_rejected() {
        let credentials = Credentials::new();
        let (port, task) = support::server(|stream| {
            verify(stream);
            challenge(stream, b"ABCD1");
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        });
        let error = credentials.config(port).connect(NoTls).await.err().unwrap();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .to_string()
                .contains("repeated")
        );
        task.join().unwrap();
    }

    #[tokio::test]
    async fn certificate_auth_does_not_bypass_required_channel_binding() {
        let credentials = Credentials::new();
        let (port, task) = support::server(|stream| {
            verify(stream);
            support::request(stream, 0);
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        });
        let error = credentials
            .config(port)
            .channel_binding(tokio_kingbase::config::ChannelBinding::Require)
            .connect(NoTls)
            .await
            .err()
            .unwrap();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .to_string()
                .contains("channel binding")
        );
        task.join().unwrap();
    }
}
