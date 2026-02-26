mod handler;
mod upstream;

use crate::config::NodeConfig;
use crate::raft::RaftNode;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{error, info};

/// The WebSocket proxy that sits between clients and SpacetimeDB.
///
/// Clients connect to this proxy as if it were SpacetimeDB. The proxy inspects
/// each message's BSATN tag byte to classify it as a read or write:
///
/// - **Writes** (tag 3: CallReducer, tag 4: CallProcedure) go through Raft
///   consensus before being forwarded to the local SpacetimeDB instance.
/// - **Reads** (tag 0: Subscribe, tag 1: Unsubscribe, tag 2: OneOffQuery)
///   are forwarded directly — no consensus needed since all replicas have
///   identical state.
pub struct Proxy {
    config: NodeConfig,
    raft: Arc<RaftNode>,
    /// Broadcasts the database path (e.g. "/database/subscribe/mydb") to the
    /// forwarder task so it can connect to SpacetimeDB with the correct URL.
    db_path_tx: watch::Sender<String>,
}

impl Proxy {
    pub fn new(config: NodeConfig, raft: RaftNode, db_path_tx: watch::Sender<String>) -> Self {
        Self {
            config,
            raft: Arc::new(raft),
            db_path_tx,
        }
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.config.listen_addr).await?;
        info!(addr = %self.config.listen_addr, "WebSocket proxy listening");

        loop {
            let (stream, addr) = listener.accept().await?;
            let raft = self.raft.clone();
            let stdb_url = self.config.stdb_url.clone();
            let db_path_tx = self.db_path_tx.clone();

            tokio::spawn(async move {
                match handler::handle_client(stream, raft, &stdb_url, db_path_tx).await {
                    Ok(()) => info!(%addr, "Client disconnected"),
                    Err(e) => error!(%addr, error = %e, "Client connection error"),
                }
            });
        }
    }
}
