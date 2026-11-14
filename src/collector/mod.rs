#[derive(Debug, Clone)]
pub struct TcpConnection {
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: u32,
    pub process: String,
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
