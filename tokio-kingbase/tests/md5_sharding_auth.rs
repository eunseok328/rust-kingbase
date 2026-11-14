#![cfg(all(feature = "runtime", not(target_arch = "wasm32")))]

use std::io::Read;
use tokio_kingbase::config::{ChannelBinding, SslMode};
use tokio_kingbase::error::SqlState;
use tokio_kingbase::{Config, NoTls};

#[allow(dead_code)]
#[path = "support/mac_auth.rs"]
mod support;

fn challenge(stream: &mut std::net::TcpStream, code: i32) {
    let mut body = code.to_be_bytes().to_vec();
    body.extend_from_slice(&[1, 2, 3, 4]);
    support::send(stream, b'R', &body);
}

fn config(port: u16, password: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .ssl_mode(SslMode::Disable);
    if let Some(password) = password {
        config.password(password);
    }
    config
}

#[tokio::test]
async fn md5_variants_use_their_own_proofs_with_optional_mac() {
    // Verify each authentication variant against its fixed response bytes.
    for (code, expected) in [
        (5, &b"md5594f15006e55e17dff918116ee00778f\0"[..]),
        (21, &b"md56e73ef0161ee52e551e9c80594f811f2\0"[..]),
    ] {
        for (before, after) in [(false, false), (true, false), (false, true), (true, true)] {
            let (port, task) = support::server(move |stream| {
                if before {
                    support::mac(stream);
                }
                challenge(stream, code);
                assert_eq!(support::receive(stream, b'p'), expected);
                if after {
                    support::mac(stream);
                }
                support::finish(stream);
            });
            let (client, connection) = config(port, Some("secret")).connect(NoTls).await.unwrap();
            assert!(!client.is_closed());
            drop(client);
            drop(connection);
            task.join().unwrap();
        }
    }
}

#[tokio::test]
async fn sharding_rejection_preserves_database_error() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        challenge(stream, 21);
        assert_eq!(
            support::receive(stream, b'p'),
            b"md561869183d3ab1b7f1fdbe67042048c7b\0"
        );
        support::reject(stream, "28P01");
    });
    let error = config(port, Some("wrong"))
        .connect(NoTls)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), Some(&SqlState::INVALID_PASSWORD));
    task.join().unwrap();
}

#[tokio::test]
async fn sharding_requires_password_and_respects_channel_binding() {
    for binding_required in [false, true] {
        let (port, task) = support::server(|stream| {
            support::mac(stream);
            challenge(stream, 21);
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        });
        let mut config = config(port, binding_required.then_some("secret"));
        if binding_required {
            config.channel_binding(ChannelBinding::Require);
        }
        let error = config.connect(NoTls).await.err().unwrap();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .to_string()
                .contains(if binding_required {
                    "channel binding"
                } else {
                    "password missing"
                })
        );
        task.join().unwrap();
    }
}
