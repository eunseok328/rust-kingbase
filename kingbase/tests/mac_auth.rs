#[path = "../../tokio-kingbase/tests/support/mac_auth.rs"]
mod support;

#[test]
fn synchronous_client_supports_mac_before_password() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"secret\0");
        support::finish(stream);
    });
    let mut config = kingbase::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .password("secret")
        .ssl_mode(kingbase::config::SslMode::Disable);
    let client = config.connect(kingbase::NoTls).unwrap();
    assert!(!client.is_closed());
    drop(client);
    task.join().unwrap();
}

#[test]
fn synchronous_client_returns_mac_rejection() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::reject(stream, "28000");
    });
    let mut config = kingbase::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .ssl_mode(kingbase::config::SslMode::Disable);
    let error = config.connect(kingbase::NoTls).err().unwrap();
    assert_eq!(
        error.code(),
        Some(&kingbase::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    task.join().unwrap();
}
