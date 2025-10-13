use std::net::IpAddr;

use ipnet::{Ipv4Net, Ipv6Net};
use libp2p::Multiaddr;
use tracing::debug;

/// Extract IP address from a Multiaddr
pub fn extract_ip(addr: &Multiaddr) -> Option<IpAddr> {
    use libp2p::multiaddr::Protocol;

    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => return Some(IpAddr::V4(ip)),
            Protocol::Ip6(ip) => return Some(IpAddr::V6(ip)),
            _ => continue,
        }
    }
    None
}

/// Check if an IP address is private (non-globally routable)
///
/// For IPv4: RFC1918 private addresses (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
/// For IPv6: Unique Local Addresses (fc00::/7) and Link-Local addresses (fe80::/10)
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_private(),
        IpAddr::V6(ipv6) => {
            ipv6.is_unique_local()           // fc00::/7 (ULA)
            || ipv6.is_unicast_link_local() // fe80::/10 (Link-Local)
        }
    }
}

/// Check if two IPs are in the same subnet using the ipnet crate
pub fn same_subnet(ip1: &IpAddr, ip2: &IpAddr, prefix_len: u8) -> bool {
    match (ip1, ip2) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            // Create network from first IP + prefix length
            Ipv4Net::new(*a, prefix_len)
                .map(|net| net.contains(b))
                .unwrap_or(false)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            // Create network from first IP + prefix length
            Ipv6Net::new(*a, prefix_len)
                .map(|net| net.contains(b))
                .unwrap_or(false)
        }
        _ => false, // IPv4 vs IPv6 - different families
    }
}

/// Filter addresses based on reachability from our network context
///
/// Rules:
/// - always filter loopback addresses (unless that's all we have)
/// - if both are private IPs, only keep addresses in the same subnet
/// - if we're public, filter all private IPs from peers
/// - if we're private and they're public, keep their public IPs
///
/// Handles multi-homed nodes by checking reachability from ANY local address
pub fn filter_reachable_addresses(
    addrs: &[Multiaddr],
    own_addrs: &[Multiaddr],
    peer_info: &str,
) -> Vec<Multiaddr> {
    // Filter loopback addresses (127.0.0.1, ::1) from peer addresses
    let non_loopback_addrs: Vec<_> = addrs
        .iter()
        .filter(|addr| {
            let addr_str = addr.to_string();
            !addr_str.contains("127.0.0.1") && !addr_str.contains("::1")
        })
        .cloned()
        .collect();

    // If peer only has loopback addresses (local testing), keep them
    if non_loopback_addrs.is_empty() {
        if !addrs.is_empty() {
            debug!(
                "Peer {} only has loopback addresses, keeping for local testing",
                peer_info
            );
        }
        return addrs.to_vec();
    }

    // Filter loopback from our own addresses
    let own_addrs_filtered: Vec<_> = own_addrs
        .iter()
        .filter(|addr| {
            let addr_str = addr.to_string();
            !addr_str.contains("127.0.0.1") && !addr_str.contains("::1")
        })
        .collect();

    // If we have no own addresses, return non-loopback (conservative filtering)
    if own_addrs_filtered.is_empty() {
        debug!(
            "No own addresses available, using conservative filtering for peer {}",
            peer_info
        );
        return non_loopback_addrs;
    }

    // Second pass: filter by network reachability from ANY of our addresses
    let filtered: Vec<_> = non_loopback_addrs
        .iter()
        .filter(|addr| {
            let Some(peer_ip) = extract_ip(addr) else {
                // Keep non-IP addresses (e.g., DNS names)
                return true;
            };

            let peer_is_private = is_private_ip(&peer_ip);

            // Check if reachable from ANY of our local addresses
            for own_addr in &own_addrs_filtered {
                let Some(own_ip) = extract_ip(own_addr) else {
                    continue;
                };

                let own_is_private = is_private_ip(&own_ip);

                let is_reachable = match (own_is_private, peer_is_private) {
                    (true, true) => {
                        // Both private: reachable if same /16 subnet
                        same_subnet(&own_ip, &peer_ip, 16)
                    }
                    (true, false) => true, // We're private, they're public: reachable
                    (false, true) => false, // We're public, they're private: not reachable
                    (false, false) => true, // Both public: reachable
                };

                if is_reachable {
                    return true; // Reachable from this local address
                }
            }

            // Not reachable from any of our addresses
            debug!(
                "Filtering peer {} address {} - not reachable from any local address",
                peer_info, addr
            );
            false
        })
        .cloned()
        .collect();

    if filtered.len() != addrs.len() {
        debug!(
            "Filtered reachable addresses for peer {}: {} -> {} addresses",
            peer_info,
            addrs.len(),
            filtered.len()
        );
    }

    filtered
}
