mod log_store;
mod network;
pub mod state_machine;
pub mod types;

pub use types::*;

use crate::config::NodeConfig;
use anyhow::Result;
use openraft::{BasicNode, Config, Raft};
use state_machine::StateMachineStore;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{info, warn};

/// A single Raft consensus node that replicates SpacetimeDB reducer calls.
pub struct RaftNode {
    pub raft: Raft<TypeConfig>,
    pub config: Arc<NodeConfig>,
    pub state_machine: StateMachineStore,
    pub peers: BTreeMap<u64, BasicNode>,
}

/// Parse peer strings in the format "id=host:port" into a BTreeMap.
/// Example: ["2=node-2:4001", "3=node-3:4001"] → {2: BasicNode{addr: "node-2:4001"}, ...}
pub fn parse_peers(peers: &[String]) -> BTreeMap<u64, BasicNode> {
    let mut map = BTreeMap::new();
    for peer in peers {
        if let Some((id_str, addr)) = peer.split_once('=') {
            if let Ok(id) = id_str.parse::<u64>() {
                map.insert(id, BasicNode { addr: addr.to_string() });
            } else {
                warn!(peer = %peer, "Failed to parse peer node ID");
            }
        } else {
            warn!(peer = %peer, "Invalid peer format, expected 'id=host:port'");
        }
    }
    map
}

impl RaftNode {
    pub async fn new(config: &NodeConfig) -> Result<Self> {
        let raft_config = Config {
            cluster_name: "rafttimedb".to_string(),
            heartbeat_interval: 200,
            election_timeout_min: 1000,
            election_timeout_max: 2000,
            ..Default::default()
        };
        let raft_config = Arc::new(raft_config.validate()?);

        let log_store = log_store::LogStore::new();
        let state_machine = StateMachineStore::new();
        let network = network::NetworkFactory::new();

        let raft = Raft::new(
            config.node_id,
            raft_config,
            network,
            log_store,
            state_machine.clone(),
        )
        .await?;

        let peers = parse_peers(&config.peers);

        info!(
            node_id = config.node_id,
            peer_count = peers.len(),
            "Raft node initialized"
        );

        Ok(Self {
            raft,
            config: Arc::new(config.clone()),
            state_machine,
            peers,
        })
    }

    /// Propose a write (reducer call) through Raft consensus.
    /// If this node is not the leader, forwards the request to the leader via HTTP.
    pub async fn propose_write(&self, data: Vec<u8>) -> Result<()> {
        let request = ReducerCallRequest {
            raw_message: data.clone(),
        };

        match self.raft.client_write(request).await {
            Ok(_) => Ok(()),
            Err(err) => {
                // Check for ForwardToLeader — this node isn't the leader
                if let openraft::error::RaftError::APIError(
                    openraft::error::ClientWriteError::ForwardToLeader(forward),
                ) = &err
                {
                    if let Some(leader_node) = &forward.leader_node {
                        let url = format!("http://{}/cluster/write", leader_node.addr);
                        info!(leader_addr = %leader_node.addr, "Forwarding write to leader");

                        let client = reqwest::Client::new();
                        let resp = client
                            .post(&url)
                            .json(&ReducerCallRequest { raw_message: data })
                            .send()
                            .await
                            .map_err(|e| anyhow::anyhow!("Leader forward failed: {}", e))?;

                        if resp.status().is_success() {
                            return Ok(());
                        }
                        return Err(anyhow::anyhow!(
                            "Leader forward returned {}",
                            resp.status()
                        ));
                    }
                }
                Err(err.into())
            }
        }
    }

    /// Check if this node is the current Raft leader.
    pub async fn is_leader(&self) -> bool {
        self.raft.ensure_linearizable().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peers_valid() {
        let peers = vec![
            "2=node-2:4001".to_string(),
            "3=node-3:4001".to_string(),
        ];
        let map = parse_peers(&peers);
        assert_eq!(map.len(), 2);
        assert_eq!(map[&2].addr, "node-2:4001");
        assert_eq!(map[&3].addr, "node-3:4001");
    }

    #[test]
    fn test_parse_peers_empty() {
        let peers: Vec<String> = vec![];
        let map = parse_peers(&peers);
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_peers_invalid_format() {
        let peers = vec!["bad-format".to_string()];
        let map = parse_peers(&peers);
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_peers_invalid_id() {
        let peers = vec!["abc=node:4001".to_string()];
        let map = parse_peers(&peers);
        assert!(map.is_empty());
    }
}
