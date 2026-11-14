use super::TcpConnection;
use anyhow::{bail, Result};
use std::{net::Ipv4Addr, slice};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER},
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    },
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

#[repr(C)]
struct MibTcpTableOwnerPid {
    count: u32,
    table: [MIB_TCPROW_OWNER_PID; 1],
}

pub fn tcp_connections() -> Result<Vec<TcpConnection>> {
    let mut size = 0u32;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            2,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    if size == 0 {
        bail!("GetExtendedTcpTable did not return a buffer size")
    }

    let mut bytes = vec![0u8; size as usize];
    let result = unsafe {
        GetExtendedTcpTable(
            bytes.as_mut_ptr().cast(),
            &mut size,
            0,
            2,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if result != 0 && result != ERROR_INSUFFICIENT_BUFFER {
        bail!("GetExtendedTcpTable failed with Windows error {result}")
    }

    let table = bytes.as_ptr().cast::<MibTcpTableOwnerPid>();
    let count = unsafe { (*table).count as usize };
    let first = unsafe { (*table).table.as_ptr() };
    let rows = unsafe { slice::from_raw_parts(first, count) };

    Ok(rows
        .iter()
        .map(|row| TcpConnection {
            local: endpoint(row.dwLocalAddr, row.dwLocalPort),
            remote: endpoint(row.dwRemoteAddr, row.dwRemotePort),
            state: state_name(row.dwState).to_owned(),
            pid: row.dwOwningPid,
            process: process_name(row.dwOwningPid),
        })
        .collect())
}

fn endpoint(address: u32, port: u32) -> String {
    // IP Helper stores IPv4 fields in network byte order.
    let ip = Ipv4Addr::from(address.to_ne_bytes());
    let port = u16::from_be(port as u16);
    format!("{ip}:{port}")
}

fn process_name(pid: u32) -> String {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return "<access denied>".to_owned();
    }

    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(handle) };
    if result == 0 {
        return "<unknown>".to_owned();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

fn state_name(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN-SENT",
        4 => "SYN-RECV",
        5 => "ESTABLISHED",
        6 => "FIN-WAIT-1",
        7 => "FIN-WAIT-2",
        8 => "CLOSE-WAIT",
        9 => "CLOSING",
        10 => "LAST-ACK",
        11 => "TIME-WAIT",
        12 => "DELETE-TCB",
        _ => "UNKNOWN",
    }
}
