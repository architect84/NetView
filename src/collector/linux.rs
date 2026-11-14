use super::TcpConnection;
use anyhow::{Context, Result};
use std::{collections::HashMap, fs, path::Path};

pub fn tcp_connections() -> Result<Vec<TcpConnection>> {
    let owners = socket_owners();
    let contents = fs::read_to_string("/proc/net/tcp").context("reading /proc/net/tcp")?;
    let mut connections = Vec::new();

    for line in contents.lines().skip(1) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        let inode = fields[9];
        let (pid, process) = owners
            .get(inode)
            .cloned()
            .unwrap_or((0, "<unknown>".to_owned()));
        connections.push(TcpConnection {
            local: parse_endpoint(fields[1]).unwrap_or_else(|| fields[1].to_owned()),
            remote: parse_endpoint(fields[2]).unwrap_or_else(|| fields[2].to_owned()),
            state: state_name(fields[3]).to_owned(),
            pid,
            process,
        });
    }
    Ok(connections)
}

fn socket_owners() -> HashMap<String, (u32, String)> {
    let mut owners = HashMap::new();
    let Ok(processes) = fs::read_dir("/proc") else {
        return owners;
    };
    for process in processes.flatten() {
        let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let name = fs::read_to_string(process.path().join("comm"))
            .map(|value| value.trim().to_owned())
            .unwrap_or_else(|_| "<unknown>".to_owned());
        let fd_dir = process.path().join("fd");
        let Ok(fds) = fs::read_dir(fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else {
                continue;
            };
            if let Some(inode) = socket_inode(&target) {
                owners.entry(inode).or_insert((pid, name.clone()));
            }
        }
    }
    owners
}

fn socket_inode(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    text.strip_prefix("socket:[")?
        .strip_suffix(']')
        .map(ToOwned::to_owned)
}

fn parse_endpoint(value: &str) -> Option<String> {
    let (address, port) = value.split_once(':')?;
    if address.len() != 8 {
        return None;
    }
    let raw = u32::from_str_radix(address, 16).ok()?;
    let ip = std::net::Ipv4Addr::from(raw.to_le_bytes());
    let port = u16::from_str_radix(port, 16).ok()?;
    Some(format!("{ip}:{port}"))
}

fn state_name(state: &str) -> &'static str {
    match state {
        "01" => "ESTABLISHED",
        "02" => "SYN-SENT",
        "03" => "SYN-RECV",
        "04" => "FIN-WAIT-1",
        "05" => "FIN-WAIT-2",
        "06" => "TIME-WAIT",
        "07" => "CLOSED",
        "08" => "CLOSE-WAIT",
        "09" => "LAST-ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}
