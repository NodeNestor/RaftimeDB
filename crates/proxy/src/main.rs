use anyhow::Result;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

mod api;
mod config;
mod metrics;
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

    /// Path to TLS certificate file (PEM). Enables TLS when set with --tls-key.
    #[arg(long, env = "RTDB_TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to TLS private key file (PEM). Required when --tls-cert is set.
    #[arg(long, env = "RTDB_TLS_KEY")]
    tls_key: Option<String>,

    /// Path to CA certificate for verifying peer nodes (PEM). Optional.
    #[arg(long, env = "RTDB_TLS_CA_CERT")]
    tls_ca_cert: Option<String>,
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

    // Validate TLS config
    if cli.tls_cert.is_some() != cli.tls_key.is_some() {
        anyhow::bail!("Both --tls-cert and --tls-key must be provided together");
    }

    info!(
        node_id = cli.node_id,
        listen_addr = %cli.listen_addr,
        raft_addr = %cli.raft_addr,
        stdb_url = %cli.stdb_url,
        tls = cli.tls_cert.is_some(),
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
        tls_cert: cli.tls_cert,
        tls_key: cli.tls_key,
        tls_ca_cert: cli.tls_ca_cert,
    };

    // Initialize the Raft node
    let raft_node = raft::RaftNode::new(&config).await?;

    // Shutdown coordination: send `true` to signal all tasks to stop
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

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
    let mut forwarder_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        // Wait until a client connects and we learn the database path
        loop {
            tokio::select! {
                result = db_path_rx.changed() => {
                    if result.is_err() {
                        return; // sender dropped
                    }
                    let path = db_path_rx.borrow().clone();
                    if !path.is_empty() {
                        info!(path = %path, "Forwarder: received database path from client");
                        break;
                    }
                }
                _ = forwarder_shutdown.changed() => {
                    return;
                }
            }
        }

        let db_path = db_path_rx.borrow().clone();
        let full_url = format!("{}{}", stdb_url_for_forwarder, db_path);

        loop {
            if *forwarder_shutdown.borrow() {
                info!("Forwarder shutting down");
                return;
            }

            match tokio_tungstenite::connect_async(&full_url).await {
                Ok((ws, _)) => {
                    info!(url = %full_url, "Forwarder connected to SpacetimeDB");
                    let (mut ws_tx, _ws_rx): (futures_util::stream::SplitSink<_, Message>, _) = ws.split();

                    loop {
                        tokio::select! {
                            msg = rx.recv() => {
                                let Some((origin_node_id, data)) = msg else {
                                    return;
                                };

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
                            _ = forwarder_shutdown.changed() => {
                                info!("Forwarder shutting down");
                                return;
                            }
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

    // Spawn metrics updater (periodically updates Raft metrics for Prometheus)
    let raft_for_metrics = raft_node.raft.clone();
    let node_id_for_metrics = config.node_id;
    let mut metrics_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let m = raft_for_metrics.metrics().borrow().clone();
                    metrics::RAFT_TERM.set(m.vote.leader_id().term as i64);
                    let is_leader = m.current_leader == Some(node_id_for_metrics);
                    metrics::RAFT_IS_LEADER.set(if is_leader { 1 } else { 0 });
                }
                _ = metrics_shutdown.changed() => {
                    return;
                }
            }
        }
    });

    // Build TLS config if cert and key are provided
    let tls_config = if config.tls_enabled() {
        Some(build_tls_config(
            config.tls_cert.as_ref().unwrap(),
            config.tls_key.as_ref().unwrap(),
        )?)
    } else {
        None
    };

    // Start the HTTP management API server on the raft address
    let app_state = Arc::new(api::AppState {
        raft: raft_node.raft.clone(),
        node_id: config.node_id,
        peers: raft_node.peers.clone(),
        http_scheme: config.http_scheme().to_string(),
    });
    let http_router = api::router(app_state);
    let http_listener = tokio::net::TcpListener::bind(&cli.raft_addr).await?;

    if let Some(ref tls) = tls_config {
        info!(addr = %cli.raft_addr, "HTTPS management API listening (TLS enabled)");
        let acceptor = tokio_rustls::TlsAcceptor::from(tls.clone());
        let router = http_router.clone();
        let http_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            serve_tls(http_listener, acceptor, router, http_shutdown).await;
        });
    } else {
        info!(addr = %cli.raft_addr, "HTTP management API listening");
        let mut http_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let graceful = axum::serve(http_listener, http_router)
                .with_graceful_shutdown(async move {
                    let _ = http_shutdown.changed().await;
                });
            if let Err(e) = graceful.await {
                tracing::error!(error = %e, "HTTP server error");
            }
        });
    };

    // Start the WebSocket proxy
    let proxy = websocket::Proxy::new(config, raft_node, db_path_tx, shutdown_rx.clone());

    // Run proxy until shutdown signal
    tokio::select! {
        result = proxy.run() => {
            if let Err(e) = result {
                tracing::error!(error = %e, "WebSocket proxy error");
            }
        }
        _ = shutdown_signal() => {
            info!("Initiating graceful shutdown...");
            let _ = shutdown_tx.send(true);

            // Give connections time to drain
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            info!("Shutdown complete");
        }
    }

    Ok(())
}

/// Wait for a shutdown signal (Ctrl+C on all platforms, SIGTERM on Unix).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ).expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }

    info!("Shutdown signal received");
}

/// Build a rustls ServerConfig from PEM cert and key files.
fn build_tls_config(cert_path: &str, key_path: &str) -> Result<Arc<rustls::ServerConfig>> {
    use rustls_pemfile::{certs, private_key};
    use std::fs::File;
    use std::io::BufReader;

    let cert_file = File::open(cert_path)?;
    let key_file = File::open(key_path)?;

    let certs: Vec<_> = certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()?;

    let key = private_key(&mut BufReader::new(key_file))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in {}", key_path))?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(Arc::new(config))
}

/// Serve an axum router over TLS using tokio-rustls.
async fn serve_tls(
    listener: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    app: axum::Router,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use tower::ServiceExt;

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _addr) = match result {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "TLS listener accept error");
                        continue;
                    }
                };

                let acceptor = acceptor.clone();
                let app = app.clone();

                tokio::spawn(async move {
                    let tls_stream = match acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(error = %e, "TLS handshake failed");
                            return;
                        }
                    };

                    let io = TokioIo::new(tls_stream);
                    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                        let app = app.clone();
                        async move {
                            let (parts, body) = req.into_parts();
                            let req = hyper::Request::from_parts(parts, axum::body::Body::new(body));
                            app.oneshot(req).await
                        }
                    });

                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection(io, service)
                        .await
                    {
                        warn!(error = %e, "TLS connection serve error");
                    }
                });
            }
            _ = shutdown.changed() => {
                info!("TLS server shutting down");
                break;
            }
        }
    }
}
