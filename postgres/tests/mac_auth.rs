#[path = "../../tokio-postgres/tests/support/mac_auth.rs"]
mod support;

#[test]
fn synchronous_client_supports_mac_before_password() {
    let (port, task) = support::server(|stream| {
        support::mac(stream);
        support::request(stream, 3);
        assert_eq!(support::receive(stream, b'p'), b"secret\0");
        support::finish(stream);
    });
    let mut config = postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .password("secret")
        .ssl_mode(postgres::config::SslMode::Disable);
    let client = config.connect(postgres::NoTls).unwrap();
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
    let mut config = postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("test")
        .ssl_mode(postgres::config::SslMode::Disable);
    let error = config.connect(postgres::NoTls).err().unwrap();
    assert_eq!(
        error.code(),
        Some(&postgres::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
    );
    task.join().unwrap();
}
