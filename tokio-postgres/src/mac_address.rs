//! MAC address reporting for KingbaseES authentication type 19.

use std::io;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "illumos",
    target_os = "windows",
))]
pub(crate) fn mac_addresses() -> io::Result<String> {
    use network_interface::{NetworkInterface, NetworkInterfaceConfig};

    let mut interfaces = NetworkInterface::show().map_err(io::Error::other)?;
    interfaces.sort_by_key(|interface| interface.index);

    // Report non-internal interfaces' hardware addresses in uppercase,
    // joined by '/' without a trailing separator.
    Ok(interfaces
        .into_iter()
        .filter(|interface| !interface.internal)
        .filter_map(|interface| interface.mac_addr)
        .filter(|address| !address.is_empty())
        .map(|address| address.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("/"))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "illumos",
    target_os = "windows",
)))]
pub(crate) fn mac_addresses() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "KingbaseES MAC authentication is not supported on this platform",
    ))
}
