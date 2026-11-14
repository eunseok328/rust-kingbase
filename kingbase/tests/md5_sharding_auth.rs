use kingbase::config::{ChannelBinding, SslMode};
use kingbase::{Config, NoTls};
use std::io::Read;

#[allow(dead_code)]
#[path = "../../tokio-kingbase/tests/support/mac_auth.rs"]
mod support;

fn challenge(stream: &mut std::net::TcpStream) {
    let mut body = 21_i32.to_be_bytes().to_vec();
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

#[test]
fn synchronous_sharding_authentication_with_mac() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        challenge(stream);
        assert_eq!(
            support::receive(stream, b'p'),
            b"md56e73ef0161ee52e551e9c80594f811f2\0"
        );
        support::mac(stream);
        support::finish(stream);
    });
    let client = config(port, Some("secret")).connect(NoTls).unwrap();
    assert!(!client.is_closed());
    drop(client);
    task.join().unwrap();
}

#[test]
fn synchronous_sharding_rejection_preserves_database_error() {
    let (port, task) = support::server(|stream| {
        challenge(stream);
        assert_eq!(
            support::receive(stream, b'p'),
            b"md561869183d3ab1b7f1fdbe67042048c7b\0"
        );
        support::reject(stream, "28P01");
    });
    let error = config(port, Some("wrong")).connect(NoTls).err().unwrap();
    assert_eq!(
        error.code(),
        Some(&kingbase::error::SqlState::INVALID_PASSWORD)
    );
    task.join().unwrap();
}

#[test]
fn synchronous_sharding_requires_password_and_respects_channel_binding() {
    for binding_required in [false, true] {
        let (port, task) = support::server(|stream| {
            challenge(stream);
            assert_eq!(stream.read(&mut [0]).unwrap(), 0);
        });
        let mut config = config(port, binding_required.then_some("secret"));
        if binding_required {
            config.channel_binding(ChannelBinding::Require);
        }
        let error = config.connect(NoTls).err().unwrap();
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
