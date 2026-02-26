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
}
