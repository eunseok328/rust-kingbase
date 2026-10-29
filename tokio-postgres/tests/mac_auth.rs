#![cfg(all(feature = "runtime", not(target_arch = "wasm32")))]

use std::io::Read;
use tokio_postgres::config::ChannelBinding;
use tokio_postgres::error::SqlState;
use tokio_postgres::{Config, NoTls};

#[path = "support/mac_auth.rs"]
mod support;

fn config(port: u16, password: Option<&str>) -> Config {
    let mut config = Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .ssl_mode(tokio_postgres::config::SslMode::Disable);
    if let Some(password) = password {
        config.password(password);
    }
    config
}

async fn connect_ok(config: Config) {
    let (client, connection) = config.connect(NoTls).await.unwrap();
    assert!(!client.is_closed());
    drop(client);
    drop(connection);
}

#[tokio::test]
async fn mac_before_password() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"secret\0");
        support::finish(stream);
    });
    connect_ok(config(port, Some("secret"))).await;
    task.join().unwrap();
}

#[tokio::test]
async fn mac_after_password() {
    let (port, task) = support::server(|stream| {
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"secret\0");
        support::mac(stream);
        support::finish(stream);
    });
    connect_ok(config(port, Some("secret"))).await;
    task.join().unwrap();
}

#[tokio::test]
async fn repeated_mac_requests_and_md5() {
    let (port, task) = support::server(|stream| {
        let addresses = support::mac(stream);
        assert_eq!(support::mac(stream), addresses);
        let mut request = 5_i32.to_be_bytes().to_vec();
        let salt = [1, 2, 3, 4];
        request.extend(salt);
        support::send(stream, b'R', &request);
        let expected = postgres_protocol::authentication::md5_hash(b"test", b"secret", salt);
        assert_eq!(
            support::receive(stream, b'p'),
            format!("{expected}\0").as_bytes()
        );
        assert_eq!(support::mac(stream), addresses);
        support::finish(stream);
    });
    connect_ok(config(port, Some("secret"))).await;
    task.join().unwrap();
}

#[tokio::test]
async fn mac_rejection_is_returned_to_caller() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::reject(stream, "28000");
    });
    let error = config(port, Some("secret"))
        .connect(NoTls)
        .await
        .err()
        .unwrap();
    assert_eq!(
        error.code(),
        Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    task.join().unwrap();
}

#[tokio::test]
async fn mac_does_not_replace_missing_password() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 3);
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    });
    let error = config(port, None).connect(NoTls).await.err().unwrap();
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .to_string()
            .contains("password missing")
    );
    task.join().unwrap();
}

#[tokio::test]
async fn mac_does_not_hide_password_rejection() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"wrong\0");
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
async fn mac_does_not_bypass_required_channel_binding() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 0);
    });
    let error = config(port, None)
        .channel_binding(ChannelBinding::Require)
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

#[tokio::test]
async fn old_server_does_not_receive_unsolicited_mac() {
    let (port, task) = support::server(|stream| {
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"secret\0");
        support::finish(stream);
    });
    connect_ok(config(port, Some("secret"))).await;
    task.join().unwrap();
}

#[tokio::test]
async fn mac_can_be_followed_by_authentication_ok() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::finish(stream);
    });
    connect_ok(config(port, None)).await;
    task.join().unwrap();
}
