use std::hash::{Hash, Hasher};

/// Unique socket bind identifier (transport level)
/// A real server instance is defined only by host + port.
/// TLS is resolved later during handshake (SNI).
#[derive(Debug, Clone, Eq)]
pub struct ListenerKey {
    pub host: String,
    pub port: u16,
}

impl PartialEq for ListenerKey {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host && self.port == other.port
    }
}

impl Hash for ListenerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.host.hash(state);
        self.port.hash(state);
    }
}
