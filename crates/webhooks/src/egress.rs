//! Outbound HTTP for webhooks with an SSRF guard (SSRF-01..07, docs/14 §1.7, docs/06 T-09).
//!
//! The Smokescreen pattern in-process: resolve the host, refuse private, loopback, link-local,
//! CGNAT, multicast, broadcast, documentation, benchmark, ULA, IPv4-mapped and cloud-metadata
//! addresses, then connect to exactly the address that was checked (so DNS rebinding can't swap
//! it), with redirects off, http(s) only on allowed ports, a short timeout and a capped response.
//! Self-hosted operators can allow internal ranges explicitly with `PROCERA_EGRESS_ALLOW`
//! (comma-separated CIDRs, e.g. `10.0.0.0/8,127.0.0.1/32`); those also allow plain http and any port.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

/// Ports reachable without an allow-list entry.
const PORTS: &[u16] = &[443, 80, 8443, 8080];
/// Response bytes kept for the delivery log.
pub const MAX_BODY: usize = 4 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cidr {
    net: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(s: &str) -> Option<Self> {
        let (ip, prefix) = match s.trim().split_once('/') {
            Some((ip, p)) => (ip.parse::<IpAddr>().ok()?, p.parse::<u8>().ok()?),
            None => {
                let ip = s.trim().parse::<IpAddr>().ok()?;
                (ip, if ip.is_ipv4() { 32 } else { 128 })
            }
        };
        let max = if ip.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { net: ip, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                (u32::from(n) & mask) == (u32::from(a) & mask)
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                (u128::from(n) & mask) == (u128::from(a) & mask)
            }
            _ => false,
        }
    }
}

/// The operator's allow-list (`PROCERA_EGRESS_ALLOW`).
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub allow: Vec<Cidr>,
}

impl Policy {
    pub fn from_env() -> Self {
        let allow = std::env::var("PROCERA_EGRESS_ALLOW")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| {
                let c = Cidr::parse(s);
                if c.is_none() {
                    tracing::warn!(entry = s, "PROCERA_EGRESS_ALLOW: not a CIDR, ignored");
                }
                c
            })
            .collect();
        Self { allow }
    }

    fn allowed(&self, ip: IpAddr) -> bool {
        self.allow.iter().any(|c| c.contains(ip))
    }
}

/// Whether an address is on the public internet (not private, loopback, link-local, metadata…).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => public_v4(a),
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = a.segments();
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64 well-known prefix (would reach v4 space)
                || s[0] == 0x2002) // 6to4
        }
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    !(a.is_private()
        || a.is_loopback()
        || a.is_link_local() // 169.254.0.0/16, incl. cloud metadata 169.254.169.254
        || a.is_broadcast()
        || a.is_documentation()
        || a.is_unspecified()
        || a.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64.0.0/10
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // benchmarking 198.18.0.0/15
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || o[0] >= 240) // reserved
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EgressError {
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Failed(String),
}

/// A URL that passed the static checks (scheme, host, port, no credentials).
#[derive(Debug, Clone)]
pub struct Target {
    pub url: reqwest::Url,
    pub host: String,
    pub port: u16,
}

/// Checks a URL's shape: http(s), a host name or address, no user info, an allowed port (unless
/// allow-listed later), and https unless the operator allow-lists the address.
pub fn parse(raw: &str) -> Result<Target, EgressError> {
    let refuse = |m: &str| EgressError::Refused(m.to_owned());
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| refuse("That isn't a valid URL."))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(refuse("Only https URLs can receive webhooks."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refuse("Put credentials in a header or the path, not in the URL."));
    }
    let host =
        url.host_str().ok_or_else(|| refuse("The URL needs a host."))?.trim_matches(['[', ']']).to_owned();
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
        || host.ends_with(".local")
    {
        return Err(refuse("That address is inside a private network, so Procera won't send to it."));
    }
    let port = url.port_or_known_default().ok_or_else(|| refuse("The URL needs a port."))?;
    Ok(Target { url, host, port })
}

