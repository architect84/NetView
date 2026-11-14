use crate::collector::TcpConnection;
use std::{collections::HashMap, net::SocketAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Listening,
    Inbound,
    Outbound,
    LikelyInbound,
    LikelyOutbound,
    Unknown,
}

impl Direction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Listening => "LISTENING",
            Self::Inbound => "INBOUND",
            Self::Outbound => "OUTBOUND",
            Self::LikelyInbound => "INBOUND?",
            Self::LikelyOutbound => "OUTBOUND?",
            Self::Unknown => "UNKNOWN",
        }
    }
}

type ConnectionKey = (u32, String, String);

fn key(entry: &TcpConnection) -> ConnectionKey {
    (entry.pid, entry.local.clone(), entry.remote.clone())
}

#[derive(Default)]
pub(crate) struct Directions {
    values: HashMap<ConnectionKey, Direction>,
}

impl Directions {
    pub(crate) fn get(&self, entry: &TcpConnection) -> Direction {
        self.values
            .get(&key(entry))
            .copied()
            .unwrap_or(Direction::Unknown)
    }

    pub(crate) fn update(&mut self, entries: &[TcpConnection]) {
        let listeners: Vec<_> = entries
            .iter()
            .filter(|entry| entry.state == "LISTEN")
            .filter_map(|entry| {
                entry
                    .local
                    .parse::<SocketAddr>()
                    .ok()
                    .map(|local| (entry.pid, local))
            })
            .collect();
        // Replace the map so disappeared connections don't leave stale, unbounded history.
        self.values = entries
            .iter()
            .map(|entry| {
                let direction = infer(entry, &listeners, self.get(entry));
                (key(entry), direction)
            })
            .collect();
    }
}

