use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// One upstream proxy / tunnel node (mirrors Connections mock table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyNode {
    pub name: String,
    pub endpoint: String,
    pub protocol: String, // SOCKS5 | HTTP/2 | WireGuard | ...
    pub enabled: bool,
    #[serde(default = "default_status")]
    pub status: String, // Active | Connected | Standby
    #[serde(default = "default_latency")]
    pub latency: String, // e.g. "24ms"
    #[serde(default = "default_health")]
    pub health: String, // e.g. "99.8%"
}

fn default_status() -> String {
    "Active".into()
}
fn default_latency() -> String {
    "—".into()
}
fn default_health() -> String {
    "—".into()
}

impl ProxyNode {
    pub fn new(name: &str, endpoint: &str, protocol: &str) -> Self {
        Self {
            name: name.into(),
            endpoint: endpoint.into(),
            protocol: protocol.into(),
            enabled: true,
            status: default_status(),
            latency: default_latency(),
            health: default_health(),
        }
    }
}

/// Round-robin pool with auto-failover cursor.
#[derive(Debug, Clone)]
pub struct ProxyPool {
    nodes: Vec<ProxyNode>,
    cursor: Arc<AtomicUsize>,
}

impl ProxyPool {
    pub fn new(nodes: Vec<ProxyNode>) -> Self {
        Self {
            nodes,
            cursor: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn enabled(&self) -> Vec<ProxyNode> {
        self.nodes.iter().filter(|n| n.enabled).cloned().collect()
    }

    /// Next enabled node (round-robin). Returns None when pool empty/disabled.
    pub fn next(&self) -> Option<ProxyNode> {
        let enabled = self.enabled();
        if enabled.is_empty() {
            return None;
        }
        let i = self.cursor.fetch_add(1, Ordering::Relaxed) % enabled.len();
        Some(enabled[i].clone())
    }

    /// Convert to reqwest-compatible proxy (HTTP/SOCKS5 only; WireGuard = direct).
    pub fn to_reqwest(&self, node: &ProxyNode) -> Option<reqwest::Proxy> {
        match node.protocol.as_str() {
            "SOCKS5" | "SOCKS5 Res" => reqwest::Proxy::all(&node.endpoint).ok(),
            "HTTP/2" | "HTTP" | "HTTPS" => reqwest::Proxy::all(&node.endpoint).ok(),
            _ => None, // WireGuard / direct bypass
        }
    }
}

impl Default for ProxyPool {
    fn default() -> Self {
        Self::new(vec![
            ProxyNode::new("US-East SOCKS5 Turbo", "socks5://104.28.19.44:1080", "SOCKS5"),
            ProxyNode::new("EU-Central Squid", "http://185.220.101.5:8080", "HTTP/2"),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_cycles() {
        let pool = ProxyPool::new(vec![
            ProxyNode::new("a", "socks5://a:1080", "SOCKS5"),
            ProxyNode::new("b", "socks5://b:1080", "SOCKS5"),
        ]);
        assert_eq!(pool.next().unwrap().name, "a");
        assert_eq!(pool.next().unwrap().name, "b");
        assert_eq!(pool.next().unwrap().name, "a");
    }
}
