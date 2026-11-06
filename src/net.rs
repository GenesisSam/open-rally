use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_private_or_tailscale(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 100 && (64..=127).contains(&o[1]))
}

pub fn is_tailscale(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..=127).contains(&o[1])
}

fn host_part(host_header: &str) -> &str {
    let h = host_header.trim();
    if let Some(rest) = h.strip_prefix('[') {
        return match rest.find(']') {
            Some(i) => &rest[..i],
            None => "",
        };
    }
    if h.matches(':').count() == 1 {
        return &h[..h.find(':').unwrap()];
    }
    h
}

pub fn host_allowed(host_header: &str) -> bool {
    let host = host_part(host_header);
    if host.is_empty() {
        return false;
    }
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        return is_private_or_tailscale(v4);
    }
    if let Ok(v6) = host.parse::<Ipv6Addr>() {
        return v6 == Ipv6Addr::LOCALHOST;
    }
    let lower = host.to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".ts.net")
}

pub fn origin_allowed(origin_header: &str) -> bool {
    let Some((_, rest)) = origin_header.split_once("://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    host_allowed(authority)
}

fn interface_v4s() -> Vec<Ipv4Addr> {
    if_addrs::get_if_addrs()
        .map(|ifs| {
            ifs.into_iter()
                .filter_map(|i| match i.ip() {
                    IpAddr::V4(v4) if !v4.is_loopback() => Some(v4),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn tailscale_ip() -> Option<Ipv4Addr> {
    interface_v4s().into_iter().find(|ip| is_tailscale(*ip))
}

pub fn lan_ip() -> Option<Ipv4Addr> {
    interface_v4s()
        .into_iter()
        .find(|ip| !is_tailscale(*ip) && is_private_or_tailscale(*ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_allowlist() {
        for ok in [
            "localhost",
            "localhost:7777",
            "127.0.0.1:7777",
            "[::1]:7777",
            "[::1]",
            "100.100.100.100:7777",
            "192.168.0.12",
            "10.0.0.5:80",
            "172.20.1.1",
            "mymac.local:7777",
            "MyMac.local",
            "mymac.tailnet.ts.net",
        ] {
            assert!(host_allowed(ok), "{ok}");
        }
        for bad in [
            "evil.com",
            "evil.com:7777",
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "ts.net.evil.com",
            "",
            "local",
            "[2001:db8::1]:7777",
        ] {
            assert!(!host_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn origin_allowlist() {
        assert!(origin_allowed("http://100.100.100.100:7777"));
        assert!(origin_allowed("http://[::1]:7777"));
        assert!(origin_allowed("http://localhost"));
        assert!(!origin_allowed("http://evil.com"));
        assert!(!origin_allowed("null"));
        assert!(!origin_allowed(""));
    }

    #[test]
    fn interface_lookup_does_not_panic() {
        let _ = tailscale_ip();
        let _ = lan_ip();
    }
}
