# Rust-Kingbase

Kingbase support for Rust.

## kingbase

[Documentation](kingbase/src/lib.rs)

A native, synchronous Kingbase client.

## tokio-kingbase

[Documentation](tokio-kingbase/src/lib.rs)

A native, asynchronous Kingbase client.

## kingbase-types

[Documentation](kingbase-types/src/lib.rs)

Conversions between Rust and Kingbase types.

## kingbase-native-tls

[Documentation](kingbase-native-tls/src/lib.rs)

TLS support for kingbase and tokio-kingbase via native-tls.

## kingbase-openssl

[Documentation](kingbase-openssl/src/lib.rs)

TLS support for kingbase and tokio-kingbase via openssl.

# Running test suite

The test suite requires postgres to be running in the correct configuration. The easiest way to do this is with docker:

1. Install `docker` and `docker-compose`.
   1. On ubuntu: `sudo apt install docker.io docker-compose`.
1. Make sure your user has permissions for docker.
   1. On ubuntu: ``sudo usermod -aG docker $USER``
1. Change to top-level directory of `rust-kingbase` repo.
1. Run `docker-compose up -d`.
1. Run `cargo test`.
1. Run `docker-compose stop`.