fn infer(entry: &TcpConnection, listeners: &[(u32, SocketAddr)], previous: Direction) -> Direction {
    match entry.state.as_str() {
        "LISTEN" => return Direction::Listening,
        "SYN-SENT" => return Direction::Outbound,
        "SYN-RECV" => return Direction::Inbound,
        _ => {}
    }
    let (Ok(local), Ok(remote)) = (
        entry.local.parse::<SocketAddr>(),
        entry.remote.parse::<SocketAddr>(),
    ) else {
        return Direction::Unknown;
    };
    if local.port() == 0 || remote.port() == 0 || remote.ip().is_unspecified() {
        return Direction::Unknown;
    }
    if matches!(previous, Direction::Inbound | Direction::Outbound) {
        return previous;
    }
    if listeners.iter().any(|(pid, listener)| {
        (*pid == entry.pid || *pid == 0 || entry.pid == 0)
            && listener.port() == local.port()
            && listener.is_ipv4() == local.is_ipv4()
            && (listener.ip().is_unspecified() || listener.ip() == local.ip())
    }) {
        return Direction::LikelyInbound;
    }
    if matches!(
        previous,
        Direction::LikelyInbound | Direction::LikelyOutbound
    ) {
        return previous;
    }
    // No listener is only weak evidence: it may have closed or be invisible.
    if entry.state == "ESTABLISHED" && entry.pid != 0 {
        Direction::LikelyOutbound
    } else {
        Direction::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn entry(pid: u32, local_port: u16, state: &str) -> TcpConnection {
        TcpConnection {
            local: SocketAddr::from((Ipv4Addr::LOCALHOST, local_port)).to_string(),
            remote: if state == "LISTEN" {
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).to_string()
            } else {
                SocketAddr::from((Ipv4Addr::LOCALHOST, 50000)).to_string()
            },
            state: state.into(),
            pid,
            process: "test".into(),
        }
    }

    #[test]
    fn handshake_roles_are_retained_until_connection_disappears() {
        for (state, direction) in [
            ("SYN-SENT", Direction::Outbound),
            ("SYN-RECV", Direction::Inbound),
        ] {
            let mut directions = Directions::default();
            let mut connection = entry(7, 8080, state);
            directions.update(&[connection.clone()]);
            assert_eq!(directions.get(&connection), direction);
            for state in ["ESTABLISHED", "FIN-WAIT-1", "TIME-WAIT"] {
                connection.state = state.into();
                directions.update(&[connection.clone()]);
                assert_eq!(directions.get(&connection), direction);
            }
            directions.update(&[]);
            assert!(directions.values.is_empty());
            assert_eq!(directions.get(&connection), Direction::Unknown);
        }
    }

    #[test]
    fn listener_matching_checks_port_address_and_owner() {
        let connection = entry(7, 8080, "ESTABLISHED");
        let mut listener = entry(7, 8080, "LISTEN");
        let mut directions = Directions::default();
        // The listener can occur after the connection in the snapshot.
        directions.update(&[connection.clone(), listener.clone()]);
        assert_eq!(directions.get(&listener), Direction::Listening);
        assert_eq!(directions.get(&connection), Direction::LikelyInbound);
        listener.local = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080)).to_string();
        directions = Directions::default();
        directions.update(&[listener.clone(), connection.clone()]);
        assert_eq!(directions.get(&connection), Direction::LikelyInbound);
        for (pid, address, port) in [
            (8, Ipv4Addr::LOCALHOST, 8080),
            (7, Ipv4Addr::LOCALHOST, 8081),
            (7, Ipv4Addr::new(192, 0, 2, 1), 8080),
        ] {
            listener.pid = pid;
            listener.local = SocketAddr::from((address, port)).to_string();
            directions = Directions::default();
            directions.update(&[listener.clone(), connection.clone()]);
            assert_eq!(directions.get(&connection), Direction::LikelyOutbound);
        }
    }

    #[test]
    fn inferred_inbound_survives_listener_closing() {
        let mut directions = Directions::default();
        let mut connection = entry(7, 8080, "ESTABLISHED");
        directions.update(&[entry(7, 8080, "LISTEN"), connection.clone()]);
        connection.state = "CLOSE-WAIT".into();
        directions.update(&[connection.clone()]);
        assert_eq!(directions.get(&connection), Direction::LikelyInbound);
    }

    #[test]
    fn ambiguous_or_invalid_connections_are_unknown() {
        let mut directions = Directions::default();
        for mut connection in [entry(0, 8080, "ESTABLISHED"), entry(7, 8080, "TIME-WAIT")] {
            directions.update(&[connection.clone()]);
            assert_eq!(directions.get(&connection), Direction::Unknown);
            connection.local = "invalid".into();
            directions.update(&[connection.clone()]);
            assert_eq!(directions.get(&connection), Direction::Unknown);
        }
        let mut connection = entry(7, 8080, "ESTABLISHED");
        connection.remote = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)).to_string();
        directions.update(&[connection.clone()]);
        assert_eq!(directions.get(&connection), Direction::Unknown);
    }

    #[test]
    fn unknown_owner_can_match_a_listener_and_ipv6_wildcards_work() {
        let mut connection = entry(0, 8080, "ESTABLISHED");
        let mut listener = entry(7, 8080, "LISTEN");
        connection.local = "[::1]:8080".into();
        connection.remote = "[::1]:50000".into();
        listener.local = "[::]:8080".into();
        let mut directions = Directions::default();
        directions.update(&[connection.clone(), listener.clone()]);
        assert_eq!(directions.get(&connection), Direction::LikelyInbound);
        listener.local = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080)).to_string();
        directions = Directions::default();
        directions.update(&[connection.clone(), listener]);
        assert_eq!(directions.get(&connection), Direction::Unknown);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_loopback_connection_has_both_inbound_and_outbound_estimates() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let server_address = listener.local_addr().unwrap();
        let client =
            TcpStream::connect_timeout(&server_address, std::time::Duration::from_secs(2)).unwrap();
        let (server, _) = listener.accept().unwrap();
        let client_address = client.local_addr().unwrap();
        let entries = crate::collector::tcp_connections().unwrap();
        let mut directions = Directions::default();
        directions.update(&entries);
        let find = |local: SocketAddr, remote: SocketAddr| {
            entries
                .iter()
                .find(|entry| {
                    entry.local == local.to_string()
                        && entry.remote == remote.to_string()
                        && entry.state == "ESTABLISHED"
                })
                .expect("loopback connection should be visible in /proc/net/tcp")
        };
        assert_eq!(
            directions.get(find(server_address, client_address)),
            Direction::LikelyInbound
        );
        assert_eq!(
            directions.get(find(client_address, server_address)),
            Direction::LikelyOutbound
        );
        drop((server, client, listener));
    }

    #[test]
    fn a_new_handshake_overrides_cached_estimates() {
        let mut directions = Directions::default();
        let mut connection = entry(7, 8080, "ESTABLISHED");
        directions.update(&[connection.clone()]);
        assert_eq!(directions.get(&connection), Direction::LikelyOutbound);
        connection.state = "SYN-RECV".into();
        directions.update(&[connection.clone()]);
        assert_eq!(directions.get(&connection), Direction::Inbound);
    }
}
