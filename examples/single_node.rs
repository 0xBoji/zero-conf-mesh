//! Minimal single-node `coding_agent_mesh_presence` demo.
//!
//! Run with:
//! - `cargo run --example single_node`
//! - `ZCM_MDNS_PORT=5454 cargo run --example single_node`

use std::{error::Error, time::Duration};

use coding_agent_mesh_presence::{DEFAULT_MDNS_PORT, ZeroConfMesh};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mdns_port = std::env::var("ZCM_MDNS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_MDNS_PORT);

    let mesh = ZeroConfMesh::builder()
        .agent_id("agent-solo")
        .role("worker")
        .project("demo")
        .branch("main")
        .port(7000)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_secs(1))
        .ttl(Duration::from_secs(4))
        .metadata("capability", "demo")
        .build()
        .await?;

    let local = mesh.local_agent().await;
    let agents = mesh.agents().await;

    println!("local agent id: {}", mesh.local_agent_id());
    println!("instance name: {}", local.instance_name());
    println!("branch: {}", local.branch());
    println!("known agents in registry: {}", agents.len());

    mesh.shutdown().await?;
    Ok(())
}
