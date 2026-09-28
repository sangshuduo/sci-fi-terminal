//! Active socket listing via netstat2 (OS APIs, never the `netstat` binary).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use netstat2::{
    AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, SocketInfo, TcpState,
    iterate_sockets_info,
};

/// Hard upper bound on the number of connections returned.
pub const MAX_CONNECTIONS: usize = 200;

/// Upper bound on raw sockets inspected per call, to keep work bounded.
const MAX_SOCKETS_SCANNED: usize = 20_000;

/// Upper bound on pids kept per connection.
const MAX_PIDS_PER_CONNECTION: usize = 8;

/// Transport protocol of a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// TCP socket.
    Tcp,
    /// UDP socket.
    Udp,
}

/// Simplified socket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Established TCP connection.
    Established,
    /// Listening TCP socket.
    Listen,
    /// Any other TCP state, or a UDP socket.
    Other,
}

/// One active socket.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    /// Transport protocol.
    pub protocol: Protocol,
    /// Local address and port.
    pub local: SocketAddr,
    /// Remote address and port; `None` for UDP and for unconnected TCP.
    pub remote: Option<SocketAddr>,
    /// Simplified state.
    pub state: ConnectionState,
    /// Owning process ids (may be empty if the OS does not expose them).
    pub pids: Vec<u32>,
}

/// Active sockets via netstat2 (IPv4+IPv6, TCP+UDP). Established TCP first,
/// then listeners, then others; capped at [`MAX_CONNECTIONS`]. Returns
/// `Err(String)` with a user-safe message if the OS denies access.
pub fn list_connections() -> Result<Vec<Connection>, String> {
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let proto = ProtocolFlags::TCP | ProtocolFlags::UDP;
    let iter = iterate_sockets_info(af, proto)
        .map_err(|e| format!("Unable to read network connections from the OS: {e}"))?;
    // Individual socket errors (e.g. a process exiting mid-scan) are skipped.
    let mut conns: Vec<Connection> = iter
        .take(MAX_SOCKETS_SCANNED)
        .filter_map(Result::ok)
        .map(to_connection)
        .collect();
    conns.sort_by_key(|c| state_rank(c.state));
    conns.truncate(MAX_CONNECTIONS);
    Ok(conns)
}

/// Converts a netstat2 record into a [`Connection`].
fn to_connection(info: SocketInfo) -> Connection {
    let mut pids = info.associated_pids;
    pids.truncate(MAX_PIDS_PER_CONNECTION);
    match info.protocol_socket_info {
        ProtocolSocketInfo::Tcp(tcp) => {
            let state = map_tcp_state(tcp.state);
            let remote = (state != ConnectionState::Listen && !tcp.remote_addr.is_unspecified())
                .then(|| SocketAddr::new(tcp.remote_addr, tcp.remote_port));
            Connection {
                protocol: Protocol::Tcp,
                local: SocketAddr::new(tcp.local_addr, tcp.local_port),
                remote,
                state,
                pids,
            }
        }
        ProtocolSocketInfo::Udp(udp) => Connection {
            protocol: Protocol::Udp,
            local: SocketAddr::new(udp.local_addr, udp.local_port),
            remote: None,
            state: ConnectionState::Other,
            pids,
        },
    }
}

/// Maps the detailed TCP state onto [`ConnectionState`].
fn map_tcp_state(state: TcpState) -> ConnectionState {
    match state {
        TcpState::Established => ConnectionState::Established,
        TcpState::Listen => ConnectionState::Listen,
        _ => ConnectionState::Other,
    }
}

/// Sort rank: established first, then listeners, then everything else.
fn state_rank(state: ConnectionState) -> u8 {
    match state {
        ConnectionState::Established => 0,
        ConnectionState::Listen => 1,
        ConnectionState::Other => 2,
    }
}

/// Returns true only for globally routable unicast addresses.
///
/// False for loopback, private (10/8, 172.16/12, 192.168/16, fc00::/7),
/// link-local, unspecified, multicast, broadcast, documentation, CGNAT
/// (100.64/10) and other reserved ranges. IPv4-mapped IPv6 addresses are
/// judged by their embedded IPv4 address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

/// IPv4 half of [`is_public`].
fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let reserved = a == 0 // "this network" 0/8
        || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments 192.0.0/24
        || (a == 198 && (b == 18 || b == 19)) // benchmarking 198.18/15
        || a >= 240; // reserved 240/4 and broadcast
    !(reserved
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation())
}

/// IPv6 half of [`is_public`].
fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let unique_local = (s[0] & 0xfe00) == 0xfc00; // fc00::/7
    let link_local = (s[0] & 0xffc0) == 0xfe80; // fe80::/10
    let site_local = (s[0] & 0xffc0) == 0xfec0; // deprecated fec0::/10
    let documentation = s[0] == 0x2001 && s[1] == 0x0db8; // 2001:db8::/32
    let discard = s[0] == 0x0100 && s[1..4] == [0, 0, 0]; // 100::/64
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || unique_local
        || link_local
        || site_local
        || documentation
        || discard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn public_v4_addresses() {
        for s in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "100.63.255.255",
            "100.128.0.1",
        ] {
            assert!(is_public(ip(s)), "{s} should be public");
        }
        for s in ["172.15.255.255", "172.32.0.1", "192.167.1.1", "11.0.0.1"] {
            assert!(is_public(ip(s)), "{s} should be public");
        }
    }

    #[test]
    fn non_public_v4_addresses() {
        let cases = [
            "127.0.0.1",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.1.1",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "239.255.255.255",
            "255.255.255.255",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "100.64.0.1",
            "100.127.255.255",
            "198.18.0.1",
            "240.0.0.1",
            "192.0.0.1",
        ];
        for s in cases {
            assert!(!is_public(ip(s)), "{s} should not be public");
        }
    }

    #[test]
    fn public_v6_addresses() {
        for s in [
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "2a00:1450::1",
        ] {
            assert!(is_public(ip(s)), "{s} should be public");
        }
    }

    #[test]
    fn non_public_v6_addresses() {
        let cases = [
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "100::1",
        ];
        for s in cases {
            assert!(!is_public(ip(s)), "{s} should not be public");
        }
    }

    #[test]
    fn ipv4_mapped_v6_uses_inner_address() {
        assert!(!is_public(ip("::ffff:192.168.0.1")));
        assert!(!is_public(ip("::ffff:127.0.0.1")));
        assert!(is_public(ip("::ffff:8.8.8.8")));
    }

    #[test]
    fn state_rank_orders_established_listen_other() {
        assert!(state_rank(ConnectionState::Established) < state_rank(ConnectionState::Listen));
        assert!(state_rank(ConnectionState::Listen) < state_rank(ConnectionState::Other));
        assert_eq!(map_tcp_state(TcpState::TimeWait), ConnectionState::Other);
        assert_eq!(map_tcp_state(TcpState::Listen), ConnectionState::Listen);
    }
}
