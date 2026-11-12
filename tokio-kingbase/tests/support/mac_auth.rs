use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub fn server(authenticate: impl FnOnce(&mut TcpStream) + Send + 'static) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut startup = vec![0; u32::from_be_bytes(length) as usize - 4];
        stream.read_exact(&mut startup).unwrap();
        assert_eq!(&startup[..4], &196608_u32.to_be_bytes());
        assert!(!startup.windows(8).any(|window| window == b"macaddr\0"));
        authenticate(&mut stream);
    });
    (port, task)
}

pub fn send(stream: &mut TcpStream, tag: u8, body: &[u8]) {
    stream.write_all(&[tag]).unwrap();
    stream
        .write_all(&(body.len() as u32 + 4).to_be_bytes())
        .unwrap();
    stream.write_all(body).unwrap();
}

pub fn request(stream: &mut TcpStream, kind: i32) {
    send(stream, b'R', &kind.to_be_bytes());
}

pub fn receive(stream: &mut TcpStream, expected_tag: u8) -> Vec<u8> {
    let mut header = [0; 5];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(header[0], expected_tag);
    let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
    assert!((4..=65536).contains(&length));
    let mut body = vec![0; length - 4];
    stream.read_exact(&mut body).unwrap();
    body
}

pub fn mac(stream: &mut TcpStream) -> Vec<u8> {
    request(stream, 19);
    let body = receive(stream, b'p');
    assert_eq!(body.last(), Some(&0));
    let addresses = std::str::from_utf8(&body[..body.len() - 1]).unwrap();
    if !addresses.is_empty() {
        for address in addresses.split('/') {
            assert!(!address.is_empty());
            for octet in address.split(':') {
                assert_eq!(octet.len(), 2);
                assert!(
                    octet
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
                );
            }
            assert_ne!(
                address, "00:00:00:00:00:00",
                "loopback must not be reported"
            );
        }
    }
    body
}

pub fn finish(stream: &mut TcpStream) {
    request(stream, 0);
    send(stream, b'Z', b"I");
    // The driver probes KES compatibility after authentication.
    receive(stream, b'Q');
    send(stream, b'C', b"SELECT 0\0");
    send(stream, b'Z', b"I");
    // Keep the transport open until the client is dropped.
    let mut byte = [0];
    let _ = stream.read(&mut byte).unwrap();
}

pub fn reject(stream: &mut TcpStream, code: &str) {
    let body = format!("SFATAL\0C{code}\0Mlogin rejected\0\0");
    send(stream, b'E', body.as_bytes());
}
