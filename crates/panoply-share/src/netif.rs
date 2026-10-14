//! Which network interfaces to use, and which hosts to scan.
//!
//! Only private IPv4 networks on real adapters are used by default. VPNs, containers and the
//! other virtual adapters are skipped, because announcing on them leaks the device to networks
//! the user did not mean to share with and often never reaches a phone anyway.

use std::net::Ipv4Addr;

/// One usable local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lan {
    /// Adapter name, for diagnostics.
    pub name: String,
    /// Our address on it.
    pub ip: Ipv4Addr,
    /// Subnet mask.
    pub netmask: Ipv4Addr,
}

impl Lan {
    /// Hosts on this subnet, capped to the /24 around our address so a /16 is not swept
    /// (65,534 probes). Larger networks stay discoverable by multicast. Our own address is
    /// included: another device on this computer (a second app, a test) listens there too,
    /// and our own answer is filtered out by fingerprint.
    pub fn scan_hosts(&self) -> Vec<Ipv4Addr> {
        let mask = u32::from(self.netmask);
        let ip = u32::from(self.ip);
        let wide = mask & 0xFFFF_FF00;
        // Use the real mask when it is /24 or narrower, otherwise the /24 containing us.
        let mask = if mask.count_ones() >= 24 {
            mask
        } else {
            wide.max(0xFFFF_FF00)
        };
        let net = ip & mask;
        let bcast = net | !mask;
        ((net + 1)..bcast).map(Ipv4Addr::from).collect()
    }
}

/// True for an address on this computer or its local network: loopback, RFC 1918 private,
/// link-local, and for IPv6 the unique-local and link-local ranges. Everything else is the
/// internet, and sharing never connects to it or accepts a connection from it
/// (docs/PRIVACY.md).
pub fn is_local_addr(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_local_addr(IpAddr::V4(v4)),
            None => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
        },
    }
}

/// True for RFC 1918 addresses.
pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

const VIRTUAL_PREFIXES: [&str; 18] = [
    "utun",
    "awdl",
    "llw",
    "bridge",
    "docker",
    "veth",
    "vmnet",
    "vboxnet",
    "tun",
    "tap",
    "ap",
    "anpi",
    "gif",
    "stf",
    "br-",
    "virbr",
    "tailscale",
    "zt",
];

fn looks_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| n.starts_with(p))
        || n.contains("vethernet")
        || n.contains("virtual")
        || n.contains("vpn")
}

/// Private IPv4 networks on real adapters.
pub fn lan_interfaces() -> Vec<Lan> {
    let Ok(ifaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    ifaces
        .into_iter()
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4)
                if !v4.ip.is_loopback() && is_private(v4.ip) && !looks_virtual(&i.name) =>
            {
                Some(Lan {
                    name: i.name,
                    ip: v4.ip,
                    netmask: v4.netmask,
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_addresses_count_as_local() {
        use std::net::IpAddr;
        let ok = |a: &str| is_local_addr(a.parse::<IpAddr>().unwrap());
        for a in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.20",
            "169.254.3.4",
            "::1",
            "fe80::1",
            "fd12:3456::1",
            "::ffff:192.168.0.5",
        ] {
            assert!(ok(a), "{a} is local");
        }
        for a in [
            "8.8.8.8",
            "93.184.216.34",
            "100.64.0.1",
            "172.32.0.1",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(!ok(a), "{a} is not local");
        }
    }

    #[test]
    fn scan_hosts_cover_the_slash_24() {
        let lan = Lan {
            name: "en0".into(),
            ip: Ipv4Addr::new(192, 168, 1, 20),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
        };
        let hosts = lan.scan_hosts();
        assert_eq!(hosts.len(), 254);
        assert!(hosts.contains(&lan.ip));
        assert!(hosts.contains(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 255)));
    }

    #[test]
    fn wide_networks_are_capped_to_a_slash_24() {
        let lan = Lan {
            name: "en0".into(),
            ip: Ipv4Addr::new(10, 0, 5, 9),
            netmask: Ipv4Addr::new(255, 255, 0, 0),
        };
        let hosts = lan.scan_hosts();
        assert_eq!(hosts.len(), 254);
        assert!(hosts.iter().all(|h| h.octets()[2] == 5));
    }

    #[test]
    fn virtual_adapters_are_skipped() {
        assert!(looks_virtual("utun3"));
        assert!(looks_virtual("docker0"));
        assert!(!looks_virtual("en0"));
        assert!(!looks_virtual("wlan0"));
    }
}