/// Resolves the target and picks an address it may connect to.
pub async fn resolve(t: &Target, policy: &Policy) -> Result<SocketAddr, EgressError> {
    let addrs: Vec<SocketAddr> = match t.host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, t.port)],
        Err(_) => tokio::net::lookup_host((t.host.as_str(), t.port))
            .await
            .map_err(|_| EgressError::Failed(format!("couldn't resolve {}", t.host)))?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(EgressError::Failed(format!("{} has no address", t.host)));
    }
    // Every address must be acceptable: a name that also points inside the network is refused.
    for a in &addrs {
        let ip = a.ip();
        if policy.allowed(ip) {
            continue;
        }
        if !is_public(ip) {
            return Err(EgressError::Refused(format!(
                "{} resolves to a private or reserved address ({ip}), so Procera won't send to it.",
                t.host
            )));
        }
        if t.url.scheme() != "https" {
            return Err(EgressError::Refused("Only https URLs can receive webhooks.".into()));
        }
        if !PORTS.contains(&t.port) {
            return Err(EgressError::Refused(format!("Port {} isn't allowed; use 443.", t.port)));
        }
    }
    Ok(addrs[0])
}

/// The outcome of one POST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
    pub millis: u64,
    pub retry_after: Option<u64>,
}

/// POSTs `body` with `headers` to `target`, connecting only to `addr`. No redirects; capped body.
pub async fn post(
    target: &Target,
    addr: SocketAddr,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<Response, EgressError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(5))
        .resolve(&target.host, addr)
        .no_proxy()
        .user_agent("Procera-Webhooks/1")
        .build()
        .map_err(|e| EgressError::Failed(e.to_string()))?;
    let mut req = client.post(target.url.clone()).body(body);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let started = Instant::now();
    let res = req.send().await.map_err(|e| {
        EgressError::Failed(if e.is_timeout() {
            "timed out".into()
        } else if e.is_connect() {
            "couldn't connect".into()
        } else {
            "the request failed".into()
        })
    })?;
    let status = res.status().as_u16();
    let retry_after = res
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let mut body = Vec::new();
    let mut stream = res;
    while body.len() < MAX_BODY {
        match stream.chunk().await {
            Ok(Some(c)) => body.extend_from_slice(&c[..c.len().min(MAX_BODY - body.len())]),
            _ => break,
        }
    }
    Ok(Response {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
        millis: started.elapsed().as_millis() as u64,
        retry_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_reserved_addresses_are_not_public() {
        for ip in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.1.1",
            "0.0.0.0",
            "198.18.0.5",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2002::1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn cidrs() {
        let c = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(c.contains("10.9.8.7".parse().unwrap()));
        assert!(!c.contains("11.0.0.1".parse().unwrap()));
        assert!(Cidr::parse("127.0.0.1").unwrap().contains("127.0.0.1".parse().unwrap()));
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("fd00::/8").unwrap().contains("fd12::1".parse().unwrap()));
    }

    #[test]
    fn url_shape() {
        assert!(parse("https://hooks.example.com/procera").is_ok());
        assert!(parse("ftp://example.com").is_err());
        assert!(parse("https://user:pw@example.com").is_err());
        assert!(parse("http://localhost:8080/x").is_err());
        assert!(parse("https://metadata.google.internal/").is_err());
    }

    #[tokio::test]
    async fn resolution_refuses_private_targets_unless_allow_listed() {
        let none = Policy::default();
        let meta = parse("http://169.254.169.254/latest/meta-data").unwrap();
        assert!(matches!(resolve(&meta, &none).await, Err(EgressError::Refused(_))));
        let local = parse("http://127.0.0.1:9999/hook").unwrap();
        assert!(matches!(resolve(&local, &none).await, Err(EgressError::Refused(_))));
        let allow = Policy { allow: vec![Cidr::parse("127.0.0.1/32").unwrap()] };
        assert_eq!(resolve(&local, &allow).await.unwrap(), "127.0.0.1:9999".parse().unwrap());
        let plain = parse("http://93.184.216.34/hook").unwrap();
        assert!(matches!(resolve(&plain, &none).await, Err(EgressError::Refused(_))), "public but not https");
        let odd = parse("https://93.184.216.34:6379/").unwrap();
        assert!(matches!(resolve(&odd, &none).await, Err(EgressError::Refused(_))), "port not allowed");
    }
}
