#[derive(Debug, Clone)]
pub struct TcpConnection {
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: u32,
    pub process: String,
}

#[cfg(test)]
mod tests {
    use super::endpoint_parts;

    #[test]
    fn splits_addresses_and_ports_without_losing_invalid_endpoints() {
        for (endpoint, address, port) in [
            ("192.0.2.1:443", "192.0.2.1", "443"),
            ("0.0.0.0:0", "0.0.0.0", "0"),
            ("[::1]:65535", "::1", "65535"),
            ("invalid", "invalid", "-"),
        ] {
            assert_eq!(endpoint_parts(endpoint), (address.into(), port.into()));
        }
    }
}

pub(crate) fn endpoint_parts(endpoint: &str) -> (String, String) {
    match endpoint.parse::<std::net::SocketAddr>() {
        Ok(address) => (address.ip().to_string(), address.port().to_string()),
        Err(_) => (endpoint.to_owned(), "-".to_owned()),
    }
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::tcp_connections;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::tcp_connections;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn tcp_connections() -> anyhow::Result<Vec<TcpConnection>> {
    anyhow::bail!("NetView currently supports Windows and Linux")
}
