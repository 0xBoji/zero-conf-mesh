//! Runtime integration tests for multi-node mesh discovery behavior.

use std::{
    error::Error,
    net::{Ipv4Addr, UdpSocket},
    time::Duration,
};

use tokio::time;
use zero_conf_mesh::{AgentStatus, ZeroConfMesh};

#[tokio::test]
async fn mesh_should_propagate_status_updates_to_remote_peers() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = mesh("agent-a", "alpha", "main", 8081, mdns_port).await?;
    let mesh_b = mesh("agent-b", "alpha", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_b, "agent-a").await?;

    mesh_a.update_status(AgentStatus::Busy).await?;

    let peer = wait_for_agent_with_status(&mesh_b, "agent-a", AgentStatus::Busy).await?;

    assert_eq!(peer.status(), AgentStatus::Busy);

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_discover_multiple_peers_on_same_mdns_port() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_a = mesh("agent-a", "alpha", "main", 8081, mdns_port).await?;
    let mesh_b = mesh("agent-b", "alpha", "feature/mesh", 8082, mdns_port).await?;
    let mesh_c = mesh("agent-c", "beta", "main", 8083, mdns_port).await?;

    wait_for_registry_size(&mesh_a, 3).await?;
    wait_for_registry_size(&mesh_b, 3).await?;
    wait_for_registry_size(&mesh_c, 3).await?;

    assert_eq!(
        ids(&mesh_a.agents().await),
        vec!["agent-a", "agent-b", "agent-c"]
    );
    assert_eq!(
        ids(&mesh_a.agents_by_project("alpha").await),
        vec!["agent-a", "agent-b"]
    );
    assert_eq!(
        ids(&mesh_a.agents_by_branch("main").await),
        vec!["agent-a", "agent-c"]
    );

    mesh_a.shutdown().await?;
    mesh_b.shutdown().await?;
    mesh_c.shutdown().await?;

    Ok(())
}

#[tokio::test]
async fn mesh_should_filter_projects_independently_on_shared_lan() -> Result<(), Box<dyn Error>> {
    let mdns_port = available_udp_port();
    let mesh_alpha = mesh("agent-alpha", "alpha", "main", 8081, mdns_port).await?;
    let mesh_beta = mesh("agent-beta", "beta", "main", 8082, mdns_port).await?;

    wait_for_agent(&mesh_alpha, "agent-beta").await?;

    assert_eq!(
        ids(&mesh_alpha.agents_by_project("alpha").await),
        vec!["agent-alpha"]
    );
    assert_eq!(
        ids(&mesh_alpha.agents_by_project("beta").await),
        vec!["agent-beta"]
    );
    assert_eq!(
        ids(&mesh_alpha
            .agents_by_project_and_branch("alpha", "main")
            .await),
        vec!["agent-alpha"]
    );
    assert_eq!(
        ids(&mesh_alpha
            .agents_by_project_and_branch("beta", "main")
            .await),
        vec!["agent-beta"]
    );

    mesh_alpha.shutdown().await?;
    mesh_beta.shutdown().await?;

    Ok(())
}

async fn mesh(
    agent_id: &str,
    project: &str,
    branch: &str,
    port: u16,
    mdns_port: u16,
) -> Result<ZeroConfMesh, zero_conf_mesh::ZeroConfError> {
    ZeroConfMesh::builder()
        .agent_id(agent_id)
        .role("worker")
        .project(project)
        .branch(branch)
        .port(port)
        .mdns_port(mdns_port)
        .heartbeat_interval(Duration::from_millis(200))
        .ttl(Duration::from_secs(2))
        .build()
        .await
}

async fn wait_for_agent(mesh: &ZeroConfMesh, agent_id: &str) -> Result<(), Box<dyn Error>> {
    let _ = wait_for_agent_matching(mesh, agent_id, |_| true).await?;
    Ok(())
}

async fn wait_for_agent_with_status(
    mesh: &ZeroConfMesh,
    agent_id: &str,
    status: AgentStatus,
) -> Result<zero_conf_mesh::AgentInfo, Box<dyn Error>> {
    wait_for_agent_matching(mesh, agent_id, |agent| agent.status() == status).await
}

async fn wait_for_agent_matching<F>(
    mesh: &ZeroConfMesh,
    agent_id: &str,
    predicate: F,
) -> Result<zero_conf_mesh::AgentInfo, Box<dyn Error>>
where
    F: Fn(&zero_conf_mesh::AgentInfo) -> bool,
{
    let deadline = time::Instant::now() + Duration::from_secs(5);
    while time::Instant::now() < deadline {
        if let Some(agent) = mesh.get_agent(agent_id).await
            && predicate(&agent)
        {
            return Ok(agent);
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("timed out waiting for agent `{agent_id}`").into())
}

async fn wait_for_registry_size(
    mesh: &ZeroConfMesh,
    expected: usize,
) -> Result<(), Box<dyn Error>> {
    let deadline = time::Instant::now() + Duration::from_secs(5);
    while time::Instant::now() < deadline {
        if mesh.agents().await.len() == expected {
            return Ok(());
        }

        time::sleep(Duration::from_millis(100)).await;
    }

    Err(format!("timed out waiting for registry size {expected}").into())
}

fn ids(agents: &[zero_conf_mesh::AgentInfo]) -> Vec<&str> {
    agents.iter().map(|agent| agent.id()).collect()
}

fn available_udp_port() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral UDP port should be allocated")
        .local_addr()
        .expect("local address should be available")
        .port()
}
