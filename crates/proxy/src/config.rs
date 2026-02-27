/// Configuration for a single RaftTimeDB node.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Unique node ID within the cluster.
    pub node_id: u64,

    /// Address to listen for client WebSocket connections.
    pub listen_addr: String,

    /// Address to listen for Raft inter-node communication.
    pub raft_addr: String,

    /// Local SpacetimeDB WebSocket URL.
    pub stdb_url: String,

    /// Raft peer addresses.
    pub peers: Vec<String>,

    /// Data directory for persistent state.
    pub data_dir: String,

    /// Path to TLS certificate file (PEM). Enables TLS when set.
    pub tls_cert: Option<String>,

    /// Path to TLS private key file (PEM). Required when tls_cert is set.
    pub tls_key: Option<String>,

    /// Path to CA certificate for verifying peer nodes (PEM). Optional.
    pub tls_ca_cert: Option<String>,
}

impl NodeConfig {
    /// Returns true if TLS is configured for inter-node communication.
    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }

    /// Returns the URL scheme to use for inter-node HTTP requests.
    pub fn http_scheme(&self) -> &str {
        if self.tls_enabled() { "https" } else { "http" }
    }
}
