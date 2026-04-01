//! Runnable two-node `zero-conf-mesh` demo.
//!
//! Run with:
//! - `cargo run --example two_nodes`
//! - `ZCM_MDNS_PORT=5454 cargo run --example two_nodes`

use std::{
    error::Error,
    net::{Ipv4Addr, UdpSocket},
    time::Duration,
};

use tokio::time;
use tracing_subscriber::{EnvFilter, fmt};
use zero_conf_mesh::{AgentEvent, AgentStatus, DEFAULT_MDNS_PORT, ZeroConfMesh};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let mdns_port = std::env::var("ZCM_MDNS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_MDNS_PORT);

    println!("starting demo with mdns_port={mdns_port}");
    println!("tip: if default 5353 conflicts on your machine, rerun with ZCM_MDNS_PORT=5454");

    let node_a = ZeroConfMesh::builder()
        .agent_id("agent-a")
        .role("coder")
        .project("demo")
        .branch("feature/mesh")
        .port(7001)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_millis(500))
        .ttl(Duration::from_secs(3))
        .metadata("capability", "implementation")
        .build()
        .await?;

    let node_b = ZeroConfMesh::builder()
        .agent_id("agent-b")
        .role("reviewer")
        .project("demo")
        .branch("main")
        .port(7002)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_millis(500))
        .ttl(Duration::from_secs(3))
        .metadata("capability", "review")
        .build()
        .await?;

    let events_a = spawn_event_logger("agent-a", node_a.subscribe());
    let events_b = spawn_event_logger("agent-b", node_b.subscribe());

    wait_for_discovery(&node_a, "agent-b").await?;
    wait_for_discovery(&node_b, "agent-a").await?;

    println!("both nodes discovered each other");
    println!(
        "node_a peers: {:?}",
        ids(&node_a.agents_by_project("demo").await)
    );
    println!(
        "node_b peers: {:?}",
        ids(&node_b.agents_by_project("demo").await)
    );

    node_a.update_status(AgentStatus::Busy).await?;
    time::sleep(Duration::from_secs(1)).await;

    node_a.shutdown().await?;
    node_b.shutdown().await?;

    events_a.abort();
    events_b.abort();

    Ok(())
}

async fn wait_for_discovery(
    mesh: &ZeroConfMesh,
    target_agent_id: &str,
) -> Result<(), Box<dyn Error>> {
    let deadline = time::Instant::now() + Duration::from_secs(8);
    while time::Instant::now() < deadline {
        if mesh.get_agent(target_agent_id).await.is_some() {
            return Ok(());
        }
        time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("timed out waiting to discover `{target_agent_id}`").into())
}

fn spawn_event_logger(
    name: &'static str,
    mut rx: tokio::sync::broadcast::Receiver<AgentEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            println!("[{name}] event: {event:?}");
        }
    })
}

fn ids(agents: &[zero_conf_mesh::AgentInfo]) -> Vec<&str> {
    agents.iter().map(|agent| agent.id()).collect()
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).try_init();
}

#[allow(dead_code)]
fn suggested_test_mdns_port() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral UDP port should be allocated")
        .local_addr()
        .expect("local address should be available")
        .port()
}
