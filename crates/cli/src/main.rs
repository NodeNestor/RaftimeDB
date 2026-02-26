use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use openraft::BasicNode;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Parser)]
#[command(name = "rtdb", about = "RaftTimeDB cluster management CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show cluster status
    Status {
        /// RaftTimeDB node address
        #[arg(long, default_value = "http://127.0.0.1:4001")]
        addr: String,
    },

    /// Add a node to the cluster
    AddNode {
        /// Node ID to add
        #[arg(long)]
        node_id: u64,
        /// Node's Raft address (host:port)
        #[arg(long)]
        addr: String,
        /// Existing cluster node to send the request to
        #[arg(long, default_value = "http://127.0.0.1:4001")]
        cluster: String,
    },

    /// Remove a node from the cluster
    RemoveNode {
        /// Node ID to remove
        #[arg(long)]
        node_id: u64,
        /// Existing cluster node to send the request to
        #[arg(long, default_value = "http://127.0.0.1:4001")]
        cluster: String,
    },

    /// Initialize a new cluster
    Init {
        /// Node IDs and addresses (format: "id=host:port", e.g., "1=node1:4001")
        #[arg(long, num_args = 1..)]
        nodes: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match cli.command {
        Commands::Status { addr } => {
            let url = format!("{}/cluster/status", addr);
            let resp = client
                .get(&url)
                .send()
                .await
                .context("Failed to connect to node")?;

            let status: Value = resp
                .json()
                .await
                .context("Failed to parse status response")?;

            println!("Cluster Status:");
            println!("  Node ID:    {}", status["node_id"]);
            println!("  State:      {}", status["state"]);
            println!("  Leader:     {}", status["current_leader"]);
            println!("  Term:       {}", status["current_term"]);
            println!("  Applied:    {}", status["last_applied"]);
            println!("  Membership: {}", status["membership"]);
        }

        Commands::Init { nodes } => {
            // Parse "id=host:port" entries into a BTreeMap
            let mut members: BTreeMap<u64, BasicNode> = BTreeMap::new();
            let mut first_addr: Option<String> = None;

            for node_spec in &nodes {
                let (id_str, addr) = node_spec
                    .split_once('=')
                    .context(format!("Invalid node format '{}', expected 'id=host:port'", node_spec))?;

                let id: u64 = id_str
                    .parse()
                    .context(format!("Invalid node ID '{}'", id_str))?;

                if first_addr.is_none() {
                    first_addr = Some(format!("http://{}", addr));
                }

                members.insert(id, BasicNode { addr: addr.to_string() });
            }

            let target = first_addr.context("No nodes specified")?;
            let url = format!("{}/cluster/init", target);

            println!("Initializing cluster via {}...", url);
            for (id, node) in &members {
                println!("  Node {}: {}", id, node.addr);
            }

            let resp = client
                .post(&url)
                .json(&members)
                .send()
                .await
                .context("Failed to connect to node")?;

            if resp.status().is_success() {
                println!("Cluster initialized successfully!");
            } else {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Initialization failed: {}", body);
            }
        }

        Commands::AddNode {
            node_id,
            addr,
            cluster,
        } => {
            let url = format!("{}/cluster/add-node", cluster);
            println!("Adding node {} ({}) via {}...", node_id, addr, cluster);

            let resp = client
                .post(&url)
                .json(&serde_json::json!({
                    "node_id": node_id,
                    "addr": addr,
                }))
                .send()
                .await
                .context("Failed to connect to cluster")?;

            if resp.status().is_success() {
                println!("Node {} added successfully!", node_id);
            } else {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Add node failed: {}", body);
            }
        }

        Commands::RemoveNode { node_id, cluster } => {
            let url = format!("{}/cluster/remove-node", cluster);
            println!("Removing node {} via {}...", node_id, cluster);

            let resp = client
                .post(&url)
                .json(&serde_json::json!({
                    "node_id": node_id,
                }))
                .send()
                .await
                .context("Failed to connect to cluster")?;

            if resp.status().is_success() {
                println!("Node {} removed successfully!", node_id);
            } else {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Remove node failed: {}", body);
            }
        }
    }

    Ok(())
}
