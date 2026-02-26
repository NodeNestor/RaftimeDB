use anyhow::Result;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

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
    // Channel carries (origin_node_id, raw_message) so we can skip writes
    // that the local handler already forwarded to SpacetimeDB.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Vec<u8>)>();
    raft_node.state_machine.set_forwarder(tx);

    // Watch channel: the first client connection broadcasts the database path
    // (e.g. "/database/subscribe/mydb") so the forwarder knows what URL to use.
    let (db_path_tx, mut db_path_rx) = tokio::sync::watch::channel(String::new());

    // Spawn forwarder task: waits for the database path from the first client,
    // then connects to SpacetimeDB and forwards committed writes from other nodes.
    let self_node_id = config.node_id;
    let stdb_url_for_forwarder = config.stdb_url.clone();
    tokio::spawn(async move {
        // Wait until a client connects and we learn the database path
        loop {
            if db_path_rx.changed().await.is_err() {
                return; // sender dropped
            }
            let path = db_path_rx.borrow().clone();
            if !path.is_empty() {
                info!(path = %path, "Forwarder: received database path from client");
                break;
            }
        }

        let db_path = db_path_rx.borrow().clone();
        let full_url = format!("{}{}", stdb_url_for_forwarder, db_path);

        loop {
            match tokio_tungstenite::connect_async(&full_url).await {
                Ok((ws, _)) => {
                    info!(url = %full_url, "Forwarder connected to SpacetimeDB");
                    let (mut ws_tx, _ws_rx): (futures_util::stream::SplitSink<_, Message>, _) = ws.split();

                    while let Some((origin_node_id, data)) = rx.recv().await {
                        // Skip writes from this node — the handler already
                        // forwarded them through the client's connection
                        if origin_node_id == self_node_id {
                            continue;
                        }

                        if let Err(e) = ws_tx.send(Message::Binary(data.into())).await {
                            warn!(error = %e, "Forwarder: SpacetimeDB send failed, reconnecting");
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, url = %full_url, "Forwarder: SpacetimeDB connect failed, retrying in 1s");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    });

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
    let proxy = websocket::Proxy::new(config, raft_node, db_path_tx);
    proxy.run().await?;

    Ok(())
}
