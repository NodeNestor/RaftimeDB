use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::info;

mod api;
mod config;
mod raft;
mod router;
mod websocket;

#[derive(Parser)]
#[command(name = "rafttimedb", about = "Distributed clustering layer for SpacetimeDB")]
struct Cli {
    /// Node ID (unique within the cluster)
    #[arg(long, env = "RTDB_NODE_ID")]
    node_id: u64,

    /// Listen address for client WebSocket connections
    #[arg(long, default_value = "0.0.0.0:3001", env = "RTDB_LISTEN_ADDR")]
    listen_addr: String,

    /// Listen address for Raft inter-node communication
    #[arg(long, default_value = "0.0.0.0:4001", env = "RTDB_RAFT_ADDR")]
    raft_addr: String,

    /// Local SpacetimeDB WebSocket URL
    #[arg(long, default_value = "ws://127.0.0.1:3000", env = "RTDB_STDB_URL")]
    stdb_url: String,

    /// Comma-separated list of peer Raft addresses (format: "id=host:port")
    #[arg(long, env = "RTDB_PEERS", value_delimiter = ',')]
    peers: Vec<String>,

    /// Data directory for Raft log persistence
    #[arg(long, default_value = "./data", env = "RTDB_DATA_DIR")]
    data_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rafttimedb=info".into()),
        )
        .init();

    let cli = Cli::parse();

    info!(
        node_id = cli.node_id,
        listen_addr = %cli.listen_addr,
        raft_addr = %cli.raft_addr,
        stdb_url = %cli.stdb_url,
        peers = ?cli.peers,
        "Starting RaftTimeDB"
    );

    let config = config::NodeConfig {
        node_id: cli.node_id,
        listen_addr: cli.listen_addr,
        raft_addr: cli.raft_addr.clone(),
        stdb_url: cli.stdb_url,
        peers: cli.peers,
        data_dir: cli.data_dir,
    };

    // Initialize the Raft node
    let raft_node = raft::RaftNode::new(&config).await?;

    // Set up the forwarder channel (state machine → SpacetimeDB)
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    raft_node.state_machine.set_forwarder(tx);

    // Start the HTTP management API server on the raft address
    let app_state = Arc::new(api::AppState {
        raft: raft_node.raft.clone(),
        node_id: config.node_id,
    });
    let http_router = api::router(app_state);
    let http_listener = tokio::net::TcpListener::bind(&cli.raft_addr).await?;
    info!(addr = %cli.raft_addr, "HTTP management API listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(http_listener, http_router).await {
            tracing::error!(error = %e, "HTTP server error");
        }
    });

    // Start the WebSocket proxy
    let proxy = websocket::Proxy::new(config, raft_node);
    proxy.run().await?;

    Ok(())
}
